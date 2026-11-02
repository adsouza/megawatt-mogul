//! Immutable representative-day checks, evaluated one month at a time by the client worker.
use super::{DayPhase, GameSession, YearAheadOutlook};
use crate::{
    error::DomainError,
    grid::{Network, SupplyForecast},
    ids::CityId,
    time::{SimMonth, TICKS_PER_DAY},
    units::{MegawattHours, Megawatts},
};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct OperationalDay {
    pub forecast: SupplyForecast,
    /// Sum of local deficits: spare supply in another island cannot cancel a shortage.
    pub shortfalls: Vec<Megawatts>,
    pub worst_balance: Megawatts,
    pub worst_minute: u32,
    pub shortfall_minutes: u32,
    /// Expected unmet energy in the representative day, not a monthly total.
    pub unserved_energy: MegawattHours,
}

impl OperationalDay {
    fn from_forecasts(forecasts: Vec<SupplyForecast>) -> Self {
        let mut total = forecasts.first().expect("home grid forecast").clone();
        for next in forecasts.iter().skip(1) {
            total.has_storage |= next.has_storage;
            total.assess_every_minute |= next.assess_every_minute;
            total.hour_sample_minutes.extend(&next.hour_sample_minutes);
            for (a, b) in total.minutes.iter_mut().zip(&next.minutes) {
                a.demand += b.demand;
                a.gross_supply += b.gross_supply;
                a.battery_charge += b.battery_charge;
                a.battery_discharge += b.battery_discharge;
                a.battery_power += b.battery_power;
                a.battery_deliverable_energy += b.battery_deliverable_energy;
                for (&kind, &mw) in &b.generation {
                    *a.generation.entry(kind).or_insert(Megawatts::ZERO) += mw;
                }
            }
        }
        let mut day = Self {
            forecast: total,
            shortfalls: Vec::with_capacity(TICKS_PER_DAY as usize),
            worst_balance: Megawatts::new(f64::MAX),
            worst_minute: 0,
            shortfall_minutes: 0,
            unserved_energy: MegawattHours::ZERO,
        };
        for (minute, point) in day.forecast.minutes.iter().enumerate() {
            let shortage: f64 = forecasts
                .iter()
                .map(|f| {
                    let p = &f.minutes[minute];
                    let deficit = (p.demand - p.net_supply()).value();
                    if deficit > 1e-6 { deficit } else { 0.0 }
                })
                .sum();
            let balance = if shortage > 0.0 {
                -shortage
            } else {
                (point.net_supply() - point.demand).value().max(0.0)
            };
            if balance < day.worst_balance.value() {
                day.worst_balance = Megawatts::new(balance);
                day.worst_minute = minute as u32;
            }
            if shortage > 0.0 {
                day.shortfall_minutes += 1;
            }
            day.unserved_energy += MegawattHours::new(shortage / 60.0);
            day.shortfalls.push(Megawatts::new(shortage));
        }
        day
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperationalMonth {
    pub baseline: Arc<OperationalDay>,
    pub with_city: Option<Arc<OperationalDay>>,
}
impl OperationalMonth {
    pub fn selected(&self) -> &OperationalDay {
        self.with_city.as_deref().unwrap_or(&self.baseline)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperationalOutlook {
    pub capacity: YearAheadOutlook,
    pub months: Vec<OperationalMonth>,
    source_hash: u64,
}
impl OperationalOutlook {
    /// Retain shared baseline curves while dropping the selected city's overlay.
    pub fn baseline_only(&self) -> Self {
        let mut baseline = self.clone();
        baseline.capacity.connection = None;
        for point in &mut baseline.capacity.months {
            point.with_city = None;
        }
        for month in &mut baseline.months {
            month.with_city = None;
        }
        baseline
    }
}

/// Work owns a snapshot. No RNG, clock, live reserves or session state are changed.
pub struct OperationalOutlookWork {
    session: GameSession,
    proposed: Network,
    result: OperationalOutlook,
    baseline: Option<Arc<OperationalOutlook>>,
}
impl OperationalOutlookWork {
    /// A cached baseline is reused only when its complete source state matches.
    pub fn new(
        session: GameSession,
        city: Option<CityId>,
        baseline: Option<Arc<OperationalOutlook>>,
    ) -> Result<Self, DomainError> {
        let source_hash = session.state_hash();
        let capacity = session.year_ahead_outlook(city)?;
        let mut proposed = session.power.network().clone();
        if let Some(city) = capacity.connection {
            session.outlook_connection(city.city, &mut proposed)?;
        }
        Ok(Self {
            session,
            proposed,
            result: OperationalOutlook {
                capacity,
                months: Vec::with_capacity(12),
                source_hash,
            },
            baseline: baseline.filter(|b| b.source_hash == source_hash),
        })
    }

    /// Completes one monthly baseline/scenario pair. True means all twelve months are ready.
    pub fn advance(&mut self) -> bool {
        let offset = self.result.months.len();
        if offset == 12 {
            return true;
        }
        let month = self.result.capacity.months[offset].month;
        // Review has already advanced construction at month close; its first future
        // Planning month must not advance those projects a second time.
        let ahead = offset as u32;
        let baseline = self
            .baseline
            .as_ref()
            .map(|b| b.months[offset].baseline.clone())
            .unwrap_or_else(|| {
                Arc::new(
                    self.session
                        .operational_day(self.session.power.network(), ahead, month),
                )
            });
        let with_city = self.result.capacity.connection.map(|c| {
            if !c.already_scheduled && c.months.is_some_and(|ready| ready <= ahead) {
                Arc::new(self.session.operational_day(&self.proposed, ahead, month))
            } else {
                baseline.clone()
            }
        });
        self.result.months.push(OperationalMonth {
            baseline,
            with_city,
        });
        self.result.months.len() == 12
    }

    pub fn finish(self) -> OperationalOutlook {
        assert_eq!(self.result.months.len(), 12, "completed outlook work");
        self.result
    }
}

impl GameSession {
    pub fn operational_outlook(
        &self,
        city: Option<CityId>,
    ) -> Result<OperationalOutlook, DomainError> {
        let mut work = OperationalOutlookWork::new(self.clone(), city, None)?;
        while !work.advance() {}
        Ok(work.finish())
    }

    fn operational_day(&self, network: &Network, ahead: u32, month: SimMonth) -> OperationalDay {
        if month == self.clock.month()
            && self.phase != DayPhase::Review
            && network == self.power.network()
        {
            if self.region.areas().is_none() || self.power.island_nodes().is_empty() {
                return OperationalDay::from_forecasts(vec![self.supply_forecast()]);
            }
            let planned = self.power.with_transfer_plan(&self.region, month);
            return OperationalDay::from_forecasts(
                planned
                    .island_nodes()
                    .keys()
                    .filter_map(|id| {
                        planned
                            .scoped_island(*id, &self.region)
                            .map(|p| p.supply_forecast(&self.region, month))
                    })
                    .collect(),
            );
        }
        let projected = self.connection_power(network.projected_after(ahead), ahead, month);
        if self.region.areas().is_none() {
            return OperationalDay::from_forecasts(vec![
                projected.supply_forecast(&self.region, month),
            ]);
        }
        OperationalDay::from_forecasts(
            projected
                .island_nodes()
                .keys()
                .filter_map(|id| {
                    projected
                        .scoped_connection_island(*id, &self.region)
                        .map(|p| p.supply_forecast(&self.region, month))
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ids::CellCoord,
        region::{PlantKind, RegionDefinition, StartPlantDef, fixtures},
        session::GameSettings,
    };

    fn session() -> GameSession {
        GameSession::new(
            Arc::new(fixtures::tiny_region()),
            7,
            GameSettings::default(),
        )
    }

    #[test]
    fn operational_empty_storage_is_not_accredited_supply_and_hydro_is_water_limited() {
        let mut parts = fixtures::tiny_region_parts();
        parts.start.fleet.retain(|p| p.kind == PlantKind::Ccgt);
        parts.start.fleet.extend([
            StartPlantDef {
                kind: PlantKind::Battery,
                cell: CellCoord { x: 5, y: 1 },
            },
            StartPlantDef {
                kind: PlantKind::Hydro,
                cell: CellCoord { x: 10, y: 1 },
            },
        ]);
        parts.hydro.monthly_flow = [0.01; 12];
        for climate in &mut parts.climates {
            climate.monthly = [1.0; 12];
            climate.shapes.fill([1.0; 24]);
        }
        let thermal = parts
            .catalog
            .iter()
            .find(|p| p.kind == PlantKind::Ccgt)
            .unwrap()
            .capacity
            .value();
        let count = parts.start.connected_cities.len() as f64;
        for city in &mut parts.cities {
            city.base_load = Megawatts::new((thermal + 100.0) / count);
        }
        let s = GameSession::new(
            Arc::new(RegionDefinition::try_from(parts).unwrap()),
            7,
            GameSettings::default(),
        );
        let outlook = s.operational_outlook(None).unwrap();
        for (capacity, operation) in outlook.capacity.months.iter().zip(&outlook.months) {
            assert!(capacity.baseline.firm_capacity > capacity.baseline.peak_demand);
            assert!(operation.baseline.worst_balance.value() < 0.0);
            assert_eq!(operation.baseline.shortfall_minutes, TICKS_PER_DAY);
            assert!(
                operation
                    .baseline
                    .forecast
                    .minutes
                    .iter()
                    .all(|p| p.battery_charge == Megawatts::ZERO
                        && p.battery_discharge == Megawatts::ZERO)
            );
        }
    }

    #[test]
    fn operational_city_preview_reuses_baseline_and_waits_for_connection() {
        let s = session();
        let baseline = Arc::new(s.operational_outlook(None).unwrap());
        let mut work =
            OperationalOutlookWork::new(s.clone(), Some(CityId(3)), Some(baseline.clone()))
                .unwrap();
        while !work.advance() {}
        let scenario = work.finish();
        let ready = scenario.capacity.connection.unwrap().months.unwrap() as usize;
        for (index, month) in scenario.months.iter().enumerate() {
            assert!(Arc::ptr_eq(
                &month.baseline,
                &baseline.months[index].baseline
            ));
            if index < ready {
                assert!(Arc::ptr_eq(
                    &month.baseline,
                    month.with_city.as_ref().unwrap()
                ));
            } else {
                assert!(
                    month.selected().forecast.minutes[0].demand
                        > month.baseline.forecast.minutes[0].demand
                );
            }
        }
        let mut changed = s;
        let plant = changed
            .plant_views()
            .iter()
            .find(|p| p.kind == PlantKind::Ccgt)
            .unwrap()
            .id;
        changed.commit_plant(plant, [false; 24]).unwrap();
        let mut work = OperationalOutlookWork::new(changed, None, Some(baseline.clone())).unwrap();
        while !work.advance() {}
        assert!(!Arc::ptr_eq(
            &work.finish().months[0].baseline,
            &baseline.months[0].baseline
        ));
    }

    #[test]
    fn operational_future_orders_wait_for_both_generator_and_line() {
        let mut s = session();
        let (plant, _) = s
            .build_plant(PlantKind::Ccgt, CellCoord { x: 1, y: 7 })
            .unwrap();
        let node = s.power.plants()[&plant].node();
        let ready = s
            .region
            .spec(PlantKind::Ccgt)
            .build_months
            .max(s.line_quote(s.anchor_node(), node).unwrap().months);
        let before = s.operational_outlook(None).unwrap();
        s.build_line(s.anchor_node(), node).unwrap();
        let after = s.operational_outlook(None).unwrap();
        for (index, month) in after.months.iter().enumerate() {
            let old =
                before.months[index].selected().forecast.minutes[0].generation[&PlantKind::Ccgt];
            let new = month.selected().forecast.minutes[0].generation[&PlantKind::Ccgt];
            assert_eq!(
                new - old,
                if (index as u32) < ready {
                    Megawatts::ZERO
                } else {
                    s.region.spec(PlantKind::Ccgt).capacity
                }
            );
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(32))]
        #[test]
        fn operational_local_shortages_cannot_be_cancelled_by_remote_surplus(
            demand in 1.0f64..1000.0, shortage in 0.01f64..100.0, spare in 100.0f64..1000.0,
        ) {
            let mut a = session().supply_forecast();
            let mut b = a.clone();
            for p in &mut a.minutes { p.demand = Megawatts::new(demand + shortage); p.gross_supply = Megawatts::new(demand); p.battery_charge = Megawatts::ZERO; }
            for p in &mut b.minutes { p.demand = Megawatts::new(demand); p.gross_supply = Megawatts::new(demand + spare); p.battery_charge = Megawatts::ZERO; }
            let day = OperationalDay::from_forecasts(vec![a, b]);
            proptest::prop_assert!(day.forecast.minutes[0].net_supply() > day.forecast.minutes[0].demand);
            proptest::prop_assert!((day.worst_balance.value() + shortage).abs() < 1e-6);
            proptest::prop_assert_eq!(day.shortfall_minutes, TICKS_PER_DAY);
            proptest::prop_assert_eq!(day.worst_minute, 0);
            proptest::prop_assert!((day.unserved_energy.value() - shortage * 24.0).abs() < 1e-6);
        }
    }
}
