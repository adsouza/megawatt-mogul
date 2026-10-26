//! The hydro peak-shaving schedule (spec §12.3–§12.6).
//!
//! Each scheduled hydro plant's remaining water is poured onto the day's
//! highest forecast net-load hours ([`water_fill`]), at `BeginOperating` and
//! again at every hour boundary over the hours left. Each hour's allocation
//! then caps what AGC asks of the plant in that hour (see
//! [`PowerSystem::retarget_agc`]). Planning is a pure function of state: it
//! draws no random numbers.

use std::collections::BTreeMap;

use super::{PowerSystem, day_hydro_budget};
use crate::generation::{HydroFill, Plant, PlantStatus, water_fill};
use crate::grid::frequency::FrequencyBand;
use crate::ids::PlantId;
use crate::region::{PlantKind, RegionDefinition};
use crate::time::{SimMonth, TICKS_PER_DAY, TICKS_PER_HOUR};
use crate::units::{MegawattHours, Megawatts, Ratio};

const HOURS: usize = 24;

/// The forecast for hour `h` is taken at minute 30, the hour's mean.
const MID_HOUR: u32 = TICKS_PER_HOUR / 2;

/// The tick at minute 30 of hour `hour`, where each hour's forecast is taken.
pub(super) fn mid_hour_tick(hour: u8) -> u32 {
    u32::from(hour) * TICKS_PER_HOUR + MID_HOUR
}

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

impl PlantHydroPlan {
    pub(super) fn unplan(&mut self) {
        self.level = None;
        self.hourly = [Megawatts::ZERO; HOURS];
        self.planned = false;
    }
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

impl PowerSystem {
    /// The coming day's hour-0 water allocation, shared by Planning charts
    /// and the shortfall check. Uses fresh daily budgets, never the live
    /// hydro plan or leftover water. Batteries do not displace scheduled hydro.
    pub(crate) fn preview_hydro(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
    ) -> BTreeMap<PlantId, HydroFill> {
        self.compute_hydro_plan(region, month, 0, |p| day_hydro_budget(region, month, p))
    }

    /// Expected (noise-free) demand of connected cities less expected solar at
    /// minute 30 of each hour of a day in `month`: the forecast net load the
    /// dispatchable fleet has to follow, hour by hour.
    pub(crate) fn hourly_net_load(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
    ) -> [Megawatts; HOURS] {
        std::array::from_fn(|h| {
            let tick = mid_hour_tick(h as u8);
            self.connected_demand_forecast(region, month, tick)
                - self.expected_variable(region, month, tick)
                - Megawatts::new(self.injections[h])
        })
    }

    /// Re-plans every scheduled hydro plant over hours `from_hour..24` with
    /// its remaining budget (spec §12.3), and freezes each plan's allocation
    /// for `from_hour`, the hour now beginning.
    ///
    /// Plants are filled in id order against the residual: the forecast net
    /// load, zero before `from_hour`, less each earlier plant's allocation. A
    /// plant is eligible only in the hours its commitment pattern commits it.
    /// A stopped or tripped plant whose commitment is suppressed gets no
    /// allocation until manually restarted; starting and online plants can
    /// participate even while their automatic commitment remains suppressed.
    /// Plants no longer scheduled lose their plan; manual hydro is neither
    /// planned nor subtracted. Planning from hour 0 starts a new day's frozen
    /// record. Draws no random numbers.
    pub(super) fn plan_hydro(&mut self, region: &RegionDefinition, month: SimMonth, from_hour: u8) {
        let from = usize::from(from_hour);
        let fills = self.compute_hydro_plan(region, month, from_hour, |p| {
            // Today's override cannot fulfil the base pattern. Keep this
            // check out of the shared water-fill: Planning previews next
            // month's pattern, and unsuppressed offline plants can still
            // have a scheduled start ahead of them today.
            if p.is_commitment_suppressed()
                && !matches!(
                    p.status(),
                    PlantStatus::Starting { .. } | PlantStatus::Online { .. }
                )
            {
                MegawattHours::ZERO
            } else {
                p.hydro_budget().unwrap_or(MegawattHours::ZERO)
            }
        });
        let plans = fills
            .into_iter()
            .map(|(id, fill)| {
                let mut frozen = match self.hydro_plan.get(&id) {
                    Some(previous) if from > 0 => previous.frozen,
                    _ => [None; HOURS],
                };
                if let Some(slot) = frozen.get_mut(from) {
                    *slot = Some(fill.hourly[from]);
                }
                let plan = PlantHydroPlan {
                    level: fill.level,
                    hourly: fill.hourly,
                    frozen,
                    planned: true,
                };
                (id, plan)
            })
            .collect();
        self.hydro_plan = plans;
        self.hydro_planned_from = from_hour;
    }

    /// The pure water-filling behind [`PowerSystem::plan_hydro`] (spec §12.3,
    /// §15.5): each scheduled hydro plant's fill over hours `from_hour..24`,
    /// in id order against the residual (the forecast net load, zero before
    /// `from_hour`, less each earlier plant's allocation). `budget` gives the
    /// water a plant has to place; a plant is scheduled when it is connected
    /// and commissioned hydro on AGC with a positive budget. Reads no plan
    /// state and draws no random numbers, so a Planning-time preview can call
    /// it with the coming day's budgets (see [`super::day_hydro_budget`]).
    pub(crate) fn compute_hydro_plan(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        from_hour: u8,
        budget: impl Fn(&Plant) -> MegawattHours,
    ) -> BTreeMap<PlantId, HydroFill> {
        let from = usize::from(from_hour);
        let mut residual = self.hourly_net_load(region, month);
        residual
            .iter_mut()
            .take(from)
            .for_each(|mw| *mw = Megawatts::ZERO);

        let mut fills = BTreeMap::new();
        for plant in self.connected_plants() {
            let water = budget(plant);
            if plant.kind() != PlantKind::Hydro
                || !plant.auto_follow()
                || water <= MegawattHours::ZERO
            {
                continue;
            }
            let commitment = plant.commitment();
            let eligible: [bool; HOURS] =
                std::array::from_fn(|h| h >= from && commitment.is_committed(h as u8));
            let fill = water_fill(&residual, &eligible, plant.spec().capacity, water);
            for (left, used) in residual.iter_mut().zip(fill.hourly) {
                *left -= used;
            }
            fills.insert(plant.id(), fill);
        }
        fills
    }

    /// Whether the emergency override lifts every hydro cap (spec §12.4):
    /// while any load is shed, in either grid mode, or the frequency band is
    /// `Shedding`. Keying on the shed as well keeps the cap off for the whole
    /// restoration instead of flapping as the band recovers. A rotating outage
    /// alone does not lift it (spec §15.3): the plan already spends water on
    /// the shortage hours and outage sizing counts that allocation, so lifting
    /// the cap would spend the water early and deepen later hours' outages.
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
        self.unplan_island_hydro(id);
        if let Some(plan) = self.hydro_plan.get_mut(&id) {
            plan.unplan();
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
        self.finish_island_hydro(false);
        self.hydro_planned_from = HOURS as u8;
    }

    /// Clears the schedule when the next Planning phase begins: nothing is
    /// planned until `begin_day` plans the new day.
    pub(crate) fn clear_hydro_plan(&mut self) {
        self.finish_island_hydro(true);
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

// Portable state schema 1: explicit field order and enum tags.
crate::state_record!(PlantHydroPlan {
    level,
    hourly,
    frozen,
    planned
});
impl crate::state::State for HydroOverride {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Off => {
                crate::state::State::write(&0u8, out);
            }
            Self::Lifted => {
                crate::state::State::write(&1u8, out);
            }
            Self::Recovering => {
                crate::state::State::write(&2u8, out);
            }
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(match <u8 as crate::state::State>::read(input)? {
            0 => Self::Off,
            1 => Self::Lifted,
            2 => Self::Recovering,
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}
