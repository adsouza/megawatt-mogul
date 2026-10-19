//! Rotating outages (spec §15.3): a shortfall the forecast can see is shed on
//! purpose, ahead of protection.
//!
//! At `BeginOperating` and at every hour boundary in Normal mode, right after
//! the hydro re-plan, the hour starting is sized ([`PowerSystem::size_outage`])
//! and its exact shortfall share becomes the `outage_target`; the current
//! hour is sized again on a Normal-mode tick on which a connected dispatchable
//! plant comes online. The tick's load is then `demand × (1 − outage)`, and
//! the outage moves toward its target by at most one `shed_step` per tick, no
//! faster than the AGC fleet can follow ([`PowerSystem::next_outage`]).
//! Sizing is a pure function of state: it draws no random numbers.

use super::PowerSystem;
use crate::events::DomainEvent;
use crate::generation::{CommitmentAction, Plant, PlantStatus, commitment_actions};
use crate::region::{PlantKind, RegionDefinition};
use crate::time::{SimMonth, TICKS_PER_HOUR};
use crate::units::{Megawatts, Ratio};

/// An outage within this of its target counts as there, so stepwise float
/// arithmetic lands on the target exactly.
const SNAP: f64 = 1e-9;

/// The last minute of an hour, relative to its start.
const LAST_MINUTE: u32 = TICKS_PER_HOUR - 1;

/// A share in `0..=1`, clamped; NaN reads as zero.
fn ratio(value: f64) -> Ratio {
    Ratio::new(value.clamp(0.0, 1.0)).unwrap_or(Ratio::ZERO)
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
    /// in [`PowerSystem::deliverable`]. The result is the larger share,
    /// exactly, clamped to `0..=1`: 0 when the fleet covers both ends.
    ///
    /// *Builder's call (controller ruling R3): no rounding up to `shed_step`.*
    /// A shortage costs the load the fleet could not serve, not a whole
    /// block more; rotating blocks are a presentation concern. Pure: draws no
    /// random numbers and changes nothing.
    pub(super) fn size_outage(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        hour: u8,
    ) -> Ratio {
        let start = u32::from(hour) * TICKS_PER_HOUR;
        let worst = [0, LAST_MINUTE]
            .into_iter()
            .map(|minute| self.shortfall_share(region, month, start, minute))
            .fold(0.0, f64::max);
        ratio(worst)
    }

    /// The forecast shortfall at `minute` of the hour starting at tick
    /// `start`, as a share of the demand forecast then, floored at 0.
    fn shortfall_share(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
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
            .map(|p| self.deliverable(p, start, minute))
            .sum();
        let deliverable = plants + self.expected_solar(region, month, tick);
        ((demand - deliverable) / demand).max(0.0)
    }

    /// What `plant` can deliver at `minute` of the hour starting at tick
    /// `start` (spec §15.3), judged from the current state. Only `Online` dispatchable
    /// plants count; solar is counted separately as expected solar, and a
    /// plant whose commitment stops it at `start` counts as stopping.
    ///
    /// - **Scheduled hydro:** its AGC ceiling for the tick of `minute` (see
    ///   [`PowerSystem::hydro_ceilings`]): its allocation for `hour` (spec
    ///   §12.3), lowered to the ramp envelope ahead of a falling next hour.
    ///   *Builder's call:* the envelope is what AGC will actually allow at
    ///   minute 59, so crediting the full allocation there over-counts a
    ///   foreseen fall, which exact sizing (ruling R3) no longer rounds over.
    ///   Unplanned hydro on AGC counts 0 until the next re-plan.
    /// - **Manual hydro:** its target, limited by the budget it would have
    ///   left after running at that target until `minute`.
    /// - **Manual non-hydro:** `min(target, output + ramp × minute)`, so a
    ///   plant still ramping toward its target counts only its current output
    ///   at minute 0.
    /// - **Non-hydro on AGC:** `min(capacity, output + ramp × 59)` at both
    ///   ends: the reach it has by the end of the hour.
    ///
    /// *Builder's call (AGC reach at minute 0), accepted as ruling R4.* A load-following AGC plant's
    /// output is held down by the outage itself, so counting only its output
    /// at minute 0 would size every hour at least at the outage in effect and
    /// the outage could never fall. The outage does not drop at minute 0
    /// anyway: it falls one step per tick, and only while the fleet's one-tick
    /// headroom covers the step (see [`PowerSystem::next_outage`]), so the
    /// target is where the outage goes, not what minute 0 needs.
    fn deliverable(&self, plant: &Plant, start: u32, minute: u32) -> Megawatts {
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
                .hydro_ceilings(start + minute)
                .get(&plant.id())
                .copied()
                .unwrap_or(Megawatts::ZERO),
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

    /// Re-sizes the outage for `hour`, at its boundary or when a plant comes
    /// online during it, and returns the order to emit when the target
    /// changed, including a change to zero.
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

    /// The outage one Normal-mode tick on (spec §15.3, controller rulings R1
    /// and R2): it moves toward its target by `min(|target − outage|,
    /// shed_step, cover / demand)`, so it never passes the target and never
    /// moves faster than the AGC fleet can follow.
    ///
    /// - **Cutting** (below the target), `cover` is the fleet's one-tick
    ///   ramp-down room plus the shortfall it is already behind by (see
    ///   [`PowerSystem::agc_headroom_down`]): a cut the fleet cannot back down
    ///   from would open a surplus, and in a deep outage one step of demand is
    ///   a large share of the load left.
    /// - **Restoring** (above the target), `cover` is the fleet's one-tick
    ///   headroom net of that shortfall, a surplus counting as cover (see
    ///   [`PowerSystem::agc_headroom`]), so restored load never outruns the
    ///   fleet's ramp.
    ///
    /// A negative cover counts as none. Any positive cover moves the outage,
    /// so shares in transit need not be multiples of `shed_step`. With no
    /// demand there is nothing to follow and the outage moves a full step.
    pub(super) fn next_outage(&self, step: Ratio, demand: Megawatts, cover: Megawatts) -> Ratio {
        let (now, target) = (self.outage.value(), self.outage_target.value());
        if (now - target).abs() <= SNAP {
            return self.outage_target;
        }
        let pace = if demand > Megawatts::ZERO {
            (cover / demand).max(0.0)
        } else {
            f64::INFINITY
        };
        let moved = step.value().min(pace).min((now - target).abs());
        let next = if now < target {
            now + moved
        } else {
            now - moved
        };
        if (next - target).abs() <= SNAP {
            self.outage_target
        } else {
            ratio(next)
        }
    }
}
