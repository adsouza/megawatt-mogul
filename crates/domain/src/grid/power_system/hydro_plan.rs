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
use crate::time::{SimMonth, TICKS_PER_HOUR};
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
}

/// The hydro schedule as the player sees it (spec §12.6).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HydroScheduleView {
    /// Every scheduled plant, in id order.
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

    /// The allocation for `hour` of every planned plant on AGC: the AGC
    /// ceilings. A hydro plant with no plan (switched to AGC or connected since
    /// the last re-plan) gets none, so it is uncapped.
    pub(super) fn hydro_ceilings(&self, hour: usize) -> BTreeMap<PlantId, Megawatts> {
        self.hydro_plan
            .iter()
            .filter(|(id, _)| self.plants.get(id).is_some_and(Plant::auto_follow))
            .filter_map(|(id, plan)| plan.hourly.get(hour).map(|mw| (*id, *mw)))
            .collect()
    }

    /// The current hydro schedule of every scheduled plant.
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
