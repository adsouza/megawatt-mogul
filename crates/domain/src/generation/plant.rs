//! A generating plant: its lifecycle state machine, ramping and pattern-day initialisation.
//!
//! The plant owns only its own physical state. Resource availability (sun,
//! water) is passed in by the caller each tick, so the plant stays free of
//! weather and market concerns.

use super::commitment::{BoundaryStart, CommitmentSchedule};
use crate::error::DomainError;
use crate::events::DomainEvent;
use crate::ids::{CellCoord, NodeId, PlantId};
use crate::region::{PlantKind, PlantSpec};
use crate::time::{SimMonth, TICKS_PER_HOUR};
use crate::units::{MegawattHours, Megawatts};

/// Below this a float is treated as sitting on its boundary (0 MW, 0 MWh).
pub(super) const EPSILON: f64 = 1e-9;

/// Where a plant is in its life.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlantStatus {
    UnderConstruction {
        months_left: u32,
    },
    Offline,
    Starting {
        ticks_left: u32,
    },
    Online {
        output: Megawatts,
        target: Megawatts,
    },
    Stopping {
        output: Megawatts,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plant {
    id: PlantId,
    node: NodeId,
    cell: CellCoord,
    spec: PlantSpec,
    status: PlantStatus,
    auto_follow: bool,
    commitment: CommitmentSchedule,
    commissioned_on: Option<SimMonth>,
    /// Remaining energy for the month; `Some` only for hydro.
    hydro_budget: Option<MegawattHours>,
    /// Whether the plant sat offline for the whole previous month.
    previous_month_offline: bool,
    initial_target: Megawatts,
    /// Set by the caller to ignore the commitment schedule for the current day.
    commitment_suppressed: bool,
}

impl Plant {
    fn new(
        id: PlantId,
        node: NodeId,
        cell: CellCoord,
        spec: PlantSpec,
        status: PlantStatus,
    ) -> Self {
        let hydro_budget = (spec.kind == PlantKind::Hydro).then_some(MegawattHours::ZERO);
        let initial_target = spec.min_output();
        Self {
            id,
            node,
            cell,
            spec,
            status,
            auto_follow: false,
            commitment: CommitmentSchedule::none(),
            commissioned_on: None,
            hydro_budget,
            previous_month_offline: true,
            initial_target,
            commitment_suppressed: false,
        }
    }

    /// A plant that has just been ordered. It commissions after `build_months`
    /// calls to [`Plant::advance_construction`].
    pub fn new_under_construction(
        id: PlantId,
        node: NodeId,
        cell: CellCoord,
        spec: PlantSpec,
    ) -> Self {
        let months_left = spec.build_months;
        Self::new(
            id,
            node,
            cell,
            spec,
            PlantStatus::UnderConstruction { months_left },
        )
    }

    /// A finished plant. Solar starts online at zero output (it cannot be
    /// started); every other kind starts offline. Hydro starts with an empty
    /// budget until [`Plant::reset_hydro_budget`] is called.
    pub fn new_commissioned(
        id: PlantId,
        node: NodeId,
        cell: CellCoord,
        spec: PlantSpec,
        month: SimMonth,
    ) -> Self {
        let status = Self::commissioned_status(&spec);
        let mut plant = Self::new(id, node, cell, spec, status);
        plant.commissioned_on = Some(month);
        plant
    }

    fn commissioned_status(spec: &PlantSpec) -> PlantStatus {
        if spec.kind.is_dispatchable() {
            PlantStatus::Offline
        } else {
            PlantStatus::Online {
                output: Megawatts::ZERO,
                target: Megawatts::ZERO,
            }
        }
    }

    pub fn id(&self) -> PlantId {
        self.id
    }

    pub fn node(&self) -> NodeId {
        self.node
    }

    pub fn cell(&self) -> CellCoord {
        self.cell
    }

    pub fn kind(&self) -> PlantKind {
        self.spec.kind
    }

    pub fn spec(&self) -> &PlantSpec {
        &self.spec
    }

    pub fn status(&self) -> PlantStatus {
        self.status
    }

    /// Current output: nonzero only while online or stopping.
    pub fn output(&self) -> Megawatts {
        match self.status {
            PlantStatus::Online { output, .. } | PlantStatus::Stopping { output } => output,
            _ => Megawatts::ZERO,
        }
    }

    pub fn is_online(&self) -> bool {
        matches!(self.status, PlantStatus::Online { .. })
    }

    pub fn auto_follow(&self) -> bool {
        self.auto_follow
    }

    pub fn commitment(&self) -> CommitmentSchedule {
        self.commitment
    }

    pub fn commissioned_on(&self) -> Option<SimMonth> {
        self.commissioned_on
    }

    /// Remaining hydro energy this month; `None` for non-hydro plants.
    pub fn hydro_budget(&self) -> Option<MegawattHours> {
        self.hydro_budget
    }

    pub fn initial_target(&self) -> Megawatts {
        self.initial_target
    }

    pub fn previous_month_offline(&self) -> bool {
        self.previous_month_offline
    }

    pub fn is_commitment_suppressed(&self) -> bool {
        self.commitment_suppressed
    }

    fn require_dispatchable(&self) -> Result<(), DomainError> {
        if self.spec.kind.is_dispatchable() {
            Ok(())
        } else {
            Err(DomainError::PlantNotDispatchable { plant: self.id })
        }
    }

    pub fn set_auto_follow(&mut self, on: bool) -> Result<(), DomainError> {
        self.require_dispatchable()?;
        self.auto_follow = on;
        Ok(())
    }

    pub fn set_commitment(&mut self, schedule: CommitmentSchedule) {
        self.commitment = schedule;
    }

    /// The target the plant assumes when a day begins already running. Stored
    /// as given; the caller is responsible for choosing a value in range.
    pub fn set_initial_target(&mut self, target: Megawatts) {
        self.initial_target = target;
    }

    pub fn set_previous_month_offline(&mut self, offline: bool) {
        self.previous_month_offline = offline;
    }

    /// Ignore the commitment schedule for the rest of the current day. Only the
    /// caller sets this; `start`, `stop` and `trip` never do.
    pub fn suppress_commitment_for_day(&mut self) {
        self.commitment_suppressed = true;
    }

    /// Clears the per-day overrides (currently just the suppression flag).
    pub fn clear_day_overrides(&mut self) {
        self.commitment_suppressed = false;
    }

    /// Sets the physical state at instant 0 of a new representative day from
    /// the commitment pattern alone. This is a synthetic boundary, not an
    /// operating tick: last month's physical state is discarded (no 48 h
    /// start-up carries across months) and no events are emitted.
    ///
    /// - Clears the day overrides and clamps the initial target into
    ///   `[min_output, capacity]`.
    /// - Committed at hour 0: `Online` at the initial target. Otherwise a run
    ///   whose shutdown tail crosses midnight is `Stopping` (conservatively from
    ///   capacity), a run whose start-up began before midnight is `Starting`
    ///   with the implied ticks left, and anything else is `Offline`.
    /// - Solar is always reconnected `Online` at 0 MW; it cannot be commanded.
    /// - Plants under construction are left untouched.
    ///
    /// Returns [`BoundaryStart::Charged`] only for an all-day pattern on a plant
    /// that sat offline all last month. A mixed pattern's restart is counted
    /// when its `Start` action fires (including a late-day start preparing an
    /// hour-0 run), so initialisation never counts it a second time.
    pub fn initialise_pattern_day(&mut self) -> BoundaryStart {
        if matches!(self.status, PlantStatus::UnderConstruction { .. }) {
            return BoundaryStart::None;
        }
        self.clear_day_overrides();
        if !self.spec.kind.is_dispatchable() {
            self.status = PlantStatus::Online {
                output: Megawatts::ZERO,
                target: Megawatts::ZERO,
            };
            return BoundaryStart::None;
        }
        self.initial_target = self
            .initial_target
            .clamp(self.spec.min_output(), self.spec.capacity);
        self.status = self
            .commitment
            .initial_status(&self.spec, self.initial_target);
        if self.previous_month_offline && self.commitment.is_all_day() {
            BoundaryStart::Charged
        } else {
            BoundaryStart::None
        }
    }

    /// `Offline` to `Starting`. A zero-length start-up goes straight online at
    /// minimum stable output.
    pub fn start(&mut self) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_dispatchable()?;
        if self.status != PlantStatus::Offline {
            return Err(DomainError::PlantNotOffline { plant: self.id });
        }
        let mut events = vec![DomainEvent::PlantStarting { plant: self.id }];
        let ticks = self.spec.startup.ticks();
        if ticks == 0 {
            self.go_online();
            events.push(DomainEvent::PlantOnline { plant: self.id });
        } else {
            self.status = PlantStatus::Starting { ticks_left: ticks };
        }
        Ok(events)
    }

    /// Begins a ramp-down from `Online`, or cancels a start-up in progress
    /// (`Starting` goes straight to `Offline` with `PlantStopped`). Rejects
    /// every other state with `PlantNotRunning`.
    pub fn stop(&mut self) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_dispatchable()?;
        match self.status {
            PlantStatus::Online { output, .. } => {
                self.status = PlantStatus::Stopping { output };
                Ok(Vec::new())
            }
            PlantStatus::Starting { .. } => {
                self.status = PlantStatus::Offline;
                Ok(vec![DomainEvent::PlantStopped { plant: self.id }])
            }
            _ => Err(DomainError::PlantNotRunning { plant: self.id }),
        }
    }

    /// Sets the dispatch target of an online dispatchable plant.
    pub fn set_target(&mut self, mw: Megawatts) -> Result<(), DomainError> {
        self.require_dispatchable()?;
        let PlantStatus::Online { output, .. } = self.status else {
            return Err(DomainError::PlantNotOnline { plant: self.id });
        };
        let min = self.spec.min_output();
        if mw < min {
            return Err(DomainError::BelowMinimumStableOutput {
                plant: self.id,
                min,
            });
        }
        if mw > self.spec.capacity {
            return Err(DomainError::AboveCapacity {
                plant: self.id,
                max: self.spec.capacity,
            });
        }
        self.status = PlantStatus::Online { output, target: mw };
        Ok(())
    }

    fn go_online(&mut self) {
        let min = self.spec.min_output();
        self.status = PlantStatus::Online {
            output: min,
            target: min,
        };
    }

    /// Advances the plant by one tick. `available` is the most the plant's
    /// resource allows right now (sunlight for solar, the hydro budget for
    /// hydro, capacity otherwise).
    ///
    /// Rules:
    /// - `Starting` counts down and becomes `Online` at minimum output.
    /// - `Online` dispatchable: the goal is `min(target, available)`. Output moves
    ///   toward it by at most one tick of ramp and never falls below minimum
    ///   stable output through ramping. Availability is a hard physical cap
    ///   applied last, so it can pull output below minimum (a dry reservoir
    ///   drops hydro to zero at once).
    /// - Solar: output is `available` (capped at capacity) with no ramp limit.
    /// - `Stopping` falls by one tick of ramp (also capped by `available`);
    ///   at or below 1e-9 MW it becomes `Offline`.
    /// - Hydro then deducts this tick's energy from its budget, snapping to
    ///   exactly zero within 1e-9 MWh and reporting exhaustion on that transition.
    pub fn step(&mut self, available: Megawatts) -> Vec<DomainEvent> {
        let mut events = Vec::new();
        let cap = available.max(Megawatts::ZERO).min(self.spec.capacity);
        match self.status {
            PlantStatus::Starting { ticks_left } => {
                let left = ticks_left.saturating_sub(1);
                if left == 0 {
                    self.go_online();
                    events.push(DomainEvent::PlantOnline { plant: self.id });
                } else {
                    self.status = PlantStatus::Starting { ticks_left: left };
                }
            }
            PlantStatus::Online { .. } if !self.spec.kind.is_dispatchable() => {
                self.status = PlantStatus::Online {
                    output: cap,
                    target: cap,
                };
            }
            PlantStatus::Online { output, target } => {
                let ramp = self.spec.ramp.per_tick();
                let lo = (output - ramp).max(self.spec.min_output());
                let hi = output + ramp;
                let goal = target.min(cap);
                self.status = PlantStatus::Online {
                    output: goal.max(lo).min(hi).min(cap),
                    target,
                };
            }
            PlantStatus::Stopping { output } => {
                let next = (output - self.spec.ramp.per_tick()).min(cap);
                if next.value() <= EPSILON {
                    self.status = PlantStatus::Offline;
                    events.push(DomainEvent::PlantStopped { plant: self.id });
                } else {
                    self.status = PlantStatus::Stopping { output: next };
                }
            }
            PlantStatus::UnderConstruction { .. } | PlantStatus::Offline => {}
        }
        self.deduct_hydro_energy(&mut events);
        events
    }

    fn deduct_hydro_energy(&mut self, events: &mut Vec<DomainEvent>) {
        let used = self.output().energy_over_ticks(1);
        let Some(budget) = self.hydro_budget else {
            return;
        };
        if budget == MegawattHours::ZERO {
            return;
        }
        let left = budget - used;
        if left.value() <= EPSILON {
            self.hydro_budget = Some(MegawattHours::ZERO);
            events.push(DomainEvent::HydroBudgetExhausted { plant: self.id });
        } else {
            self.hydro_budget = Some(left);
        }
    }

    /// How much output could rise next tick: one tick of ramp, bounded by
    /// `min(available, capacity) - output`. Zero unless online and dispatchable.
    pub fn headroom_up(&self, available: Megawatts) -> Megawatts {
        match self.status {
            PlantStatus::Online { output, .. } if self.spec.kind.is_dispatchable() => {
                let room = available.min(self.spec.capacity) - output;
                self.spec.ramp.per_tick().min(room).max(Megawatts::ZERO)
            }
            _ => Megawatts::ZERO,
        }
    }

    /// How much output could fall next tick without dropping below minimum
    /// stable output. Zero unless online and dispatchable.
    pub fn headroom_down(&self) -> Megawatts {
        match self.status {
            PlantStatus::Online { output, .. } if self.spec.kind.is_dispatchable() => {
                let room = output - self.spec.min_output();
                self.spec.ramp.per_tick().min(room).max(Megawatts::ZERO)
            }
            _ => Megawatts::ZERO,
        }
    }

    /// Unplanned loss: `Starting`, `Online` or `Stopping` go straight to
    /// `Offline`. Offline and under-construction plants are unaffected.
    pub fn trip(&mut self) -> Vec<DomainEvent> {
        match self.status {
            PlantStatus::Starting { .. }
            | PlantStatus::Online { .. }
            | PlantStatus::Stopping { .. } => {
                self.status = PlantStatus::Offline;
                vec![DomainEvent::PlantTripped { plant: self.id }]
            }
            PlantStatus::UnderConstruction { .. } | PlantStatus::Offline => Vec::new(),
        }
    }

    /// A tripped solar plant re-synchronises with a re-energised grid: `Offline`
    /// becomes `Online` at 0 MW. No effect on any other kind or state, since
    /// dispatchable plants must be restarted.
    pub fn reconnect(&mut self) {
        if !self.spec.kind.is_dispatchable() && self.status == PlantStatus::Offline {
            self.status = PlantStatus::Online {
                output: Megawatts::ZERO,
                target: Megawatts::ZERO,
            };
        }
    }

    /// Sets an online dispatchable plant's output and target to its minimum
    /// stable output. No effect in any other state.
    pub fn force_to_minimum(&mut self) {
        if self.is_online() && self.spec.kind.is_dispatchable() {
            self.go_online();
        }
    }

    /// Refills the hydro energy budget (a negative value is treated as zero).
    /// No effect on non-hydro plants.
    pub fn reset_hydro_budget(&mut self, budget: MegawattHours) {
        if self.hydro_budget.is_some() {
            self.hydro_budget = Some(if budget.value() > 0.0 {
                budget
            } else {
                MegawattHours::ZERO
            });
        }
    }

    /// The most a hydro plant can produce this tick: the remaining budget spread
    /// over an hour, capped at capacity. For non-hydro plants there is no such
    /// resource limit, so this is simply capacity.
    pub fn hydro_available(&self) -> Megawatts {
        match self.hydro_budget {
            Some(budget) => {
                Megawatts::new(budget.value() * f64::from(TICKS_PER_HOUR)).min(self.spec.capacity)
            }
            None => self.spec.capacity,
        }
    }

    /// Counts down construction. At zero the plant commissions in `month`:
    /// solar comes online at 0 MW, everything else sits offline. No effect
    /// unless the plant is under construction.
    pub fn advance_construction(&mut self, month: SimMonth) -> Vec<DomainEvent> {
        let PlantStatus::UnderConstruction { months_left } = self.status else {
            return Vec::new();
        };
        let left = months_left.saturating_sub(1);
        if left > 0 {
            self.status = PlantStatus::UnderConstruction { months_left: left };
            return Vec::new();
        }
        self.status = Self::commissioned_status(&self.spec);
        self.commissioned_on = Some(month);
        vec![DomainEvent::PlantCommissioned {
            plant: self.id,
            kind: self.spec.kind,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DomainError;
    use crate::events::DomainEvent;
    use crate::ids::{CellCoord, NodeId, PlantId};
    use crate::region::PlantKind;
    use crate::region::fixtures::tiny_region;
    use crate::time::SimMonth;
    use crate::units::{MegawattHours, Megawatts};
    use proptest::prelude::*;

    fn april() -> SimMonth {
        SimMonth::new(2027, 4).unwrap()
    }

    fn new_plant(kind: PlantKind) -> Plant {
        Plant::new_commissioned(
            PlantId(1),
            NodeId(1),
            CellCoord { x: 7, y: 3 },
            tiny_region().spec(kind).clone(),
            april(),
        )
    }

    fn online(kind: PlantKind) -> Plant {
        let mut p = new_plant(kind);
        if p.is_online() {
            return p;
        }
        p.start().unwrap();
        while !p.is_online() {
            p.step(p.spec().capacity);
        }
        p
    }

    #[test]
    fn coal_takes_its_startup_ticks() {
        let mut p = new_plant(PlantKind::Coal);
        p.start().unwrap();
        for _ in 0..479 {
            p.step(Megawatts::new(600.0));
        }
        assert!(matches!(
            p.status(),
            PlantStatus::Starting { ticks_left: 1 }
        ));
        assert_eq!(
            p.step(Megawatts::new(600.0)),
            vec![DomainEvent::PlantOnline { plant: PlantId(1) }]
        );
        assert_eq!(p.output(), Megawatts::new(240.0));
    }

    #[test]
    fn peaker_ramps_40_mw_per_tick() {
        let mut p = online(PlantKind::Peaker);
        p.set_target(Megawatts::new(200.0)).unwrap();
        let outs: Vec<f64> = (0..4)
            .map(|_| {
                p.step(Megawatts::new(200.0));
                p.output().value()
            })
            .collect();
        assert_eq!(outs, vec![80.0, 120.0, 160.0, 200.0]);
    }

    #[test]
    fn nuclear_target_below_minimum_is_rejected() {
        let mut p = online(PlantKind::Nuclear);
        assert_eq!(
            p.set_target(Megawatts::new(700.0)),
            Err(DomainError::BelowMinimumStableOutput {
                plant: PlantId(1),
                min: Megawatts::new(770.0)
            })
        );
    }

    #[test]
    fn target_above_capacity_is_rejected() {
        let mut p = online(PlantKind::Peaker);
        assert_eq!(
            p.set_target(Megawatts::new(201.0)),
            Err(DomainError::AboveCapacity {
                plant: PlantId(1),
                max: Megawatts::new(200.0)
            })
        );
    }

    #[test]
    fn set_target_requires_an_online_dispatchable_plant() {
        let mut offline = new_plant(PlantKind::Peaker);
        assert_eq!(
            offline.set_target(Megawatts::new(100.0)),
            Err(DomainError::PlantNotOnline { plant: PlantId(1) })
        );
        let mut solar = new_plant(PlantKind::Solar);
        assert_eq!(
            solar.set_target(Megawatts::new(100.0)),
            Err(DomainError::PlantNotDispatchable { plant: PlantId(1) })
        );
    }

    #[test]
    fn solar_cannot_be_started() {
        assert!(matches!(
            new_plant(PlantKind::Solar).start(),
            Err(DomainError::PlantNotDispatchable { .. })
        ));
    }

    #[test]
    fn start_requires_offline() {
        let mut p = online(PlantKind::Peaker);
        assert_eq!(
            p.start(),
            Err(DomainError::PlantNotOffline { plant: PlantId(1) })
        );
        let mut u = Plant::new_under_construction(
            PlantId(2),
            NodeId(1),
            CellCoord { x: 7, y: 3 },
            tiny_region().spec(PlantKind::Coal).clone(),
        );
        assert_eq!(
            u.start(),
            Err(DomainError::PlantNotOffline { plant: PlantId(2) })
        );
    }

    #[test]
    fn start_emits_starting_and_zero_startup_goes_straight_online() {
        let mut p = new_plant(PlantKind::Coal);
        assert_eq!(
            p.start().unwrap(),
            vec![DomainEvent::PlantStarting { plant: PlantId(1) }]
        );
        let mut spec = tiny_region().spec(PlantKind::Peaker).clone();
        spec.startup = crate::time::SimDuration::from_ticks(0);
        let mut q = Plant::new_commissioned(
            PlantId(3),
            NodeId(1),
            CellCoord { x: 7, y: 3 },
            spec,
            april(),
        );
        assert_eq!(
            q.start().unwrap(),
            vec![
                DomainEvent::PlantStarting { plant: PlantId(3) },
                DomainEvent::PlantOnline { plant: PlantId(3) }
            ]
        );
        assert_eq!(q.output(), Megawatts::new(40.0));
    }

    #[test]
    fn stopping_ramps_down_then_goes_offline() {
        let mut p = online(PlantKind::Ccgt);
        p.set_target(Megawatts::new(500.0)).unwrap();
        while p.output() < Megawatts::new(200.0) {
            p.step(Megawatts::new(500.0));
        }
        assert_eq!(p.output(), Megawatts::new(200.0));
        assert!(p.stop().unwrap().is_empty());
        for _ in 0..7 {
            assert!(p.step(Megawatts::new(500.0)).is_empty());
        }
        assert_eq!(
            p.status(),
            PlantStatus::Stopping {
                output: Megawatts::new(25.0)
            }
        );
        assert_eq!(
            p.step(Megawatts::new(500.0)),
            vec![DomainEvent::PlantStopped { plant: PlantId(1) }]
        );
        assert_eq!(p.status(), PlantStatus::Offline);
        assert_eq!(p.output(), Megawatts::ZERO);
    }

    #[test]
    fn stop_rejects_plants_that_are_not_running() {
        let mut p = new_plant(PlantKind::Ccgt);
        assert_eq!(
            p.stop(),
            Err(DomainError::PlantNotRunning { plant: PlantId(1) })
        );
        assert_eq!(
            new_plant(PlantKind::Solar).stop(),
            Err(DomainError::PlantNotDispatchable { plant: PlantId(1) })
        );
    }

    #[test]
    fn stopping_a_starting_plant_cancels_the_start() {
        let mut p = new_plant(PlantKind::Coal);
        p.start().unwrap();
        assert_eq!(
            p.stop().unwrap(),
            vec![DomainEvent::PlantStopped { plant: PlantId(1) }]
        );
        assert_eq!(p.status(), PlantStatus::Offline);
    }

    #[test]
    fn hydro_never_exceeds_budget_and_reports_once() {
        let mut p = online(PlantKind::Hydro);
        p.reset_hydro_budget(MegawattHours::new(10.0));
        p.set_target(Megawatts::new(800.0)).unwrap();
        let events: Vec<_> = (0..120)
            .flat_map(|_| {
                let a = p.hydro_available();
                p.step(a)
            })
            .collect();
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, DomainEvent::HydroBudgetExhausted { .. }))
                .count(),
            1
        );
        assert_eq!(p.hydro_budget(), Some(MegawattHours::ZERO));
        assert_eq!(p.output(), Megawatts::ZERO);
    }

    #[test]
    fn hydro_available_is_budget_times_sixty_capped_at_capacity() {
        let mut p = online(PlantKind::Hydro);
        p.reset_hydro_budget(MegawattHours::new(5.0));
        assert_eq!(p.hydro_available(), Megawatts::new(300.0));
        p.reset_hydro_budget(MegawattHours::new(100.0));
        assert_eq!(p.hydro_available(), Megawatts::new(800.0));
        assert_eq!(new_plant(PlantKind::Coal).hydro_budget(), None);
    }

    #[test]
    fn construction_commissions_after_build_months() {
        let mut spec = tiny_region().spec(PlantKind::Coal).clone();
        spec.build_months = 2;
        let mut p =
            Plant::new_under_construction(PlantId(1), NodeId(1), CellCoord { x: 7, y: 3 }, spec);
        assert_eq!(
            p.status(),
            PlantStatus::UnderConstruction { months_left: 2 }
        );
        assert!(p.advance_construction(april()).is_empty());
        let second = april().next();
        assert_eq!(
            p.advance_construction(second),
            vec![DomainEvent::PlantCommissioned {
                plant: PlantId(1),
                kind: PlantKind::Coal
            }]
        );
        assert_eq!(p.status(), PlantStatus::Offline);
        assert_eq!(p.commissioned_on(), Some(second));
        assert!(p.advance_construction(second.next()).is_empty());
    }

    #[test]
    fn commissioned_solar_starts_online_at_zero() {
        let p = new_plant(PlantKind::Solar);
        assert_eq!(
            p.status(),
            PlantStatus::Online {
                output: Megawatts::ZERO,
                target: Megawatts::ZERO
            }
        );
        assert_eq!(new_plant(PlantKind::Coal).status(), PlantStatus::Offline);
    }

    #[test]
    fn solar_follows_availability_without_ramp_limit() {
        let mut p = new_plant(PlantKind::Solar);
        p.step(Megawatts::new(250.0));
        assert_eq!(p.output(), Megawatts::new(250.0));
        p.step(Megawatts::new(900.0));
        assert_eq!(p.output(), Megawatts::new(300.0));
        p.step(Megawatts::ZERO);
        assert_eq!(p.output(), Megawatts::ZERO);
    }

    #[test]
    fn availability_caps_a_dispatchable_target() {
        let mut p = online(PlantKind::Peaker);
        p.set_target(Megawatts::new(200.0)).unwrap();
        for _ in 0..10 {
            p.step(Megawatts::new(100.0));
        }
        assert_eq!(p.output(), Megawatts::new(100.0));
    }

    #[test]
    fn trip_takes_a_running_plant_offline_once() {
        let mut p = online(PlantKind::Coal);
        assert_eq!(
            p.trip(),
            vec![DomainEvent::PlantTripped { plant: PlantId(1) }]
        );
        assert_eq!(p.status(), PlantStatus::Offline);
        assert!(p.trip().is_empty());

        let mut s = new_plant(PlantKind::Coal);
        s.start().unwrap();
        assert_eq!(s.trip().len(), 1);
        assert_eq!(s.status(), PlantStatus::Offline);

        let mut u = Plant::new_under_construction(
            PlantId(2),
            NodeId(1),
            CellCoord { x: 7, y: 3 },
            tiny_region().spec(PlantKind::Coal).clone(),
        );
        assert!(u.trip().is_empty());
        assert!(matches!(u.status(), PlantStatus::UnderConstruction { .. }));
    }

    #[test]
    fn headroom_is_ramp_limited_and_zero_unless_online_dispatchable() {
        let mut p = online(PlantKind::Ccgt);
        assert_eq!(p.headroom_up(Megawatts::new(500.0)), Megawatts::new(25.0));
        assert_eq!(p.headroom_down(), Megawatts::ZERO);
        p.set_target(Megawatts::new(500.0)).unwrap();
        p.step(Megawatts::new(500.0));
        p.step(Megawatts::new(500.0));
        assert_eq!(p.headroom_down(), Megawatts::new(25.0));
        assert_eq!(p.headroom_up(Megawatts::new(260.0)), Megawatts::new(10.0));
        assert_eq!(p.headroom_up(Megawatts::new(100.0)), Megawatts::ZERO);
        assert_eq!(
            new_plant(PlantKind::Ccgt).headroom_up(Megawatts::new(500.0)),
            Megawatts::ZERO
        );
        assert_eq!(new_plant(PlantKind::Ccgt).headroom_down(), Megawatts::ZERO);
        let solar = new_plant(PlantKind::Solar);
        assert_eq!(solar.headroom_up(Megawatts::new(300.0)), Megawatts::ZERO);
        assert_eq!(solar.headroom_down(), Megawatts::ZERO);
    }

    #[test]
    fn force_to_minimum_drops_an_online_plant_to_min_output() {
        let mut p = online(PlantKind::Ccgt);
        p.set_target(Megawatts::new(500.0)).unwrap();
        p.step(Megawatts::new(500.0));
        p.force_to_minimum();
        assert_eq!(
            p.status(),
            PlantStatus::Online {
                output: Megawatts::new(200.0),
                target: Megawatts::new(200.0)
            }
        );
        let mut off = new_plant(PlantKind::Ccgt);
        off.force_to_minimum();
        assert_eq!(off.status(), PlantStatus::Offline);
    }

    #[test]
    fn reconnect_brings_tripped_solar_back_at_zero_only() {
        let mut solar = new_plant(PlantKind::Solar);
        solar.step(Megawatts::new(120.0));
        solar.trip();
        solar.reconnect();
        assert_eq!(
            solar.status(),
            PlantStatus::Online {
                output: Megawatts::ZERO,
                target: Megawatts::ZERO
            }
        );
        let mut coal = new_plant(PlantKind::Coal);
        coal.reconnect();
        assert_eq!(coal.status(), PlantStatus::Offline);
    }

    #[test]
    fn auto_follow_is_for_dispatchable_plants_only() {
        let mut p = new_plant(PlantKind::Coal);
        assert!(!p.auto_follow());
        p.set_auto_follow(true).unwrap();
        assert!(p.auto_follow());
        assert_eq!(
            new_plant(PlantKind::Solar).set_auto_follow(true),
            Err(DomainError::PlantNotDispatchable { plant: PlantId(1) })
        );
    }

    #[test]
    fn day_override_and_boundary_state_defaults() {
        let mut p = new_plant(PlantKind::Coal);
        assert_eq!(p.commitment(), CommitmentSchedule::none());
        assert_eq!(p.initial_target(), Megawatts::new(240.0));
        assert!(p.previous_month_offline() && !p.is_commitment_suppressed());
        p.set_commitment(CommitmentSchedule::all_day());
        p.set_initial_target(Megawatts::new(400.0));
        p.set_previous_month_offline(false);
        p.suppress_commitment_for_day();
        assert_eq!(p.commitment(), CommitmentSchedule::all_day());
        assert_eq!(p.initial_target(), Megawatts::new(400.0));
        assert!(!p.previous_month_offline() && p.is_commitment_suppressed());
        p.clear_day_overrides();
        assert!(!p.is_commitment_suppressed());
        // start/stop/trip never touch the suppression flag.
        p.suppress_commitment_for_day();
        p.start().unwrap();
        p.trip();
        assert!(p.is_commitment_suppressed());
        let u = Plant::new_under_construction(
            PlantId(2),
            NodeId(1),
            CellCoord { x: 7, y: 3 },
            tiny_region().spec(PlantKind::Coal).clone(),
        );
        assert!(u.previous_month_offline());
    }

    proptest! {
        #[test]
        fn online_output_respects_ramp_and_bounds(targets in prop::collection::vec(0.0f64..=500.0, 1..200)) {
            let mut p = online(PlantKind::Ccgt);
            for t in targets {
                let before = p.output();
                let _ = p.set_target(Megawatts::new(t));
                p.step(Megawatts::new(500.0));
                prop_assert!((p.output() - before).abs().value() <= 25.0 + 1e-9);
                prop_assert!(p.output().value() >= 200.0 - 1e-9 && p.output().value() <= 500.0 + 1e-9);
            }
        }
    }
}
