//! The game session: the aggregate root that owns one game's whole state and
//! accepts the player's commands.
//!
//! A played month is one representative day in three phases: **Planning**
//! (build, commit and set initial targets), **Operating** (tick through the
//! day's 1,440 minutes, with manual overrides), and **Review** (the month is
//! closed). Every command checks first that the game is still active, then
//! that the phase allows it, and validates fully before mutating anything, so
//! a command that returns `Err` leaves [`GameSession::state_hash`] unchanged.

mod activation;
#[cfg(test)]
mod battery_tests;
mod commands;
mod connection;
#[cfg(test)]
mod forecast_contract_tests;
mod geothermal;
pub use geothermal::DrySite;
mod invariants;
mod links;
mod month_close;
mod report;
mod settings;
mod views;
#[cfg(test)]
mod wind_tests;

#[cfg(test)]
mod tests;

pub use activation::{ActivationAssessment, ActivationBlocker};
pub use commands::{ExplorationQuote, LineQuote, PlantQuote};
pub use connection::{ConnectionKind, ConnectionWarning};
pub use invariants::InvariantViolation;
pub use report::{
    Incident, IncidentKind, IslandMonthReport, MeritOrderEntry, MonthAccumulator, MonthlyReport,
};
pub use settings::{GameSettings, SimulationFidelity};
pub use views::{
    BatteryView, CapacityShortfallWarning, CityDetail, CityView, CompanyView, GridSnapshot,
    LineView, MinimumGenerationPoint, MinimumGenerationWarning, PlantView, RateReviewEstimate,
    WindView,
};

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use crate::company::{AssetProfile, AssetRef, Company};
use crate::demand::DemandCenter;
use crate::error::DomainError;
use crate::events::DomainEvent;
use crate::generation::{CommitmentSchedule, Plant};
use crate::grid::{LineStatus, Network, Node, NodeKind, PowerSystem, TickSummary};
use crate::ids::{CellCoord, CityId, IdAllocator, NodeId, PlantId};
#[cfg(test)]
use crate::region::LineParams;
use crate::region::RegionDefinition;
use crate::regulation::Regulator;
use crate::rng::SimRng;
use crate::time::SimClock;
use crate::units::{Money, Ratio};

/// FNV-1a 64-bit offset basis and prime.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Where the session is within the played month.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayPhase {
    Planning,
    Operating,
    Review,
}

/// Why a game ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOverReason {
    Bankrupt,
    LicenseRevoked,
}

/// Whether the game still accepts commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Active,
    GameOver(GameOverReason),
}

/// One game's complete state.
///
/// `Debug` prints every field, including the RNG's seed and stream position,
/// because [`GameSession::state_hash`] is computed from it.
#[derive(Debug, Clone)]
pub struct GameSession {
    region: Arc<RegionDefinition>,
    settings: GameSettings,
    clock: SimClock,
    phase: DayPhase,
    status: SessionStatus,
    rng: SimRng,
    /// One counter for node, plant and line ids.
    ids: IdAllocator,
    /// The home city's existing node, which anchors network connectivity.
    anchor: NodeId,
    power: PowerSystem,
    company: Company,
    regulator: Regulator,
    /// Construction instalments still to pay, per asset, earliest first.
    instalments: BTreeMap<AssetRef, VecDeque<Money>>,
    /// The played day so far, for the month close.
    month_to_date: MonthAccumulator,
    /// Rate-base depreciation booked since the last annual rate review.
    year_depreciation: Money,
    /// Every tick summary of the current day, oldest first; cleared when the
    /// month closes.
    day_ticks: Vec<TickSummary>,
    geothermal_available: bool,
    dry_cells: BTreeMap<CellCoord, DrySite>,
}

impl GameSession {
    /// Starts a new game in `region`'s starting scenario.
    ///
    /// The home city's node remains headquarters. Optional inherited island homes anchor
    /// independent local grids; starter cities and plants connect only to their area's home.
    /// Optional inherited HVDC corridors join served city buses without merging AC grids;
    /// they start in service with zero schedules and enter the legacy rate base.
    /// Each start-fleet plant is commissioned on the start month on its own node
    /// with an in-service line to the home city, and is treated as
    /// established (not returning from a month offline), so month 1 charges
    /// no boundary start. The new operator inherits the previous operator's
    /// schedule, the hands-off plan (spec §16.3): every dispatchable
    /// start-fleet plant is committed all day with AGC on, and solar is left
    /// as commissioned. The start fleet and start lines enter the rate base as
    /// legacy capital at full cost. The game begins in `Planning` with the
    /// start cash.
    pub fn new(region: Arc<RegionDefinition>, seed: u64, settings: GameSettings) -> Self {
        let start = region.start();
        let month = start.month;
        let mut ids = IdAllocator::default();
        let home = region
            .cities()
            .iter()
            .find(|city| city.id == start.home_city)
            .expect("validated home city exists");
        let anchor: NodeId = ids.next();
        let mut network = Network::new(
            anchor,
            Node {
                kind: NodeKind::City(home.id),
                cell: home.cell,
            },
        );

        let mut cities = BTreeMap::new();
        for def in region.cities() {
            let node = if def.id == home.id {
                anchor
            } else {
                let node: NodeId = ids.next();
                add_start_node(&mut network, node, NodeKind::City(def.id), def.cell);
                if start.island_homes.is_empty() && start.connected_cities.contains(&def.id) {
                    add_start_line(&mut network, &mut ids, anchor, node);
                }
                node
            };
            cities.insert(def.id, DemandCenter::new(def.clone(), node));
        }

        if !start.island_homes.is_empty() {
            for def in region
                .cities()
                .iter()
                .filter(|c| start.connected_cities.contains(&c.id))
            {
                let local_home = region.start_home_at(def.cell).id;
                if def.id != local_home {
                    add_start_line(
                        &mut network,
                        &mut ids,
                        cities[&local_home].node(),
                        cities[&def.id].node(),
                    );
                }
            }
        }
        let mut plants = BTreeMap::new();
        for def in &start.fleet {
            let id: PlantId = ids.next();
            let node: NodeId = ids.next();
            add_start_node(&mut network, node, NodeKind::Plant(id), def.cell);
            add_start_line(
                &mut network,
                &mut ids,
                cities[&region.start_home_at(def.cell).id].node(),
                node,
            );
            let spec = region
                .plant_spec_at(def.kind, def.cell)
                .expect("validated starter construction terms");
            let mut plant = Plant::new_commissioned(id, node, def.cell, spec, month);
            if def.kind == crate::region::PlantKind::Geothermal {
                let params = region.geothermal().expect("validated geothermal starter");
                let crate::region::SitingRule::GeothermalSite { min } = plant.spec().siting else {
                    unreachable!("geothermal siting")
                };
                let potential = region
                    .terrain()
                    .get(def.cell)
                    .expect("starter cell")
                    .geothermal_potential;
                let (odds, field) = params.site(potential, min);
                plant.set_geothermal(crate::generation::GeothermalState {
                    exploration_left: 0,
                    odds,
                    exploration_capital: geothermal::capital_share(
                        plant.spec().capital_cost,
                        params.exploration_share,
                    ),
                    initial_field: field,
                    field,
                    rounds: 0,
                    makeup: None,
                    crossed: 0,
                    notice: None,
                    taught: false,
                });
            }
            plant.set_previous_month_offline(false);
            if def.kind.is_dispatchable() {
                plant.set_commitment(CommitmentSchedule::all_day());
                plant
                    .set_auto_follow(true)
                    .expect("a dispatchable plant takes AGC");
            }
            plants.insert(id, plant);
        }

        for &(from, to) in &start.hvdc_links {
            network
                .add_link(
                    ids.next(),
                    cities[&from].node(),
                    cities[&to].node(),
                    LineStatus::InService,
                )
                .expect("validated starter HVDC link");
        }
        let mut power = PowerSystem::new(network, plants, cities, settings.fidelity.network);
        if !start.island_homes.is_empty() {
            power.seed_starting_service(
                start
                    .island_homes
                    .iter()
                    .map(|id| power.cities()[id].node())
                    .collect(),
            );
        } else if region.areas().is_some() {
            power.recompute_islands();
        }
        let mut company = Company::new(start.cash, region.satisfaction().starting);
        let rate_base = company.rate_base_mut();
        for plant in power.plants().values() {
            let spec = plant.spec();
            rate_base.add_legacy(
                AssetRef::Plant(plant.id()),
                spec.capital_cost,
                spec.book_life_months,
                month,
                AssetProfile::Plant {
                    firm: plant.firm_capacity(&region.regulator().storage_accreditation),
                    node: plant.node(),
                },
            );
        }
        let lines = region.lines();
        for line in power.network().lines() {
            rate_base.add_legacy(
                AssetRef::Line(line.id),
                region
                    .line_terms(
                        power.network().node(line.from).expect("line endpoint").cell,
                        power.network().node(line.to).expect("line endpoint").cell,
                        line.length_cells,
                    )
                    .expect("validated starter line terms")
                    .capital,
                lines.book_life_months,
                month,
                AssetProfile::Line {
                    from: line.from,
                    to: line.to,
                },
            );
        }

        for link in power.network().links() {
            let a = power
                .network()
                .node(link.from_grid())
                .expect("link endpoint")
                .cell;
            let b = power
                .network()
                .node(link.to_grid())
                .expect("link endpoint")
                .cell;
            rate_base.add_legacy(
                AssetRef::Link(link.id),
                region
                    .link_terms(a, b, link.length_cells)
                    .expect("validated starter link terms")
                    .capital,
                region
                    .hvdc()
                    .expect("validated starter link catalog")
                    .book_life_months,
                month,
                AssetProfile::Link {
                    from: link.from_grid(),
                    to: link.to_grid(),
                },
            );
        }

        let regulator = Regulator::new(region.regulator());
        Self {
            geothermal_available: region.geothermal().is_some(),
            dry_cells: BTreeMap::new(),
            settings,
            clock: SimClock::new(month),
            phase: DayPhase::Planning,
            status: SessionStatus::Active,
            rng: SimRng::from_seed(seed),
            ids,
            anchor,
            power,
            company,
            regulator,
            instalments: BTreeMap::new(),
            month_to_date: MonthAccumulator::default(),
            year_depreciation: Money::ZERO,
            day_ticks: Vec::new(),
            region,
        }
    }

    /// Planning → Operating. Initialises the day from each plant's pattern
    /// (no physical state carries over from last month), resets frequency and
    /// protection, and starts the month's accumulator with the month-boundary
    /// start counts and the plants running at midnight. When the fleet cannot
    /// cover hour 0's forecast, the day starts in a rotating outage (spec
    /// §15.3), reported after the phase change as `RotatingOutageOrdered`.
    pub fn begin_operating(&mut self) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&[DayPhase::Planning])?;
        let init = self
            .power
            .begin_day(&self.region, self.clock.month(), &mut self.rng);
        self.month_to_date.reset();
        for (id, share) in init.island_outages {
            self.month_to_date.record_island_start(id, share);
        }
        self.month_to_date
            .record_boundary_starts(init.boundary_starts);
        self.month_to_date
            .observe_plants(self.power.plants().values());
        self.day_ticks.clear();
        let mut events = vec![self.change_phase(DayPhase::Operating)];
        if init.outage > Ratio::ZERO {
            let order = DomainEvent::RotatingOutageOrdered {
                hour: 0,
                share: init.outage,
            };
            // The accumulator was reset above, so hour 0's order reaches it
            // here, at tick 0, for the report's incident and peak share.
            self.month_to_date
                .add_command_events(0, std::slice::from_ref(&order));
            events.push(order);
        }
        Ok(events)
    }

    /// Runs one tick of the day. Returns the power system's events, which
    /// end with its `TickCompleted`; after the day's last tick the month is
    /// closed as well.
    pub fn tick(&mut self) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_operating()?;
        let (summary, mut events) = self.power.tick(
            &self.region,
            self.clock.month(),
            self.clock.tick(),
            &mut self.rng,
        );
        self.month_to_date.add_tick(
            &summary,
            &events,
            self.power.deviation(),
            self.region.regulator().frequency_band,
        );
        self.month_to_date
            .observe_plants(self.power.plants().values());
        self.day_ticks.push(summary);
        if self.clock.advance() {
            events.extend(self.close_month());
        }
        Ok(events)
    }

    /// Review → the next month's Planning. The finished day's hydro schedule
    /// is cleared: nothing is planned until `BeginOperating`.
    pub fn finish_review(&mut self) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&[DayPhase::Review])?;
        self.clock.next_month();
        self.power.clear_hydro_plan();
        Ok(vec![self.change_phase(DayPhase::Planning)])
    }

    fn change_phase(&mut self, to: DayPhase) -> DomainEvent {
        let from = std::mem::replace(&mut self.phase, to);
        DomainEvent::PhaseChanged { from, to }
    }

    /// The check [`GameSession::tick`] makes: `GameOver` if the game has
    /// ended, then `WrongPhase` unless the day is being operated.
    pub fn require_operating(&self) -> Result<(), DomainError> {
        self.require_phase(&[DayPhase::Operating])
    }

    /// Rejects a command when the game is over, then when the phase is not
    /// one of `allowed`.
    fn require_phase(&self, allowed: &[DayPhase]) -> Result<(), DomainError> {
        if let SessionStatus::GameOver(reason) = self.status {
            return Err(DomainError::GameOver { reason });
        }
        if allowed.contains(&self.phase) {
            Ok(())
        } else {
            Err(DomainError::WrongPhase { actual: self.phase })
        }
    }

    pub fn phase(&self) -> DayPhase {
        self.phase
    }

    pub fn status(&self) -> SessionStatus {
        self.status
    }

    pub fn clock(&self) -> &SimClock {
        &self.clock
    }

    pub fn region(&self) -> &Arc<RegionDefinition> {
        &self.region
    }

    pub fn settings(&self) -> &GameSettings {
        &self.settings
    }

    /// The starter city whose node anchors the service territory.
    pub fn home_city(&self) -> CityId {
        self.region.start().home_city
    }

    /// The home city's existing network node, where paths to the grid lead.
    pub fn anchor_node(&self) -> NodeId {
        self.anchor
    }

    pub fn power(&self) -> &PowerSystem {
        &self.power
    }

    pub fn company(&self) -> &Company {
        &self.company
    }

    pub fn regulator(&self) -> &Regulator {
        &self.regulator
    }

    /// FNV-1a 64 over the session's `Debug` output: equal hashes mean equal
    /// state, so replays can be compared cheaply.
    pub fn state_hash(&self) -> u64 {
        format!("{self:?}").bytes().fold(FNV_OFFSET, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
        })
    }
}

#[cfg(test)]
impl GameSession {
    /// Construction instalments still to pay for `asset`, earliest first;
    /// `None` once nothing is owed (or nothing was ever built).
    pub(crate) fn pending_instalments(&self, asset: AssetRef) -> Option<&VecDeque<Money>> {
        self.instalments.get(&asset)
    }

    /// Test support: undoes the inherited schedule (spec §16.3), as the
    /// player would in Planning, with `StopPlant` (the all-off pattern) and
    /// AGC off on every dispatchable, commissioned plant. Recreates the empty
    /// start the tests written before A5 assumed.
    pub(crate) fn empty_start(&mut self) {
        let plants: Vec<PlantId> = self
            .plant_views()
            .iter()
            .filter(|plant| {
                plant.kind.is_dispatchable()
                    && !matches!(
                        plant.status,
                        crate::generation::PlantStatus::UnderConstruction { .. }
                    )
            })
            .map(|plant| plant.id)
            .collect();
        for plant in plants {
            self.stop_plant(plant).expect("stop in Planning");
            self.set_auto_follow(plant, false).expect("AGC off");
        }
    }

    /// One-off month-boundary starts per plant for the current day.
    pub(crate) fn day_boundary_starts(&self) -> &BTreeMap<PlantId, u32> {
        self.month_to_date.boundary_starts()
    }

    /// Test support: a commissioned plant of `kind` at `cell`, on its own node
    /// joined to the home city by an in-service line.
    pub(crate) fn insert_commissioned_plant(
        &mut self,
        kind: crate::region::PlantKind,
        cell: CellCoord,
    ) -> PlantId {
        let id: PlantId = self.ids.next();
        let node: NodeId = self.ids.next();
        let line = self.ids.next();
        let network = self.power.network_mut();
        add_start_node(network, node, NodeKind::Plant(id), cell);
        network
            .add_line(line, self.anchor, node, LineStatus::InService)
            .expect("test plant line is valid");
        let spec = self.region.spec(kind).clone();
        self.power.insert_plant(Plant::new_commissioned(
            id,
            node,
            cell,
            spec,
            self.clock.month(),
        ));
        id
    }
}

/// Adds a node with a freshly allocated id, which cannot collide.
fn add_start_node(network: &mut Network, id: NodeId, kind: NodeKind, cell: CellCoord) {
    network
        .add_node(id, Node { kind, cell })
        .expect("freshly allocated node id is unique");
}

/// Joins a new start node to the home city with an in-service line.
fn add_start_line(network: &mut Network, ids: &mut IdAllocator, anchor: NodeId, node: NodeId) {
    network
        .add_line(ids.next(), anchor, node, LineStatus::InService)
        .expect("start line joins two distinct, existing, unjoined nodes");
}

/// Whole cells a line of `length` spans: `ceil(length)`, with a length within
/// 1e-9 of a whole number counted as that number.
#[cfg(test)]
fn whole_cells(length: f64) -> u32 {
    crate::region::whole_cells(length)
}

/// Capital cost of a line spanning `cells` whole cells.
#[cfg(test)]
fn line_cost(params: &LineParams, cells: u32) -> Money {
    params.cost_per_cell * i64::from(cells)
}

impl GameSession {
    pub fn snapshot_state(&self) -> Vec<u8> {
        self.snapshot_state_schema(crate::state::CURRENT_STATE_SCHEMA)
    }

    pub fn snapshot_state_schema(&self, schema: u32) -> Vec<u8> {
        use crate::state::State;
        let mut out = Vec::new();
        schema.write_schema(&mut out, schema);
        self.settings.write_schema(&mut out, schema);
        self.clock.write_schema(&mut out, schema);
        self.phase.write_schema(&mut out, schema);
        self.status.write_schema(&mut out, schema);
        self.rng.write_schema(&mut out, schema);
        self.ids.write_schema(&mut out, schema);
        self.anchor.write_schema(&mut out, schema);
        self.power.write_schema(&mut out, schema);
        self.company.write_schema(&mut out, schema);
        self.regulator.write_schema(&mut out, schema);
        self.instalments.write_schema(&mut out, schema);
        self.month_to_date.write_schema(&mut out, schema);
        self.year_depreciation.write_schema(&mut out, schema);
        self.day_ticks.write_schema(&mut out, schema);
        if schema >= 6 {
            self.geothermal_available.write(&mut out);
            self.dry_cells.write(&mut out);
        }
        out
    }
    /// Invoked only after verifying the original schema-1 canonical boundary.
    pub fn upgrade_schema1(&mut self) {
        let tick =
            (self.phase == DayPhase::Operating).then_some(self.clock.tick().saturating_sub(1));
        self.power
            .upgrade_schema1(&self.region, self.clock.month(), tick);
    }

    pub fn from_snapshot_state(
        bytes: &[u8],
        region: Arc<RegionDefinition>,
    ) -> Result<Self, crate::state::StateError> {
        use crate::state::{State, StateError, StateReader};
        let mut input = StateReader::new(bytes);
        let schema = u32::read(&mut input)?;
        input.schema = schema;
        if !crate::state::SUPPORTED_STATE_SCHEMAS.contains(&schema) {
            return Err(StateError::Version);
        }
        let session = Self {
            region,
            settings: State::read(&mut input)?,
            clock: State::read(&mut input)?,
            phase: State::read(&mut input)?,
            status: State::read(&mut input)?,
            rng: State::read(&mut input)?,
            ids: State::read(&mut input)?,
            anchor: State::read(&mut input)?,
            power: State::read(&mut input)?,
            company: State::read(&mut input)?,
            regulator: State::read(&mut input)?,
            instalments: State::read(&mut input)?,
            month_to_date: State::read(&mut input)?,
            year_depreciation: State::read(&mut input)?,
            day_ticks: State::read(&mut input)?,
            geothermal_available: if schema >= 6 {
                State::read(&mut input)?
            } else {
                false
            },
            dry_cells: if schema >= 6 {
                State::read(&mut input)?
            } else {
                BTreeMap::new()
            },
        };
        input.finish()?;
        session.validate_snapshot(schema)?;
        session
            .check_invariants()
            .map_err(|_| StateError::Invalid)?;
        Ok(session)
    }
}

impl crate::state::State for DayPhase {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Planning => {
                crate::state::State::write(&0u8, out);
            }
            Self::Operating => {
                crate::state::State::write(&1u8, out);
            }
            Self::Review => {
                crate::state::State::write(&2u8, out);
            }
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(match <u8 as crate::state::State>::read(input)? {
            0 => Self::Planning,
            1 => Self::Operating,
            2 => Self::Review,
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}

impl crate::state::State for SessionStatus {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Active => {
                crate::state::State::write(&0u8, out);
            }
            Self::GameOver(v0) => {
                crate::state::State::write(&1u8, out);
                crate::state::State::write(v0, out);
            }
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(match <u8 as crate::state::State>::read(input)? {
            0 => Self::Active,
            1 => Self::GameOver(crate::state::State::read(input)?),
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}

impl crate::state::State for GameOverReason {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Bankrupt => {
                crate::state::State::write(&0u8, out);
            }
            Self::LicenseRevoked => {
                crate::state::State::write(&1u8, out);
            }
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(match <u8 as crate::state::State>::read(input)? {
            0 => Self::Bankrupt,
            1 => Self::LicenseRevoked,
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}

impl GameSession {
    fn validate_snapshot(&self, schema: u32) -> Result<(), crate::state::StateError> {
        use crate::state::State;
        use crate::state::StateError;
        if schema >= 2
            && (!(0.0..=1.0).contains(&self.power.wind_factor())
                || self
                    .power
                    .wind_state_invalid(self.clock.month(), self.clock.tick()))
        {
            return Err(StateError::Invalid);
        }
        match self.phase {
            DayPhase::Planning if self.clock.tick() != 0 => return Err(StateError::Invalid),
            DayPhase::Operating if self.clock.tick() >= crate::time::TICKS_PER_DAY => {
                return Err(StateError::Invalid);
            }
            DayPhase::Review if self.clock.tick() != crate::time::TICKS_PER_DAY => {
                return Err(StateError::Invalid);
            }
            _ => {}
        }
        if self
            .power
            .cities()
            .get(&self.region.start().home_city)
            .is_none_or(|c| c.node() != self.anchor)
        {
            return Err(StateError::Invalid);
        }
        if !self.power.network().valid_snapshot(self.anchor) {
            return Err(StateError::Invalid);
        }
        let ledger = self
            .company
            .ledger()
            .iter()
            .try_fold(0i64, |sum, e| sum.checked_add(e.amount.cents()))
            .ok_or(StateError::Invalid)?;
        let delta = self
            .company
            .cash()
            .cents()
            .checked_sub(self.company.starting_cash().cents())
            .ok_or(StateError::Invalid)?;
        if ledger != delta {
            return Err(StateError::Invalid);
        }
        if (!self.geothermal_available
            && (!self.dry_cells.is_empty()
                || self
                    .power
                    .plants()
                    .values()
                    .any(|p| p.geothermal().is_some())))
            || (self.geothermal_available && self.region.geothermal().is_none())
        {
            return Err(StateError::Invalid);
        }
        if !self.company.rate_base().valid_additions() {
            return Err(StateError::Invalid);
        }
        let mut max_id = 0;
        let mut dry_ids = BTreeSet::new();
        for (cell, site) in &self.dry_cells {
            let id = &site.plant;
            if site.capital_lost.is_negative()
                || site.node.0 == id.0
                || self.power.network().node(site.node).is_some()
            {
                return Err(StateError::Invalid);
            }
            max_id = max_id.max(site.node.0);
            if !dry_ids.insert(*id)
                || self.power.plant(*id).is_some()
                || self.region.terrain().get(*cell).is_none_or(|c| !c.land)
                || self.power.network().nodes().any(|(_, node)| {
                    node.cell == *cell
                        && match node.kind {
                            NodeKind::City(_) => true,
                            NodeKind::Plant(plant) => self
                                .power
                                .plant(plant)
                                .is_none_or(|p| p.kind() == crate::region::PlantKind::Geothermal),
                        }
                })
            {
                return Err(StateError::Invalid);
            }
            max_id = max_id.max(id.0);
        }
        for (id, node) in self.power.network().nodes() {
            max_id = max_id.max(id.0);
            match node.kind {
                NodeKind::Plant(plant) => {
                    let p = self.power.plant(plant).ok_or(StateError::Invalid)?;
                    if p.node() != id || p.cell() != node.cell {
                        return Err(StateError::Invalid);
                    }
                }
                NodeKind::City(city) => {
                    let c = self.power.cities().get(&city).ok_or(StateError::Invalid)?;
                    if c.node() != id || c.def().cell != node.cell {
                        return Err(StateError::Invalid);
                    }
                }
            }
        }
        for line in self.power.network().lines() {
            if self.line_construction(line.id).is_none() {
                return Err(StateError::Invalid);
            }
            max_id = max_id.max(line.id.0);
        }
        for link in self.power.network().links() {
            let spec = self.region.hvdc().ok_or(StateError::Invalid)?;
            let network = self.power.network();
            if self.region.area_at(
                network
                    .node(link.from_grid())
                    .ok_or(StateError::Invalid)?
                    .cell,
            ) == self.region.area_at(
                network
                    .node(link.to_grid())
                    .ok_or(StateError::Invalid)?
                    .cell,
            ) || link
                .schedule
                .iter()
                .any(|v| !v.is_finite() || v.abs() > spec.rating_mw)
                || link
                    .override_mw
                    .is_some_and(|v| !v.is_finite() || v.abs() > spec.rating_mw)
                || link.flow.value().abs() > spec.rating_mw
                || link.sent.value() < 0.0
                || link.loss.value() < 0.0
                || link.runback != link.runback_terminal.is_some()
                || link
                    .runback_terminal
                    .is_some_and(|node| node != link.from_grid() && node != link.to_grid())
                || self.link_construction(link.id).is_none()
            {
                return Err(StateError::Invalid);
            }
            max_id = max_id.max(link.id.0);
        }
        if self.region.areas().is_some() {
            if self
                .power
                .island_state_invalid(&self.region, self.clock.month(), self.clock.tick())
            {
                return Err(StateError::Invalid);
            }
            for line in self.power.network().lines() {
                let network = self.power.network();
                if self
                    .region
                    .area_at(network.node(line.from).ok_or(StateError::Invalid)?.cell)
                    != self
                        .region
                        .area_at(network.node(line.to).ok_or(StateError::Invalid)?.cell)
                {
                    return Err(StateError::Invalid);
                }
            }
            if self.power.island_nodes() != &self.power.network().islands() {
                return Err(StateError::Invalid);
            }
            if self.phase == DayPhase::Operating
                && self
                    .power
                    .island_states()
                    .keys()
                    .ne(self.power.island_nodes().keys())
            {
                return Err(StateError::Invalid);
            }
        }
        for (id, plant) in self.power.plants() {
            max_id = max_id.max(id.0);
            if (plant.kind() == crate::region::PlantKind::Geothermal)
                != plant.geothermal().is_some()
            {
                return Err(StateError::Invalid);
            }
            if let Some(g) = plant.geothermal() {
                let params = self.region.geothermal().ok_or(StateError::Invalid)?;
                let crate::region::SitingRule::GeothermalSite { min } = plant.spec().siting else {
                    return Err(StateError::Invalid);
                };
                let cell = self
                    .region
                    .terrain()
                    .get(plant.cell())
                    .ok_or(StateError::Invalid)?;
                let (odds, quality) = params.site(cell.geothermal_potential, min);
                // Rasterization uses libm in infrastructure; retain original portable bits across targets.
                if (g.odds.value() - odds.value()).abs() > 1e-12
                    || (g.initial_field.value() - quality.value()).abs() > 1e-12
                    || g.exploration_capital
                        != geothermal::capital_share(
                            plant.spec().capital_cost,
                            params.exploration_share,
                        )
                {
                    return Err(StateError::Invalid);
                }
                let ceiling = g.initial_field.value()
                    * (1.0 - f64::from(g.rounds) * params.ceiling_step.value());
                if g.field.value() > ceiling + 1e-12
                    || g.notice.is_some_and(|t| {
                        g.field.value() / g.initial_field.value() >= f64::from(t) / 100.0
                    })
                {
                    return Err(StateError::Invalid);
                }
                if let Some(m) = &g.makeup {
                    let capital = geothermal::makeup_capital(
                        self.region
                            .spec(crate::region::PlantKind::Geothermal)
                            .capital_cost,
                        plant.spec().capital_cost,
                        params.makeup_capital,
                    )
                    .map_err(|_| StateError::Invalid)?;
                    if m.capital != capital {
                        return Err(StateError::Invalid);
                    }
                    if m.months_left > params.makeup_months {
                        return Err(StateError::Invalid);
                    }
                    let tail: VecDeque<_> = m
                        .capital
                        .split_even(params.makeup_months)
                        .into_iter()
                        .skip((params.makeup_months - m.months_left) as usize)
                        .collect();
                    let ceiling = g.initial_field.value()
                        * (1.0 - (f64::from(g.rounds) + 1.0) * params.ceiling_step.value());
                    if m.payments != tail || (m.ceiling.value() - ceiling).abs() > f64::EPSILON {
                        return Err(StateError::Invalid);
                    }
                }
                if !g.valid() || g.exploration_left > params.exploration_months {
                    return Err(StateError::Invalid);
                }
                let expected = match plant.status() {
                    crate::generation::PlantStatus::UnderConstruction { months_left } => {
                        months_left
                            .saturating_sub(plant.spec().build_months - params.exploration_months)
                    }
                    _ => 0,
                };
                if g.exploration_left != expected {
                    return Err(StateError::Invalid);
                }
                if matches!(
                    plant.status(),
                    crate::generation::PlantStatus::UnderConstruction { .. }
                ) && (g.field != g.initial_field
                    || g.rounds != 0
                    || g.makeup.is_some()
                    || g.crossed != 0
                    || g.notice.is_some()
                    || g.taught)
                {
                    return Err(StateError::Invalid);
                }
                if g.exploration_left > 0
                    && (self
                        .power
                        .network()
                        .lines()
                        .any(|l| l.from == plant.node() || l.to == plant.node())
                        || self
                            .power
                            .network()
                            .links()
                            .any(|l| l.from_grid() == plant.node() || l.to_grid() == plant.node()))
                {
                    return Err(StateError::Invalid);
                }
            }
            if *id != plant.id()
                || plant.spec().state_bytes_schema(schema)
                    != self
                        .region
                        .plant_spec_at(plant.kind(), plant.cell())
                        .map_err(|_| StateError::Invalid)?
                        .state_bytes_schema(schema)
            {
                return Err(StateError::Invalid);
            }
        }
        if self.ids.next_value() <= max_id || self.ids.next_value() == u32::MAX {
            return Err(StateError::Invalid);
        }
        if self.power.cities().len() != self.region.cities().len() {
            return Err(StateError::Invalid);
        }
        for def in self.region.cities() {
            if self
                .power
                .cities()
                .get(&def.id)
                .is_none_or(|c| c.def() != def)
            {
                return Err(StateError::Invalid);
            }
        }
        Ok(())
    }
}
