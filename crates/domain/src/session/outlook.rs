//! Read-only twelve-month capacity planning, including a proposed closest-node city connection.
use super::{ConnectionKind, GameSession};
use crate::{
    error::DomainError,
    ids::{CityId, NodeId},
    time::SimMonth,
    units::Megawatts,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapacityEstimate {
    pub peak_demand: Megawatts,
    pub existing_capacity: Megawatts,
    pub scheduled_capacity: Megawatts,
    pub firm_capacity: Megawatts,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutlookMonth {
    pub month: SimMonth,
    pub baseline: CapacityEstimate,
    pub with_city: Option<CapacityEstimate>,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CityConnectionPreview {
    pub city: CityId,
    pub from: Option<NodeId>,
    pub kind: Option<ConnectionKind>,
    pub months: Option<u32>,
    pub already_scheduled: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub struct YearAheadOutlook {
    pub months: Vec<OutlookMonth>,
    pub connection: Option<CityConnectionPreview>,
}
impl GameSession {
    /// Mean seasonal demand and connected capacity under existing construction orders. The
    /// current representative month is included, except in Review, where it has already closed.
    /// A selected city is connected to the closest eligible currently active grid node, using
    /// the real local line/link quote. A pre-existing ordered path is never ordered twice.
    pub fn year_ahead_outlook(
        &self,
        city: Option<CityId>,
    ) -> Result<YearAheadOutlook, DomainError> {
        let mut proposed = self.power.network().clone();
        let connection = city
            .filter(|id| self.power.cities().contains_key(id))
            .map(|city| self.outlook_connection(city, &mut proposed))
            .transpose()?;
        let first = self
            .clock
            .month()
            .checked_add_months(u32::from(self.phase == super::DayPhase::Review))?;
        let mut months = Vec::with_capacity(12);
        for offset in 0..12 {
            let month = first.checked_add_months(offset)?;
            let baseline = self.outlook_estimate(self.power.network(), offset, month);
            let with_city = connection.map(|c| {
                if !c.already_scheduled && c.months.is_some_and(|ready| ready <= offset) {
                    self.outlook_estimate(&proposed, offset, month)
                } else {
                    baseline
                }
            });
            months.push(OutlookMonth {
                month,
                baseline,
                with_city,
            });
        }
        Ok(YearAheadOutlook { months, connection })
    }

    pub(super) fn outlook_connection(
        &self,
        city: CityId,
        proposed: &mut crate::grid::Network,
    ) -> Result<CityConnectionPreview, DomainError> {
        let target = &self.power.cities()[&city]; // caller checked the ID
        if let Some(months) = proposed.months_until_connected(target.node()) {
            return Ok(CityConnectionPreview {
                city,
                from: None,
                kind: None,
                months: Some(months),
                already_scheduled: true,
            });
        }
        let target_cell = target.def().cell;
        let mut choices: Vec<_> = self
            .power
            .network()
            .nodes()
            .filter(|(node, _)| self.power.node_active(*node) && *node != target.node())
            .filter_map(|(from, node)| {
                let kind = if self.region.area_at(node.cell) == self.region.area_at(target_cell) {
                    ConnectionKind::Ac
                } else {
                    ConnectionKind::Dc
                };
                let months = match kind {
                    ConnectionKind::Ac => self.line_quote(from, target.node()).ok()?.months,
                    ConnectionKind::Dc => self.link_quote(from, target.node()).ok()?.months,
                };
                Some((
                    crate::grid::distance_cells(node.cell, target_cell),
                    from,
                    kind,
                    months,
                ))
            })
            .collect();
        choices.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let Some((_, from, kind, months)) = choices.first().copied() else {
            return Ok(CityConnectionPreview {
                city,
                from: None,
                kind: None,
                months: None,
                already_scheduled: false,
            });
        };
        let status = crate::grid::LineStatus::UnderConstruction {
            months_left: months,
        };
        match kind {
            ConnectionKind::Ac => proposed.add_line(
                crate::ids::LineId(self.ids.next_value()),
                from,
                target.node(),
                status,
            )?,
            ConnectionKind::Dc => proposed.add_link(
                crate::ids::LinkId(self.ids.next_value()),
                from,
                target.node(),
                status,
            )?,
        }
        Ok(CityConnectionPreview {
            city,
            from: Some(from),
            kind: Some(kind),
            months: Some(months),
            already_scheduled: false,
        })
    }

    fn outlook_estimate(
        &self,
        network: &crate::grid::Network,
        offset: u32,
        month: SimMonth,
    ) -> CapacityEstimate {
        let projected = self.connection_power(
            network.projected_after(offset),
            offset,
            month,
            self.growth_on(network, month),
        );
        // Peak of the summed profiles, not a sum of each city's independent maximum. Minute
        // resolution preserves authored sub-hour peaks and local time offsets.
        let peak_demand = (0..crate::time::TICKS_PER_DAY)
            .map(|tick| projected.connected_demand_forecast(&self.region, month, tick))
            .fold(Megawatts::ZERO, Megawatts::max);
        let mut estimate = CapacityEstimate {
            peak_demand,
            existing_capacity: Megawatts::ZERO,
            scheduled_capacity: Megawatts::ZERO,
            firm_capacity: Megawatts::ZERO,
        };
        for plant in projected.plants().values().filter(|plant| {
            projected.network().is_connected(plant.node())
                && !matches!(
                    plant.status(),
                    crate::generation::PlantStatus::UnderConstruction { .. }
                )
        }) {
            if self.power.plants().get(&plant.id()).is_some_and(|p| {
                matches!(
                    p.status(),
                    crate::generation::PlantStatus::UnderConstruction { .. }
                )
            }) {
                estimate.scheduled_capacity += plant.available_capacity();
            } else {
                estimate.existing_capacity += plant.available_capacity();
            }
            estimate.firm_capacity +=
                plant.firm_capacity(&self.region.regulator().storage_accreditation);
        }
        estimate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{region::fixtures, session::GameSettings};
    use std::sync::Arc;
    #[test]
    fn operational_current_month_matches_daily_forecast_without_mutating_state() {
        let s = GameSession::new(
            Arc::new(fixtures::tiny_region()),
            7,
            GameSettings::default(),
        );
        let before = s.state_hash();
        let outlook = s.operational_outlook(None).unwrap();
        assert_eq!(outlook.months.len(), 12);
        assert_eq!(outlook.months[0].baseline.forecast, s.supply_forecast());
        assert_eq!(s.state_hash(), before);
        assert_eq!(outlook, s.operational_outlook(None).unwrap());
    }
    #[test]
    fn outlook_has_twelve_consecutive_months_and_preserves_the_session() {
        let s = GameSession::new(
            Arc::new(fixtures::tiny_region()),
            7,
            GameSettings::default(),
        );
        let before = s.state_hash();
        let outlook = s.year_ahead_outlook(None).unwrap();
        assert_eq!(outlook.months.len(), 12);
        for (i, point) in outlook.months.iter().enumerate() {
            assert_eq!(
                point.month,
                s.clock().month().checked_add_months(i as u32).unwrap()
            );
            assert!(point.baseline.peak_demand > Megawatts::ZERO);
        }
        assert_eq!(s.state_hash(), before);
    }
    #[test]
    fn city_preview_uses_the_closest_current_grid_node_and_waits_for_the_line() {
        let s = GameSession::new(
            Arc::new(fixtures::tiny_region()),
            7,
            GameSettings::default(),
        );
        let before = s.state_hash();
        let outlook = s.year_ahead_outlook(Some(CityId(3))).unwrap();
        let connection = outlook.connection.unwrap();
        let target = s.power.cities().get(&CityId(3)).unwrap();
        let expected = s
            .power
            .network()
            .nodes()
            .filter(|(id, _)| s.power.node_active(*id))
            .min_by(|(a, na), (b, nb)| {
                crate::grid::distance_cells(na.cell, target.def().cell)
                    .total_cmp(&crate::grid::distance_cells(nb.cell, target.def().cell))
                    .then_with(|| a.cmp(b))
            })
            .unwrap()
            .0;
        assert_eq!(connection.from, Some(expected));
        let months = s.line_quote(expected, target.node()).unwrap().months;
        assert_eq!(connection.months, Some(months));
        assert!(months > 0 && months < 12);
        for (offset, point) in outlook.months.iter().enumerate() {
            let scenario = point.with_city.unwrap();
            if offset < months as usize {
                assert_eq!(scenario, point.baseline);
            } else {
                let peak = (0..crate::time::TICKS_PER_DAY)
                    .map(|tick| {
                        s.power
                            .cities()
                            .values()
                            .map(|city| {
                                city.def().base_load
                                    * s.region.load_factor(city.id(), point.month, tick)
                            })
                            .sum::<Megawatts>()
                    })
                    .fold(Megawatts::ZERO, Megawatts::max);
                assert_eq!(scenario.peak_demand, peak);
                assert!(scenario.peak_demand > point.baseline.peak_demand);
            }
        }
        assert_eq!(s.state_hash(), before);
    }

    #[test]
    fn a_previewed_connection_that_completes_the_region_grows_its_months() {
        let outlook = |rate| {
            let mut parts = fixtures::tiny_region_parts();
            parts.demand_growth_per_year = rate;
            let s = GameSession::new(
                Arc::new(crate::region::RegionDefinition::try_from(parts).unwrap()),
                7,
                GameSettings::default(),
            );
            s.year_ahead_outlook(Some(CityId(3))).unwrap()
        };
        let (flat, grown) = (outlook(0.0), outlook(0.024));
        let ready = grown.connection.unwrap().months.unwrap();
        assert!(ready > 0 && ready < 11);
        for (k, (f, g)) in flat.months.iter().zip(&grown.months).enumerate() {
            // Without the proposal no city is added, so nothing grows.
            assert_eq!(g.baseline, f.baseline);
            let steps = (k as u32 + 1).saturating_sub(ready);
            let factor = crate::demand::DemandGrowth::NONE
                .ahead(1.002, steps)
                .factor();
            let (f, g) = (f.with_city.unwrap(), g.with_city.unwrap());
            let expected = f.peak_demand * factor;
            assert!(
                (g.peak_demand - expected).value().abs() <= expected.value() * 1e-12,
                "month {k}: {:?} vs {expected:?}",
                g.peak_demand
            );
        }
    }

    #[test]
    fn ordered_city_paths_are_already_in_the_baseline_and_never_count_twice() {
        let mut s = GameSession::new(
            Arc::new(fixtures::tiny_region()),
            7,
            GameSettings::default(),
        );
        let target = s.power.cities()[&CityId(3)].node();
        let before_order = s.year_ahead_outlook(None).unwrap();
        let ready = s.line_quote(s.anchor_node(), target).unwrap().months as usize;
        s.build_line(s.anchor_node(), target).unwrap();
        let outlook = s.year_ahead_outlook(Some(CityId(3))).unwrap();
        assert!(outlook.connection.unwrap().already_scheduled);
        for (offset, point) in outlook.months.iter().enumerate() {
            assert_eq!(point.with_city.unwrap(), point.baseline);
            if offset < ready {
                assert_eq!(
                    point.baseline.peak_demand,
                    before_order.months[offset].baseline.peak_demand
                );
            } else {
                assert!(
                    point.baseline.peak_demand > before_order.months[offset].baseline.peak_demand,
                    "scheduled city demand joins the baseline when its connection is ready"
                );
            }
        }
        assert!(
            s.year_ahead_outlook(Some(CityId(u32::MAX)))
                .unwrap()
                .connection
                .is_none()
        );
    }

    #[test]
    fn review_starts_with_the_next_unplayed_month() {
        let mut s = GameSession::new(
            Arc::new(fixtures::tiny_region()),
            7,
            GameSettings::default(),
        );
        s.begin_operating().unwrap();
        for _ in 0..crate::time::TICKS_PER_DAY {
            s.tick().unwrap();
        }
        assert_eq!(s.phase(), super::super::DayPhase::Review);
        assert_eq!(
            s.year_ahead_outlook(None).unwrap().months[0].month,
            s.clock().month().next()
        );
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(16))]
        #[test]
        fn scheduled_capacity_waits_for_both_plant_and_connection(build_months in 1u32..8) {
            let mut parts = fixtures::tiny_region_parts();
            parts.catalog.iter_mut().find(|p| p.kind == crate::region::PlantKind::Ccgt).unwrap().build_months = build_months;
            let mut s = GameSession::new(Arc::new(crate::region::RegionDefinition::try_from(parts).unwrap()), 7, GameSettings::default());
            let (plant, _) = s.build_plant(crate::region::PlantKind::Ccgt, crate::ids::CellCoord { x: 1, y: 7 }).unwrap();
            let node = s.power.plants()[&plant].node();
            proptest::prop_assert!(s.year_ahead_outlook(None).unwrap().months.iter().all(|p| p.baseline.scheduled_capacity == Megawatts::ZERO));
            let quote = s.line_quote(s.anchor_node(), node).unwrap();
            s.build_line(s.anchor_node(), node).unwrap();
            let ready = build_months.max(quote.months);
            let capacity = s.power.plants()[&plant].spec().capacity;
            let before = s.state_hash();
            let outlook = s.year_ahead_outlook(None).unwrap();
            for (offset, point) in outlook.months.iter().enumerate() {
                proptest::prop_assert_eq!(point.baseline.scheduled_capacity, if (offset as u32) < ready { Megawatts::ZERO } else { capacity });
                proptest::prop_assert!(point.baseline.firm_capacity <= point.baseline.existing_capacity + point.baseline.scheduled_capacity);
            }
            proptest::prop_assert_eq!(s.state_hash(), before);
        }
    }
}
