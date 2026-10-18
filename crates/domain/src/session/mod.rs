//! The game session: the aggregate root that owns one game's whole state and
//! accepts the player's commands.
//!
//! A played month is one representative day in three phases: **Planning**
//! (build, commit and set initial targets), **Operating** (tick through the
//! day's 1,440 minutes, with manual overrides), and **Review** (the month is
//! closed). Every command checks first that the game is still active, then
//! that the phase allows it, and validates fully before mutating anything, so
//! a command that returns `Err` leaves [`GameSession::state_hash`] unchanged.

mod commands;
mod invariants;
mod month_close;
mod report;
mod settings;
mod views;

#[cfg(test)]
mod tests;

pub use invariants::InvariantViolation;
pub use report::{Incident, IncidentKind, MeritOrderEntry, MonthAccumulator, MonthlyReport};
pub use settings::{GameSettings, SimulationFidelity};
pub use views::{CityView, CompanyView, GridSnapshot, PlantView};

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use crate::company::{AssetProfile, AssetRef, Company};
use crate::demand::DemandCenter;
use crate::error::DomainError;
use crate::events::DomainEvent;
use crate::generation::Plant;
use crate::grid::{LineStatus, Network, Node, NodeKind, PowerSystem, TickSummary};
use crate::ids::{CellCoord, IdAllocator, NodeId, PlantId};
use crate::region::{LineParams, RegionDefinition};
use crate::regulation::Regulator;
use crate::rng::SimRng;
use crate::time::SimClock;
use crate::units::Money;

/// FNV-1a 64-bit offset basis and prime.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Below this a float is treated as sitting on a whole number of cells.
const CELL_EPSILON: f64 = 1e-9;

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
    /// The network root: the player's home substation.
    substation: NodeId,
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
}

impl GameSession {
    /// Starts a new game in `region`'s starting scenario.
    ///
    /// The substation is the network root. Every city gets a node, and each
    /// start-connected city an in-service line from the substation. Each
    /// start-fleet plant is commissioned on the start month on its own node
    /// with an in-service line to the substation, and is treated as
    /// established (not returning from a month offline). The start fleet and
    /// start lines enter the rate base as legacy capital at full cost. The
    /// game begins in `Planning` with the start cash.
    pub fn new(region: Arc<RegionDefinition>, seed: u64, settings: GameSettings) -> Self {
        let start = region.start();
        let month = start.month;
        let mut ids = IdAllocator::default();
        let substation: NodeId = ids.next();
        let mut network = Network::new(substation, start.substation);

        let mut cities = BTreeMap::new();
        for def in region.cities() {
            let node: NodeId = ids.next();
            add_start_node(&mut network, node, NodeKind::City(def.id), def.cell);
            if start.connected_cities.contains(&def.id) {
                add_start_line(&mut network, &mut ids, substation, node);
            }
            cities.insert(def.id, DemandCenter::new(def.clone(), node));
        }

        let mut plants = BTreeMap::new();
        for def in &start.fleet {
            let id: PlantId = ids.next();
            let node: NodeId = ids.next();
            add_start_node(&mut network, node, NodeKind::Plant(id), def.cell);
            add_start_line(&mut network, &mut ids, substation, node);
            let spec = region.spec(def.kind).clone();
            let mut plant = Plant::new_commissioned(id, node, def.cell, spec, month);
            plant.set_previous_month_offline(false);
            plants.insert(id, plant);
        }

        let power = PowerSystem::new(network, plants, cities, settings.fidelity.network);
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
                    firm: spec.capacity_credit.of(spec.capacity),
                    node: plant.node(),
                },
            );
        }
        let lines = region.lines();
        for line in power.network().lines() {
            rate_base.add_legacy(
                AssetRef::Line(line.id),
                line_cost(lines, whole_cells(line.length_cells)),
                lines.book_life_months,
                month,
                AssetProfile::Line {
                    from: line.from,
                    to: line.to,
                },
            );
        }

        let regulator = Regulator::new(region.regulator());
        Self {
            settings,
            clock: SimClock::new(month),
            phase: DayPhase::Planning,
            status: SessionStatus::Active,
            rng: SimRng::from_seed(seed),
            ids,
            substation,
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
    /// start counts and the plants running at midnight.
    pub fn begin_operating(&mut self) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&[DayPhase::Planning])?;
        let init = self
            .power
            .begin_day(&self.region, self.clock.month(), &mut self.rng);
        self.month_to_date.reset();
        self.month_to_date
            .record_boundary_starts(init.boundary_starts);
        self.month_to_date
            .observe_plants(self.power.plants().values());
        self.day_ticks.clear();
        Ok(vec![self.change_phase(DayPhase::Operating)])
    }

    /// Runs one tick of the day. Returns the power system's events, which
    /// end with its `TickCompleted`; after the day's last tick the month is
    /// closed as well.
    pub fn tick(&mut self) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&[DayPhase::Operating])?;
        let (summary, mut events) = self.power.tick(
            &self.region,
            self.clock.month(),
            self.clock.tick(),
            &mut self.rng,
        );
        self.month_to_date.add_tick(&summary, &events);
        self.month_to_date
            .observe_plants(self.power.plants().values());
        self.day_ticks.push(summary);
        if self.clock.advance() {
            events.extend(self.close_month());
        }
        Ok(events)
    }

    /// Review → the next month's Planning.
    pub fn finish_review(&mut self) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&[DayPhase::Review])?;
        self.clock.next_month();
        Ok(vec![self.change_phase(DayPhase::Planning)])
    }

    fn change_phase(&mut self, to: DayPhase) -> DomainEvent {
        let from = std::mem::replace(&mut self.phase, to);
        DomainEvent::PhaseChanged { from, to }
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

    /// The network root, where every line to the grid ultimately leads.
    pub fn substation(&self) -> NodeId {
        self.substation
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

    /// One-off month-boundary starts per plant for the current day.
    pub(crate) fn day_boundary_starts(&self) -> &BTreeMap<PlantId, u32> {
        self.month_to_date.boundary_starts()
    }

    /// Test support: a commissioned plant of `kind` at `cell`, on its own node
    /// joined to the substation by an in-service line.
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
            .add_line(line, self.substation, node, LineStatus::InService)
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

/// Joins a new start node to the substation with an in-service line.
fn add_start_line(network: &mut Network, ids: &mut IdAllocator, substation: NodeId, node: NodeId) {
    network
        .add_line(ids.next(), substation, node, LineStatus::InService)
        .expect("start line joins two distinct, existing, unjoined nodes");
}

/// Whole cells a line of `length` spans: `ceil(length)`, with a length within
/// 1e-9 of a whole number counted as that number.
fn whole_cells(length: f64) -> u32 {
    let nearest = length.round();
    let cells = if (length - nearest).abs() <= CELL_EPSILON {
        nearest
    } else {
        length.ceil()
    };
    // Float-to-int `as` saturates, which is the intent for absurd lengths.
    cells as u32
}

/// Capital cost of a line spanning `cells` whole cells.
fn line_cost(params: &LineParams, cells: u32) -> Money {
    params.cost_per_cell * i64::from(cells)
}
