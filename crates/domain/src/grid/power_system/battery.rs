//! Coordinated storage plans and current-tick flow proposals. Inventory settles after protection.
use super::{PowerSystem, is_commissioned};
use crate::events::DomainEvent;
use crate::generation::{BatteryFlow, Plant, PlantStatus, fill_minutes};
use crate::grid::GridMode;
use crate::ids::PlantId;
use crate::region::{BatterySpec, PlantKind, RegionDefinition};
use crate::time::{SimMonth, TICKS_PER_DAY, TICKS_PER_HOUR};
use crate::units::{MegawattHours, Megawatts, Ratio};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct PlantBatteryScheduleView {
    pub charge: Vec<Megawatts>,
    pub discharge: Vec<Megawatts>,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BatteryScheduleView {
    pub plants: BTreeMap<PlantId, PlantBatteryScheduleView>,
    pub expected_charging: bool,
}

pub(super) fn charging_window(
    region: &RegionDefinition,
    month: SimMonth,
    spec: BatterySpec,
    tick: u32,
) -> bool {
    let hour = f64::from(tick) / f64::from(TICKS_PER_HOUR);
    let (rise, set) = region.solar().daylight[month.index0()];
    rise <= hour && hour < set.min(f64::from(spec.discharge_start_hour))
}
fn discharging_window(spec: BatterySpec, tick: u32) -> bool {
    tick >= u32::from(spec.discharge_start_hour) * TICKS_PER_HOUR && tick < TICKS_PER_DAY
}

impl PowerSystem {
    pub(crate) fn invalidate_battery_plan(&mut self) {
        self.battery_plan_dirty = true;
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
    fn thermal_for_charge(&self, tick: u32, from: u32, preview: bool) -> Megawatts {
        self.connected_plants()
            .filter(|p| p.kind().is_dispatchable() && p.kind() != PlantKind::Hydro)
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
        let n = TICKS_PER_DAY as usize;
        let mut charge_residual: Vec<_> = (0..TICKS_PER_DAY)
            .map(|t| {
                self.connected_demand_forecast(region, month, t)
                    - self.expected_solar(region, month, t)
            })
            .collect();
        let mut discharge_residual = charge_residual.clone();
        let mut headroom: Vec<_> = (0..TICKS_PER_DAY)
            .map(|t| {
                (self.thermal_for_charge(t, from, preview) - charge_residual[t as usize])
                    .max(Megawatts::ZERO)
            })
            .collect();
        let mut result = BatteryScheduleView {
            expected_charging: preview,
            ..Default::default()
        };
        for p in self.connected_plants() {
            let (Some(spec), Some(state)) = (p.spec().battery, p.battery()) else {
                continue;
            };
            let enabled = state.enabled();
            let stored = if preview { 0.0 } else { state.stored().value() };
            let charge_caps: Vec<_> = (0..TICKS_PER_DAY)
                .map(|t| {
                    if enabled && t >= from && charging_window(region, month, spec, t) {
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
            let expected_input = charge.iter().map(|p| p.value()).sum::<f64>() / 60.0;
            let energy = stored
                + if preview {
                    expected_input * spec.charge_efficiency.value()
                } else {
                    0.0
                };
            let caps: Vec<_> = (0..TICKS_PER_DAY)
                .map(|t| {
                    if enabled && t >= from && discharging_window(spec, t) {
                        p.spec()
                            .capacity
                            .min(discharge_residual[t as usize].max(Megawatts::ZERO))
                    } else {
                        Megawatts::ZERO
                    }
                })
                .collect();
            let discharge = fill_minutes(
                &discharge_residual,
                &caps,
                MegawattHours::new(energy * spec.discharge_efficiency.value()),
                false,
            );
            for t in 0..n {
                charge_residual[t] += charge[t];
                headroom[t] = (headroom[t] - charge[t]).max(Megawatts::ZERO);
                discharge_residual[t] -= discharge[t];
            }
            result
                .plants
                .insert(p.id(), PlantBatteryScheduleView { charge, discharge });
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
    pub(super) fn battery_request(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
        flows: &BTreeMap<PlantId, BatteryFlow>,
        customer_load: Megawatts,
    ) -> Megawatts {
        let mut net = Megawatts::ZERO;
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
                let end = region.solar().daylight[month.index0()]
                    .1
                    .min(f64::from(spec.discharge_start_hour))
                    * 60.0;
                let input_room = (spec.energy_capacity.value() - stored).max(0.0)
                    / spec.charge_efficiency.value();
                let request = Self::taper(
                    plan.charge[tick as usize].value(),
                    input_room,
                    self.battery_ramp_share(),
                    end - f64::from(tick) - 1.0,
                );
                let request = Megawatts::new(request).min(charge_budget);
                net += request;
                charge_budget -= request;
            } else if discharging_window(spec, tick) {
                let request = Self::taper(
                    plan.discharge[tick as usize].value(),
                    stored * spec.discharge_efficiency.value(),
                    self.battery_ramp_share(),
                    f64::from(TICKS_PER_DAY - tick - 1),
                );
                net -= Megawatts::new(request);
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
        let mut flows = BTreeMap::new();
        for p in self.plants.values().filter(|p| is_commissioned(p)) {
            let (Some(spec), Some(state)) = (p.spec().battery, p.battery()) else {
                continue;
            };
            let connected = self.on_grid(p.node());
            let mut charge = Megawatts::ZERO;
            let mut discharge = Megawatts::ZERO;
            if connected && active && state.enabled() {
                if charging_window(region, month, spec, tick) && charge_ok {
                    // Room for the generating fleet's ramp-down tail takes priority over extra absorption.
                    let end = region.solar().daylight[month.index0()]
                        .1
                        .min(f64::from(spec.discharge_start_hour))
                        * 60.0;
                    let safe = Self::taper(
                        spec.charge_power.value(),
                        (spec.energy_capacity.value() - state.stored().value())
                            / spec.charge_efficiency.value(),
                        self.battery_ramp_share(),
                        end - f64::from(tick),
                    );
                    // Already available solar surplus needs no thermal ramp tail.
                    let solar: Megawatts = self
                        .connected_plants()
                        .filter(|p| p.kind() == PlantKind::Solar)
                        .map(Plant::output)
                        .sum();
                    let solar_surplus = (solar - demand).max(Megawatts::ZERO);
                    charge = surplus.min(Megawatts::new(safe).max(solar_surplus));
                } else if discharging_window(spec, tick) {
                    // Real deficits (including noise/emergencies) may exceed the planned allowance.
                    discharge = deficit;
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
    pub(super) fn battery_hourly_discharge(
        &self,
        plans: &BatteryScheduleView,
        hour: usize,
    ) -> Megawatts {
        plans
            .plants
            .values()
            .map(|p| {
                p.discharge[hour * 60..(hour + 1) * 60]
                    .iter()
                    .copied()
                    .sum::<Megawatts>()
                    * (1.0 / 60.0)
            })
            .sum()
    }
}

impl PowerSystem {
    /// One sequential inventory per projected horizon. Future receipts are allowed only in Planning.
    pub(super) fn storage_forecast_minutes(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        start: u32,
        end: u32,
        preview: bool,
    ) -> Vec<super::outage::HourEnd> {
        let plans = if preview {
            self.preview_batteries(region, month)
        } else {
            self.battery_plan.clone()
        };
        let hydro = self.compute_hydro_plan_with_storage(
            region,
            month,
            (start / 60) as u8,
            |p| {
                if preview {
                    super::day_hydro_budget(region, month, p)
                } else {
                    p.hydro_budget().unwrap_or(MegawattHours::ZERO)
                }
            },
            &plans,
        );
        let mut inventory: BTreeMap<_, _> = self
            .connected_plants()
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
        let mut water: BTreeMap<_, _> = self
            .connected_plants()
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
        for tick in start..end.min(TICKS_PER_DAY) {
            let hour = (tick / 60) as u8;
            let solar = self.expected_solar(region, month, tick);
            let demand = self.connected_demand_forecast(region, month, tick);
            let mut primary = solar;
            for p in self
                .connected_plants()
                .filter(|p| p.kind().is_dispatchable())
            {
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
            }
            let mut remaining = (demand - primary).max(Megawatts::ZERO);
            let mut charging_room = (primary - demand).max(Megawatts::ZERO);
            let mut storage = Megawatts::ZERO;
            for p in self.connected_plants() {
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
                remaining = (remaining - flow.discharge).max(Megawatts::ZERO);
                storage += flow.discharge;
                charging_room = (charging_room - flow.charge).max(Megawatts::ZERO);
            }
            minutes.push(super::outage::HourEnd {
                demand,
                solar,
                deliverable: primary + storage,
            });
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
    pub(crate) fn storage_net_adjustment(&self, from: u32, tick: u32) -> Megawatts {
        if tick < from || tick >= TICKS_PER_DAY || self.protection.mode != GridMode::Normal {
            return Megawatts::ZERO;
        }
        let mut net = Megawatts::ZERO;
        for p in self.connected_plants() {
            let (Some(spec), Some(b), Some(plan)) = (
                p.spec().battery,
                p.battery(),
                self.battery_plan.plants.get(&p.id()),
            ) else {
                continue;
            };
            if !b.enabled() {
                continue;
            }
            let mut inventory = b.clone();
            for t in from..=tick {
                let d = if discharging_window(spec, t) {
                    plan.discharge[t as usize]
                } else {
                    Megawatts::ZERO
                };
                // No future charging receipt is spent by a live forecast.
                let f = inventory.settle(spec, p.spec().capacity, Megawatts::ZERO, d);
                if t == tick {
                    net += plan.charge[t as usize] - f.discharge;
                }
            }
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
