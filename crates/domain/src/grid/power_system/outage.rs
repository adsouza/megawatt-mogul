//! Rotating outages (spec §15.3): a shortfall the forecast can see is shed on
//! purpose, ahead of protection.
//!
//! At `BeginOperating` and at every hour boundary in Normal mode, right after
//! the hydro re-plan, the hour starting is sized ([`PowerSystem::size_outage`])
//! and its exact shortfall share becomes the `outage_target`; the current
//! hour is sized again on a Normal-mode tick on which a connected dispatchable
//! plant comes online, and when a black start completes. The tick's load is
//! then `demand × (1 − outage)`, and the outage moves toward its target no
//! faster than the AGC fleet can follow: a cut by at most one `shed_step` per
//! tick, a restore by at most one `shed_step` or the persisting surplus
//! ([`PowerSystem::next_outage`]).
//! Sizing is a pure function of state: it draws no random numbers.

use std::collections::BTreeMap;

use super::{PowerSystem, SupplyForecast, SupplyForecastPoint};
use crate::events::DomainEvent;
use crate::generation::{CommitmentAction, HydroFill, Plant, PlantStatus, commitment_actions};
use crate::ids::PlantId;
use crate::region::{PlantKind, RegionDefinition};
use crate::time::{SimMonth, TICKS_PER_DAY, TICKS_PER_HOUR};
use crate::units::{Megawatts, Ratio};

/// An outage within this of its target counts as there, so stepwise float
/// arithmetic lands on the target exactly.
const SNAP: f64 = 1e-9;

/// The last minute of an hour, relative to its start.
const LAST_MINUTE: u32 = TICKS_PER_HOUR - 1;

/// The minutes at which an hour is judged: demand is interpolated linearly
/// within an hour, so its extremes are at the two ends.
const HOUR_ENDS: [u32; 2] = [0, LAST_MINUTE];

struct PlannedGeneration {
    total: Megawatts,
    by_kind: BTreeMap<PlantKind, Megawatts>,
}

/// One end of an hour (minute 0 or 59), judged against a fleet rule: the
/// evaluation shared by operating-time sizing (spec §15.3) and the two
/// Planning-time warnings (spec §14.3 and §15.5).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HourEnd {
    /// The connected demand forecast at that minute.
    pub(crate) demand: Megawatts,
    /// What the dispatchable fleet delivers at that minute by the rule, plus
    /// expected solar then.
    pub(crate) deliverable: Megawatts,
    /// The expected solar output at that minute, already in `deliverable`.
    pub(crate) solar: Megawatts,
}

impl HourEnd {
    /// `demand − deliverable`: positive when the fleet falls short.
    pub(crate) fn shortfall(self) -> Megawatts {
        self.demand - self.deliverable
    }

    /// `demand − solar`: the load left to the dispatchable fleet.
    pub(crate) fn net_load(self) -> Megawatts {
        self.demand - self.solar
    }

    /// The worse of an hour's two ends: the one with the larger shortfall,
    /// minute 0 on a tie.
    #[cfg(test)]
    pub(crate) fn worse(ends: [HourEnd; 2]) -> HourEnd {
        Self::worse_by(ends, HourEnd::shortfall)
    }

    /// The end of an hour that scores higher on `badness`, minute 0 on a tie:
    /// one tie rule for every warning that judges an hour by its worse end.
    pub(crate) fn worse_by(
        [first, last]: [HourEnd; 2],
        badness: impl Fn(HourEnd) -> Megawatts,
    ) -> HourEnd {
        if badness(last) > badness(first) {
            last
        } else {
            first
        }
    }

    /// The shortfall as a share of demand, floored at 0; 0 with no demand.
    fn share(self) -> f64 {
        if self.demand <= Megawatts::ZERO {
            return 0.0;
        }
        (self.shortfall() / self.demand).max(0.0)
    }
}

/// What committed dispatchable `plant` could deliver in `hour` by the
/// Planning check's rules (spec §15.5), the same at both ends of the hour: on
/// AGC its availability, which for a dispatchable non-hydro plant is its
/// capacity at every minute, and for scheduled hydro its allocation for
/// `hour` in `preview` (the pure hour-0 plan with the coming day's budgets),
/// nothing if it has no water; on a manual target its initial target, manual
/// hydro included, unlimited by the day's water.
fn planned_deliverable(
    plant: &Plant,
    hour: u8,
    preview: &BTreeMap<PlantId, HydroFill>,
) -> Megawatts {
    if !plant.auto_follow() {
        plant.initial_target()
    } else if plant.kind() == PlantKind::Hydro {
        preview
            .get(&plant.id())
            .map_or(Megawatts::ZERO, |fill| fill.hourly[usize::from(hour)])
    } else {
        plant.spec().capacity
    }
}

/// A share in `0..=1`, clamped; NaN reads as zero.
fn ratio(value: f64) -> Ratio {
    Ratio::new(value.clamp(0.0, 1.0)).unwrap_or(Ratio::ZERO)
}

impl PowerSystem {
    /// Generation credited by the Planning warning, before expected solar.
    /// Preserve plant-ID summation order and the warning's manual-target rule.
    fn planned_generation_hours(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
    ) -> [PlannedGeneration; 24] {
        let preview = self.preview_hydro(region, month);
        std::array::from_fn(|h| {
            let mut total = Megawatts::ZERO;
            let mut by_kind = BTreeMap::new();
            for plant in self
                .connected_plants()
                .filter(|p| p.kind().is_dispatchable() && p.commitment().is_committed(h as u8))
            {
                let output = planned_deliverable(plant, h as u8, &preview);
                total += output;
                *by_kind.entry(plant.kind()).or_insert(Megawatts::ZERO) += output;
            }
            PlannedGeneration { total, by_kind }
        })
    }

    pub(crate) fn supply_forecast(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
    ) -> SupplyForecast {
        if self.has_batteries() {
            return SupplyForecast {
                has_storage: true,
                minutes: self.battery_supply_forecast(region, month),
            };
        }
        let hours = self.planned_generation_hours(region, month);
        let minutes = (0..TICKS_PER_DAY)
            .map(|tick| {
                let hour = &hours[(tick / TICKS_PER_HOUR) as usize];
                let solar = self.expected_solar(region, month, tick);
                let mut generation = hour.by_kind.clone();
                generation.insert(PlantKind::Solar, solar);
                SupplyForecastPoint {
                    demand: self.connected_demand_forecast(region, month, tick),
                    generation,
                    gross_supply: hour.total + solar,
                    battery_charge: Megawatts::ZERO,
                    battery_discharge: Megawatts::ZERO,
                }
            })
            .collect();
        SupplyForecast {
            has_storage: false,
            minutes,
        }
    }

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

    /// Test support: sets the grid mode and the outage in effect directly,
    /// bypassing the rules that keep them consistent, so tests can corrupt
    /// state to exercise invariant checks.
    #[cfg(test)]
    pub(crate) fn set_outage_unchecked(&mut self, mode: crate::grid::GridMode, outage: Ratio) {
        self.protection.mode = mode;
        self.outage = outage;
    }

    /// Both ends of `hour` in `month` (see [`HourEnd`]): at minute 0 and
    /// minute 59, the connected demand forecast and `fleet(minute)` plus
    /// expected solar, all taken at that minute. Operating sizing and the
    /// Planning check differ only in `fleet`, the rule for what the
    /// dispatchable plants deliver.
    pub(crate) fn hour_ends(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        hour: u8,
        fleet: impl Fn(u32) -> Megawatts,
    ) -> [HourEnd; 2] {
        let start = u32::from(hour) * TICKS_PER_HOUR;
        HOUR_ENDS.map(|minute| {
            let tick = start + minute;
            let solar = self.expected_solar(region, month, tick);
            HourEnd {
                demand: self.connected_demand_forecast(region, month, tick),
                deliverable: fleet(minute) + solar,
                solar,
            }
        })
    }

    /// Sizes the rotating outage for `hour`, from the state at its first
    /// instant (spec §15.3). Demand within an hour is interpolated linearly,
    /// so its worst minute is one of its ends: at minute 0 and minute 59 the
    /// shortfall share is `(demand − deliverable) / demand`, floored at 0,
    /// with demand the connected forecast at that minute and deliverable as
    /// in [`PowerSystem::deliverable`] (see [`PowerSystem::hour_ends`]). The
    /// result is the larger share, exactly, clamped to `0..=1`: 0 when the
    /// fleet covers both ends.
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
        if self.has_batteries() {
            let minutes = self.storage_forecast_minutes(
                region,
                month,
                start.max(self.battery_tick),
                start + 60,
                false,
            );
            return ratio(minutes.into_iter().map(HourEnd::share).fold(0.0, f64::max));
        }
        let ends = self.hour_ends(region, month, hour, |minute| {
            self.connected_plants()
                .map(|p| self.deliverable(p, start, minute))
                .sum()
        });
        ratio(ends.into_iter().map(HourEnd::share).fold(0.0, f64::max))
    }

    /// Both ends of every hour of the coming day in `month`, judged by the
    /// Planning check's upper bound (spec §15.5): each connected,
    /// commissioned, dispatchable plant committed in the hour by its pattern
    /// (not today's suppression override) counts as in
    /// [`planned_deliverable`], plus expected solar, with demand
    /// and solar taken at the same minute.
    ///
    /// An upper bound by design: it ignores the start-up and ramp limits the
    /// operating-time sizing applies. Pure.
    #[cfg(test)]
    pub(crate) fn planned_hour_ends(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
    ) -> [[HourEnd; 2]; 24] {
        let forecast = self.supply_forecast(region, month);
        let end = |point: &SupplyForecastPoint| HourEnd {
            demand: point.demand,
            solar: point.generation[&PlantKind::Solar],
            deliverable: point.gross_supply,
        };
        std::array::from_fn(|h| {
            let minutes =
                &forecast.minutes[h * TICKS_PER_HOUR as usize..(h + 1) * TICKS_PER_HOUR as usize];
            if forecast.has_storage {
                let worst = minutes
                    .iter()
                    .map(end)
                    .reduce(|a, b| if b.shortfall() > a.shortfall() { b } else { a })
                    .expect("sixty minutes");
                [worst; 2]
            } else {
                [end(&minutes[0]), end(&minutes[LAST_MINUTE as usize])]
            }
        })
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
    /// anyway: it is restored at a paced rate, by at most one `shed_step` (or
    /// a persisting surplus) per tick and never faster than the fleet's net
    /// one-tick headroom covers (see [`PowerSystem::next_outage`]), so the
    /// target is where the outage goes, not what minute 0 needs.
    pub(super) fn deliverable(&self, plant: &Plant, start: u32, minute: u32) -> Megawatts {
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

    /// Re-sizes the outage target for `hour`: at its boundary, when a plant
    /// comes online during it (ruling R5) and when a black start completes
    /// (ruling M1). The tick emits at most one order, with its final target
    /// (see [`PowerSystem::outage_order`]).
    pub(super) fn resize_outage(&mut self, region: &RegionDefinition, month: SimMonth, hour: u8) {
        self.outage_target = self.size_outage(region, month, hour);
    }

    /// The order to emit for a tick of `hour` that started with the target
    /// `before`: `RotatingOutageOrdered` with the final target when it
    /// changed, including a change to zero; one per tick at most.
    pub(super) fn outage_order(&self, hour: u8, before: Ratio) -> Option<DomainEvent> {
        (self.outage_target != before).then_some(DomainEvent::RotatingOutageOrdered {
            hour,
            share: self.outage_target,
        })
    }

    /// The AGC fleet's cover for moving the outage one Normal-mode tick
    /// toward its target, given `shortfall`, AGC's signed requirement at the
    /// outage in effect (positive when the fleet is already behind, negative
    /// in a surplus):
    ///
    /// - restoring, its one-tick headroom for `effect_tick` net of the
    ///   shortfall, a surplus counting as cover (see
    ///   [`PowerSystem::agc_headroom`]);
    /// - cutting, its one-tick ramp-down room plus the shortfall, a deficit
    ///   counting as room (see [`PowerSystem::agc_headroom_down`]).
    ///
    /// *Builder's call (accepted, ruling R4): both covers are net of the
    /// shortfall*, as in hydro recovery.
    pub(super) fn outage_cover(
        &self,
        available: &BTreeMap<PlantId, Megawatts>,
        shortfall: Megawatts,
        effect_tick: u32,
    ) -> Megawatts {
        if self.outage > self.outage_target {
            let ceilings = self.agc_ceilings(effect_tick);
            self.agc_headroom(available, &ceilings, None) - shortfall
        } else {
            self.agc_headroom_down() + shortfall
        }
    }

    /// The outage one Normal-mode tick on (spec §15.3, controller rulings R1,
    /// R2 and I2). It moves toward its target, never passing it and never
    /// faster than the AGC fleet can follow, with `cover` from
    /// [`PowerSystem::outage_cover`]:
    ///
    /// - **Cutting** (below the target), by `min(gap, shed_step, cover /
    ///   demand)`: a cut the fleet cannot back down from would open a surplus,
    ///   and in a deep outage one step of demand is a large share of the load
    ///   left.
    /// - **Restoring** (above the target), by `min(gap, max(shed_step,
    ///   surplus / demand), cover / demand)`, where `surplus()` is supply less
    ///   the outage's load this tick, floored at 0, asked for only on a
    ///   restoring tick. Restoring load that only
    ///   absorbs a persisting surplus is safe: it can only lower frequency,
    ///   and `cover` already counts the surplus. So a unit whose minimum
    ///   output exceeds a step of demand, started into a deep outage, is given
    ///   its load at once. The caller passes no surplus while any plant is in
    ///   an unrecoverable ramp-down (a commitment stop, a manual plant above
    ///   its target, scheduled hydro above its foreseen ceiling): that surplus
    ///   is about to vanish, and restoring it as load would be shed again
    ///   (ruling N1).
    ///
    /// A negative cover counts as none. Any positive cover moves the outage,
    /// so shares in transit need not be multiples of `shed_step`. With no
    /// demand there is nothing to follow and no cover is needed: a cut moves
    /// a full step, and a restore closes the gap at once.
    pub(super) fn next_outage(
        &self,
        step: Ratio,
        demand: Megawatts,
        cover: Megawatts,
        surplus: impl FnOnce() -> Megawatts,
    ) -> Ratio {
        let (now, target) = (self.outage.value(), self.outage_target.value());
        let gap = (now - target).abs();
        if gap <= SNAP {
            return self.outage_target;
        }
        let share = |mw: Megawatts| {
            if demand > Megawatts::ZERO {
                (mw / demand).max(0.0)
            } else {
                f64::INFINITY
            }
        };
        let next = if now < target {
            now + gap.min(step.value()).min(share(cover))
        } else {
            let pace = step.value().max(share(surplus()));
            now - gap.min(pace).min(share(cover))
        };
        if (next - target).abs() <= SNAP {
            self.outage_target
        } else {
            ratio(next)
        }
    }

    /// Both ends of every hour of the coming day in `month`, judged by the
    /// minimum-generation rule (spec §14.3), each with the hour's committed
    /// minimum: the fleet is [`PowerSystem::hourly_committed_minimum`] at both
    /// ends, so an end's [`HourEnd::net_load`] is the load the committed
    /// minimums must fit under. Pure.
    pub(crate) fn committed_minimum_hour_ends(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
    ) -> [(Megawatts, [HourEnd; 2]); 24] {
        let committed = self.hourly_committed_minimum();
        std::array::from_fn(|h| {
            let fleet = committed[h];
            (fleet, self.hour_ends(region, month, h as u8, |_| fleet))
        })
    }
}
