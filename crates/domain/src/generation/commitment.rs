//! Daily commitment: a 24-hour on/off pattern that repeats every day of the month.
//!
//! The 24 hourly flags form a circle, so the off-gap across midnight is one gap
//! and a start preparing an early-hour run can fire late in the day.
//!
//! # Timing convention
//!
//! Tick `t` is game minute `t` of the representative day: the interval from
//! instant `t` to instant `t + 1`. Processing tick `t` applies
//! [`commitment_actions`] for `t` first and then steps every plant once, so a
//! plant's state after tick `t`'s step is its state at instant `t + 1` (what an
//! observer sees once the clock reads `t + 1`). Hour `h` is committed when the
//! plant is to be online from instant `60·h` to instant `60·(h + 1)`.
//!
//! - **Start.** A run beginning at boundary `b = 60·h` gets a `Start` at tick
//!   `s = (b − startup) mod 1440`. The steps of ticks `s ..= s + startup − 1`
//!   count the start-up down, so the plant is `Online` at instant `b` exactly:
//!   still `Starting { ticks_left: 1 }` at instant `b − 1`, with no one-tick delay.
//! - **Stop.** A run ending at boundary `e` gets a `Stop` at tick `e`. The plant
//!   is still `Online` at instant `e` and ramps down from tick `e`.
//! - **Initialisation** ([`Plant::initialise_pattern_day`]) sets the state at
//!   instant 0 that this cyclic day implies, as if the previous day had been the
//!   same pattern: a start begun before midnight has `b` ticks left, and a tail
//!   whose stop was at `e` has ramped for `1440 − e` ticks from capacity.

use super::plant::{EPSILON, Plant, PlantStatus};
use crate::error::DomainError;
use crate::ids::PlantId;
use crate::region::PlantSpec;
use crate::time::{SimDuration, TICKS_PER_DAY, TICKS_PER_HOUR};
use crate::units::Megawatts;

/// Which hours of the day (0..=23) a plant is committed to run.
///
/// Only [`CommitmentSchedule::try_new`] builds a mixed pattern, so every mixed
/// schedule fits its plant's shutdown and start-up in every off-gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitmentSchedule([bool; 24]);

impl CommitmentSchedule {
    /// Committed in no hour.
    pub fn none() -> Self {
        Self([false; 24])
    }

    /// Committed in every hour.
    pub fn all_day() -> Self {
        Self([true; 24])
    }

    /// Validates `hours` against `spec` on the 24-hour circle.
    ///
    /// Every maximal off-gap, including one that wraps across midnight, must
    /// hold the worst-case shutdown `ceil(capacity / ramp_per_tick)` followed by
    /// the start-up; an exact fit is valid. All-day and all-off patterns have no
    /// recurring restart and are always valid. A failure reports the first
    /// failing gap in order of the hour it begins.
    ///
    /// Non-dispatchable plants (solar) accept any pattern without validation:
    /// they are never commanded, so the pattern has no physical effect.
    pub fn try_new(hours: [bool; 24], spec: &PlantSpec) -> Result<Self, DomainError> {
        let schedule = Self(hours);
        if !spec.kind.is_dispatchable() {
            return Ok(schedule);
        }
        let shutdown = worst_case_shutdown_ticks(spec);
        let required = shutdown.saturating_add(spec.startup.ticks());
        match schedule.off_gaps().into_iter().find(|&gap| gap < required) {
            Some(gap) => Err(DomainError::CommitmentInfeasible {
                gap: SimDuration::from_ticks(gap),
                shutdown: SimDuration::from_ticks(shutdown),
                startup: spec.startup,
            }),
            None => Ok(schedule),
        }
    }

    /// Whether `hour` is committed. Hours past 23 are never committed.
    pub fn is_committed(&self, hour: u8) -> bool {
        self.0.get(usize::from(hour)).copied().unwrap_or(false)
    }

    pub fn hours(&self) -> [bool; 24] {
        self.0
    }

    pub(super) fn is_all_day(&self) -> bool {
        self.0 == [true; 24]
    }

    /// Hour `h` on the circle (taken mod 24).
    fn on(&self, hour: u32) -> bool {
        self.0[(hour % 24) as usize]
    }

    /// Length in ticks of every maximal off-gap on the circle, ordered by the
    /// hour each gap begins. Empty for all-day and all-off patterns.
    fn off_gaps(&self) -> Vec<u32> {
        let Some(first_on) = (0..24).find(|&h| self.on(h)) else {
            return Vec::new();
        };
        // Walk once round the circle from a committed hour, so no gap is split.
        let mut gaps: Vec<(u32, u32)> = Vec::new();
        let mut open: Option<(u32, u32)> = None;
        for hour in (first_on + 1..=first_on + 24).map(|h| h % 24) {
            match (self.on(hour), open.as_mut()) {
                (true, _) => gaps.extend(open.take()),
                (false, Some((_, len))) => *len += 1,
                (false, None) => open = Some((hour, 1)),
            }
        }
        gaps.sort_unstable_by_key(|&(start, _)| start);
        gaps.into_iter()
            .map(|(_, hours)| hours * TICKS_PER_HOUR)
            .collect()
    }

    /// Whether instant `tick` is an hour boundary entering a committed run.
    fn run_begins_at(&self, tick: u32) -> bool {
        let hour = tick / TICKS_PER_HOUR;
        tick.is_multiple_of(TICKS_PER_HOUR) && !self.on(hour + 23) && self.on(hour)
    }

    /// Whether instant `tick` is an hour boundary leaving a committed run.
    fn run_ends_at(&self, tick: u32) -> bool {
        let hour = tick / TICKS_PER_HOUR;
        tick.is_multiple_of(TICKS_PER_HOUR) && self.on(hour + 23) && !self.on(hour)
    }

    /// The physical state at instant 0 implied by repeating this pattern
    /// daily (see the module's timing convention). `target` is the already
    /// clamped initial target of a run in progress at midnight.
    pub(super) fn initial_status(&self, spec: &PlantSpec, target: Megawatts) -> PlantStatus {
        if self.on(0) {
            return PlantStatus::Online {
                output: target,
                target,
            };
        }
        let Some(last_on) = (0..24).rev().find(|&h| self.on(h)) else {
            return PlantStatus::Offline;
        };
        // The run before midnight stopped at `run_end` (1440 when it ends at
        // midnight itself); assume it was at capacity, the worst case.
        let run_end = (last_on + 1) * TICKS_PER_HOUR;
        let tail = spec.capacity - spec.ramp.over_ticks(TICKS_PER_DAY - run_end);
        if tail.value() > EPSILON {
            return PlantStatus::Stopping { output: tail };
        }
        let next_run = (1..24).find(|&h| self.on(h)).unwrap_or(0) * TICKS_PER_HOUR;
        if next_run < spec.startup.ticks() {
            PlantStatus::Starting {
                ticks_left: next_run,
            }
        } else {
            PlantStatus::Offline
        }
    }
}

/// Ticks to ramp from capacity to zero, `ceil(capacity / ramp_per_tick)`. A
/// quotient within 1e-9 of a whole number counts as that number. A plant that
/// cannot ramp down never finishes a shutdown.
fn worst_case_shutdown_ticks(spec: &PlantSpec) -> u32 {
    let ramp = spec.ramp.per_tick().value();
    if ramp <= 0.0 {
        return u32::MAX;
    }
    let ticks = spec.capacity.value() / ramp;
    let whole = ticks.round();
    let ticks = if (ticks - whole).abs() <= EPSILON {
        whole
    } else {
        ticks.ceil()
    };
    // Float-to-int `as` saturates, which is the intent for absurd quotients.
    ticks as u32
}

/// What a commitment pattern asks of a plant at one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitmentAction {
    Start(PlantId),
    Stop(PlantId),
}

impl CommitmentAction {
    pub fn plant(self) -> PlantId {
        match self {
            Self::Start(plant) | Self::Stop(plant) => plant,
        }
    }
}

/// The automatic start and stop actions due at `tick` (`0..1440`), ordered by
/// `PlantId`. Apply them before stepping the plants (see the module docs).
///
/// Edge-triggered on exact ticks: a `Start` only for an `Offline` plant and a
/// `Stop` only for an `Online` or `Starting` one, so a plant the player has
/// already started or stopped gets no redundant action. Plants whose
/// commitment is suppressed for the day, non-dispatchable plants and plants
/// under construction get none; all-day and all-off patterns have no edges.
pub fn commitment_actions<'a>(
    plants: impl IntoIterator<Item = &'a Plant>,
    tick: u32,
) -> Vec<CommitmentAction> {
    let mut actions: Vec<CommitmentAction> = plants
        .into_iter()
        .filter_map(|plant| action_for(plant, tick))
        .collect();
    actions.sort_by_key(|action| action.plant());
    actions
}

fn action_for(plant: &Plant, tick: u32) -> Option<CommitmentAction> {
    if tick >= TICKS_PER_DAY || !plant.kind().is_dispatchable() || plant.is_commitment_suppressed()
    {
        return None;
    }
    let schedule = plant.commitment();
    match plant.status() {
        PlantStatus::Offline => {
            // `tick == (boundary - startup) mod 1440` exactly when this holds.
            let boundary = (tick + plant.spec().startup.ticks() % TICKS_PER_DAY) % TICKS_PER_DAY;
            schedule
                .run_begins_at(boundary)
                .then_some(CommitmentAction::Start(plant.id()))
        }
        PlantStatus::Online { .. } | PlantStatus::Starting { .. } => schedule
            .run_ends_at(tick)
            .then_some(CommitmentAction::Stop(plant.id())),
        PlantStatus::Stopping { .. } | PlantStatus::UnderConstruction { .. } => None,
    }
}

/// Whether initialising a pattern day incurs a one-off month-boundary start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryStart {
    None,
    /// An all-day plant returning from a whole month offline: no recurring
    /// restart covers its return, so it is charged one boundary start.
    Charged,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DomainError;
    use crate::generation::{Plant, PlantStatus};
    use crate::ids::{CellCoord, NodeId, PlantId};
    use crate::region::fixtures::tiny_region;
    use crate::region::{PlantKind, PlantSpec};
    use crate::time::{SimDuration, SimMonth, TICKS_PER_DAY};
    use crate::units::Megawatts;
    use proptest::prelude::*;

    fn spec(kind: PlantKind) -> PlantSpec {
        tiny_region().spec(kind).clone()
    }

    fn hours_of(committed: impl IntoIterator<Item = u8>) -> [bool; 24] {
        let mut hours = [false; 24];
        for h in committed {
            hours[usize::from(h)] = true;
        }
        hours
    }

    fn all_but(off: &[u8]) -> [bool; 24] {
        let mut hours = [true; 24];
        for &h in off {
            hours[usize::from(h)] = false;
        }
        hours
    }

    fn plant_with_spec(id: u32, spec: PlantSpec, committed: impl IntoIterator<Item = u8>) -> Plant {
        let schedule =
            CommitmentSchedule::try_new(hours_of(committed), &spec).expect("feasible test pattern");
        let mut p = Plant::new_commissioned(
            PlantId(id),
            NodeId(1),
            CellCoord { x: 7, y: 3 },
            spec,
            SimMonth::new(2027, 4).unwrap(),
        );
        p.set_commitment(schedule);
        p
    }

    fn offline_with(kind: PlantKind, committed: impl IntoIterator<Item = u8>) -> Plant {
        plant_with_spec(1, spec(kind), committed)
    }

    fn online_with(kind: PlantKind, committed: impl IntoIterator<Item = u8>) -> Plant {
        let mut p = offline_with(kind, committed);
        p.start().unwrap();
        while !p.is_online() {
            p.step(p.spec().capacity);
        }
        p
    }

    fn infeasible(gap: SimDuration, shutdown: u32, startup: SimDuration) -> DomainError {
        DomainError::CommitmentInfeasible {
            gap,
            shutdown: SimDuration::from_ticks(shutdown),
            startup,
        }
    }

    /// What one representative day did: `states[i][n]` is plant `n`'s status at
    /// instant `i` (0..=1440), and `actions` lists every action with its tick.
    struct DayTrace {
        states: Vec<Vec<PlantStatus>>,
        actions: Vec<(u32, CommitmentAction)>,
    }

    /// Runs one day the way the tick pipeline does: per tick, `manual` (player
    /// commands), then the commitment actions, then one step of every plant at
    /// full availability. Any rejected action panics.
    fn run_day(plants: &mut [Plant], mut manual: impl FnMut(u32, &mut [Plant])) -> DayTrace {
        let snapshot = |ps: &[Plant]| ps.iter().map(Plant::status).collect::<Vec<_>>();
        let mut states = vec![snapshot(plants)];
        let mut actions = Vec::new();
        for tick in 0..TICKS_PER_DAY {
            manual(tick, plants);
            for action in commitment_actions(plants.iter(), tick) {
                let p = plants
                    .iter_mut()
                    .find(|p| p.id() == action.plant())
                    .expect("action names a known plant");
                match action {
                    CommitmentAction::Start(_) => p.start().unwrap(),
                    CommitmentAction::Stop(_) => p.stop().unwrap(),
                };
                actions.push((tick, action));
            }
            for p in plants.iter_mut() {
                let available = p.spec().capacity;
                p.step(available);
            }
            states.push(snapshot(plants));
        }
        DayTrace { states, actions }
    }

    #[test]
    fn ccgt_starts_exactly_at_lead_time() {
        // committed hours 6..=17, startup 2 h
        let p = offline_with(PlantKind::Ccgt, 6..=17);
        assert!(commitment_actions([&p], 239).is_empty());
        assert_eq!(
            commitment_actions([&p], 240),
            vec![CommitmentAction::Start(p.id())]
        );
    }

    #[test]
    fn nuclear_partial_day_is_infeasible() {
        let p = Plant::new_commissioned(
            PlantId(1),
            NodeId(1),
            CellCoord { x: 7, y: 3 },
            spec(PlantKind::Nuclear),
            SimMonth::new(2027, 4).unwrap(),
        );
        let before = p.clone();
        assert_eq!(
            CommitmentSchedule::try_new(hours_of(10..=23), p.spec()),
            Err(infeasible(
                SimDuration::from_hours(10),
                220,
                SimDuration::from_hours(48)
            ))
        );
        assert_eq!(p, before);
        assert_eq!(p.commitment(), CommitmentSchedule::none());
    }

    #[test]
    fn nuclear_all_day_is_online_at_tick_zero() {
        // The 48 h start-up is not carried across months: the boundary is synthetic.
        let mut p = offline_with(PlantKind::Nuclear, 0..24);
        assert_eq!(p.initialise_pattern_day(), BoundaryStart::Charged);
        let min = Megawatts::new(770.0);
        assert_eq!(
            p.status(),
            PlantStatus::Online {
                output: min,
                target: min
            }
        );
        let mut plants = [p];
        let trace = run_day(&mut plants, |_, _| {});
        assert!(trace.actions.is_empty());
        assert!(
            trace
                .states
                .iter()
                .all(|s| matches!(s[0], PlantStatus::Online { .. }))
        );
    }

    #[test]
    fn nuclear_all_off_is_valid() {
        // No restart requirement: all-off is not a 24-hour restart gap.
        let mut p = offline_with(PlantKind::Nuclear, 0..0);
        assert_eq!(p.commitment(), CommitmentSchedule::none());
        assert_eq!(p.initialise_pattern_day(), BoundaryStart::None);
        assert_eq!(p.status(), PlantStatus::Offline);
        assert!((0..TICKS_PER_DAY).all(|t| commitment_actions([&p], t).is_empty()));
    }

    #[test]
    fn off_gap_includes_shutdown_time() {
        // 1500 MW at 25 MW/min: a 60-tick worst-case shutdown plus a 2 h start-up.
        let mut big = spec(PlantKind::Ccgt);
        big.capacity = Megawatts::new(1500.0);
        assert_eq!(
            CommitmentSchedule::try_new(all_but(&[10, 11]), &big),
            Err(infeasible(
                SimDuration::from_hours(2),
                60,
                SimDuration::from_hours(2)
            ))
        );
        assert!(CommitmentSchedule::try_new(all_but(&[10, 11, 12]), &big).is_ok());
    }

    #[test]
    fn midnight_gap_is_validated() {
        // Hours 23 and 0 are one 2 h cyclic gap, not two 1 h fragments.
        let ccgt = spec(PlantKind::Ccgt);
        assert_eq!(
            CommitmentSchedule::try_new(hours_of(1..=22), &ccgt),
            Err(infeasible(
                SimDuration::from_hours(2),
                20,
                SimDuration::from_hours(2)
            ))
        );
        assert!(CommitmentSchedule::try_new(hours_of(1..=21), &ccgt).is_ok());
    }

    #[test]
    fn early_run_initialises_starting_before_midnight() {
        // CCGT first run at hour 1: its 2 h start began at 23:00, so 60 ticks remain.
        let mut p = offline_with(PlantKind::Ccgt, 1..=12);
        assert_eq!(p.initialise_pattern_day(), BoundaryStart::None);
        assert_eq!(p.status(), PlantStatus::Starting { ticks_left: 60 });
    }

    #[test]
    fn late_action_prepares_next_days_early_run() {
        let p = offline_with(PlantKind::Ccgt, 1..=12);
        assert!(commitment_actions([&p], 1379).is_empty());
        assert_eq!(
            commitment_actions([&p], 1380),
            vec![CommitmentAction::Start(p.id())]
        );
        let starts: Vec<u32> = (0..TICKS_PER_DAY)
            .filter(|&t| !commitment_actions([&p], t).is_empty())
            .collect();
        assert_eq!(starts, vec![1380]);
    }

    #[test]
    fn shutdown_tail_can_cross_midnight() {
        // A run ending at midnight is initialised conservatively from capacity.
        let mut p = offline_with(PlantKind::Ccgt, 18..=23);
        p.initialise_pattern_day();
        assert_eq!(
            p.status(),
            PlantStatus::Stopping {
                output: Megawatts::new(500.0)
            }
        );
        assert!(p.output() <= p.spec().capacity);
        // Coal ending at 23:00 has ramped 60 min x 6 MW from 600 MW by midnight.
        let mut coal = offline_with(PlantKind::Coal, 10..=22);
        coal.initialise_pattern_day();
        assert_eq!(
            coal.status(),
            PlantStatus::Stopping {
                output: Megawatts::new(240.0)
            }
        );
    }

    #[test]
    fn scheduled_start_is_online_at_committed_boundary() {
        // Online at boundary b means: Online once tick b-1 has stepped, i.e. at
        // instant b, before tick b's actions. One tick earlier it is still starting.
        let mut plants = vec![
            plant_with_spec(1, spec(PlantKind::Ccgt), 6..=17),
            plant_with_spec(2, spec(PlantKind::Ccgt), 1..=12),
            plant_with_spec(3, spec(PlantKind::Peaker), 8..=8),
            plant_with_spec(4, spec(PlantKind::Coal), 10..=22),
        ];
        for p in &mut plants {
            p.initialise_pattern_day();
        }
        let trace = run_day(&mut plants, |_, _| {});
        for (n, boundary) in [(0, 360), (1, 60), (2, 480), (3, 600)] {
            assert_eq!(
                trace.states[boundary - 1][n],
                PlantStatus::Starting { ticks_left: 1 },
                "plant {n} one tick before its boundary"
            );
            assert!(
                matches!(trace.states[boundary][n], PlantStatus::Online { .. }),
                "plant {n} at its boundary {boundary}"
            );
        }
        // The CCGT runs through 18:00 and begins ramping down on tick 1080.
        assert!(matches!(trace.states[1080][0], PlantStatus::Online { .. }));
        assert!(matches!(
            trace.states[1081][0],
            PlantStatus::Stopping { .. }
        ));
        // The late start leaves the hour-1 CCGT exactly where initialisation put it.
        assert_eq!(
            trace.states[1440][1],
            PlantStatus::Starting { ticks_left: 60 }
        );
    }

    #[test]
    fn no_scheduled_start_for_a_plant_already_running() {
        // Manual start inside the off-gap: no Start at the lead time, no error.
        let mut plants = [offline_with(PlantKind::Ccgt, 6..=17)];
        plants[0].initialise_pattern_day();
        let trace = run_day(&mut plants, |tick, ps| {
            if tick == 100 {
                ps[0].start().unwrap();
            }
        });
        assert_eq!(
            trace.actions,
            vec![(1080, CommitmentAction::Stop(PlantId(1)))]
        );
        assert!(matches!(trace.states[240][0], PlantStatus::Online { .. }));
    }

    proptest! {
        #[test]
        fn accepted_patterns_fit_all_cyclic_gaps(
            hours in any::<[bool; 24]>(),
            // (kind, worst-case shutdown + start-up in ticks), from the fixture.
            (kind, required) in prop::sample::select(vec![
                (PlantKind::Nuclear, 220 + 2880),
                (PlantKind::Coal, 100 + 480),
                (PlantKind::Ccgt, 20 + 120),
                (PlantKind::Peaker, 5 + 10),
                (PlantKind::Hydro, 8 + 5),
            ]),
        ) {
            // Independent oracle: every off hour whose predecessor is on starts a
            // maximal cyclic gap; walk forward around the circle to measure it.
            let mut gaps = Vec::new();
            for start in 0..24 {
                if !hours[start] && hours[(start + 23) % 24] {
                    let len = (0..24).take_while(|k| !hours[(start + k) % 24]).count();
                    gaps.push(u32::try_from(len).unwrap() * 60);
                }
            }
            let mixed = hours.contains(&true) && hours.contains(&false);
            let fits = gaps.iter().all(|&g| g >= required);
            match CommitmentSchedule::try_new(hours, &spec(kind)) {
                Ok(s) => {
                    prop_assert!(!mixed || fits);
                    prop_assert_eq!(s.hours(), hours);
                }
                Err(DomainError::CommitmentInfeasible { gap, shutdown, startup }) => {
                    prop_assert!(mixed && !fits);
                    prop_assert!(gaps.contains(&gap.ticks()) && gap.ticks() < required);
                    prop_assert_eq!(shutdown.ticks() + startup.ticks(), required);
                }
                Err(other) => prop_assert!(false, "unexpected error {other:?}"),
            }
        }
    }

    #[test]
    fn stops_at_first_uncommitted_hour_boundary() {
        let p = online_with(PlantKind::Ccgt, 6..=17);
        assert!(commitment_actions([&p], 1079).is_empty());
        assert_eq!(
            commitment_actions([&p], 1080),
            vec![CommitmentAction::Stop(p.id())]
        );
    }

    #[test]
    fn uncommitted_manual_plant_is_never_touched() {
        let p = online_with(PlantKind::Peaker, 0..0);
        assert!((0..1440).all(|t| commitment_actions([&p], t).is_empty()));
    }

    // Extra edge cases beyond the brief.

    #[test]
    fn commitment_schedule_basics() {
        assert!(!CommitmentSchedule::none().is_committed(3));
        let all = CommitmentSchedule::all_day();
        assert!(all.is_committed(0) && all.is_committed(23) && !all.is_committed(24));
        assert_eq!(all.hours(), [true; 24]);
        assert_eq!(CommitmentSchedule::none().hours(), [false; 24]);
        assert_eq!(
            CommitmentSchedule::try_new([true; 24], &spec(PlantKind::Nuclear)),
            Ok(CommitmentSchedule::all_day())
        );
    }

    #[test]
    fn actions_are_ordered_by_plant_id() {
        let a = plant_with_spec(7, spec(PlantKind::Ccgt), 6..=17);
        let b = plant_with_spec(3, spec(PlantKind::Ccgt), 6..=17);
        assert_eq!(
            commitment_actions([&a, &b], 240),
            vec![
                CommitmentAction::Start(PlantId(3)),
                CommitmentAction::Start(PlantId(7))
            ]
        );
    }

    #[test]
    fn suppression_blocks_actions_until_the_next_initialisation() {
        let mut p = online_with(PlantKind::Ccgt, 6..=17);
        p.suppress_commitment_for_day();
        assert!(commitment_actions([&p], 1080).is_empty());
        p.initialise_pattern_day();
        assert!(!p.is_commitment_suppressed());
        assert_eq!(p.commitment().hours(), hours_of(6..=17));
        assert_eq!(
            commitment_actions([&p], 240),
            vec![CommitmentAction::Start(p.id())]
        );
    }

    #[test]
    fn solar_accepts_any_pattern_but_is_never_commanded() {
        let solar = spec(PlantKind::Solar);
        let mut p = plant_with_spec(1, solar, [3, 9, 15]);
        assert!((0..TICKS_PER_DAY).all(|t| commitment_actions([&p], t).is_empty()));
        // A tripped solar plant reconnects at the next pattern boundary.
        p.trip();
        assert_eq!(p.initialise_pattern_day(), BoundaryStart::None);
        assert_eq!(
            p.status(),
            PlantStatus::Online {
                output: Megawatts::ZERO,
                target: Megawatts::ZERO
            }
        );
    }

    #[test]
    fn initialisation_leaves_plants_under_construction_alone() {
        let mut p = Plant::new_under_construction(
            PlantId(1),
            NodeId(1),
            CellCoord { x: 7, y: 3 },
            spec(PlantKind::Ccgt),
        );
        p.set_commitment(CommitmentSchedule::all_day());
        p.suppress_commitment_for_day();
        let before = p.clone();
        assert_eq!(p.initialise_pattern_day(), BoundaryStart::None);
        assert_eq!(p, before);
        assert!((0..TICKS_PER_DAY).all(|t| commitment_actions([&p], t).is_empty()));
    }

    #[test]
    fn boundary_start_is_charged_only_for_an_all_day_return_from_offline() {
        let mut all_day = offline_with(PlantKind::Coal, 0..24);
        assert_eq!(all_day.initialise_pattern_day(), BoundaryStart::Charged);
        all_day.set_previous_month_offline(false);
        assert_eq!(all_day.initialise_pattern_day(), BoundaryStart::None);
        // A mixed pattern's recurring restart covers the return, even one
        // committed at hour 0 (its start fires late in the day).
        let mut mixed = offline_with(PlantKind::Ccgt, 0..=12);
        assert!(mixed.previous_month_offline());
        assert_eq!(mixed.initialise_pattern_day(), BoundaryStart::None);
        assert!(mixed.is_online());
        let mut off = offline_with(PlantKind::Coal, 0..0);
        assert_eq!(off.initialise_pattern_day(), BoundaryStart::None);
    }

    #[test]
    fn initialisation_clamps_the_initial_target() {
        let mut p = offline_with(PlantKind::Ccgt, 0..24);
        p.set_initial_target(Megawatts::new(9_999.0));
        p.initialise_pattern_day();
        let cap = Megawatts::new(500.0);
        assert_eq!(
            p.status(),
            PlantStatus::Online {
                output: cap,
                target: cap
            }
        );
        assert_eq!(p.initial_target(), cap);
        p.set_initial_target(Megawatts::new(10.0));
        p.initialise_pattern_day();
        assert_eq!(p.initial_target(), Megawatts::new(200.0));
    }

    #[test]
    fn initialisation_replaces_last_months_physical_state() {
        // A plant left mid-start or mid-ramp is reset to what its pattern implies.
        let mut p = offline_with(PlantKind::Coal, 0..0);
        p.start().unwrap();
        p.initialise_pattern_day();
        assert_eq!(p.status(), PlantStatus::Offline);
    }

    /// A pattern built from alternating on/off blocks, rotated: mostly feasible
    /// for fast plants, unlike a uniformly random 24-bit pattern.
    fn block_pattern() -> impl Strategy<Value = [bool; 24]> {
        (
            prop::collection::vec((1usize..=12, 1usize..=12), 1..=6),
            0usize..24,
        )
            .prop_map(|(blocks, rotate)| {
                let mut flat = Vec::new();
                for (on, off) in blocks {
                    flat.extend(std::iter::repeat_n(true, on));
                    flat.extend(std::iter::repeat_n(false, off));
                }
                flat.resize(24, false);
                let mut hours = [false; 24];
                for (h, slot) in hours.iter_mut().enumerate() {
                    *slot = flat[(h + rotate) % 24];
                }
                hours
            })
    }

    proptest! {
        #[test]
        fn initialisation_agrees_with_the_cyclic_day(
            hours in block_pattern(),
            kind in prop::sample::select(vec![
                PlantKind::Coal, PlantKind::Ccgt, PlantKind::Peaker, PlantKind::Hydro,
            ]),
        ) {
            let s = spec(kind);
            let Ok(schedule) = CommitmentSchedule::try_new(hours, &s) else {
                return Ok(());
            };
            let mut p = offline_with(kind, 0..0);
            p.set_commitment(schedule);
            p.initialise_pattern_day();
            let start = p.status();
            let mut plants = [p];
            let end = run_day(&mut plants, |_, _| {}).states[1440][0];
            // The day ends where the next one's initialisation begins, except a
            // stop due at midnight itself and the conservative tail estimate.
            match (start, end) {
                (PlantStatus::Online { .. }, PlantStatus::Online { .. })
                | (PlantStatus::Offline, PlantStatus::Offline)
                | (PlantStatus::Stopping { .. }, PlantStatus::Online { .. } | PlantStatus::Offline) => {}
                (PlantStatus::Starting { ticks_left: a }, PlantStatus::Starting { ticks_left: b }) => {
                    prop_assert_eq!(a, b);
                }
                (PlantStatus::Stopping { output: a }, PlantStatus::Stopping { output: b }) => {
                    prop_assert!(b.value() <= a.value() + 1e-9);
                }
                (a, b) => prop_assert!(false, "initialised {a:?} but the day ended {b:?}"),
            }
        }
    }
}
