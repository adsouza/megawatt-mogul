//! The hydro peak-shaving schedule (spec §12.3–§12.6).
//!
//! Each scheduled hydro plant's remaining water is poured onto the day's
//! highest forecast net-load hours ([`water_fill`]), at `BeginOperating` and
//! again at every hour boundary over the hours left. Each hour's allocation
//! then caps what AGC asks of the plant in that hour (see
//! [`PowerSystem::retarget_agc`]). Planning is a pure function of state: it
//! draws no random numbers.

use std::collections::BTreeMap;

use super::PowerSystem;
use crate::generation::{Plant, water_fill};
use crate::grid::frequency::FrequencyBand;
use crate::ids::PlantId;
use crate::region::{PlantKind, RegionDefinition};
use crate::time::{SimMonth, TICKS_PER_DAY, TICKS_PER_HOUR};
use crate::units::{MegawattHours, Megawatts, Ratio};

const HOURS: usize = 24;

/// The forecast for hour `h` is taken at minute 30, the hour's mean.
const MID_HOUR: u32 = TICKS_PER_HOUR / 2;

/// One scheduled plant's current plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlantHydroPlan {
    /// The water level `λ` of the latest plan (see [`crate::generation::HydroFill`]).
    pub(crate) level: Option<Megawatts>,
    /// The latest plan's allocation for each hour; 0 before the hour it was
    /// planned from.
    pub(crate) hourly: [Megawatts; HOURS],
    /// `frozen[h]` is hour `h`'s allocation as it stood when hour `h` began.
    pub(crate) frozen: [Option<Megawatts>; HOURS],
    /// False once the plant has tripped or had AGC turned off: it is then
    /// unplanned (uncapped) until the next re-plan, with no current or future
    /// allocation, while `frozen` keeps the elapsed hours' record.
    pub(crate) planned: bool,
}

/// Where the emergency override of the hydro cap stands (spec §12.4), as of
/// the last AGC pass (see [`PowerSystem::retarget_agc`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum HydroOverride {
    /// The cap applies as planned.
    #[default]
    Off,
    /// The cap is lifted (see [`PowerSystem::hydro_cap_lifted`]): only the
    /// budget limits scheduled hydro.
    Lifted,
    /// The lift has ended and scheduled hydro is still above the lowest
    /// ceiling it meets before the next re-plan: it ramps back down no faster
    /// than the rest of the fleet can follow.
    Recovering,
}

/// The hydro schedule as the player sees it (spec §12.6), valid per phase:
/// the day's plan while Operating, the finished day (every hour frozen) during
/// Review, and empty during Planning, until `BeginOperating` plans the day.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HydroScheduleView {
    /// Every plant planned today, in id order. A plant that has since tripped
    /// or been taken off AGC keeps its entry, with zero allocations from that
    /// hour on (and no level), until a re-plan no longer schedules it.
    pub plants: BTreeMap<PlantId, PlantHydroScheduleView>,
}

/// One scheduled plant's schedule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlantHydroScheduleView {
    /// The water level `λ`: `None` when nothing is allocated or the water spills.
    pub level: Option<Megawatts>,
    /// Elapsed hours show the allocation as it stood when each began, so a
    /// later re-plan never rewrites them; the current and later hours show the
    /// latest plan.
    pub hourly: [Megawatts; HOURS],
    pub budget_remaining: MegawattHours,
}

/// Whether the schedule plans `plant`'s water: hydro on AGC with water left.
/// The caller checks that it is connected and commissioned.
fn is_scheduled(plant: &Plant) -> bool {
    plant.kind() == PlantKind::Hydro
        && plant.auto_follow()
        && plant
            .hydro_budget()
            .is_some_and(|budget| budget > MegawattHours::ZERO)
}

impl PowerSystem {
    /// Expected (noise-free) demand of connected cities less expected solar at
    /// minute 30 of each hour of a day in `month`: the forecast net load the
    /// dispatchable fleet has to follow, hour by hour.
    pub(crate) fn hourly_net_load(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
    ) -> [Megawatts; HOURS] {
        std::array::from_fn(|h| {
            let tick = h as u32 * TICKS_PER_HOUR + MID_HOUR;
            self.connected_demand_forecast(region, month, tick)
                - self.expected_solar(region, month, tick)
        })
    }

    /// Re-plans every scheduled hydro plant over hours `from_hour..24` with
    /// its remaining budget (spec §12.3), and freezes each plan's allocation
    /// for `from_hour`, the hour now beginning.
    ///
    /// Plants are filled in id order against the residual: the forecast net
    /// load, zero before `from_hour`, less each earlier plant's allocation. A
    /// plant is eligible only in the hours its commitment pattern commits it.
    /// Plants no longer scheduled lose their plan; manual hydro is neither
    /// planned nor subtracted. Planning from hour 0 starts a new day's frozen
    /// record. Draws no random numbers.
    pub(super) fn plan_hydro(&mut self, region: &RegionDefinition, month: SimMonth, from_hour: u8) {
        let from = usize::from(from_hour);
        let mut residual = self.hourly_net_load(region, month);
        residual
            .iter_mut()
            .take(from)
            .for_each(|mw| *mw = Megawatts::ZERO);

        let mut plans = BTreeMap::new();
        for plant in self.connected_plants().filter(|p| is_scheduled(p)) {
            let commitment = plant.commitment();
            let eligible: [bool; HOURS] =
                std::array::from_fn(|h| h >= from && commitment.is_committed(h as u8));
            let budget = plant.hydro_budget().unwrap_or(MegawattHours::ZERO);
            let fill = water_fill(&residual, &eligible, plant.spec().capacity, budget);
            for (left, used) in residual.iter_mut().zip(fill.hourly) {
                *left -= used;
            }
            let mut frozen = match self.hydro_plan.get(&plant.id()) {
                Some(previous) if from > 0 => previous.frozen,
                _ => [None; HOURS],
            };
            if let Some(slot) = frozen.get_mut(from) {
                *slot = Some(fill.hourly[from]);
            }
            plans.insert(
                plant.id(),
                PlantHydroPlan {
                    level: fill.level,
                    hourly: fill.hourly,
                    frozen,
                    planned: true,
                },
            );
        }
        self.hydro_plan = plans;
        self.hydro_planned_from = from_hour;
    }

    /// Whether the emergency override lifts every hydro cap (spec §12.4):
    /// while any load is shed, in either grid mode, or the frequency band is
    /// `Shedding`. Keying on the shed as well keeps the cap off for the whole
    /// restoration instead of flapping as the band recovers.
    pub(super) fn hydro_cap_lifted(&self) -> bool {
        self.protection.shed > Ratio::ZERO || self.band == FrequencyBand::Shedding
    }

    /// The override state for the AGC pass setting targets that act on
    /// `effect_tick`, given the last pass's state: `Lifted` whenever the cap
    /// is lifted, including during a recovery. Otherwise a lift or recovery
    /// continues as `Recovering` while any connected online plant is above
    /// the lowest ceiling it meets before the next re-plan (see
    /// [`PowerSystem::foreseen_ceilings`]), and ends as `Off`.
    pub(super) fn next_hydro_override(&self, effect_tick: u32) -> HydroOverride {
        if self.hydro_cap_lifted() {
            return HydroOverride::Lifted;
        }
        if self.hydro_override == HydroOverride::Off {
            return HydroOverride::Off;
        }
        let foreseen = self.foreseen_ceilings(effect_tick);
        let above = self
            .connected_plants()
            .any(|p| p.is_online() && foreseen.get(&p.id()).is_some_and(|c| p.output() > *c));
        if above {
            HydroOverride::Recovering
        } else {
            HydroOverride::Off
        }
    }

    /// Makes a plant that tripped or had AGC turned off unplanned until the
    /// next re-plan (spec §12.3 step 4): its current and future allocations
    /// and its level are cleared, so it is budget-limited only, while the
    /// frozen record of elapsed hours stays visible. The next hourly re-plan
    /// treats it by the normal criteria.
    pub(super) fn unplan_hydro(&mut self, id: PlantId) {
        if let Some(plan) = self.hydro_plan.get_mut(&id) {
            plan.level = None;
            plan.hourly = [Megawatts::ZERO; HOURS];
            plan.planned = false;
        }
    }

    /// The AGC ceiling of every planned plant on AGC for targets that first
    /// act on tick `effect_tick` (clamped into the day): its allocation for
    /// that tick's hour `h`, lowered to the ramp envelope `hourly[h + 1] +
    /// ramp × k`, where `k` counts the ticks from `effect_tick` to the next
    /// hour boundary, so it can reach the next hour's allocation by then (see
    /// [`PowerSystem::retarget_agc`]). Hour 23 has no envelope. A hydro plant
    /// with no plan (switched to AGC, connected, tripped or with AGC turned
    /// off since the last re-plan) gets no ceiling, so it is uncapped.
    pub(super) fn hydro_ceilings(&self, effect_tick: u32) -> BTreeMap<PlantId, Megawatts> {
        let tick = effect_tick.min(TICKS_PER_DAY - 1);
        let hour = (tick / TICKS_PER_HOUR) as usize;
        let to_boundary = (tick / TICKS_PER_HOUR + 1) * TICKS_PER_HOUR - tick;
        self.hydro_plan
            .iter()
            .filter(|(_, plan)| plan.planned)
            .filter_map(|(id, plan)| {
                let plant = self.plants.get(id).filter(|p| p.auto_follow())?;
                let mut ceiling = plan.hourly[hour];
                if let Some(next) = plan.hourly.get(hour + 1) {
                    ceiling = ceiling.min(*next + plant.spec().ramp.over_ticks(to_boundary));
                }
                Some((*id, ceiling))
            })
            .collect()
    }

    /// Ends the day's schedule at month close: every hour has now elapsed, so
    /// [`PowerSystem::hydro_schedule`] shows each hour's allocation as it
    /// stood when the hour began, for plan-versus-actual review.
    pub(crate) fn finish_hydro_day(&mut self) {
        self.hydro_planned_from = HOURS as u8;
    }

    /// Clears the schedule when the next Planning phase begins: nothing is
    /// planned until `begin_day` plans the new day.
    pub(crate) fn clear_hydro_plan(&mut self) {
        self.hydro_plan.clear();
        self.hydro_planned_from = 0;
    }

    /// The hydro schedule of every plant planned today, valid per phase:
    /// while Operating, elapsed hours show their frozen allocation and the
    /// current and later hours the latest plan; during Review, the finished
    /// day with every hour frozen (see `finish_hydro_day`); during Planning,
    /// nothing (see `clear_hydro_plan`).
    pub fn hydro_schedule(&self) -> HydroScheduleView {
        let current = usize::from(self.hydro_planned_from);
        let plants = self
            .hydro_plan
            .iter()
            .map(|(id, plan)| {
                let hourly = std::array::from_fn(|h| {
                    if h < current {
                        plan.frozen[h].unwrap_or(Megawatts::ZERO)
                    } else {
                        plan.hourly[h]
                    }
                });
                let budget_remaining = self
                    .plants
                    .get(id)
                    .and_then(Plant::hydro_budget)
                    .unwrap_or(MegawattHours::ZERO);
                let view = PlantHydroScheduleView {
                    level: plan.level,
                    hourly,
                    budget_remaining,
                };
                (*id, view)
            })
            .collect();
        HydroScheduleView { plants }
    }
}
