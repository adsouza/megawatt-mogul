//! The power system aggregate: plants, cities and the network, advanced
//! together one tick at a time.
//!
//! Only plants and cities on connected nodes take part in the balance.
//! Disconnected plants still progress physically (they start, ramp and spend
//! hydro budget) but supply nothing; disconnected cities are not customers.

use std::collections::BTreeMap;

use super::dispatch::agc_retarget;
use super::frequency::{FrequencyBand, band, next_deviation};
use super::network::Network;
use super::protection::{GridMode, ProtectionOutcome, ProtectionState, evaluate};
use super::summary::TickSummary;
use crate::demand::{DemandCenter, DemandNoise, forecast_demand, hourly_forecast};
use crate::events::DomainEvent;
use crate::generation::{
    BoundaryStart, CommitmentAction, Plant, PlantStatus, commitment_actions, draw_clearness,
    solar_available, sun_factor,
};
use crate::ids::{CellCoord, CityId, NodeId, PlantId};
use crate::region::{Cell, PlantKind, RegionDefinition};
use crate::rng::SimRng;
use crate::time::{SimDuration, SimMonth};
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
}

fn is_curtailable(plant: &Plant) -> bool {
    plant.kind() == PlantKind::Solar && plant.is_online()
}

fn is_commissioned(plant: &Plant) -> bool {
    !matches!(plant.status(), PlantStatus::UnderConstruction { .. })
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
    /// forecast error and solar curtailment return to zero.
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
        DayInitialisation { boundary_starts }
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
    ///    so the cheaper solar returns as thermal ramps down.
    /// 6. Imbalance between connected supply and served load.
    /// 7. Frequency update, held at nominal in a blackout. System size is the
    ///    connected demand.
    /// 8. Protection: load shedding; a blackout that trips every plant,
    ///    suppresses its commitment for the day and resets the deviation and
    ///    the curtailment; and solar curtailment, described below. Tripped
    ///    solar reconnects once restoration first re-energises the grid.
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
    /// what forcing removes.
    pub fn tick(
        &mut self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
        rng: &mut SimRng,
    ) -> (TickSummary, Vec<DomainEvent>) {
        let params = region.frequency();
        let mut events = Vec::new();

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
        let leftover = self.retarget_agc(&available, demand * to_serve - supply - withheld);

        let imbalance = supply - demand * (1.0 - shed);
        self.deviation = match mode {
            GridMode::Normal => next_deviation(self.deviation, imbalance, demand, params),
            GridMode::Blackout => Hertz::ZERO,
        };

        let outcome = evaluate(&mut self.protection, self.deviation, params, supply, demand);
        let shed = self.protection.shed;
        if outcome == ProtectionOutcome::Curtail || self.curtailment > Megawatts::ZERO {
            self.update_curtailment(outcome, -leftover, curtailable, &mut events);
        }
        self.apply_protection(outcome, demand * shed.value(), &mut events);
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
        if outcome == ProtectionOutcome::Curtail {
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
    fn retarget_agc(
        &mut self,
        available: &BTreeMap<PlantId, Megawatts>,
        shortfall: Megawatts,
    ) -> Megawatts {
        let network = &self.network;
        let mut connected: Vec<&mut Plant> = self
            .plants
            .values_mut()
            .filter(|p| network.is_connected(p.node()))
            .collect();
        let caps: BTreeMap<PlantId, Megawatts> = connected
            .iter()
            .map(|p| (p.id(), available[&p.id()]))
            .collect();
        agc_retarget(&mut connected, &caps, shortfall)
    }

    fn apply_protection(
        &mut self,
        outcome: ProtectionOutcome,
        shed_mw: Megawatts,
        events: &mut Vec<DomainEvent>,
    ) {
        match outcome {
            ProtectionOutcome::Shed => events.push(DomainEvent::LoadShed { mw: shed_mw }),
            ProtectionOutcome::Blackout => {
                events.push(DomainEvent::Blackout);
                for plant in self.plants.values_mut() {
                    events.extend(plant.trip());
                    plant.suppress_commitment_for_day();
                }
                self.deviation = Hertz::ZERO;
                self.curtailment = Megawatts::ZERO;
            }
            ProtectionOutcome::BlackoutRestored => events.push(DomainEvent::BlackoutRestored),
            // Curtailment is handled by `update_curtailment`.
            ProtectionOutcome::None | ProtectionOutcome::Restored | ProtectionOutcome::Curtail => {}
        }
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
