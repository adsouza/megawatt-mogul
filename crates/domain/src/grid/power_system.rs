//! The power system aggregate: plants, cities and the network, advanced
//! together one tick at a time.
//!
//! Only plants and cities on connected nodes take part in the balance.
//! Disconnected plants still progress physically (they start, ramp and spend
//! hydro budget) but supply nothing; disconnected cities are not customers.

use std::collections::BTreeMap;

mod hydro_plan;
mod outage;

use hydro_plan::{HydroOverride, PlantHydroPlan, mid_hour_tick};
pub use hydro_plan::{HydroScheduleView, PlantHydroScheduleView};

use super::dispatch::{agc_retarget, boundary_dispatch};
use super::frequency::{FrequencyBand, band, next_deviation};
use super::network::Network;
use super::protection::{GridMode, ProtectionOutcome, ProtectionState, evaluate, restoration_step};
use super::summary::TickSummary;
use crate::demand::{DemandCenter, DemandNoise, forecast_demand, hourly_forecast};
use crate::error::DomainError;
use crate::events::DomainEvent;
use crate::generation::{
    BoundaryStart, CommitmentAction, Plant, PlantStatus, commitment_actions, draw_clearness,
    solar_available, sun_factor,
};
use crate::ids::{CellCoord, CityId, NodeId, PlantId};
use crate::region::{Cell, FrequencyParams, PlantKind, RegionDefinition};
use crate::rng::SimRng;
use crate::time::{SimDuration, SimMonth, TICKS_PER_HOUR};
use crate::units::{Hertz, MegawattHours, Megawatts, Ratio};

const HOURS_PER_DAY: f64 = 24.0;
const MONTHS_PER_YEAR: usize = 12;

/// How power flows between connected nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkModel {
    /// One shared bus with unlimited line capacity: lines matter only for
    /// connectivity.
    #[default]
    CopperPlate,
}

/// What starting a representative day reported to the session.
#[derive(Debug, Clone, PartialEq)]
pub struct DayInitialisation {
    /// One-off month-boundary starts per plant (see [`BoundaryStart::Charged`]).
    pub boundary_starts: BTreeMap<PlantId, u32>,
    /// The rotating-outage share ordered for hour 0 (spec §15.3); zero when
    /// the fleet covers it.
    pub outage: Ratio,
}

/// Step 5's AGC inputs for the tick (see [`PowerSystem::tick`]), kept so AGC
/// can be re-run on the post-trip state after an over-frequency trip.
struct Redispatch<'a> {
    /// Every plant's availability this tick (after curtailment).
    available: &'a BTreeMap<PlantId, Megawatts>,
    /// The load AGC aims to serve, `demand × to_serve`.
    to_serve: Megawatts,
    /// Solar availability withheld this tick.
    withheld: Megawatts,
    /// Connected solar availability before curtailment.
    curtailable: Megawatts,
    /// The tick the new targets first act on.
    effect_tick: u32,
}

/// The physical grid: every plant and city, the network joining them, and the
/// frequency and protection state between ticks.
#[derive(Debug, Clone, PartialEq)]
pub struct PowerSystem {
    network: Network,
    plants: BTreeMap<PlantId, Plant>,
    cities: BTreeMap<CityId, DemandCenter>,
    model: NetworkModel,
    noise: DemandNoise,
    /// Today's cloud cover; clear sky until the first [`PowerSystem::begin_day`].
    clearness: f64,
    deviation: Hertz,
    /// The band last reported, so `FrequencyDeviated` fires only on a change.
    band: FrequencyBand,
    protection: ProtectionState,
    /// Solar availability withheld by over-frequency protection, applied from
    /// the next tick (see [`PowerSystem::tick`], step 8).
    curtailment: Megawatts,
    /// Each scheduled hydro plant's water plan (spec §12.3).
    hydro_plan: BTreeMap<PlantId, PlantHydroPlan>,
    /// The hour the hydro plan was last made from, or 24 once the day is
    /// finished.
    hydro_planned_from: u8,
    /// The emergency override of the hydro cap, as of the last AGC pass:
    /// off, lifted or recovering (see [`PowerSystem::retarget_agc`]).
    hydro_override: HydroOverride,
    /// The rotating-outage share in effect (spec §15.3): the share of demand
    /// cut on purpose, ahead of protection. Zero outside a shortage and
    /// during a blackout.
    outage: Ratio,
    /// The rotating-outage share ordered for the current hour, which
    /// `outage` moves toward; frozen during a blackout.
    outage_target: Ratio,
}

fn is_curtailable(plant: &Plant) -> bool {
    plant.kind() == PlantKind::Solar && plant.is_online()
}

fn is_commissioned(plant: &Plant) -> bool {
    !matches!(plant.status(), PlantStatus::UnderConstruction { .. })
}

/// The water a hydro plant starts a day of `month` with: `capacity × 24 h ×
/// hydro_potential(cell) × monthly_flow[month]` (spec §12.2). `begin_day`
/// resets each commissioned hydro plant to it, and the Planning-time hydro
/// preview plans with it before the day begins (spec §15.5).
pub(crate) fn day_hydro_budget(
    region: &RegionDefinition,
    month: SimMonth,
    plant: &Plant,
) -> MegawattHours {
    let flow = region.hydro().monthly_flow[month.index0()];
    let potential = cell_ratio(region, plant.cell(), |c| c.hydro_potential);
    MegawattHours::new(plant.spec().capacity.value() * HOURS_PER_DAY * potential.value() * flow)
}

/// The most AGC may ask of plant `id`: its availability, lowered to its hydro
/// ceiling where one applies.
fn agc_cap(
    available: &BTreeMap<PlantId, Megawatts>,
    ceilings: &BTreeMap<PlantId, Megawatts>,
    id: PlantId,
) -> Megawatts {
    let cap = available[&id];
    ceilings.get(&id).map_or(cap, |ceiling| cap.min(*ceiling))
}

impl PowerSystem {
    pub fn new(
        network: Network,
        plants: BTreeMap<PlantId, Plant>,
        cities: BTreeMap<CityId, DemandCenter>,
        model: NetworkModel,
    ) -> Self {
        Self {
            network,
            plants,
            cities,
            model,
            noise: DemandNoise::default(),
            clearness: 1.0,
            deviation: Hertz::ZERO,
            band: FrequencyBand::Normal,
            protection: ProtectionState::default(),
            curtailment: Megawatts::ZERO,
            hydro_plan: BTreeMap::new(),
            hydro_planned_from: 0,
            hydro_override: HydroOverride::Off,
            outage: Ratio::ZERO,
            outage_target: Ratio::ZERO,
        }
    }

    pub fn plants(&self) -> &BTreeMap<PlantId, Plant> {
        &self.plants
    }

    pub fn plant(&self, id: PlantId) -> Option<&Plant> {
        self.plants.get(&id)
    }

    pub fn plant_mut(&mut self, id: PlantId) -> Option<&mut Plant> {
        self.plants.get_mut(&id)
    }

    /// Adds a plant under its own id, replacing any plant with that id.
    pub fn insert_plant(&mut self, plant: Plant) {
        self.plants.insert(plant.id(), plant);
    }

    pub fn cities(&self) -> &BTreeMap<CityId, DemandCenter> {
        &self.cities
    }

    pub fn network(&self) -> &Network {
        &self.network
    }

    pub fn network_mut(&mut self) -> &mut Network {
        &mut self.network
    }

    pub fn model(&self) -> NetworkModel {
        self.model
    }

    /// Frequency deviation from nominal.
    pub fn deviation(&self) -> Hertz {
        self.deviation
    }

    pub fn protection(&self) -> ProtectionState {
        self.protection
    }

    fn on_grid(&self, node: NodeId) -> bool {
        self.network.is_connected(node)
    }

    fn connected_cities(&self) -> impl Iterator<Item = &DemandCenter> {
        self.cities.values().filter(|c| self.on_grid(c.node()))
    }

    /// Commissioned plants on connected nodes, in id order.
    fn connected_plants(&self) -> impl Iterator<Item = &Plant> {
        self.plants
            .values()
            .filter(|p| is_commissioned(p) && self.on_grid(p.node()))
    }

    /// For each hour, the least that the connected, commissioned,
    /// dispatchable plants committed in that hour (by their commitment
    /// pattern, not today's suppression override) must generate: a plant on
    /// AGC can back down to its minimum output, a manual plant holds its
    /// initial target. Pure.
    pub(crate) fn hourly_committed_minimum(&self) -> [Megawatts; 24] {
        let mut minimum = [Megawatts::ZERO; 24];
        for plant in self
            .connected_plants()
            .filter(|p| p.kind().is_dispatchable())
        {
            let floor = if plant.auto_follow() {
                plant.spec().min_output()
            } else {
                plant.initial_target()
            };
            for (hour, total) in minimum.iter_mut().enumerate() {
                if plant.commitment().is_committed(hour as u8) {
                    *total += floor;
                }
            }
        }
        minimum
    }

    /// For each hour of the coming day in `month`, the most the connected,
    /// commissioned plants committed in that hour (by their commitment
    /// pattern, not today's suppression override) could deliver at minute 30
    /// (spec §15.5): a plant on AGC its availability, a manual plant its
    /// initial target, scheduled hydro its allocation from a pure preview of
    /// the hour-0 plan with the coming day's budgets, plus expected solar.
    ///
    /// An upper bound by design: it ignores the start-up and ramp limits the
    /// operating-time outage sizing applies. Manual hydro counts its initial
    /// target like any manual plant, unlimited by the day's water. A hydro
    /// plant on AGC with no water counts nothing. Pure.
    pub(crate) fn hourly_deliverable(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
    ) -> [Megawatts; 24] {
        let preview =
            self.compute_hydro_plan(region, month, 0, |p| day_hydro_budget(region, month, p));
        let mut deliverable: [Megawatts; 24] =
            std::array::from_fn(|h| self.expected_solar(region, month, mid_hour_tick(h as u8)));
        for plant in self
            .connected_plants()
            .filter(|p| p.kind().is_dispatchable())
        {
            let commitment = plant.commitment();
            for (hour, total) in deliverable.iter_mut().enumerate() {
                if !commitment.is_committed(hour as u8) {
                    continue;
                }
                *total += if !plant.auto_follow() {
                    plant.initial_target()
                } else if plant.kind() == PlantKind::Hydro {
                    preview
                        .get(&plant.id())
                        .map_or(Megawatts::ZERO, |f| f.hourly[hour])
                } else {
                    let sun = sun_factor(region.solar(), month, mid_hour_tick(hour as u8));
                    self.available(region, plant, sun)
                };
            }
        }
        deliverable
    }

    /// Starts a representative day of `month`. Nothing physical carries over
    /// from the previous day: each commissioned hydro plant's budget is reset
    /// to `capacity × 24 h × hydro_potential(cell) × monthly_flow[month]`, the
    /// day's clearness is drawn (exactly one normal), every plant is
    /// initialised from its cyclic pattern (clearing operating overrides),
    /// frequency and protection return to nominal and `Normal`, the demand
    /// forecast error and solar curtailment return to zero, and any hydro
    /// recovery ends (see `retarget_agc`). The hydro
    /// schedule is then planned over the whole day (spec §12.3). Last, the
    /// auto-follow fleet is dispatched to balance the tick-0 load (the cyclic
    /// day's boundary state), so a day does not open in a deficit that
    /// ramp-limited AGC cannot recover; plants on manual targets keep theirs.
    ///
    /// **Hour 0's rotating outage (spec §15.3).** The day starts with the
    /// outage at hour 0's target, and the boundary dispatch balances against
    /// the load it leaves (the cyclic-boundary principle: ramping in from 0
    /// would black out a large shortfall on tick 1). The hour is sized from
    /// the boundary dispatch for the full forecast, which puts every AGC plant
    /// as high as that load needs, rather than from the initial outputs the
    /// pattern gives it; when an outage is ordered the boundary is then
    /// dispatched again against the reduced load. The share is reported in
    /// [`DayInitialisation::outage`].
    ///
    /// *Builder's call (size after a first boundary dispatch).* Sizing
    /// straight after the hydro plan would read the pattern's initial outputs
    /// (AGC plants at minimum stable output) and order outages on days the
    /// fleet covers.
    pub fn begin_day(
        &mut self,
        region: &RegionDefinition,
        month: SimMonth,
        rng: &mut SimRng,
    ) -> DayInitialisation {
        for plant in self.plants.values_mut() {
            if plant.kind() == PlantKind::Hydro && is_commissioned(plant) {
                plant.reset_hydro_budget(day_hydro_budget(region, month, plant));
            }
        }
        self.clearness = draw_clearness(region.solar(), month, rng);
        let boundary_starts = self
            .plants
            .iter_mut()
            .filter_map(|(id, plant)| {
                (plant.initialise_pattern_day() == BoundaryStart::Charged).then_some((*id, 1))
            })
            .collect();
        self.deviation = Hertz::ZERO;
        self.band = FrequencyBand::Normal;
        self.protection = ProtectionState::default();
        self.noise = DemandNoise::default();
        self.curtailment = Megawatts::ZERO;
        self.hydro_override = HydroOverride::Off;
        self.outage = Ratio::ZERO;
        self.outage_target = Ratio::ZERO;
        self.plan_hydro(region, month, 0);
        self.balance_boundary(region, month);
        self.outage_target = self.size_outage(region, month, 0);
        self.outage = self.outage_target;
        if self.outage > Ratio::ZERO {
            self.balance_boundary(region, month);
        }
        DayInitialisation {
            boundary_starts,
            outage: self.outage,
        }
    }

    /// Sets the dispatch state at instant 0 as the end of the previous,
    /// identical day left it: the auto-follow fleet fills the tick-0 forecast
    /// in merit order (see [`boundary_dispatch`]). The load is the connected
    /// demand forecast less the rotating outage, less expected solar and less
    /// the output of connected online plants that are not auto-follow, which
    /// keep their initial target. Scheduled hydro is capped at its hour-0 ceiling: its hour-0
    /// allocation, lowered to the ramp envelope (see
    /// [`PowerSystem::hydro_ceilings`]). Draws no random numbers.
    fn balance_boundary(&mut self, region: &RegionDefinition, month: SimMonth) {
        let manual: Megawatts = self
            .connected_plants()
            .filter(|p| p.is_online() && !p.auto_follow())
            .map(Plant::output)
            .sum();
        let forecast = self.connected_demand_forecast(region, month, 0);
        let solar = self.expected_solar(region, month, 0);
        let load = forecast * (1.0 - self.outage.value()) - solar - manual;
        let (mut available, _, _) = self.availability(region, month, 0);
        for (id, ceiling) in self.hydro_ceilings(0) {
            if let Some(mw) = available.get_mut(&id) {
                *mw = mw.min(ceiling);
            }
        }
        let network = &self.network;
        let mut connected: Vec<&mut Plant> = self
            .plants
            .values_mut()
            .filter(|p| is_commissioned(p) && network.is_connected(p.node()))
            .collect();
        boundary_dispatch(&mut connected, &available, load);
    }

    /// The most `plant` can produce this tick given its resource.
    fn available(&self, region: &RegionDefinition, plant: &Plant, sun: f64) -> Megawatts {
        match plant.kind() {
            PlantKind::Solar => {
                let irradiance = cell_ratio(region, plant.cell(), |c| c.irradiance);
                solar_available(plant.spec().capacity, irradiance, sun, self.clearness)
            }
            PlantKind::Hydro => plant.hydro_available(),
            _ => plant.spec().capacity,
        }
    }

    /// Advances the grid by tick `tick` (`0..1440`) of a day in `month`.
    ///
    /// In order (spec §6.4 steps 2–10):
    /// 0. At every hour boundary after midnight, the hydro schedule is
    ///    re-planned over the hours left with the remaining budgets (spec
    ///    §12.3), so the new hour's allocation is in place for this tick's AGC.
    ///    Then, in Normal mode only, the rotating outage is re-sized for the
    ///    hour starting (spec §15.3, see [`PowerSystem::size_outage`]). During
    ///    a blackout the target ordered before it stays frozen. A tick whose
    ///    target ends different from where it began emits one
    ///    `RotatingOutageOrdered` with the final target, just before any
    ///    `FrequencyDeviated` (ruling M3).
    /// 1. Demand of connected cities: forecast × (1 + forecast error). The
    ///    error advances by exactly one normal draw every tick, in any state.
    /// 2. Solar and hydro availability for every plant. The standing solar
    ///    curtailment is withheld from connected online solar pro rata to each
    ///    plant's availability.
    /// 3. The commitment actions due at `tick` (suppressed plants get none).
    /// 4. Every plant steps once. If a connected dispatchable plant came
    ///    online in Normal mode, the current hour's outage is re-sized
    ///    (controller ruling R5): a tick event, not a command.
    /// 5. The tick's **load** is `demand × (1 − outage)`, with the outage in
    ///    effect now; steps 7–9 read it wherever they read demand before. In
    ///    Normal mode the outage then moves toward its target for the load
    ///    from tick t + 1, paced by the AGC fleet's one-tick ramp room (see
    ///    [`PowerSystem::next_outage`]).
    ///
    ///    *Builder's call (the move comes before AGC, not after protection).*
    ///    AGC's targets set in step 6 act on tick t + 1's step, so AGC aims at
    ///    the load they will meet; moving after protection would open a
    ///    one-tick imbalance of every moved share that AGC had not dispatched
    ///    for, a standing deficit while restoring.
    ///
    ///    *Builder's call (the covers are net).* Both covers are measured on
    ///    the post-step outputs, with the hydro ceilings for `effect_tick`,
    ///    and net of AGC's signed requirement at the current outage: a deficit
    ///    the fleet is already behind by uses up headroom and adds to the room
    ///    for a cut; a surplus does the reverse, as in hydro recovery.
    /// 6. AGC re-targets connected auto-follow plants toward the load to serve:
    ///    `demand × (1 − outage) × (1 − shed)` with the moved outage, or in a
    ///    blackout `demand × (1 − next)`, with `next` the shed after the next
    ///    restoration step, so supply is pre-positioned for it. While solar is
    ///    curtailed, AGC also makes room for the withheld solar, so the cheaper
    ///    solar returns as thermal ramps down. Scheduled hydro is held to its
    ///    hourly allocation, except during an emergency lift and the recovery
    ///    after it (see [`PowerSystem::retarget_agc`]); a rotating outage alone
    ///    never lifts the cap (see [`PowerSystem::hydro_cap_lifted`]).
    /// 7. Imbalance between connected supply and the load served,
    ///    `load × (1 − shed)`.
    /// 8. Frequency update, held at nominal in a blackout, and on a
    ///    Normal-mode tick with no load energised, where protection then
    ///    resets over-frequency tracking (builder's call, rulings R8, I3 and
    ///    M7: a fully cut grid has no frequency to measure). System size is
    ///    the load.
    /// 9. Protection on the load: load shedding as a share of the load; a
    ///    blackout that trips every plant, suppresses its commitment for the
    ///    day, resets the deviation and the curtailment and sets the outage to
    ///    zero, keeping its target; black-start restoration down to that
    ///    frozen target, its last step needing supply for that floor only
    ///    (see [`restoration_step`]), after which Normal mode resumes with the
    ///    outage at the target, protection's shed at zero and the hour
    ///    re-sized; solar curtailment,
    ///    described below; and an over-frequency trip of one plant, after
    ///    which AGC re-runs step 6 on the post-trip outputs (see
    ///    [`PowerSystem::over_frequency_trip`]). Tripped solar reconnects once
    ///    restoration first re-energises the grid.
    /// 10. The summary of the state after protection (see [`TickSummary`]),
    ///     also emitted last as `TickCompleted`: the outage and shed it
    ///     reports are those from instant t + 1. Steps 6–9 decide on the
    ///     supply measured after step 4.
    ///
    /// **Solar curtailment** is physical: it lowers solar availability from
    /// the next tick. It engages on an over-frequency `Curtail` outcome and is
    /// then recomputed every tick as the surplus AGC cannot absorb within its
    /// one-tick ramp (the negated leftover of step 6), clamped to the solar
    /// available to curtail. It therefore follows the surplus down as demand
    /// rises, solar falls or AGC makes room, and disengages at zero, so solar is
    /// never stranded and nothing oscillates. AGC plants are forced to minimum
    /// only on a `Curtail` tick where curtailing every connected solar plant
    /// still could not cover that surplus; the curtailment is then reduced by
    /// what forcing removes. After an over-frequency trip the curtailment is
    /// recomputed from the post-trip AGC pass, without forcing.
    pub fn tick(
        &mut self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
        rng: &mut SimRng,
    ) -> (TickSummary, Vec<DomainEvent>) {
        let params = region.frequency();
        let mut events = Vec::new();
        let hour = u8::try_from(tick / TICKS_PER_HOUR).unwrap_or(u8::MAX);
        let ordered = self.outage_target;
        if tick > 0 && tick.is_multiple_of(TICKS_PER_HOUR) {
            self.plan_hydro(region, month, hour);
            if self.protection.mode == GridMode::Normal {
                self.resize_outage(region, month, hour);
            }
        }

        let error = self.noise.step(region.demand(), rng);
        let demand = (self.connected_demand_forecast(region, month, tick) * (1.0 + error))
            .max(Megawatts::ZERO);

        let (available, curtailable, withheld) = self.availability(region, month, tick);

        self.apply_commitment(tick, &mut events);
        let mut came_online = false;
        for (id, plant) in &mut self.plants {
            let stepped = plant.step(available[id]);
            came_online |= plant.kind().is_dispatchable()
                && self.network.is_connected(plant.node())
                && stepped.contains(&DomainEvent::PlantOnline { plant: *id });
            events.extend(stepped);
        }
        if came_online && self.protection.mode == GridMode::Normal {
            // A plant whose start-up just completed can deliver this hour:
            // re-size it now rather than leaving its output with no load to
            // meet until the next boundary.
            self.resize_outage(region, month, hour);
        }

        let supply: Megawatts = self.connected_plants().map(Plant::output).sum();

        let mode = self.protection.mode;
        let shed = self.protection.shed.value();
        // The targets AGC sets now first act on the next tick's step.
        let effect_tick = tick + 1;
        let load = demand * (1.0 - self.outage.value());
        // No load energised this tick: fully cut by the outage in effect.
        let dark = mode == GridMode::Normal && load <= Megawatts::ZERO;
        if mode == GridMode::Normal && self.outage != self.outage_target {
            let served = load * (1.0 - shed);
            let cover = self.outage_cover(&available, served - supply - withheld, effect_tick);
            // Only a surplus that persists may be restored as load at once
            // (ruling N1): none while any plant is losing output AGC cannot
            // hold, judged as at the trip gate. The surplus is over the
            // outage's load, so protection's own restoration does not stack
            // on it.
            let falling =
                self.has_unrecoverable_ramp_down(None, &self.foreseen_ceilings(effect_tick));
            let surplus = if falling {
                Megawatts::ZERO
            } else {
                (supply - load).max(Megawatts::ZERO)
            };
            self.outage = self.next_outage(params.shed_step, demand, cover, surplus);
        }
        let to_serve = match mode {
            GridMode::Normal => (1.0 - self.outage.value()) * (1.0 - shed),
            // Pre-positioned for the next restoration step, which stops at
            // the frozen target (ruling I1).
            GridMode::Blackout => {
                let next =
                    restoration_step(self.protection.shed, params.shed_step, self.outage_target);
                1.0 - next.value()
            }
        };
        let redispatch = Redispatch {
            available: &available,
            to_serve: demand * to_serve,
            withheld,
            curtailable,
            effect_tick,
        };
        let leftover = self.redispatch(&redispatch);

        let imbalance = supply - load * (1.0 - shed);
        // Builder's call (controller rulings R8, I3 and M7): a fully cut grid
        // has no frequency to measure; the 1 MW system-size floor is for
        // numerical safety, not physics. So on a Normal-mode tick with no load
        // energised the deviation is held at nominal, as in a blackout, and
        // protection sees nominal frequency: over-frequency tracking resets
        // and disarms, so it cannot stay armed through the dark and trip on
        // the first lit tick. The mode stays Normal. Once the outage falls
        // below 1.0 (after a re-size, for example when a plant comes online)
        // the frequency model resumes.
        self.deviation = match mode {
            GridMode::Normal if !dark => next_deviation(self.deviation, imbalance, load, params),
            GridMode::Normal | GridMode::Blackout => Hertz::ZERO,
        };
        let outcome = evaluate(
            &mut self.protection,
            self.deviation,
            params,
            supply,
            load,
            self.outage_target,
        );
        match outcome {
            // All dark load is protection's; the target stays frozen.
            ProtectionOutcome::Blackout => self.outage = Ratio::ZERO,
            // Restoration stopped at the frozen target: that much stays out
            // as a rotating outage.
            // Normal mode resumes, so the hour is re-sized: plants started
            // during the blackout never fired a coming-online re-size.
            ProtectionOutcome::BlackoutRestored => {
                self.outage = self.protection.shed;
                self.protection.shed = Ratio::ZERO;
                self.resize_outage(region, month, hour);
            }
            _ => {}
        }
        let shed = self.protection.shed;
        let after_outage = demand * (1.0 - self.outage.value());
        if outcome.curtails() || self.curtailment > Megawatts::ZERO {
            self.update_curtailment(outcome, -leftover, curtailable, &mut events);
        }
        self.apply_protection(
            outcome,
            after_outage * shed.value(),
            after_outage * (1.0 - shed.value()),
            params,
            &redispatch,
            &mut events,
        );
        if mode == GridMode::Blackout && shed < Ratio::ONE {
            // Restoration has re-energised the grid: grid-following solar resynchronises.
            self.plants.values_mut().for_each(Plant::reconnect);
        }

        events.extend(self.outage_order(hour, ordered));
        let now = band(self.deviation, params);
        if now != self.band {
            self.band = now;
            events.push(DomainEvent::FrequencyDeviated { band: now });
        }

        // The summary reports instant t + 1, after protection acted.
        let by_plant: BTreeMap<PlantId, Megawatts> = self
            .connected_plants()
            .map(|p| (p.id(), p.output()))
            .collect();
        let mut by_kind: BTreeMap<PlantKind, Megawatts> = BTreeMap::new();
        for (id, output) in &by_plant {
            *by_kind
                .entry(self.plants[id].kind())
                .or_insert(Megawatts::ZERO) += *output;
        }
        let generation: Megawatts = by_plant.values().copied().sum();
        let served = after_outage * (1.0 - shed.value());
        let outage = self.outage.value();
        let summary = TickSummary {
            tick,
            demand,
            served,
            // `demand − served`, written so a tick without an outage reports
            // exactly `demand × shed`.
            unserved: demand * (outage + (1.0 - outage) * shed.value()),
            generation,
            curtailed: withheld,
            imbalance: generation - served,
            by_kind,
            by_plant,
            frequency: params.nominal + self.deviation,
            mode: self.protection.mode,
            shed,
            outage: self.outage,
        };
        events.push(DomainEvent::TickCompleted(summary.clone()));
        (summary, events)
    }

    /// Every plant's availability this tick, with the standing curtailment
    /// withheld pro rata from connected online solar. Also returns that solar's
    /// total availability before curtailment and the amount withheld.
    fn availability(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
    ) -> (BTreeMap<PlantId, Megawatts>, Megawatts, Megawatts) {
        let sun = sun_factor(region.solar(), month, tick);
        let mut available: BTreeMap<PlantId, Megawatts> = self
            .plants
            .values()
            .map(|p| (p.id(), self.available(region, p, sun)))
            .collect();
        let solar: Vec<PlantId> = self
            .connected_plants()
            .filter(|p| is_curtailable(p))
            .map(Plant::id)
            .collect();
        let curtailable: Megawatts = solar.iter().map(|id| available[id]).sum();
        let withheld = self.curtailment.min(curtailable);
        if withheld > Megawatts::ZERO {
            let keep = 1.0 - withheld / curtailable;
            for id in &solar {
                if let Some(mw) = available.get_mut(id) {
                    *mw = *mw * keep;
                }
            }
        }
        (available, curtailable, withheld)
    }

    /// Recomputes the solar curtailment from the surplus AGC cannot absorb (see
    /// [`PowerSystem::tick`]), forcing AGC plants to minimum on a `Curtail`
    /// tick when solar alone cannot cover it.
    fn update_curtailment(
        &mut self,
        outcome: ProtectionOutcome,
        surplus: Megawatts,
        curtailable: Megawatts,
        events: &mut Vec<DomainEvent>,
    ) {
        let mut needed = surplus.max(Megawatts::ZERO);
        if outcome.curtails() {
            if needed > curtailable {
                needed -= self.force_agc_to_minimum();
            }
            needed = needed.clamp(Megawatts::ZERO, curtailable);
            if needed > Megawatts::ZERO {
                events.push(DomainEvent::SolarCurtailed { mw: needed });
            }
        }
        self.curtailment = needed.min(curtailable);
    }

    /// Forces connected AGC plants to minimum stable output. Returns how far
    /// that lowers their next-tick output below the targets AGC just set.
    fn force_agc_to_minimum(&mut self) -> Megawatts {
        let network = &self.network;
        self.plants
            .values_mut()
            .filter(|p| p.auto_follow() && p.is_online() && network.is_connected(p.node()))
            .map(|p| {
                let before = match p.status() {
                    PlantStatus::Online { target, .. } => target,
                    _ => Megawatts::ZERO,
                };
                p.force_to_minimum();
                (before - p.spec().min_output()).max(Megawatts::ZERO)
            })
            .sum()
    }

    fn apply_commitment(&mut self, tick: u32, events: &mut Vec<DomainEvent>) {
        for action in commitment_actions(self.plants.values(), tick) {
            let Some(plant) = self.plants.get_mut(&action.plant()) else {
                continue;
            };
            let result = match action {
                CommitmentAction::Start(_) => plant.start(),
                CommitmentAction::Stop(_) => plant.stop(),
            };
            // Actions are edge-triggered on states that accept them.
            debug_assert!(result.is_ok(), "{action:?} rejected: {result:?}");
            events.extend(result.unwrap_or_default());
        }
    }

    /// Runs AGC over connected plants; returns the signed unallocated shortfall.
    ///
    /// **The hydro schedule caps AGC, not physics** (spec §12.4). A scheduled
    /// hydro plant on AGC is limited, in what AGC may ask of it, to a ceiling
    /// at or below its allocation for the tick `effect_tick` in which the new
    /// targets take effect (see [`PowerSystem::hydro_ceilings`]):
    ///
    /// - **Effect tick.** Plants step before AGC runs, so targets set on tick
    ///   t first act on tick t + 1's step; `effect_tick` is t + 1. Capping by
    ///   tick t's hour would leak one tick of the previous hour's dispatch
    ///   into every new hour, and overdraw a final hour that was planned to
    ///   spend exactly the remaining budget.
    /// - **Ramp envelope.** The ceiling is `min(hourly[h], hourly[h + 1] +
    ///   ramp × k)`, where `h` is the effect tick's hour and `k` the ticks
    ///   from the effect tick to the next hour boundary. So a foreseen fall
    ///   starts early and the output reaches the next hour's allocation
    ///   exactly at the boundary: a foreseen fall never spills a ramp tail
    ///   into the next hour, so a plan that ends with the water never ramps
    ///   down with nothing behind it (which the budget limit would cut off in
    ///   one tick). Hour 23 has no next-hour term; the next day
    ///   re-initialises.
    ///
    /// AGC's view of the plant's availability becomes `min(availability,
    /// ceiling)`, so AGC never raises its target above the ceiling. The
    /// physical availability passed to `Plant::step` is untouched and keeps
    /// only the budget limit. When the ceiling is below the plant's output
    /// (the envelope's own fall, or an unforeseen drop after a re-plan), its
    /// target is set to `max(ceiling, output − step)` before AGC runs, where
    /// `step` is its ramp: the plant ramps down at its ramp rate instead of
    /// dropping in one tick, and the output it gives up next tick is added to
    /// the shortfall for the rest of the fleet. AGC leaves such a plant out for
    /// this tick.
    ///
    /// The cap is lifted, leaving only the budget limit, while load is shed or
    /// the band is `Shedding` (see [`PowerSystem::hydro_cap_lifted`]). Hydro
    /// with no plan for the hour, and manual hydro, are never capped.
    ///
    /// **Recovery (builder's call): the cap returns at a rate the rest of the
    /// fleet can follow.** Hydro that ran above its allocation during a lift
    /// would otherwise fall back at its own ramp on the first tick nothing is
    /// shed, faster than slow thermal can pick up, and shed load again. So when
    /// a lift ends, the system enters recovery (see [`HydroOverride`] and
    /// [`PowerSystem::next_hydro_override`]). While recovering, the plants
    /// above their ceiling may give back in total no more than the cover:
    /// the other AGC plants' one-tick headroom (see
    /// [`PowerSystem::agc_headroom`], the over-frequency trip gate's
    /// definition) less the shortfall they already face, never below zero, so
    /// a surplus counts as cover as it does at the gate. Each plant's `step` is
    /// `min(ramp, cover left)`, shared in plant-id order: the lowest id takes
    /// its full ramp first. Recovery lasts while any scheduled plant is above
    /// the lowest ceiling it meets before the next re-plan (the trip gate's
    /// [`PowerSystem::foreseen_ceilings`]), so a foreseen fall that follows
    /// is limited too, and it ends at the day boundary. A new emergency
    /// simply lifts the cap again. Recovery may spend water beyond the plan;
    /// the budget limit still binds (spec §12.4). Outside recovery the rules
    /// above are unchanged.
    fn retarget_agc(
        &mut self,
        available: &BTreeMap<PlantId, Megawatts>,
        shortfall: Megawatts,
        effect_tick: u32,
    ) -> Megawatts {
        let ceilings = self.agc_ceilings(effect_tick);
        self.hydro_override = self.next_hydro_override(effect_tick);
        // In recovery, what the others can pick up next tick, net of the
        // shortfall they already face; a surplus needs no cover.
        let mut cover = (self.hydro_override == HydroOverride::Recovering).then(|| {
            (self.agc_headroom(available, &ceilings, None) - shortfall).max(Megawatts::ZERO)
        });
        let mut shortfall = shortfall;
        let network = &self.network;
        let mut connected: Vec<&mut Plant> = Vec::new();
        for plant in self
            .plants
            .values_mut()
            .filter(|p| network.is_connected(p.node()))
        {
            let output = plant.output();
            match ceilings.get(&plant.id()) {
                Some(&ceiling) if plant.is_online() && output > ceiling => {
                    let spec = plant.spec();
                    let step = cover.map_or(spec.ramp.per_tick(), |c| spec.ramp.per_tick().min(c));
                    let target = (output - step)
                        .max(ceiling)
                        .clamp(spec.min_output(), spec.capacity);
                    let result = plant.set_target(target);
                    debug_assert!(result.is_ok(), "ramp-down {target:?} rejected: {result:?}");
                    if let Some(c) = cover.as_mut() {
                        *c -= output - target;
                    }
                    shortfall += output - target;
                }
                _ => connected.push(plant),
            }
        }
        let caps: BTreeMap<PlantId, Megawatts> = connected
            .iter()
            .map(|p| (p.id(), agc_cap(available, &ceilings, p.id())))
            .collect();
        agc_retarget(&mut connected, &caps, shortfall)
    }

    /// The one-tick ramp-up headroom of the connected auto-follow plants
    /// other than `except`: for each, exactly what AGC could give it next
    /// tick, `min(ramp, min(available, hydro ceiling) − output)` (see
    /// [`Plant::headroom_up`]), summed. Offline and non-dispatchable plants,
    /// and scheduled hydro above its ceiling, have none. Used by both the
    /// over-frequency trip gate and the hydro recovery phase.
    fn agc_headroom(
        &self,
        available: &BTreeMap<PlantId, Megawatts>,
        ceilings: &BTreeMap<PlantId, Megawatts>,
        except: Option<PlantId>,
    ) -> Megawatts {
        self.connected_plants()
            .filter(|p| Some(p.id()) != except && p.auto_follow())
            .map(|p| p.headroom_up(agc_cap(available, ceilings, p.id())))
            .sum()
    }

    /// The one-tick ramp-down room of the connected auto-follow plants: for
    /// each, `min(ramp, output − min_output)` (see [`Plant::headroom_down`]),
    /// summed; offline and non-dispatchable plants have none. Hydro ceilings
    /// cap only upward moves, so they play no part. The downward
    /// counterpart of [`PowerSystem::agc_headroom`], used to pace a
    /// rotating-outage cut (see [`PowerSystem::next_outage`]).
    fn agc_headroom_down(&self) -> Megawatts {
        self.connected_plants()
            .filter(|p| p.auto_follow())
            .map(Plant::headroom_down)
            .sum()
    }

    /// Runs step 5's AGC on the current outputs: the requirement is
    /// `to_serve − supply − withheld`, with `supply` the connected output now.
    /// Returns the signed unallocated shortfall (see
    /// [`PowerSystem::retarget_agc`]). Every pass computes from the current
    /// outputs alone, so a second pass replaces the first's targets rather
    /// than adding to them.
    fn redispatch(&mut self, r: &Redispatch<'_>) -> Megawatts {
        let supply: Megawatts = self.connected_plants().map(Plant::output).sum();
        self.retarget_agc(r.available, r.to_serve - supply - r.withheld, r.effect_tick)
    }

    /// The hydro ceilings AGC respects for targets acting on `effect_tick`:
    /// none while the cap is lifted (see [`PowerSystem::hydro_cap_lifted`]).
    fn agc_ceilings(&self, effect_tick: u32) -> BTreeMap<PlantId, Megawatts> {
        if self.hydro_cap_lifted() {
            BTreeMap::new()
        } else {
            self.hydro_ceilings(effect_tick)
        }
    }

    /// Acts on the protection `outcome`. `shed_mw` is the load now shed,
    /// `served` the load now served and `redispatch` this tick's AGC inputs.
    fn apply_protection(
        &mut self,
        outcome: ProtectionOutcome,
        shed_mw: Megawatts,
        served: Megawatts,
        params: &FrequencyParams,
        redispatch: &Redispatch<'_>,
        events: &mut Vec<DomainEvent>,
    ) {
        match outcome {
            ProtectionOutcome::Shed => events.push(DomainEvent::LoadShed { mw: shed_mw }),
            ProtectionOutcome::Blackout => {
                events.push(DomainEvent::Blackout);
                let ids: Vec<PlantId> = self.plants.keys().copied().collect();
                for id in ids {
                    events.extend(self.trip_plant(id));
                    if let Some(plant) = self.plants.get_mut(&id) {
                        plant.suppress_commitment_for_day();
                    }
                }
                self.deviation = Hertz::ZERO;
                self.curtailment = Megawatts::ZERO;
            }
            ProtectionOutcome::BlackoutRestored => events.push(DomainEvent::BlackoutRestored),
            ProtectionOutcome::OverFrequencyTrip => {
                self.over_frequency_trip(served, params, redispatch, events);
            }
            // Curtailment is handled by `update_curtailment`.
            ProtectionOutcome::None | ProtectionOutcome::Restored | ProtectionOutcome::Curtail => {}
        }
    }

    /// Trips the [`PowerSystem::over_frequency_trip_candidate`] (spec §14.2)
    /// exactly as after a blackout: [`PowerSystem::trip_plant`] plus
    /// `suppress_commitment_for_day`, emitting `OverFrequencyTrip` before its
    /// `PlantTripped`. Nothing happens when no plant qualifies.
    ///
    /// **Headroom gate (builder's call): protection never trips into a deficit
    /// the fleet can't cover in one tick.** The trip is withheld when the
    /// deficit it would open, the candidate's output less the current surplus
    /// over `served`, exceeds the one-tick ramp-up headroom of the *other*
    /// connected online auto-follow plants. Each plant's headroom is exactly
    /// what AGC could give it next tick: `min(ramp, ceiling − output)`, where
    /// the ceiling is its availability lowered to its hydro ceiling (see
    /// [`PowerSystem::retarget_agc`]). Frequency lags the imbalance by the
    /// 5-tick damping time constant, so a trip sized only by frequency can
    /// overshoot a nearly cleared surplus into load shedding or blackout. A
    /// withheld trip stays due (see [`ProtectionState::withhold_trip`]) and is
    /// re-checked on the next counting tick; the cost-first order is
    /// unchanged, so a cheaper unit is never tripped in the candidate's place.
    /// A residual surplus no unit can safely clear therefore stays as
    /// over-frequency, which the frequency-performance rules penalise (spec
    /// §14.4).
    ///
    /// **Falling-output gate (builder's call): protection never trips while
    /// the fleet is already losing output it can't recover, or is scheduled
    /// to before the next hydro re-plan; the trip waits until that motion
    /// ends.** The headroom gate credits one tick of ramp, but a hydro plant
    /// following its schedule's ramp envelope, a plant stopping under its
    /// commitment or a manual plant ramping to a lower target keeps falling
    /// for several ticks whatever AGC does, so a trip during that fall, or
    /// just before a foreseen one, can outrun the gate (see
    /// [`PowerSystem::has_unrecoverable_ramp_down`]). Such a trip is withheld
    /// like an unsafe one. The hydro lookahead runs to the next hour boundary,
    /// as far as the plan is known, so it is deliberately conservative: on the
    /// tick before each boundary it spans the whole next hour, so holds can
    /// chain across consecutive falling hours and a trip may wait for as long
    /// as hydro keeps being scheduled to fall.
    ///
    /// **Redispatch.** AGC already set next tick's targets (step 5) before
    /// the trip, so after a trip it runs again on the post-trip outputs with
    /// the same availability and hydro ceilings. The rest of the fleet then
    /// ramps on the next tick's step, which is the one tick of ramp the gate
    /// credits. The curtailment is recomputed from what that pass cannot
    /// absorb, without forcing AGC to minimum again. Forcing that already
    /// happened this tick (the curtail response comes before any trip) stays:
    /// it acted on outputs before the trip was sized, so the gate measured the
    /// post-forcing surplus, and the redispatch raises the forced plants from
    /// their minimum within their ramp.
    fn over_frequency_trip(
        &mut self,
        served: Megawatts,
        params: &FrequencyParams,
        redispatch: &Redispatch<'_>,
        events: &mut Vec<DomainEvent>,
    ) {
        let Some(id) = self.over_frequency_trip_candidate() else {
            return;
        };
        let surplus = self
            .connected_plants()
            .map(Plant::output)
            .sum::<Megawatts>()
            - served;
        let ceilings = self.agc_ceilings(redispatch.effect_tick);
        let headroom = self.agc_headroom(redispatch.available, &ceilings, Some(id));
        if self.plants[&id].output() - surplus > headroom
            || self.has_unrecoverable_ramp_down(
                Some(id),
                &self.foreseen_ceilings(redispatch.effect_tick),
            )
        {
            self.protection.withhold_trip(params);
            return;
        }
        events.push(DomainEvent::OverFrequencyTrip { plant: id });
        events.extend(self.trip_plant(id));
        if let Some(plant) = self.plants.get_mut(&id) {
            plant.suppress_commitment_for_day();
        }
        let leftover = self.redispatch(redispatch);
        self.update_curtailment(
            ProtectionOutcome::None,
            -leftover,
            redispatch.curtailable,
            events,
        );
        // The tick reports the curtailment it finally set.
        events.retain(|e| !matches!(e, DomainEvent::SolarCurtailed { .. }));
        if self.curtailment > Megawatts::ZERO {
            events.push(DomainEvent::SolarCurtailed {
                mw: self.curtailment,
            });
        }
    }

    /// The lowest AGC hydro ceiling of each scheduled plant over the effect
    /// ticks from `effect_tick` through the first effect tick of the next
    /// hour (see [`PowerSystem::agc_ceilings`]; empty while the cap is
    /// lifted). The plan is known only that far: it is re-planned at the
    /// next hour boundary. Hour 23 has no next-hour term, as in
    /// [`PowerSystem::hydro_ceilings`].
    fn foreseen_ceilings(&self, effect_tick: u32) -> BTreeMap<PlantId, Megawatts> {
        let next_hour = (effect_tick / TICKS_PER_HOUR + 1) * TICKS_PER_HOUR;
        let mut lowest: BTreeMap<PlantId, Megawatts> = BTreeMap::new();
        for e in effect_tick..=next_hour {
            for (id, ceiling) in self.agc_ceilings(e) {
                lowest
                    .entry(id)
                    .and_modify(|mw| *mw = mw.min(ceiling))
                    .or_insert(ceiling);
            }
        }
        lowest
    }

    /// Whether a connected plant other than `except` is losing output, or is
    /// scheduled to, that the post-trip AGC redispatch cannot reverse (see
    /// [`PowerSystem::over_frequency_trip`]):
    ///
    /// - a scheduled hydro plant on AGC above the lowest hydro ceiling it
    ///   meets before the next re-plan, `ceilings` (see
    ///   [`PowerSystem::foreseen_ceilings`]), which AGC ramps down rather
    ///   than retargets (see [`PowerSystem::retarget_agc`]);
    /// - a plant stopping under its commitment, still producing;
    /// - a manual dispatchable plant above its target.
    ///
    /// An AGC plant ramping down toward its AGC target does not count: the
    /// redispatch retargets it. Also used, with no exception, to keep a
    /// rotating-outage restore from taking a surplus that is about to vanish
    /// (see [`PowerSystem::next_outage`]).
    fn has_unrecoverable_ramp_down(
        &self,
        except: Option<PlantId>,
        ceilings: &BTreeMap<PlantId, Megawatts>,
    ) -> bool {
        self.connected_plants()
            .filter(|p| Some(p.id()) != except)
            .any(|p| match p.status() {
                PlantStatus::Stopping { output } => output > Megawatts::ZERO,
                PlantStatus::Online { output, .. } if p.auto_follow() => {
                    ceilings.get(&p.id()).is_some_and(|c| output > *c)
                }
                PlantStatus::Online { output, target } => {
                    p.kind().is_dispatchable() && output > target
                }
                _ => false,
            })
    }

    /// The plant an over-frequency trip takes (spec §14.2): the connected,
    /// online, dispatchable plant producing more than 0 MW with the highest
    /// marginal cost, ties going to the lowest plant id. Solar is never a
    /// candidate, since curtailment covers it. `None` when no plant qualifies.
    fn over_frequency_trip_candidate(&self) -> Option<PlantId> {
        self.connected_plants()
            .filter(|p| p.is_online() && p.kind().is_dispatchable() && p.output() > Megawatts::ZERO)
            .max_by(|a, b| {
                a.spec()
                    .marginal_cost
                    .value()
                    .total_cmp(&b.spec().marginal_cost.value())
                    .then_with(|| b.id().cmp(&a.id()))
            })
            .map(Plant::id)
    }

    /// Turns AGC on or off for plant `id` (see [`Plant::set_auto_follow`]).
    /// Turning it off unplans the plant's hydro schedule, so manual hydro is
    /// never capped and is still unplanned if AGC comes back on before the
    /// next hourly re-plan (spec §12.3 step 4, §12.4). Never re-plans. An
    /// `Err` leaves the system unchanged.
    pub fn set_auto_follow(&mut self, id: PlantId, on: bool) -> Result<(), DomainError> {
        let plant = self
            .plants
            .get_mut(&id)
            .ok_or(DomainError::UnknownPlant { plant: id })?;
        plant.set_auto_follow(on)?;
        if !on {
            self.unplan_hydro(id);
        }
        Ok(())
    }

    /// Trips plant `id` (see [`Plant::trip`]). Every trip goes through here: a
    /// plant that actually tripped loses its hydro plan, so once restarted it
    /// is unplanned (uncapped) until the next hourly re-plan (spec §12.3 step
    /// 4). An unknown id or a plant that was not running is a no-op.
    pub(super) fn trip_plant(&mut self, id: PlantId) -> Vec<DomainEvent> {
        let events = self
            .plants
            .get_mut(&id)
            .map(Plant::trip)
            .unwrap_or_default();
        if !events.is_empty() {
            self.unplan_hydro(id);
        }
        events
    }

    /// Expected (noise-free) demand of connected cities at `tick`.
    pub fn connected_demand_forecast(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
    ) -> Megawatts {
        forecast_demand(self.connected_cities(), region.demand(), month, tick)
    }

    /// Expected output of connected commissioned solar at `tick`, using the
    /// month's mean clearness rather than today's draw.
    pub fn expected_solar(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
    ) -> Megawatts {
        let sun = sun_factor(region.solar(), month, tick);
        let clearness = region.solar().clearness_mean[month.index0()];
        self.connected_plants()
            .filter(|p| p.kind() == PlantKind::Solar)
            .map(|p| {
                let irradiance = cell_ratio(region, p.cell(), |c| c.irradiance);
                solar_available(p.spec().capacity, irradiance, sun, clearness)
            })
            .sum()
    }

    /// How much connected dispatchable output could rise within `within`.
    /// Online plants contribute `min(ramp over w, capacity − output)`; plants
    /// starting with `ticks_left ≤ w` contribute `min_output + ramp over
    /// (w − ticks_left)`, capped at capacity.
    pub fn ramp_capability_up(&self, within: SimDuration) -> Megawatts {
        let w = within.ticks();
        self.connected_plants()
            .filter(|p| p.kind().is_dispatchable())
            .map(|p| {
                let spec = p.spec();
                match p.status() {
                    PlantStatus::Online { output, .. } => spec
                        .ramp
                        .over_ticks(w)
                        .min(spec.capacity - output)
                        .max(Megawatts::ZERO),
                    PlantStatus::Starting { ticks_left } if ticks_left <= w => (spec.min_output()
                        + spec.ramp.over_ticks(w - ticks_left))
                    .min(spec.capacity),
                    _ => Megawatts::ZERO,
                }
            })
            .sum()
    }

    /// Σ capacity × capacity credit over commissioned plants.
    pub fn firm_capacity(&self) -> Megawatts {
        self.plants
            .values()
            .filter(|p| is_commissioned(p))
            .map(|p| p.spec().capacity_credit.of(p.spec().capacity))
            .sum()
    }

    /// The highest hourly mean of the summed connected-city profiles over the
    /// twelve calendar months from `first_month`, with today's connectivity.
    pub fn projected_connected_peak(
        &self,
        region: &RegionDefinition,
        first_month: SimMonth,
    ) -> Megawatts {
        let cities: Vec<&DemandCenter> = self.connected_cities().collect();
        let mut month = first_month;
        let mut peak = Megawatts::ZERO;
        for _ in 0..MONTHS_PER_YEAR {
            for point in hourly_forecast(cities.iter().copied(), region.demand(), month) {
                peak = peak.max(point.mean);
            }
            month = month.next();
        }
        peak
    }
}

/// A terrain ratio at `cell`, zero off the map.
fn cell_ratio(region: &RegionDefinition, cell: CellCoord, field: impl Fn(&Cell) -> Ratio) -> Ratio {
    region.terrain().get(cell).map_or(Ratio::ZERO, field)
}

#[cfg(test)]
mod tests;
