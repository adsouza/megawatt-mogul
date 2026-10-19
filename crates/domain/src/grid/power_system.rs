//! The power system aggregate: plants, cities and the network, advanced
//! together one tick at a time.
//!
//! Only plants and cities on connected nodes take part in the balance.
//! Disconnected plants still progress physically (they start, ramp and spend
//! hydro budget) but supply nothing; disconnected cities are not customers.

use std::collections::BTreeMap;

mod hydro_plan;

use hydro_plan::PlantHydroPlan;
pub use hydro_plan::{HydroScheduleView, PlantHydroScheduleView};

use super::dispatch::{agc_retarget, boundary_dispatch};
use super::frequency::{FrequencyBand, band, next_deviation};
use super::network::Network;
use super::protection::{GridMode, ProtectionOutcome, ProtectionState, evaluate};
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

/// What starting a representative day reported to the session accumulator.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DayInitialisation {
    /// One-off month-boundary starts per plant (see [`BoundaryStart::Charged`]).
    pub boundary_starts: BTreeMap<PlantId, u32>,
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
    /// The hour the hydro plan was last made from.
    hydro_planned_from: u8,
}

fn is_curtailable(plant: &Plant) -> bool {
    plant.kind() == PlantKind::Solar && plant.is_online()
}

fn is_commissioned(plant: &Plant) -> bool {
    !matches!(plant.status(), PlantStatus::UnderConstruction { .. })
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

    /// Starts a representative day of `month`. Nothing physical carries over
    /// from the previous day: each commissioned hydro plant's budget is reset
    /// to `capacity × 24 h × hydro_potential(cell) × monthly_flow[month]`, the
    /// day's clearness is drawn (exactly one normal), every plant is
    /// initialised from its cyclic pattern (clearing operating overrides),
    /// frequency and protection return to nominal and `Normal`, and the demand
    /// forecast error and solar curtailment return to zero. The hydro
    /// schedule is then planned over the whole day (spec §12.3). Last, the
    /// auto-follow fleet is dispatched to balance the tick-0 load (the cyclic
    /// day's boundary state), so a day does not open in a deficit that
    /// ramp-limited AGC cannot recover; plants on manual targets keep theirs.
    pub fn begin_day(
        &mut self,
        region: &RegionDefinition,
        month: SimMonth,
        rng: &mut SimRng,
    ) -> DayInitialisation {
        let flow = region.hydro().monthly_flow[month.index0()];
        for plant in self.plants.values_mut() {
            if plant.kind() == PlantKind::Hydro && is_commissioned(plant) {
                let potential = cell_ratio(region, plant.cell(), |c| c.hydro_potential);
                let daily =
                    plant.spec().capacity.value() * HOURS_PER_DAY * potential.value() * flow;
                plant.reset_hydro_budget(MegawattHours::new(daily));
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
        self.plan_hydro(region, month, 0);
        self.balance_boundary(region, month);
        DayInitialisation { boundary_starts }
    }

    /// Sets the dispatch state at instant 0 as the end of the previous,
    /// identical day left it: the auto-follow fleet fills the tick-0 forecast
    /// in merit order (see [`boundary_dispatch`]). The load is the connected
    /// demand forecast less expected solar and less the output of connected
    /// online plants that are not auto-follow, which keep their initial
    /// target. Scheduled hydro is capped at its hour-0 allocation. Draws no
    /// random numbers.
    fn balance_boundary(&mut self, region: &RegionDefinition, month: SimMonth) {
        let manual: Megawatts = self
            .connected_plants()
            .filter(|p| p.is_online() && !p.auto_follow())
            .map(Plant::output)
            .sum();
        let forecast = self.connected_demand_forecast(region, month, 0);
        let solar = self.expected_solar(region, month, 0);
        let load = forecast - solar - manual;
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
    /// 1. Demand of connected cities: forecast × (1 + forecast error). The
    ///    error advances by exactly one normal draw every tick, in any state.
    /// 2. Solar and hydro availability for every plant. The standing solar
    ///    curtailment is withheld from connected online solar pro rata to each
    ///    plant's availability.
    /// 3. The commitment actions due at `tick` (suppressed plants get none).
    /// 4. Every plant steps once.
    /// 5. AGC re-targets connected auto-follow plants toward the load to serve:
    ///    `demand × (1 − shed)`, or in a blackout `demand × min(1, 1 − shed +
    ///    shed_step)` so supply is pre-positioned for the next restoration step.
    ///    While solar is curtailed, AGC also makes room for the withheld solar,
    ///    so the cheaper solar returns as thermal ramps down. Scheduled hydro
    ///    is held to its hourly allocation (see [`PowerSystem::retarget_agc`]).
    /// 6. Imbalance between connected supply and served load.
    /// 7. Frequency update, held at nominal in a blackout. System size is the
    ///    connected demand.
    /// 8. Protection: load shedding; a blackout that trips every plant,
    ///    suppresses its commitment for the day and resets the deviation and
    ///    the curtailment; solar curtailment, described below; and an
    ///    over-frequency trip of one plant, after which AGC re-runs step 5 on
    ///    the post-trip outputs (see [`PowerSystem::over_frequency_trip`]).
    ///    Tripped solar reconnects once restoration first re-energises the
    ///    grid.
    /// 9. The summary of the state after protection (see [`TickSummary`]),
    ///    also emitted last as `TickCompleted`. Steps 5–8 decide on the supply
    ///    measured after step 4.
    ///
    /// **Solar curtailment** is physical: it lowers solar availability from
    /// the next tick. It engages on an over-frequency `Curtail` outcome and is
    /// then recomputed every tick as the surplus AGC cannot absorb within its
    /// one-tick ramp (the negated leftover of step 5), clamped to the solar
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
        let hour = tick / TICKS_PER_HOUR;
        if tick > 0 && tick.is_multiple_of(TICKS_PER_HOUR) {
            self.plan_hydro(region, month, u8::try_from(hour).unwrap_or(u8::MAX));
        }

        let error = self.noise.step(region.demand(), rng);
        let demand = (self.connected_demand_forecast(region, month, tick) * (1.0 + error))
            .max(Megawatts::ZERO);

        let (available, curtailable, withheld) = self.availability(region, month, tick);

        self.apply_commitment(tick, &mut events);
        for (id, plant) in &mut self.plants {
            events.extend(plant.step(available[id]));
        }

        let supply: Megawatts = self.connected_plants().map(Plant::output).sum();

        let mode = self.protection.mode;
        let shed = self.protection.shed.value();
        let to_serve = match mode {
            GridMode::Normal => 1.0 - shed,
            GridMode::Blackout => (1.0 - shed + params.shed_step.value()).min(1.0),
        };
        // The targets AGC sets now first act on the next tick's step.
        let redispatch = Redispatch {
            available: &available,
            to_serve: demand * to_serve,
            withheld,
            curtailable,
            effect_tick: tick + 1,
        };
        let leftover = self.redispatch(&redispatch);

        let imbalance = supply - demand * (1.0 - shed);
        self.deviation = match mode {
            GridMode::Normal => next_deviation(self.deviation, imbalance, demand, params),
            GridMode::Blackout => Hertz::ZERO,
        };

        let outcome = evaluate(&mut self.protection, self.deviation, params, supply, demand);
        let shed = self.protection.shed;
        if outcome.curtails() || self.curtailment > Megawatts::ZERO {
            self.update_curtailment(outcome, -leftover, curtailable, &mut events);
        }
        self.apply_protection(
            outcome,
            demand * shed.value(),
            demand * (1.0 - shed.value()),
            params,
            &redispatch,
            &mut events,
        );
        if mode == GridMode::Blackout && shed < Ratio::ONE {
            // Restoration has re-energised the grid: grid-following solar resynchronises.
            self.plants.values_mut().for_each(Plant::reconnect);
        }

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
        let served = demand * (1.0 - shed.value());
        let summary = TickSummary {
            tick,
            demand,
            served,
            unserved: demand * shed.value(),
            generation,
            curtailed: withheld,
            imbalance: generation - served,
            by_kind,
            by_plant,
            frequency: params.nominal + self.deviation,
            mode: self.protection.mode,
            shed,
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
    ///   one tick). Hour 23
    ///   has no next-hour term; the next day re-initialises.
    ///
    /// AGC's view of the plant's availability becomes `min(availability,
    /// ceiling)`, so AGC never raises its target above the ceiling. The
    /// physical availability passed to `Plant::step` is untouched and keeps
    /// only the budget limit. When the ceiling is below the plant's output
    /// (the envelope's own fall, or an unforeseen drop after a re-plan), its
    /// target is set to `max(ceiling, output − ramp)` before AGC runs: the
    /// plant ramps down at its ramp rate instead of dropping in one tick, and
    /// the output it gives up next tick is added to the shortfall for the rest
    /// of the fleet. AGC leaves such a plant out for this tick.
    ///
    /// The cap is lifted, leaving only the budget limit, while load is shed or
    /// the band is `Shedding` (see [`PowerSystem::hydro_cap_lifted`]). Hydro
    /// with no plan for the hour, and manual hydro, are never capped.
    fn retarget_agc(
        &mut self,
        available: &BTreeMap<PlantId, Megawatts>,
        shortfall: Megawatts,
        effect_tick: u32,
    ) -> Megawatts {
        let ceilings = self.agc_ceilings(effect_tick);
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
                Some(&allocation) if plant.is_online() && output > allocation => {
                    let spec = plant.spec();
                    let target = (output - spec.ramp.per_tick())
                        .max(allocation)
                        .clamp(spec.min_output(), spec.capacity);
                    let result = plant.set_target(target);
                    debug_assert!(result.is_ok(), "ramp-down {target:?} rejected: {result:?}");
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
        let headroom: Megawatts = self
            .connected_plants()
            .filter(|p| p.id() != id && p.auto_follow())
            .map(|p| p.headroom_up(agc_cap(redispatch.available, &ceilings, p.id())))
            .sum();
        if self.plants[&id].output() - surplus > headroom {
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
