//! Rotating outages (spec §15.3): a shortfall the forecast can see is shed on
//! purpose, ahead of protection.
//!
//! At `BeginOperating` and at every hour boundary in Normal mode, right after
//! the hydro re-plan, the hour starting is sized ([`PowerSystem::size_outage`])
//! and its share becomes the `outage_target`. The tick's load is then
//! `demand × (1 − outage)`, and the outage moves toward its target by at most
//! one `shed_step` per tick ([`PowerSystem::next_outage`]). Sizing is a pure
//! function of state: it draws no random numbers.

use super::PowerSystem;
use crate::events::DomainEvent;
use crate::generation::{CommitmentAction, Plant, PlantStatus, commitment_actions};
use crate::region::{PlantKind, RegionDefinition};
use crate::time::{SimMonth, TICKS_PER_HOUR};
use crate::units::{Megawatts, Ratio};

/// Shares within this many steps of a step multiple round to it, so that an
/// exact multiple reached through float arithmetic gains no step; likewise an
/// outage within this of its target counts as there.
const STEP_TOLERANCE: f64 = 1e-9;

/// The last minute of an hour, relative to its start.
const LAST_MINUTE: u32 = TICKS_PER_HOUR - 1;

/// A share in `0..=1`, clamped; NaN reads as zero.
fn ratio(value: f64) -> Ratio {
    Ratio::new(value.clamp(0.0, 1.0)).unwrap_or(Ratio::ZERO)
}

/// The smallest multiple of `step` that is at least `share`, clamped to
/// `0..=1`. A share within [`STEP_TOLERANCE`] steps above a multiple rounds
/// down to it: 0.10 / 0.05 is 2.0000000000000004 in floating point, and that
/// 10% shortfall needs two steps, not three.
pub(super) fn round_up_to(step: Ratio, share: f64) -> Ratio {
    let steps = (share / step.value() - STEP_TOLERANCE).ceil().max(0.0);
    ratio(steps * step.value())
}

impl PowerSystem {
    /// The rotating-outage share in effect: the share of demand cut on
    /// purpose this tick, ahead of protection (spec §15.3). Zero outside a
    /// shortage and during a blackout.
    pub fn outage(&self) -> Ratio {
        self.outage
    }

    /// The rotating-outage share ordered for the current hour, which the
    /// outage moves toward; frozen during a blackout.
    pub fn outage_target(&self) -> Ratio {
        self.outage_target
    }

    /// Sizes the rotating outage for `hour`, from the state at its first
    /// instant (spec §15.3). Demand within an hour is interpolated linearly,
    /// so its worst minute is one of its ends: at minute 0 and minute 59 the
    /// shortfall share is `(demand − deliverable) / demand`, floored at 0,
    /// with demand the connected forecast at that minute and deliverable as
    /// in [`PowerSystem::deliverable`]. The result is the larger share rounded
    /// up to a multiple of `shed_step` (see [`round_up_to`]), clamped to
    /// `0..=1`. Pure: draws no random numbers and changes nothing.
    pub(super) fn size_outage(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        hour: u8,
    ) -> Ratio {
        let start = u32::from(hour) * TICKS_PER_HOUR;
        let worst = [0, LAST_MINUTE]
            .into_iter()
            .map(|minute| self.shortfall_share(region, month, hour, start, minute))
            .fold(0.0, f64::max);
        round_up_to(region.frequency().shed_step, worst)
    }

    /// The forecast shortfall at `minute` of `hour` (which starts at tick
    /// `start`), as a share of the demand forecast then, floored at 0.
    fn shortfall_share(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        hour: u8,
        start: u32,
        minute: u32,
    ) -> f64 {
        let tick = start + minute;
        let demand = self.connected_demand_forecast(region, month, tick);
        if demand <= Megawatts::ZERO {
            return 0.0;
        }
        let plants: Megawatts = self
            .connected_plants()
            .map(|p| self.deliverable(p, hour, start, minute))
            .sum();
        let deliverable = plants + self.expected_solar(region, month, tick);
        ((demand - deliverable) / demand).max(0.0)
    }

    /// What `plant` can deliver at `minute` of `hour` (spec §15.3), judged at
    /// tick `start`, the hour's first instant. Only `Online` dispatchable
    /// plants count; solar is counted separately as expected solar, and a
    /// plant whose commitment stops it at `start` counts as stopping.
    ///
    /// - **Scheduled hydro:** its allocation for `hour` (spec §12.3).
    ///   Unplanned hydro on AGC counts 0 until the next re-plan.
    /// - **Manual hydro:** its target, limited by the budget it would have
    ///   left after running at that target until `minute`.
    /// - **Manual non-hydro:** `min(target, output + ramp × minute)`, so a
    ///   plant still ramping toward its target counts only its current output
    ///   at minute 0.
    /// - **Non-hydro on AGC:** `min(capacity, output + ramp × 59)` at both
    ///   ends: the reach it has by the end of the hour.
    ///
    /// *Builder's call (AGC reach at minute 0).* A load-following AGC plant's
    /// output is held down by the outage itself, so counting only its output
    /// at minute 0 would size every hour at least at the outage in effect and
    /// the outage could never fall. The outage does not drop at minute 0
    /// anyway: it falls one step per tick, and only while the fleet's one-tick
    /// headroom covers the step (see [`PowerSystem::next_outage`]), so the
    /// target is where the outage goes, not what minute 0 needs.
    fn deliverable(&self, plant: &Plant, hour: u8, start: u32, minute: u32) -> Megawatts {
        let PlantStatus::Online { output, target } = plant.status() else {
            return Megawatts::ZERO;
        };
        let stops_now =
            commitment_actions([plant], start).contains(&CommitmentAction::Stop(plant.id()));
        if !plant.kind().is_dispatchable() || stops_now {
            return Megawatts::ZERO;
        }
        let spec = plant.spec();
        match (plant.kind(), plant.auto_follow()) {
            (PlantKind::Hydro, true) => self
                .hydro_plan
                .get(&plant.id())
                .filter(|plan| plan.planned)
                .map_or(Megawatts::ZERO, |plan| plan.hourly[usize::from(hour)]),
            (PlantKind::Hydro, false) => {
                let budget = plant.hydro_budget().map_or(0.0, |b| b.value());
                let left = budget - target.energy_over_ticks(minute).value();
                let limit = Megawatts::new(left.max(0.0) * f64::from(TICKS_PER_HOUR));
                target.min(limit).min(spec.capacity)
            }
            (_, true) => spec
                .capacity
                .min(output + spec.ramp.over_ticks(LAST_MINUTE)),
            (_, false) => target.min(output + spec.ramp.over_ticks(minute)),
        }
    }

    /// Re-sizes the outage for `hour` at its boundary and returns the order
    /// to emit when the target changed, including a change to zero.
    pub(super) fn order_outage(
        &mut self,
        region: &RegionDefinition,
        month: SimMonth,
        hour: u8,
    ) -> Option<DomainEvent> {
        let target = self.size_outage(region, month, hour);
        let changed = target != self.outage_target;
        self.outage_target = target;
        changed.then_some(DomainEvent::RotatingOutageOrdered {
            hour,
            share: target,
        })
    }

    /// The outage one Normal-mode tick on (spec §15.3): one `shed_step` up
    /// toward a higher target, unconditionally; one step down toward a lower
    /// target only when `cover`, the AGC fleet's one-tick headroom net of the
    /// shortfall it already faces (a surplus counts as cover), is at least
    /// `shed_step × demand`, so restored load never outruns the fleet's ramp.
    /// Otherwise it holds. It never passes the target.
    pub(super) fn next_outage(&self, step: Ratio, demand: Megawatts, cover: Megawatts) -> Ratio {
        let (now, target, step) = (
            self.outage.value(),
            self.outage_target.value(),
            step.value(),
        );
        if (now - target).abs() <= STEP_TOLERANCE {
            self.outage_target
        } else if now < target {
            ratio((now + step).min(target))
        } else if cover.value() >= (demand * step).value() - STEP_TOLERANCE {
            ratio((now - step).max(target))
        } else {
            self.outage
        }
    }
}
