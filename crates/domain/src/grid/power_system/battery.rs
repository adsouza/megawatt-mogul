//! Coordinated storage plans and current-tick flow proposals. Inventory settles after protection.
use super::{PowerSystem, is_commissioned};
use crate::events::DomainEvent;
use crate::generation::{BatteryFlow, Plant, PlantStatus, fill_minutes};
use crate::grid::GridMode;
use crate::ids::PlantId;
use crate::region::{BatterySpec, PlantKind, RegionDefinition};
use crate::time::{SimMonth, TICKS_PER_DAY, TICKS_PER_HOUR};
use crate::units::{MegawattHours, Megawatts, Ratio};
use std::cell::OnceCell;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct PlantBatteryScheduleView {
    pub charge: Vec<Megawatts>,
    /// Expected gap coverage, not an instruction for generators to back off.
    pub discharge: Vec<Megawatts>,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BatteryScheduleView {
    pub plants: BTreeMap<PlantId, PlantBatteryScheduleView>,
    pub expected_charging: bool,
}

/// One minute of expected Planning supply. Battery fleets use the sequential
/// shortfall evaluator. All flows are grid-side MW; storage starts empty each day.
#[derive(Debug, Clone, PartialEq)]
pub struct SupplyForecastPoint {
    pub demand: Megawatts,
    pub generation: BTreeMap<PlantKind, Megawatts>,
    /// Generation plus feasible discharge, before optional charging.
    pub gross_supply: Megawatts,
    pub battery_charge: Megawatts,
    pub battery_discharge: Megawatts,
}

impl SupplyForecastPoint {
    pub fn net_supply(&self) -> Megawatts {
        self.gross_supply - self.battery_charge
    }
}

/// Shared supply source for Planning charts and shortfall checks.
#[derive(Debug, Clone, PartialEq)]
pub struct SupplyForecast {
    /// Storage requires every minute to be assessed; other fleets use hour endpoints.
    pub has_storage: bool,
    pub assess_every_minute: bool,
    pub minutes: Vec<SupplyForecastPoint>,
}

impl SupplyForecast {
    /// Points assessed by the warning: all minutes with storage, hour endpoints otherwise.
    pub fn assessed_minutes(&self) -> impl Iterator<Item = (u32, &SupplyForecastPoint)> {
        self.minutes
            .iter()
            .enumerate()
            .filter(|(tick, _)| {
                let minute = tick % TICKS_PER_HOUR as usize;
                self.assess_every_minute || minute == 0 || minute == TICKS_PER_HOUR as usize - 1
            })
            .map(|(tick, point)| (tick as u32, point))
    }
}

pub(super) fn charging_window(
    region: &RegionDefinition,
    month: SimMonth,
    spec: BatterySpec,
    tick: u32,
) -> bool {
    let _ = (region, month);
    tick < u32::from(spec.charge_end_hour) * TICKS_PER_HOUR
}
fn discharging_window(spec: BatterySpec, tick: u32) -> bool {
    tick >= u32::from(spec.discharge_start_hour) * TICKS_PER_HOUR && tick < TICKS_PER_DAY
}

/// Scratch data for one immutable planning query. Membership and site potential do not
/// change while the query runs. Keep ID order and the original arithmetic; no cached data
/// enters saved state or survives a command, tick, inventory change or wind observation.
pub(super) struct BatteryPlanningInputs<'a> {
    power: &'a PowerSystem,
    region: &'a RegionDefinition,
    month: SimMonth,
    batteries: Vec<&'a Plant>,
    dispatchable: Vec<&'a Plant>,
    thermal: Vec<&'a Plant>,
    cities: Vec<&'a crate::demand::DemandCenter>,
    solar: Vec<(Megawatts, Ratio)>,
    wind_potential: Megawatts,
    minutes: Vec<OnceCell<PlanningMinute>>,
}

pub(super) struct PlanningMinute {
    pub(super) demand: Megawatts,
    pub(super) solar: Megawatts,
    pub(super) wind: Megawatts,
}

impl<'a> BatteryPlanningInputs<'a> {
    pub(super) fn new(
        power: &'a PowerSystem,
        region: &'a RegionDefinition,
        month: SimMonth,
    ) -> Self {
        let plants: Vec<_> = power.connected_plants().collect();
        Self {
            power,
            region,
            month,
            batteries: plants
                .iter()
                .copied()
                .filter(|p| p.battery().is_some())
                .collect(),
            dispatchable: plants
                .iter()
                .copied()
                .filter(|p| p.kind().is_dispatchable())
                .collect(),
            thermal: plants
                .iter()
                .copied()
                .filter(|p| p.kind().is_dispatchable() && p.kind() != PlantKind::Hydro)
                .collect(),
            cities: power.connected_cities().collect(),
            solar: plants
                .iter()
                .filter(|p| p.kind() == PlantKind::Solar)
                .map(|p| {
                    (
                        p.spec().capacity,
                        super::cell_ratio(region, p.cell(), |c| c.irradiance),
                    )
                })
                .collect(),
            wind_potential: plants
                .iter()
                .filter(|p| p.kind() == PlantKind::Wind)
                .map(|p| {
                    p.spec().capacity
                        * super::cell_ratio(region, p.cell(), |c| c.wind_potential).value()
                })
                .sum(),
            minutes: (0..TICKS_PER_DAY).map(|_| OnceCell::new()).collect(),
        }
    }

    pub(super) fn minute(&self, tick: u32) -> &PlanningMinute {
        self.minutes[tick as usize].get_or_init(|| {
            let sun = crate::generation::sun_factor(self.region.solar(), self.month, tick);
            let clearness = self.region.solar().clearness_mean[self.month.index0()];
            PlanningMinute {
                demand: crate::demand::forecast_demand(
                    self.cities.iter().copied(),
                    self.region.demand(),
                    self.month,
                    tick,
                ),
                solar: self
                    .solar
                    .iter()
                    .map(|&(capacity, irradiance)| {
                        crate::generation::solar_available(capacity, irradiance, sun, clearness)
                    })
                    .sum(),
                wind: self.wind_potential
                    * self
                        .power
                        .wind_factor_forecast(self.region, self.month, tick)
                        .0,
            }
        })
    }

    pub(super) fn thermal_for_charge(&self, tick: u32, from: u32, preview: bool) -> Megawatts {
        self.thermal
            .iter()
            .copied()
            .map(|p| {
                let committed = p.commitment().is_committed((tick / TICKS_PER_HOUR) as u8);
                if preview {
                    return if committed {
                        if p.auto_follow() {
                            p.spec().capacity
                        } else {
                            p.initial_target()
                        }
                    } else {
                        Megawatts::ZERO
                    };
                }
                if !committed && !p.is_commitment_suppressed() {
                    return Megawatts::ZERO;
                }
                match p.status() {
                    PlantStatus::Online { output, target } => {
                        if p.auto_follow() {
                            p.spec()
                                .capacity
                                .min(output + p.spec().ramp.over_ticks(tick.saturating_sub(from)))
                        } else {
                            target.min(output + p.spec().ramp.over_ticks(tick.saturating_sub(from)))
                        }
                    }
                    PlantStatus::Starting { ticks_left }
                        if tick.saturating_sub(from) >= ticks_left =>
                    {
                        p.spec().capacity.min(
                            p.spec().min_output()
                                + p.spec().ramp.over_ticks(tick - from - ticks_left),
                        )
                    }
                    _ => Megawatts::ZERO,
                }
            })
            .sum()
    }
}

impl PowerSystem {
    pub(crate) fn invalidate_battery_plan(&mut self) {
        self.battery_plan_dirty = true;
    }
    /// A generator command or trip actually changed the fleet: storage re-plans
    /// on the next tick. Without any battery there is no plan to refresh, so
    /// the state is left alone.
    pub(crate) fn generators_changed(&mut self) {
        if self.plants.values().any(|p| p.battery().is_some()) {
            self.invalidate_battery_plan();
        }
    }
    #[cfg(test)]
    pub(crate) fn battery_plan_is_dirty(&self) -> bool {
        self.battery_plan_dirty
    }
    pub(crate) fn has_batteries(&self) -> bool {
        self.connected_plants()
            .any(|p| p.battery().is_some_and(|b| b.enabled()))
    }
    pub(crate) fn battery_schedule(&self) -> BatteryScheduleView {
        self.battery_plan.clone()
    }
    pub(crate) fn preview_batteries(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
    ) -> BatteryScheduleView {
        self.compute_battery_plan(region, month, 0, true)
    }
    pub(super) fn plan_batteries(&mut self, region: &RegionDefinition, month: SimMonth, from: u32) {
        let mut next = self.compute_battery_plan(region, month, from, false);
        for (id, plan) in &mut next.plants {
            if let Some(old) = self.battery_plan.plants.get(id) {
                let end = (from as usize).min(plan.charge.len());
                plan.charge[..end].copy_from_slice(&old.charge[..end]);
                plan.discharge[..end].copy_from_slice(&old.discharge[..end]);
            }
        }
        for (id, old) in &self.battery_plan.plants {
            if !next.plants.contains_key(id) && from > 0 {
                let mut frozen = PlantBatteryScheduleView {
                    charge: vec![Megawatts::ZERO; 1440],
                    discharge: vec![Megawatts::ZERO; 1440],
                };
                let end = (from as usize).min(1440);
                frozen.charge[..end].copy_from_slice(&old.charge[..end]);
                frozen.discharge[..end].copy_from_slice(&old.discharge[..end]);
                next.plants.insert(*id, frozen);
            }
        }
        self.battery_plan = next;
        self.battery_plan_dirty = false;
    }
    fn compute_battery_plan(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        from: u32,
        preview: bool,
    ) -> BatteryScheduleView {
        if !self.connected_plants().any(|p| p.battery().is_some()) {
            return BatteryScheduleView::default();
        }
        let inputs = BatteryPlanningInputs::new(self, region, month);
        let mut plan = self.charge_plan_with_inventory(&inputs, from, preview, None);
        if !inputs
            .batteries
            .iter()
            .any(|p| p.battery().is_some_and(|b| b.enabled()))
        {
            return plan;
        }
        // Delivery is an estimate of gaps left by generation, never a generation reduction request.
        self.project_storage_supply_with_inputs(
            &inputs,
            from,
            TICKS_PER_DAY,
            preview,
            &mut plan,
            false,
        );
        plan
    }

    pub(super) fn charge_plan_with_inventory(
        &self,
        inputs: &BatteryPlanningInputs<'_>,
        from: u32,
        preview: bool,
        inventory: Option<&BTreeMap<PlantId, crate::generation::BatteryState>>,
    ) -> BatteryScheduleView {
        if inputs.batteries.is_empty() {
            return BatteryScheduleView::default();
        }
        let has_room = inputs.batteries.iter().copied().any(|p| {
            p.battery()
                .zip(p.spec().battery)
                .is_some_and(|(state, spec)| {
                    let stored = inventory.and_then(|v| v.get(&p.id())).map_or_else(
                        || if preview { 0.0 } else { state.stored().value() },
                        |s| s.stored().value(),
                    );
                    state.enabled()
                        && from < u32::from(spec.charge_end_hour) * TICKS_PER_HOUR
                        && stored < spec.energy_capacity.value()
                })
        });
        if !has_room {
            return BatteryScheduleView {
                expected_charging: preview,
                plants: inputs
                    .batteries
                    .iter()
                    .copied()
                    .map(|p| {
                        (
                            p.id(),
                            PlantBatteryScheduleView {
                                charge: vec![Megawatts::ZERO; TICKS_PER_DAY as usize],
                                discharge: vec![Megawatts::ZERO; TICKS_PER_DAY as usize],
                            },
                        )
                    })
                    .collect(),
            };
        }
        let n = TICKS_PER_DAY as usize;
        let mut charge_residual: Vec<_> = (0..TICKS_PER_DAY)
            .map(|t| {
                let point = inputs.minute(t);
                point.demand - (point.solar + point.wind)
            })
            .collect();
        // No battery can use headroom outside this window. The residual still spans
        // the full day because its extrema determine fill_minutes' bisection bounds.
        let charge_end = inputs
            .batteries
            .iter()
            .filter_map(|p| {
                p.battery().filter(|b| b.enabled())?;
                Some(u32::from(p.spec().battery?.charge_end_hour) * TICKS_PER_HOUR)
            })
            .max()
            .unwrap_or(0);
        let mut headroom: Vec<_> = (0..TICKS_PER_DAY)
            .map(|t| {
                if t >= from && t < charge_end {
                    (inputs.thermal_for_charge(t, from, preview) - charge_residual[t as usize])
                        .max(Megawatts::ZERO)
                } else {
                    Megawatts::ZERO
                }
            })
            .collect();
        let mut result = BatteryScheduleView {
            expected_charging: preview,
            ..Default::default()
        };
        for p in &inputs.batteries {
            let (Some(spec), Some(state)) = (p.spec().battery, p.battery()) else {
                continue;
            };
            let enabled = state.enabled();
            let stored = inventory
                .and_then(|values| values.get(&p.id()))
                .map_or_else(
                    || if preview { 0.0 } else { state.stored().value() },
                    |s| s.stored().value(),
                );
            let charge_caps: Vec<_> = (0..TICKS_PER_DAY)
                .map(|t| {
                    if enabled && t >= from && charging_window(inputs.region, inputs.month, spec, t)
                    {
                        headroom[t as usize].min(spec.charge_power)
                    } else {
                        Megawatts::ZERO
                    }
                })
                .collect();
            let charge = fill_minutes(
                &charge_residual,
                &charge_caps,
                MegawattHours::new(
                    (spec.energy_capacity.value() - stored).max(0.0)
                        / spec.charge_efficiency.value(),
                ),
                true,
            );
            for t in 0..n {
                charge_residual[t] += charge[t];
                headroom[t] = (headroom[t] - charge[t]).max(Megawatts::ZERO);
            }
            result.plants.insert(
                p.id(),
                PlantBatteryScheduleView {
                    charge,
                    discharge: vec![Megawatts::ZERO; n],
                },
            );
        }
        result
    }
    pub(super) fn primary_supply(&self) -> Megawatts {
        self.connected_plants()
            .filter(|p| p.kind() != PlantKind::Battery)
            .map(Plant::output)
            .sum()
    }
    /// Sharing thermal ramp cover prevents several batteries each reserving the whole fleet.
    fn battery_ramp_share(&self) -> f64 {
        let n = self
            .connected_plants()
            .filter(|p| p.battery().is_some_and(|b| b.enabled()))
            .count()
            .max(1) as f64;
        let ramp: f64 = self
            .connected_plants()
            .filter(|p| p.auto_follow() && p.is_online())
            .map(|p| p.spec().ramp.value())
            .sum();
        ramp / n
    }
    /// Max present flow whose one-minute use plus a ramp tail fits in energy/room.
    fn taper(power: f64, energy: f64, ramp: f64, minutes: f64) -> f64 {
        if ramp <= 0.0 {
            return 0.0;
        }
        power
            .min(((ramp * ramp + 2.0 * ramp * energy.max(0.0) * 60.0).sqrt() - ramp).max(0.0))
            .min(ramp * minutes.max(0.0))
    }
    pub(super) fn battery_charge_request(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
        flows: &BTreeMap<PlantId, BatteryFlow>,
        customer_load: Megawatts,
    ) -> Megawatts {
        let mut net = Megawatts::ZERO;
        let ramp_share = self.battery_ramp_share();
        if self.battery_plan.plants.is_empty() || self.protection.mode != GridMode::Normal {
            return net;
        }
        let charge_ok = self.outage == Ratio::ZERO
            && self.protection.shed == Ratio::ZERO
            && self.deviation.value() >= -region.frequency().normal_band.value();
        // Optional charging waits for cheaper generation to ramp rather than starting a
        // persistent peaker allocation merely to meet a sudden charging request.
        let cheapest = self
            .connected_plants()
            .filter(|p| {
                p.auto_follow() && p.kind() != PlantKind::Hydro && p.kind().is_dispatchable()
            })
            .filter(|p| p.headroom_up(p.spec().capacity) > Megawatts::ZERO)
            .min_by(|a, b| {
                a.spec()
                    .marginal_cost
                    .value()
                    .total_cmp(&b.spec().marginal_cost.value())
                    .then(a.id().cmp(&b.id()))
            });
        let cheap_room = cheapest.map_or(Megawatts::ZERO, |p| p.headroom_up(p.spec().capacity));
        let mut charge_budget =
            (self.primary_supply() + cheap_room - customer_load).max(Megawatts::ZERO);
        for p in self.connected_plants() {
            let (Some(spec), Some(state), Some(plan)) = (
                p.spec().battery,
                p.battery(),
                self.battery_plan.plants.get(&p.id()),
            ) else {
                continue;
            };
            if !state.enabled() {
                continue;
            }
            let stored = flows
                .get(&p.id())
                .map_or(state.stored().value(), |f| f.stored.value());
            if tick < TICKS_PER_DAY && charging_window(region, month, spec, tick) && charge_ok {
                let end = f64::from(spec.charge_end_hour) * 60.0;
                let input_room = (spec.energy_capacity.value() - stored).max(0.0)
                    / spec.charge_efficiency.value();
                let request = Self::taper(
                    plan.charge[tick as usize].value(),
                    input_room,
                    ramp_share,
                    end - f64::from(tick) - 1.0,
                );
                let request = Megawatts::new(request).min(charge_budget);
                net += request;
                charge_budget -= request;
            }
        }
        net
    }
    pub(super) fn propose_battery_flows(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
        demand: Megawatts,
        served: Megawatts,
    ) -> BTreeMap<PlantId, BatteryFlow> {
        let primary = self.primary_supply();
        let mut surplus = (primary - demand).max(Megawatts::ZERO);
        let mut deficit = (served - primary).max(Megawatts::ZERO);
        let charge_ok = self.protection.mode == GridMode::Normal
            && self.outage == Ratio::ZERO
            && self.protection.shed == Ratio::ZERO
            && self.deviation.value() >= -region.frequency().normal_band.value();
        let active = self.protection.mode == GridMode::Normal;
        let ramp_share = self.battery_ramp_share();
        let renewable: Megawatts = self
            .connected_plants()
            .filter(|p| p.kind().is_variable())
            .map(Plant::output)
            .sum();
        let renewable_surplus = (renewable - demand).max(Megawatts::ZERO);
        let mut flows = BTreeMap::new();
        for p in self.plants.values().filter(|p| is_commissioned(p)) {
            let (Some(spec), Some(state)) = (p.spec().battery, p.battery()) else {
                continue;
            };
            let connected = self.on_grid(p.node());
            let mut charge = Megawatts::ZERO;
            let mut discharge = Megawatts::ZERO;
            if connected && active && state.enabled() {
                if discharging_window(spec, tick) && deficit > Megawatts::ZERO {
                    discharge = deficit;
                } else if charging_window(region, month, spec, tick) && charge_ok {
                    // Room for the generating fleet's ramp-down tail takes priority over extra absorption.
                    let end = f64::from(spec.charge_end_hour) * 60.0;
                    let safe = Self::taper(
                        spec.charge_power.value(),
                        (spec.energy_capacity.value() - state.stored().value())
                            / spec.charge_efficiency.value(),
                        ramp_share,
                        end - f64::from(tick),
                    );
                    // Existing renewable surplus needs no additional thermal ramp tail.
                    charge = surplus.min(Megawatts::new(safe).max(renewable_surplus));
                }
            }
            let flow = state.feasible(spec, p.spec().capacity, charge, discharge);
            surplus = (surplus - flow.charge).max(Megawatts::ZERO);
            deficit = (deficit - flow.discharge).max(Megawatts::ZERO);
            flows.insert(p.id(), flow);
        }
        flows
    }
    pub(super) fn stage_batteries(&mut self, flows: &BTreeMap<PlantId, BatteryFlow>) {
        for (id, flow) in flows {
            self.plants
                .get_mut(id)
                .expect("flow plant")
                .battery_mut()
                .expect("storage")
                .stage(*flow);
        }
    }
    pub(super) fn settle_batteries(&mut self, flows: &BTreeMap<PlantId, BatteryFlow>) {
        for (id, flow) in flows {
            let p = self.plants.get_mut(id).expect("flow plant");
            let spec = p.spec().battery.expect("storage");
            let max = p.spec().capacity;
            p.battery_mut()
                .expect("storage")
                .settle(spec, max, flow.charge, flow.discharge);
        }
    }
    pub(crate) fn close_batteries(&mut self) -> Vec<DomainEvent> {
        self.plants
            .values_mut()
            .filter(|p| is_commissioned(p))
            .filter_map(|p| {
                let id = p.id();
                let b = p.battery_mut()?;
                Some(DomainEvent::BatteryDayClosed {
                    plant: id,
                    unused: b.close_day(),
                })
            })
            .collect()
    }
}

impl PowerSystem {
    /// Expected-day supply, using exactly the shortfall evaluator's finite inventory.
    /// Empty when no connected, commissioned battery is enabled.
    pub(crate) fn battery_supply_forecast(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
    ) -> Vec<SupplyForecastPoint> {
        if !self.has_batteries() {
            return Vec::new();
        }
        let inputs = BatteryPlanningInputs::new(self, region, month);
        let mut plan = self.charge_plan_with_inventory(&inputs, 0, true, None);
        self.project_storage_supply_with_inputs(&inputs, 0, TICKS_PER_DAY, true, &mut plan, true)
    }

    /// One sequential inventory per projected horizon. Future receipts are allowed only in Planning.
    pub(super) fn storage_forecast_minutes(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        start: u32,
        end: u32,
        preview: bool,
    ) -> Vec<super::outage::HourEnd> {
        let inputs = BatteryPlanningInputs::new(self, region, month);
        let mut plan = if preview {
            self.charge_plan_with_inventory(&inputs, start, true, None)
        } else {
            self.battery_plan.clone()
        };
        self.project_storage_supply_with_inputs(&inputs, start, end, preview, &mut plan, true)
            .into_iter()
            .map(|point| super::outage::HourEnd {
                demand: point.demand,
                solar: point.generation[&PlantKind::Solar] + point.generation[&PlantKind::Wind],
                deliverable: point.gross_supply,
            })
            .collect()
    }

    pub(super) fn project_storage_supply(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        start: u32,
        end: u32,
        preview: bool,
        plans: &mut BatteryScheduleView,
    ) -> Vec<SupplyForecastPoint> {
        let inputs = BatteryPlanningInputs::new(self, region, month);
        self.project_storage_supply_with_inputs(&inputs, start, end, preview, plans, true)
    }

    pub(super) fn project_storage_supply_with_inputs(
        &self,
        inputs: &BatteryPlanningInputs<'_>,
        start: u32,
        end: u32,
        preview: bool,
        plans: &mut BatteryScheduleView,
        collect_forecast: bool,
    ) -> Vec<SupplyForecastPoint> {
        let region = inputs.region;
        let month = inputs.month;
        let hydro = if preview {
            self.preview_hydro(region, month)
        } else {
            // Live projections use the applied hydro ceilings through `deliverable` below.
            BTreeMap::new()
        };
        let mut inventory: BTreeMap<_, _> = inputs
            .batteries
            .iter()
            .copied()
            .filter_map(|p| {
                p.battery().map(|b| {
                    let mut state = b.clone();
                    if preview {
                        state.close_day();
                    }
                    (p.id(), state)
                })
            })
            .collect();
        let mut water: BTreeMap<_, _> = inputs
            .dispatchable
            .iter()
            .copied()
            .filter(|p| p.kind() == PlantKind::Hydro)
            .map(|p| {
                (
                    p.id(),
                    if preview {
                        super::day_hydro_budget(region, month, p)
                    } else {
                        p.hydro_budget().unwrap_or(MegawattHours::ZERO)
                    },
                )
            })
            .collect();
        let mut minutes = Vec::new();
        let mut last_surplus = None;
        for tick in start..end.min(TICKS_PER_DAY) {
            let hour = (tick / 60) as u8;
            let point = inputs.minute(tick);
            let (solar, demand, wind) = (point.solar, point.demand, point.wind);
            let mut primary = solar + wind;
            // Schedule-only callers need the flows, not a map and point for every minute.
            let mut generation = collect_forecast
                .then(|| BTreeMap::from([(PlantKind::Solar, solar), (PlantKind::Wind, wind)]));
            for p in inputs.dispatchable.iter().copied() {
                let mut output = if preview {
                    if !p.commitment().is_committed(hour) {
                        Megawatts::ZERO
                    } else if p.kind() == PlantKind::Hydro && p.auto_follow() {
                        hydro
                            .get(&p.id())
                            .map_or(Megawatts::ZERO, |f| f.hourly[usize::from(hour)])
                    } else if p.auto_follow() {
                        p.spec().capacity
                    } else {
                        p.initial_target()
                    }
                } else {
                    self.deliverable(p, start, tick - start)
                };
                if let Some(left) = water.get_mut(&p.id()) {
                    output = output.min(Megawatts::new(left.value().max(0.0) * 60.0));
                    *left = MegawattHours::new((left.value() - output.value() / 60.0).max(0.0));
                }
                primary += output;
                if let Some(generation) = &mut generation {
                    *generation.entry(p.kind()).or_insert(Megawatts::ZERO) += output;
                }
            }
            let surplus = primary >= demand;
            if preview && (last_surplus != Some(surplus) || tick.is_multiple_of(TICKS_PER_HOUR)) {
                let next = self.charge_plan_with_inventory(inputs, tick, true, Some(&inventory));
                for (id, future) in next.plants {
                    if let Some(existing) = plans.plants.get_mut(&id) {
                        existing.charge[tick as usize..]
                            .copy_from_slice(&future.charge[tick as usize..]);
                    }
                }
            }
            last_surplus = Some(surplus);
            let mut remaining = (demand - primary).max(Megawatts::ZERO);
            let mut charging_room = (primary - demand).max(Megawatts::ZERO);
            let mut storage = Megawatts::ZERO;
            let mut charging = Megawatts::ZERO;
            for p in &inputs.batteries {
                let (Some(spec), Some(state)) = (p.spec().battery, inventory.get_mut(&p.id()))
                else {
                    continue;
                };
                let charge = if preview && charging_window(region, month, spec, tick) {
                    plans
                        .plants
                        .get(&p.id())
                        .map_or(Megawatts::ZERO, |p| p.charge[tick as usize])
                        .min(charging_room)
                } else {
                    Megawatts::ZERO
                };
                let discharge = if discharging_window(spec, tick) {
                    remaining
                } else {
                    Megawatts::ZERO
                };
                let flow = state.settle(spec, p.spec().capacity, charge, discharge);
                if let Some(plan) = plans.plants.get_mut(&p.id()) {
                    plan.discharge[tick as usize] = flow.discharge;
                }
                remaining = (remaining - flow.discharge).max(Megawatts::ZERO);
                storage += flow.discharge;
                charging += flow.charge;
                charging_room = (charging_room - flow.charge).max(Megawatts::ZERO);
            }
            if let Some(generation) = generation {
                minutes.push(SupplyForecastPoint {
                    demand,
                    generation,
                    gross_supply: primary + storage,
                    battery_charge: charging,
                    battery_discharge: storage,
                });
            }
        }
        minutes
    }
}

impl PowerSystem {
    pub(super) fn battery_headroom(&self, except: Option<PlantId>) -> Megawatts {
        if self.protection.mode != GridMode::Normal {
            return Megawatts::ZERO;
        }
        self.connected_plants()
            .filter(|p| Some(p.id()) != except)
            .filter_map(|p| {
                let (spec, state) = (p.spec().battery?, p.battery()?);
                if !state.enabled() || !discharging_window(spec, self.battery_tick + 1) {
                    return None;
                }
                // A trip/restoration cannot spend a one-minute burst as sustained cover.
                // Keep support feasible through the next hourly sizing horizon.
                let minutes = (TICKS_PER_DAY - self.battery_tick - 1).clamp(1, TICKS_PER_HOUR);
                let next = state.flow().stored.value() * spec.discharge_efficiency.value() * 60.0
                    / f64::from(minutes);
                Some(
                    (p.spec().capacity.min(Megawatts::new(next)) - p.output()).max(Megawatts::ZERO),
                )
            })
            .sum()
    }
    pub(super) fn battery_fall_ahead(&self) -> bool {
        self.connected_plants()
            .filter(|p| p.output() > Megawatts::ZERO)
            .any(|p| {
                let (Some(spec), Some(b), Some(plan)) = (
                    p.spec().battery,
                    p.battery(),
                    self.battery_plan.plants.get(&p.id()),
                ) else {
                    return false;
                };
                let end = ((self.battery_tick / 60 + 1) * 60).min(TICKS_PER_DAY - 1);
                let smallest = plan.discharge
                    [(self.battery_tick as usize + 1).min(1439)..=end as usize]
                    .iter()
                    .copied()
                    .fold(p.spec().capacity, Megawatts::min);
                let remaining = b.flow().stored.value() * spec.discharge_efficiency.value();
                p.output() > smallest
                    || remaining * 60.0
                        < p.output().value() * f64::from(end - self.battery_tick + 1)
            })
    }
}

impl PowerSystem {
    pub(crate) fn storage_charging_requirement(&self, from: u32, tick: u32) -> Megawatts {
        if tick < from || tick >= TICKS_PER_DAY || self.protection.mode != GridMode::Normal {
            return Megawatts::ZERO;
        }
        let mut net = Megawatts::ZERO;
        for p in self.connected_plants() {
            let (Some(b), Some(plan)) = (p.battery(), self.battery_plan.plants.get(&p.id())) else {
                continue;
            };
            if !b.enabled() {
                continue;
            }
            net += plan.charge[tick as usize];
        }
        net
    }
}

// Portable state schema 1: explicit field order and enum tags.
crate::state_record!(BatteryScheduleView {
    plants,
    expected_charging
});
crate::state_record!(PlantBatteryScheduleView { charge, discharge });
