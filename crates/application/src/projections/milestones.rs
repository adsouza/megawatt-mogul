//! One-off achievements, for the player's sense of progress.

use domain::events::DomainEvent;
use domain::region::{Calendar, PlantKind, RegionDefinition};
use domain::session::{GameSession, MonthlyReport};
use domain::time::{Season, SimMonth};
use domain::units::{Megawatts, Ratio};

use super::{MonthlyReportProjection, Projection};

/// An achievement the player can earn once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MilestoneKind {
    /// A month's peak demand reached this many gigawatts.
    ServePeak { gigawatts: u32 },
    /// MW-scale target for regions too small for a useful whole-GW milestone (schema 10).
    ServePeakMegawatts { megawatts: u32 },
    /// Every consecutive month of one regional season, none with a strike.
    FullSeasonWithoutStrike,
    /// This many cities are connected.
    ConnectCities { count: u32 },
    /// Twelve closed months in a row with no strike and no coal, CCGT or peaker energy: the hidden
    /// bonus, revealed once the three regional milestones are reached (schema 12).
    FossilFreeYear,
}

/// Legacy targets, retained for decoding schemas 1–9 and preserving earned awards.
pub const MILESTONES: [MilestoneKind; 3] = [
    MilestoneKind::ServePeak { gigawatts: 50 },
    MilestoneKind::FullSeasonWithoutStrike,
    MilestoneKind::ConnectCities { count: 10 },
];

impl MilestoneKind {
    fn same_objective(self, other: Self) -> bool {
        matches!(
            (self, other),
            (
                Self::ServePeak { .. } | Self::ServePeakMegawatts { .. },
                Self::ServePeak { .. } | Self::ServePeakMegawatts { .. }
            ) | (Self::FullSeasonWithoutStrike, Self::FullSeasonWithoutStrike)
                | (Self::ConnectCities { .. }, Self::ConnectCities { .. })
                | (Self::FossilFreeYear, Self::FossilFreeYear)
        )
    }
}

/// Noise-free annual peaks of simultaneous demand, including civil-time offsets. This uses
/// immutable authored loads, so growth, construction and the current month never move the goal.
pub fn regional_milestones(region: &RegionDefinition) -> [MilestoneKind; 3] {
    let mut starter_peak = 0.0f64;
    let mut full_peak = 0.0f64;
    for m in 1..=12 {
        let month = SimMonth::new(region.start().month.year(), m).expect("calendar month");
        for tick in 0..domain::time::TICKS_PER_DAY {
            let mut starter = 0.0;
            let mut full = 0.0;
            for city in region.cities() {
                let demand = city.base_load.value() * region.load_factor(city.id, month, tick);
                full += demand;
                if region.start().connected_cities.contains(&city.id) {
                    starter += demand;
                }
            }
            starter_peak = starter_peak.max(starter);
            full_peak = full_peak.max(full);
        }
    }
    scaled_targets(
        starter_peak,
        full_peak,
        region.start().connected_cities.len(),
        region.cities().len(),
    )
}

fn scaled_targets(
    starter_peak: f64,
    full_peak: f64,
    start: usize,
    total: usize,
) -> [MilestoneKind; 3] {
    let gap = full_peak - starter_peak;
    let step = if gap >= 2_000.0 {
        1_000.0
    } else if gap >= 200.0 {
        100.0
    } else if gap >= 20.0 {
        10.0
    } else {
        1.0
    };
    let midpoint = (starter_peak + full_peak) / 2.0;
    let raw = if midpoint >= 10_000.0 && step >= 1_000.0 {
        // Larger goals read more naturally as multiples of 2 or 5 GW. Keep the
        // closest to the midpoint; small regions retain their finer precision.
        let even = (midpoint / 2_000.0).round() * 2_000.0;
        let five = (midpoint / 5_000.0).round() * 5_000.0;
        if (five - midpoint).abs() < (even - midpoint).abs() {
            five
        } else {
            even
        }
    } else {
        (midpoint / step).round() * step
    };
    let upper = full_peak.floor();
    let lower = (starter_peak.floor() + 1.0).min(upper);
    let mw = raw.clamp(lower, upper) as u32;
    let peak = if mw >= 1_000 && mw.is_multiple_of(1_000) {
        MilestoneKind::ServePeak {
            gigawatts: mw / 1_000,
        }
    } else {
        MilestoneKind::ServePeakMegawatts { megawatts: mw }
    };
    let count = u32::try_from(start + (total - start).div_ceil(2)).unwrap_or(u32::MAX);
    [
        peak,
        MilestoneKind::FullSeasonWithoutStrike,
        MilestoneKind::ConnectCities { count },
    ]
}

/// How far the player is from a milestone, or when they reached it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MilestoneStatus {
    /// Reached in `month` (the month played, or the game's first month for a
    /// milestone met from the start).
    Reached {
        month: SimMonth,
    },
    InProgress(MilestoneProgress),
}

/// Progress toward a milestone not yet reached.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MilestoneProgress {
    /// The largest monthly peak so far, against the peak to serve.
    Peak { best: Megawatts, goal: Megawatts },
    /// Strike-free closed months in a row within the season being played
    /// (none while a strike issued this month taints it, or when the run
    /// belongs to an earlier season), against the months a season needs.
    StrikeFreeMonths { months: u32, needed: u32 },
    /// Cities connected now, against the cities to connect.
    Cities { connected: u32, needed: u32 },
    /// Closed months in a row with no strike and no fossil energy, against the months needed.
    CleanMonths { months: u32, needed: u32 },
}

/// Clean, strike-free closed months in a row that earn [`MilestoneKind::FossilFreeYear`].
pub const FOSSIL_FREE_MONTHS: u32 = 12;
/// Fossil energy at or below this is rounding, not generation; a month's start-up tail is above it.
const FOSSIL_EPSILON_MWH: f64 = 1e-9;
const FOSSIL_KINDS: [PlantKind; 3] = [PlantKind::Coal, PlantKind::Ccgt, PlantKind::Peaker];

/// Whether `report`'s month counts toward [`MilestoneKind::FossilFreeYear`]: no strike was issued
/// in it and no coal, CCGT or peaker produced energy. A fossil entry listed with zero energy is
/// clean; a frequency notice is not a strike.
pub fn month_is_clean(report: &MonthlyReport, struck: bool) -> bool {
    !struck
        && FOSSIL_KINDS.iter().all(|kind| {
            report
                .energy_by_kind
                .get(kind)
                .is_none_or(|energy| energy.value() <= FOSSIL_EPSILON_MWH)
        })
}

/// Whether `report`'s month drew a strike, read back from the report: it mirrors
/// `Regulator::assess_month`, where unserved energy over the threshold is a strike, and a failed
/// frequency month is a strike unless it drew the once-a-year notice. Saves written before schema
/// 12 kept no strike flag per month, so their run is rebuilt from this.
pub fn month_struck(report: &MonthlyReport, strike_threshold: Ratio) -> bool {
    let unserved_failed = report.demand.value() > 0.0
        && report.unserved.value() / report.demand.value() > strike_threshold.value();
    unserved_failed || (report.frequency_failed && !report.frequency_notice)
}

const MW_PER_GW: f64 = 1_000.0;

/// A run of strike-free closed months within one season.
#[derive(Debug, Clone, Copy, PartialEq)]
struct StrikeFreeRun {
    season: Season,
    months: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MilestoneProjection {
    /// In the order reached, each with the month it was reached in.
    reached: Vec<(MilestoneKind, SimMonth)>,
    newly_reached: Vec<MilestoneKind>,
    peak_demand: Megawatts,
    connected_cities: u32,
    /// A strike has been issued since the last month closed; it belongs to
    /// the month about to close, which is therefore not strike-free.
    strike_pending: bool,
    run: Option<StrikeFreeRun>,
    goals: [MilestoneKind; 3],
    /// Consecutive closed months that were strike-free and fossil-free, counted from the start of
    /// the game whether or not the bonus is revealed yet (schema 12).
    clean_run: u32,
    /// The month the three regional milestones were all reached, which reveals the bonus.
    bonus_revealed: Option<SimMonth>,
}

impl MilestoneProjection {
    /// Seeds the connected-city count from the session's initial state.
    pub fn new(session: &GameSession) -> Self {
        let mut projection = Self {
            reached: Vec::new(),
            newly_reached: Vec::new(),
            peak_demand: Megawatts::new(0.0),
            connected_cities: 0,
            strike_pending: false,
            run: None,
            goals: regional_milestones(session.region()),
            clean_run: 0,
            bonus_revealed: None,
        };
        projection.observe_connected_cities(session);
        projection
    }

    pub fn targets(&self) -> &[MilestoneKind; 3] {
        &self.goals
    }

    /// Upgrade pending targets only; historical achievements keep their original terms and date.
    pub(crate) fn upgrade_targets(&mut self, session: &GameSession) {
        self.goals = regional_milestones(session.region()).map(|target| {
            self.reached
                .iter()
                .find(|(earned, _)| earned.same_objective(target))
                .map_or(target, |(earned, _)| *earned)
        });
        // Credit newly attainable goals at the restored month, without replaying achievement
        // notifications. Preserve any notification state that was already in the snapshot.
        let pending = self.newly_reached.len();
        self.check(session.clock().month(), session.region().calendar());
        self.newly_reached.truncate(pending);
    }

    /// Rebuilds the fossil-free run of a save written before schema 12 from its monthly reports,
    /// then reveals and awards the bonus at the restored month if the goals and the run allow.
    /// Like [`upgrade_targets`](Self::upgrade_targets) it raises no notification.
    pub(crate) fn upgrade_fossil_free(
        &mut self,
        reports: &MonthlyReportProjection,
        session: &GameSession,
    ) {
        let threshold = session.region().regulator().strike_threshold;
        self.clean_run = u32::try_from(
            reports
                .history()
                .iter()
                .rev()
                .take_while(|report| month_is_clean(report, month_struck(report, threshold)))
                .count(),
        )
        .unwrap_or(u32::MAX);
        let pending = self.newly_reached.len();
        self.check(session.clock().month(), session.region().calendar());
        self.newly_reached.truncate(pending);
    }

    /// The hidden bonus milestone, once the three regional milestones are reached.
    pub fn bonus(&self) -> Option<MilestoneKind> {
        self.bonus_revealed.map(|_| MilestoneKind::FossilFreeYear)
    }

    /// Milestones reached so far, in the order they were reached.
    pub fn reached(&self) -> Vec<MilestoneKind> {
        self.reached.iter().map(|(kind, _)| *kind).collect()
    }

    /// When `kind` was reached, or how far the player is from it in `now`,
    /// the month being played (or reviewed).
    pub fn status(
        &self,
        kind: MilestoneKind,
        now: SimMonth,
        calendar: &Calendar,
    ) -> MilestoneStatus {
        if let Some((_, month)) = self.reached.iter().find(|(reached, _)| *reached == kind) {
            return MilestoneStatus::Reached { month: *month };
        }
        MilestoneStatus::InProgress(match kind {
            MilestoneKind::ServePeak { gigawatts } => MilestoneProgress::Peak {
                best: self.peak_demand,
                goal: Megawatts::new(f64::from(gigawatts) * MW_PER_GW),
            },
            MilestoneKind::ServePeakMegawatts { megawatts } => MilestoneProgress::Peak {
                best: self.peak_demand,
                goal: Megawatts::new(f64::from(megawatts)),
            },
            MilestoneKind::FullSeasonWithoutStrike => MilestoneProgress::StrikeFreeMonths {
                months: match self.run {
                    // A run of last season's months is no progress in this one.
                    Some(run) if !self.strike_pending && run.season == calendar.season(now) => {
                        run.months
                    }
                    _ => 0,
                },
                needed: u32::from(calendar.run_length(now)),
            },
            MilestoneKind::ConnectCities { count } => MilestoneProgress::Cities {
                connected: self.connected_cities,
                needed: count,
            },
            MilestoneKind::FossilFreeYear => MilestoneProgress::CleanMonths {
                months: self.clean_run,
                needed: FOSSIL_FREE_MONTHS,
            },
        })
    }

    /// Milestones reached since the last call.
    pub fn take_newly_reached(&mut self) -> Vec<MilestoneKind> {
        std::mem::take(&mut self.newly_reached)
    }

    fn observe_connected_cities(&mut self, session: &GameSession) {
        let connected = session
            .city_views()
            .iter()
            .filter(|city| city.connected)
            .count();
        self.set_connected_cities(
            u32::try_from(connected).unwrap_or(u32::MAX),
            session.clock().month(),
            session.region().calendar(),
        );
    }

    fn set_connected_cities(&mut self, count: u32, month: SimMonth, calendar: &Calendar) {
        self.connected_cities = count;
        self.check(month, calendar);
    }

    fn close_month(&mut self, report: &MonthlyReport, calendar: &Calendar) {
        let month = report.month;
        let season = calendar.season(month);
        self.peak_demand = self.peak_demand.max(report.peak_demand);
        // `StrikeIssued` precedes `MonthClosed`, so the flag is this month's; take it once.
        let struck = std::mem::take(&mut self.strike_pending);
        self.clean_run = if month_is_clean(report, struck) {
            self.clean_run.saturating_add(1)
        } else {
            0
        };
        self.run = if struck {
            None
        } else {
            Some(match self.run {
                Some(run) if run.season == season => StrikeFreeRun {
                    season,
                    months: run.months + 1,
                },
                _ => StrikeFreeRun { season, months: 1 },
            })
        };
        self.check(month, calendar);
    }

    /// Records every milestone whose condition now holds and that is not yet
    /// reached, in the frozen regional target order, as reached in `month`.
    ///
    /// Once all three are reached the hidden bonus is revealed in `month`, and it is earned as
    /// soon as the run, counted since the game began, is long enough.
    fn check(&mut self, month: SimMonth, calendar: &Calendar) {
        for kind in self.goals {
            if !self.is_reached(kind) && self.is_met(kind, month, calendar) {
                self.reached.push((kind, month));
                self.newly_reached.push(kind);
            }
        }
        if self.bonus_revealed.is_none() && self.goals.iter().all(|kind| self.is_reached(*kind)) {
            self.bonus_revealed = Some(month);
        }
        let bonus = MilestoneKind::FossilFreeYear;
        if !self.is_reached(bonus) && self.is_met(bonus, month, calendar) {
            self.reached.push((bonus, month));
            self.newly_reached.push(bonus);
        }
    }

    fn is_reached(&self, kind: MilestoneKind) -> bool {
        self.reached
            .iter()
            .any(|(reached, _)| reached.same_objective(kind))
    }

    fn is_met(&self, kind: MilestoneKind, month: SimMonth, calendar: &Calendar) -> bool {
        match kind {
            MilestoneKind::ServePeak { gigawatts } => {
                self.peak_demand.value() >= f64::from(gigawatts) * MW_PER_GW
            }
            MilestoneKind::ServePeakMegawatts { megawatts } => {
                self.peak_demand.value() >= f64::from(megawatts)
            }
            MilestoneKind::FullSeasonWithoutStrike => self.run.is_some_and(|run| {
                run.season == calendar.season(month)
                    && run.months >= u32::from(calendar.run_length(month))
            }),
            MilestoneKind::ConnectCities { count } => self.connected_cities >= count,
            MilestoneKind::FossilFreeYear => {
                self.bonus_revealed.is_some() && self.clean_run >= FOSSIL_FREE_MONTHS
            }
        }
    }
}

impl Projection for MilestoneProjection {
    fn apply(&mut self, event: &DomainEvent, session: &GameSession) {
        match event {
            DomainEvent::MonthClosed(report) => {
                self.close_month(report, session.region().calendar());
            }
            DomainEvent::StrikeIssued { .. } => self.strike_pending = true,
            DomainEvent::CityConnected { .. } => self.observe_connected_cities(session),
            _ => {}
        }
    }
}

// Portable state schema 1: explicit field order and enum tags.
// Schema 10 appends the frozen goals and schema 12 the fossil-free run and its reveal month; the
// `state_record!` macro supports only one such group, so this is written out.
impl domain::state::State for MilestoneProjection {
    fn write(&self, out: &mut Vec<u8>) {
        self.write_schema(out, domain::state::CURRENT_STATE_SCHEMA);
    }
    fn write_schema(&self, out: &mut Vec<u8>, schema: u32) {
        self.reached.write_schema(out, schema);
        self.newly_reached.write_schema(out, schema);
        self.peak_demand.write_schema(out, schema);
        self.connected_cities.write_schema(out, schema);
        self.strike_pending.write_schema(out, schema);
        self.run.write_schema(out, schema);
        if schema >= 10 {
            self.goals.write_schema(out, schema);
        }
        if schema >= 12 {
            self.clean_run.write_schema(out, schema);
            self.bonus_revealed.write_schema(out, schema);
        }
    }
    fn read(input: &mut domain::state::StateReader<'_>) -> Result<Self, domain::state::StateError> {
        use domain::state::State;
        Ok(Self {
            reached: State::read(input)?,
            newly_reached: State::read(input)?,
            peak_demand: State::read(input)?,
            connected_cities: State::read(input)?,
            strike_pending: State::read(input)?,
            run: State::read(input)?,
            goals: if input.schema >= 10 {
                State::read(input)?
            } else {
                MILESTONES
            },
            clean_run: if input.schema >= 12 {
                State::read(input)?
            } else {
                0
            },
            bonus_revealed: if input.schema >= 12 {
                State::read(input)?
            } else {
                None
            },
        })
    }
}
impl domain::state::State for MilestoneKind {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::ServePeak { gigawatts } => {
                domain::state::State::write(&0u8, out);
                domain::state::State::write(gigawatts, out);
            }
            Self::ServePeakMegawatts { megawatts } => {
                domain::state::State::write(&3u8, out);
                domain::state::State::write(megawatts, out);
            }
            Self::FullSeasonWithoutStrike => {
                domain::state::State::write(&1u8, out);
            }
            Self::ConnectCities { count } => {
                domain::state::State::write(&2u8, out);
                domain::state::State::write(count, out);
            }
            Self::FossilFreeYear => {
                domain::state::State::write(&4u8, out);
            }
        }
    }
    fn read(input: &mut domain::state::StateReader<'_>) -> Result<Self, domain::state::StateError> {
        Ok(match <u8 as domain::state::State>::read(input)? {
            0 => Self::ServePeak {
                gigawatts: domain::state::State::read(input)?,
            },
            1 => Self::FullSeasonWithoutStrike,
            2 => Self::ConnectCities {
                count: domain::state::State::read(input)?,
            },
            3 if input.schema >= 10 => Self::ServePeakMegawatts {
                megawatts: domain::state::State::read(input)?,
            },
            4 if input.schema >= 12 => Self::FossilFreeYear,
            _ => return Err(domain::state::StateError::Invalid),
        })
    }
}
impl domain::state::State for MilestoneProgress {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Peak { best, goal } => {
                domain::state::State::write(&0u8, out);
                domain::state::State::write(best, out);
                domain::state::State::write(goal, out);
            }
            Self::StrikeFreeMonths { months, needed } => {
                domain::state::State::write(&1u8, out);
                domain::state::State::write(months, out);
                domain::state::State::write(needed, out);
            }
            Self::Cities { connected, needed } => {
                domain::state::State::write(&2u8, out);
                domain::state::State::write(connected, out);
                domain::state::State::write(needed, out);
            }
            Self::CleanMonths { months, needed } => {
                domain::state::State::write(&3u8, out);
                domain::state::State::write(months, out);
                domain::state::State::write(needed, out);
            }
        }
    }
    fn read(input: &mut domain::state::StateReader<'_>) -> Result<Self, domain::state::StateError> {
        Ok(match <u8 as domain::state::State>::read(input)? {
            0 => Self::Peak {
                best: domain::state::State::read(input)?,
                goal: domain::state::State::read(input)?,
            },
            1 => Self::StrikeFreeMonths {
                months: domain::state::State::read(input)?,
                needed: domain::state::State::read(input)?,
            },
            2 => Self::Cities {
                connected: domain::state::State::read(input)?,
                needed: domain::state::State::read(input)?,
            },
            3 if input.schema >= 12 => Self::CleanMonths {
                months: domain::state::State::read(input)?,
                needed: domain::state::State::read(input)?,
            },
            _ => return Err(domain::state::StateError::Invalid),
        })
    }
}
impl domain::state::State for MilestoneStatus {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Reached { month } => {
                domain::state::State::write(&0u8, out);
                domain::state::State::write(month, out);
            }
            Self::InProgress(v0) => {
                domain::state::State::write(&1u8, out);
                domain::state::State::write(v0, out);
            }
        }
    }
    fn read(input: &mut domain::state::StateReader<'_>) -> Result<Self, domain::state::StateError> {
        Ok(match <u8 as domain::state::State>::read(input)? {
            0 => Self::Reached {
                month: domain::state::State::read(input)?,
            },
            1 => Self::InProgress(domain::state::State::read(input)?),
            _ => return Err(domain::state::StateError::Invalid),
        })
    }
}
domain::state_record!(StrikeFreeRun { season, months });

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projections::test_support::{report, session};
    use domain::regulation::StrikeReason;
    use domain::time::SimMonth;
    use domain::units::MegawattHours;

    #[test]
    fn regional_targets_are_above_the_starter_grid_and_below_the_full_region() {
        let session = session();
        let projection = MilestoneProjection::new(&session);
        assert_eq!(
            projection.targets()[2],
            MilestoneKind::ConnectCities { count: 3 }
        );
        assert!(
            matches!(projection.targets()[0], MilestoneKind::ServePeakMegawatts { megawatts } if megawatts > 1_000 && megawatts < 2_000)
        );
        assert!(projection.reached().is_empty());
    }

    #[test]
    fn regional_mw_wire_tag_requires_schema_ten_and_legacy_tags_keep_their_units() {
        use domain::state::{State, StateError};
        let kind = MilestoneKind::ServePeakMegawatts { megawatts: 1_500 };
        let bytes = kind.state_bytes();
        assert_eq!(bytes[0], 3);
        assert_eq!(MilestoneKind::from_state_bytes(&bytes).unwrap(), kind);
        for schema in 1..=9 {
            assert_eq!(
                MilestoneKind::from_state_bytes_schema(&bytes, schema),
                Err(StateError::Invalid)
            );
            let legacy = MilestoneKind::ServePeak { gigawatts: 50 };
            assert_eq!(
                MilestoneKind::from_state_bytes_schema(&legacy.state_bytes(), schema).unwrap(),
                legacy
            );
        }
    }

    proptest::proptest! {
        #[test]
        fn regional_target_scaling_remains_attainable_and_requires_expansion(
            starter in 0.0f64..100_000.0, added in 2.0f64..100_000.0,
            start in 1usize..20, remaining in 1usize..100,
        ) {
            let goals = scaled_targets(starter, starter + added, start, start + remaining);
            let peak = match goals[0] {
                MilestoneKind::ServePeak { gigawatts } => f64::from(gigawatts) * 1_000.0,
                MilestoneKind::ServePeakMegawatts { megawatts } => f64::from(megawatts),
                _ => unreachable!("first target is peak demand"),
            };
            proptest::prop_assert!(peak > starter && peak <= starter + added);
            let MilestoneKind::ConnectCities { count } = goals[2] else { unreachable!("third target is cities") };
            proptest::prop_assert!(count as usize > start && count as usize <= start + remaining);
            let connected_share = (count as usize - start) as f64 / remaining as f64;
            proptest::prop_assert!((0.5..=1.0).contains(&connected_share));
        }
    }

    #[test]
    fn regional_targets_are_frozen_and_preserve_earned_legacy_awards_when_upgrading() {
        use domain::state::State;
        let session = session();
        let mut projection = MilestoneProjection::new(&session);
        let targets = *projection.targets();
        projection.set_connected_cities(3, session.clock().month(), session.region().calendar());
        assert_eq!(*projection.targets(), targets);
        assert_eq!(
            projection.reached(),
            [MilestoneKind::ConnectCities { count: 3 }]
        );
        let mut legacy = projection.clone();
        legacy.reached = vec![(
            MilestoneKind::ConnectCities { count: 10 },
            session.clock().month(),
        )];
        legacy.newly_reached.clear();
        let bytes = legacy.state_bytes_schema(9);
        let mut restored = MilestoneProjection::from_state_bytes_schema(&bytes, 9).unwrap();
        restored.upgrade_targets(&session);
        assert_eq!(
            restored.targets()[2],
            MilestoneKind::ConnectCities { count: 10 }
        );
        assert_eq!(restored.reached(), legacy.reached());
        assert_eq!(restored.connected_cities, legacy.connected_cities);
        assert_eq!(restored.peak_demand, legacy.peak_demand);
        assert_eq!(
            MilestoneProjection::from_state_bytes(&restored.state_bytes()).unwrap(),
            restored
        );
    }

    #[test]
    fn regional_peak_and_city_awards_fire_once_at_their_exact_frozen_thresholds() {
        let session = session();
        let mut projection = MilestoneProjection::new(&session);
        let peak = projection.goals[0];
        let mw = match peak {
            MilestoneKind::ServePeak { gigawatts } => gigawatts * 1_000,
            MilestoneKind::ServePeakMegawatts { megawatts } => megawatts,
            _ => unreachable!("peak objective"),
        };
        let calendar = session.region().calendar();
        let feb = SimMonth::new(2027, 2).unwrap();
        projection.close_month(&report(2027, 1, f64::from(mw - 1)), calendar);
        assert!(projection.reached().is_empty());
        projection.close_month(&report(2027, 2, f64::from(mw)), calendar);
        projection.set_connected_cities(3, feb, calendar);
        projection.set_connected_cities(3, feb, calendar);
        assert_eq!(
            projection.take_newly_reached(),
            [peak, MilestoneKind::ConnectCities { count: 3 }]
        );
        projection.close_month(&report(2027, 3, f64::from(mw) * 2.0), calendar);
        assert!(projection.take_newly_reached().is_empty());
    }

    #[test]
    fn regional_upgrade_credits_saved_progress_without_duplicate_notifications() {
        let session = session();
        let mut projection = legacy_projection();
        projection.peak_demand = Megawatts::new(2_000.0);
        projection.connected_cities = 3;
        projection.upgrade_targets(&session);
        let earned = projection.reached();
        assert_eq!(earned, [projection.goals[0], projection.goals[2]]);
        for kind in &earned {
            assert_eq!(
                projection.status(*kind, session.clock().month(), session.region().calendar()),
                MilestoneStatus::Reached {
                    month: session.clock().month()
                }
            );
        }
        assert!(projection.take_newly_reached().is_empty());
        projection.upgrade_targets(&session);
        assert_eq!(projection.reached(), earned);
    }

    // These original threshold/calendar contracts exercise legacy 50 GW / 10-city saves.
    fn legacy_projection() -> MilestoneProjection {
        let mut projection = MilestoneProjection::new(&session());
        projection.goals = MILESTONES;
        projection
    }

    #[test]
    fn india_calendar_needs_two_winter_months_and_four_monsoon_months() {
        use Season::*;
        use domain::region::Calendar;
        let calendar = Calendar {
            seasons: [
                Winter,
                Winter,
                Summer,
                Summer,
                Summer,
                Monsoon,
                Monsoon,
                Monsoon,
                Monsoon,
                PostMonsoon,
                PostMonsoon,
                PostMonsoon,
            ],
        };
        let mut milestones = legacy_projection();
        let jan = SimMonth::new(2027, 1).unwrap();
        let feb = SimMonth::new(2027, 2).unwrap();
        let jul = SimMonth::new(2027, 7).unwrap();
        assert_eq!(
            milestones.status(MilestoneKind::FullSeasonWithoutStrike, jan, &calendar),
            MilestoneStatus::InProgress(MilestoneProgress::StrikeFreeMonths {
                months: 0,
                needed: 2
            })
        );
        assert_eq!(
            milestones.status(MilestoneKind::FullSeasonWithoutStrike, jul, &calendar),
            MilestoneStatus::InProgress(MilestoneProgress::StrikeFreeMonths {
                months: 0,
                needed: 4
            })
        );
        milestones.close_month(&report(2027, 1, 0.0), &calendar);
        assert_eq!(
            milestones.status(MilestoneKind::FullSeasonWithoutStrike, feb, &calendar),
            MilestoneStatus::InProgress(MilestoneProgress::StrikeFreeMonths {
                months: 1,
                needed: 2
            })
        );
        milestones.close_month(&report(2027, 2, 0.0), &calendar);
        assert_eq!(
            milestones.status(MilestoneKind::FullSeasonWithoutStrike, feb, &calendar),
            MilestoneStatus::Reached { month: feb }
        );
    }

    #[test]
    fn connecting_a_city_cannot_count_an_incomplete_previous_season() {
        use Season::*;
        let calendar = Calendar {
            seasons: [
                Winter,
                Winter,
                Summer,
                Summer,
                Summer,
                Monsoon,
                Monsoon,
                Monsoon,
                Monsoon,
                PostMonsoon,
                PostMonsoon,
                PostMonsoon,
            ],
        };
        let mut milestones = legacy_projection();
        for m in [7, 8, 9] {
            milestones.close_month(&report(2027, m, 0.0), &calendar);
        }
        milestones.set_connected_cities(3, SimMonth::new(2027, 10).unwrap(), &calendar);
        assert!(
            !milestones
                .reached()
                .contains(&MilestoneKind::FullSeasonWithoutStrike)
        );
    }

    fn close(projection: &mut MilestoneProjection, month: u8, peak_mw: f64) {
        let event = DomainEvent::MonthClosed(Box::new(report(2027, month, peak_mw)));
        projection.apply(&event, &session());
    }

    fn strike(projection: &mut MilestoneProjection) {
        let event = DomainEvent::StrikeIssued {
            strikes: 1,
            reason: StrikeReason::FrequencyPerformance,
        };
        projection.apply(&event, &session());
    }

    fn notice(projection: &mut MilestoneProjection, month: u8) {
        let month = SimMonth::new(2027, month).unwrap();
        projection.apply(&DomainEvent::FrequencyNoticeIssued { month }, &session());
    }

    const FULL_SEASON: MilestoneKind = MilestoneKind::FullSeasonWithoutStrike;
    const PEAK_50: MilestoneKind = MilestoneKind::ServePeak { gigawatts: 50 };

    #[test]
    fn serve_peak_milestone_reached_once() {
        let mut milestones = legacy_projection();
        close(&mut milestones, 1, 50_000.0);
        close(&mut milestones, 2, 50_000.0);
        assert_eq!(milestones.reached(), [PEAK_50]);
        assert_eq!(milestones.take_newly_reached(), vec![PEAK_50]);
        assert_eq!(milestones.take_newly_reached(), vec![]);
    }

    #[test]
    fn peak_below_threshold_does_not_count() {
        let mut milestones = legacy_projection();
        close(&mut milestones, 1, 49_999.0);
        assert_eq!(milestones.reached(), []);
    }

    #[test]
    fn full_season_requires_three_strike_free_months_of_one_season() {
        let mut clean = legacy_projection();
        for month in [3, 4, 5] {
            close(&mut clean, month, 100.0);
        }
        assert_eq!(clean.reached(), [FULL_SEASON]);

        // A strike issued while April closes taints April.
        let mut struck = legacy_projection();
        close(&mut struck, 3, 100.0);
        strike(&mut struck);
        close(&mut struck, 4, 100.0);
        close(&mut struck, 5, 100.0);
        assert_eq!(struck.reached(), []);
    }

    #[test]
    fn full_season_milestone_ignores_notices() {
        let mut milestones = legacy_projection();
        close(&mut milestones, 3, 100.0);
        notice(&mut milestones, 4);
        close(&mut milestones, 4, 100.0);
        close(&mut milestones, 5, 100.0);
        assert_eq!(milestones.reached(), [FULL_SEASON]);
    }

    #[test]
    fn any_strike_reason_taints_the_month() {
        for reason in [
            StrikeReason::UnservedEnergy,
            StrikeReason::FrequencyPerformance,
        ] {
            let mut milestones = legacy_projection();
            close(&mut milestones, 3, 100.0);
            milestones.apply(
                &DomainEvent::StrikeIssued { strikes: 1, reason },
                &session(),
            );
            close(&mut milestones, 4, 100.0);
            close(&mut milestones, 5, 100.0);
            assert_eq!(milestones.reached(), []);
        }
    }

    #[test]
    fn a_season_change_starts_a_new_run() {
        let mut milestones = legacy_projection();
        for month in [4, 5, 6, 7] {
            close(&mut milestones, month, 100.0);
        }
        assert_eq!(milestones.reached(), []);
        close(&mut milestones, 8, 100.0);
        assert_eq!(milestones.reached(), [FULL_SEASON]);
    }

    #[test]
    fn a_strike_resets_the_run_and_a_later_clean_season_still_counts() {
        let mut milestones = legacy_projection();
        close(&mut milestones, 3, 100.0);
        close(&mut milestones, 4, 100.0);
        strike(&mut milestones);
        close(&mut milestones, 5, 100.0);
        assert_eq!(milestones.reached(), []);
        for month in [6, 7, 8] {
            close(&mut milestones, month, 100.0);
        }
        assert_eq!(milestones.reached(), [FULL_SEASON]);
    }

    #[test]
    fn reached_preserves_order_reached_without_duplicates() {
        let mut milestones = legacy_projection();
        for month in [3, 4, 5] {
            close(&mut milestones, month, 100.0);
        }
        close(&mut milestones, 6, 60_000.0);
        for month in [7, 8] {
            close(&mut milestones, month, 100.0);
        }
        assert_eq!(milestones.reached(), [FULL_SEASON, PEAK_50]);
    }

    #[test]
    fn connect_cities_reached_at_the_threshold() {
        let mut milestones = legacy_projection();
        let month = SimMonth::new(2027, 3).unwrap();
        milestones.set_connected_cities(9, month, session().region().calendar());
        assert_eq!(milestones.reached(), []);
        milestones.set_connected_cities(10, month, session().region().calendar());
        milestones.set_connected_cities(11, month, session().region().calendar());
        assert_eq!(
            milestones.reached(),
            [MilestoneKind::ConnectCities { count: 10 }]
        );
    }

    #[test]
    fn connected_cities_are_counted_at_the_start_and_on_connection_events() {
        use domain::ids::CityId;
        use domain::region::RegionDefinition;
        use domain::region::fixtures::tiny_region_parts;
        use domain::session::GameSettings;
        use std::sync::Arc;

        let two = session();
        let mut milestones = MilestoneProjection::new(&two);
        assert_eq!(milestones.connected_cities, 2);

        let mut parts = tiny_region_parts();
        parts.start.connected_cities.push(CityId(3));
        let three = GameSession::new(
            Arc::new(RegionDefinition::try_from(parts).unwrap()),
            1,
            GameSettings::default(),
        );
        // Unrelated events never recount; a connection event does.
        milestones.apply(&DomainEvent::Blackout, &three);
        assert_eq!(milestones.connected_cities, 2);
        milestones.apply(&DomainEvent::CityConnected { city: CityId(3) }, &three);
        assert_eq!(milestones.connected_cities, 3);
    }

    #[test]
    fn status_shows_progress_until_reached_and_then_the_month() {
        let start = session().clock().month();
        // The month in review: the run's own season.
        let now = SimMonth::new(2027, 4).unwrap();
        let mut milestones = legacy_projection();
        let cities = MilestoneKind::ConnectCities { count: 10 };
        assert_eq!(
            milestones.status(PEAK_50, now, session().region().calendar()),
            MilestoneStatus::InProgress(MilestoneProgress::Peak {
                best: Megawatts::new(0.0),
                goal: Megawatts::new(50_000.0),
            })
        );
        assert_eq!(
            milestones.status(FULL_SEASON, now, session().region().calendar()),
            MilestoneStatus::InProgress(MilestoneProgress::StrikeFreeMonths {
                months: 0,
                needed: 3,
            })
        );
        assert_eq!(
            milestones.status(cities, now, session().region().calendar()),
            MilestoneStatus::InProgress(MilestoneProgress::Cities {
                connected: 2,
                needed: 10,
            }),
            "the tiny region starts with two cities"
        );

        close(&mut milestones, 3, 30_000.0);
        close(&mut milestones, 4, 20_000.0);
        assert_eq!(
            milestones.status(PEAK_50, now, session().region().calendar()),
            MilestoneStatus::InProgress(MilestoneProgress::Peak {
                best: Megawatts::new(30_000.0),
                goal: Megawatts::new(50_000.0),
            }),
            "the best peak so far, not the last"
        );
        assert_eq!(
            milestones.status(FULL_SEASON, now, session().region().calendar()),
            MilestoneStatus::InProgress(MilestoneProgress::StrikeFreeMonths {
                months: 2,
                needed: 3,
            })
        );
        strike(&mut milestones);
        assert_eq!(
            milestones.status(FULL_SEASON, now, session().region().calendar()),
            MilestoneStatus::InProgress(MilestoneProgress::StrikeFreeMonths {
                months: 0,
                needed: 3,
            }),
            "a strike this month breaks the run"
        );

        close(&mut milestones, 6, 55_000.0);
        assert_eq!(
            milestones.status(PEAK_50, now, session().region().calendar()),
            MilestoneStatus::Reached {
                month: SimMonth::new(2027, 6).unwrap()
            }
        );
        milestones.set_connected_cities(10, start, session().region().calendar());
        assert_eq!(
            milestones.status(cities, now, session().region().calendar()),
            MilestoneStatus::Reached { month: start }
        );
    }

    // The fossil-free bonus. Closes start in March 2027, so the first three are one season.
    const BIG_PEAK: f64 = 10_000.0;

    fn at(index: u32) -> SimMonth {
        let year = 2027 + i32::try_from(index / 12).unwrap();
        SimMonth::new(year, u8::try_from(index % 12).unwrap() + 1).unwrap()
    }

    fn report_at(index: u32) -> domain::session::MonthlyReport {
        let month = at(index + 2);
        report(month.year(), month.month(), BIG_PEAK)
    }

    fn close_at(projection: &mut MilestoneProjection, index: u32) {
        close_report(projection, report_at(index));
    }

    fn close_report(projection: &mut MilestoneProjection, report: domain::session::MonthlyReport) {
        projection.apply(&DomainEvent::MonthClosed(Box::new(report)), &session());
    }

    fn close_with_energy(
        projection: &mut MilestoneProjection,
        index: u32,
        kind: PlantKind,
        mwh: f64,
    ) {
        let mut report = report_at(index);
        report.energy_by_kind.insert(kind, MegawattHours::new(mwh));
        close_report(projection, report);
    }

    fn connect_third_city(projection: &mut MilestoneProjection, index: u32) {
        projection.set_connected_cities(3, at(index + 2), session().region().calendar());
    }

    /// Reveals the bonus after `closes` clean closes: peak and season done, the third city last.
    fn revealed_after(closes: u32) -> MilestoneProjection {
        let mut projection = MilestoneProjection::new(&session());
        for index in 0..closes {
            close_at(&mut projection, index);
        }
        connect_third_city(&mut projection, closes - 1);
        assert!(projection.bonus().is_some(), "{closes} closes then city");
        projection
    }

    fn clean_months(projection: &MilestoneProjection, index: u32) -> MilestoneStatus {
        projection.status(
            MilestoneKind::FossilFreeYear,
            at(index + 2),
            session().region().calendar(),
        )
    }

    fn progress(months: u32) -> MilestoneStatus {
        MilestoneStatus::InProgress(MilestoneProgress::CleanMonths {
            months,
            needed: FOSSIL_FREE_MONTHS,
        })
    }

    #[test]
    fn bonus_hidden_until_three_reached() {
        let mut milestones = MilestoneProjection::new(&session());
        close_at(&mut milestones, 0);
        close_at(&mut milestones, 1);
        connect_third_city(&mut milestones, 1);
        assert_eq!(
            milestones.reached().len(),
            2,
            "the peak and the cities, not yet a full season"
        );
        assert_eq!(milestones.bonus(), None);
    }

    #[test]
    fn bonus_revealed_in_the_month_the_third_is_reached() {
        let mut milestones = MilestoneProjection::new(&session());
        close_at(&mut milestones, 0);
        close_at(&mut milestones, 1);
        connect_third_city(&mut milestones, 1);
        close_at(&mut milestones, 2);
        assert_eq!(milestones.reached().len(), 3);
        assert_eq!(milestones.bonus(), Some(MilestoneKind::FossilFreeYear));
        assert_eq!(milestones.bonus_revealed, Some(at(4)));
        assert_eq!(clean_months(&milestones, 2), progress(3));
        assert!(
            !milestones
                .reached()
                .contains(&MilestoneKind::FossilFreeYear)
        );
    }

    #[test]
    fn run_counts_before_reveal_and_awards_at_reveal() {
        let mut milestones = MilestoneProjection::new(&session());
        for index in 0..14 {
            close_at(&mut milestones, index);
        }
        assert_eq!(milestones.bonus(), None, "the third city is still missing");
        assert_eq!(milestones.clean_run, 14);
        milestones.take_newly_reached();
        connect_third_city(&mut milestones, 13);
        assert_eq!(
            clean_months(&milestones, 13),
            MilestoneStatus::Reached { month: at(15) }
        );
        assert_eq!(
            milestones.take_newly_reached(),
            [
                MilestoneKind::ConnectCities { count: 3 },
                MilestoneKind::FossilFreeYear
            ]
        );
    }

    #[test]
    fn run_of_five_at_reveal_awards_seven_closes_later() {
        let mut milestones = revealed_after(5);
        assert_eq!(clean_months(&milestones, 4), progress(5));
        for index in 5..11 {
            close_at(&mut milestones, index);
            assert!(
                !milestones
                    .reached()
                    .contains(&MilestoneKind::FossilFreeYear),
                "close {index}"
            );
        }
        assert_eq!(clean_months(&milestones, 10), progress(11));
        milestones.take_newly_reached();
        close_at(&mut milestones, 11);
        assert_eq!(
            clean_months(&milestones, 11),
            MilestoneStatus::Reached { month: at(13) }
        );
        assert_eq!(
            milestones.take_newly_reached(),
            [MilestoneKind::FossilFreeYear]
        );
        close_at(&mut milestones, 12);
        assert!(milestones.take_newly_reached().is_empty(), "awarded once");
    }

    #[test]
    fn fossil_energy_resets_the_run() {
        for kind in [PlantKind::Coal, PlantKind::Ccgt, PlantKind::Peaker] {
            let mut milestones = revealed_after(5);
            close_with_energy(&mut milestones, 5, kind, 0.5);
            assert_eq!(clean_months(&milestones, 5), progress(0), "{kind:?}");
            close_at(&mut milestones, 6);
            assert_eq!(clean_months(&milestones, 6), progress(1), "{kind:?}");
        }
    }

    #[test]
    fn non_fossil_energy_does_not_reset_the_run() {
        let mut milestones = revealed_after(5);
        for (index, kind) in [
            PlantKind::Nuclear,
            PlantKind::Hydro,
            PlantKind::Wind,
            PlantKind::Solar,
        ]
        .into_iter()
        .enumerate()
        {
            close_with_energy(&mut milestones, 5 + index as u32, kind, 50_000.0);
        }
        assert_eq!(clean_months(&milestones, 8), progress(9));
    }

    #[test]
    fn zero_energy_fossil_entries_do_not_reset() {
        let mut milestones = revealed_after(5);
        let mut report = report_at(5);
        for kind in [PlantKind::Coal, PlantKind::Ccgt, PlantKind::Peaker] {
            report.energy_by_kind.insert(kind, MegawattHours::new(0.0));
        }
        close_report(&mut milestones, report);
        assert_eq!(clean_months(&milestones, 5), progress(6));
    }

    #[test]
    fn energy_at_epsilon_does_not_reset() {
        for kind in [PlantKind::Coal, PlantKind::Ccgt, PlantKind::Peaker] {
            let mut milestones = revealed_after(5);
            close_with_energy(&mut milestones, 5, kind, 1e-9);
            assert_eq!(clean_months(&milestones, 5), progress(6), "{kind:?}");
            close_with_energy(&mut milestones, 6, kind, 2e-9);
            assert_eq!(clean_months(&milestones, 6), progress(0), "{kind:?}");
        }
    }

    #[test]
    fn strike_resets_the_run() {
        let mut milestones = revealed_after(5);
        strike(&mut milestones);
        close_at(&mut milestones, 5);
        assert_eq!(clean_months(&milestones, 5), progress(0));
        close_at(&mut milestones, 6);
        assert_eq!(clean_months(&milestones, 6), progress(1));
    }

    #[test]
    fn frequency_notice_does_not_reset_the_run() {
        let mut milestones = revealed_after(5);
        notice(&mut milestones, 7);
        close_at(&mut milestones, 5);
        assert_eq!(clean_months(&milestones, 5), progress(6));
    }

    #[test]
    fn a_struck_fossil_month_is_not_clean_for_either_reason() {
        let clean = report_at(0);
        assert!(month_is_clean(&clean, false));
        assert!(!month_is_clean(&clean, true));
        let mut fossil = report_at(0);
        fossil
            .energy_by_kind
            .insert(PlantKind::Coal, MegawattHours::new(1.0));
        assert!(!month_is_clean(&fossil, false));
        assert!(!month_is_clean(&fossil, true));
    }

    #[test]
    fn bonus_wire_tags_require_schema_twelve() {
        use domain::state::{State, StateError};
        let kind = MilestoneKind::FossilFreeYear;
        assert_eq!(kind.state_bytes(), [4]);
        let progress = MilestoneProgress::CleanMonths {
            months: 7,
            needed: 12,
        };
        let bytes = progress.state_bytes();
        assert_eq!(bytes[0], 3);
        assert_eq!(MilestoneKind::from_state_bytes(&[4]).unwrap(), kind);
        assert_eq!(
            MilestoneProgress::from_state_bytes(&bytes).unwrap(),
            progress
        );
        for schema in 1..=11 {
            assert_eq!(
                MilestoneKind::from_state_bytes_schema(&[4], schema),
                Err(StateError::Invalid)
            );
            assert_eq!(
                MilestoneProgress::from_state_bytes_schema(&bytes, schema),
                Err(StateError::Invalid)
            );
        }
        assert_eq!(
            MilestoneKind::from_state_bytes_schema(&[4], 12).unwrap(),
            kind
        );
        assert!(kind.same_objective(MilestoneKind::FossilFreeYear));
        assert!(!kind.same_objective(MilestoneKind::FullSeasonWithoutStrike));
    }

    #[test]
    fn bonus_state_round_trips_at_schema_12() {
        use domain::state::State;
        let mut milestones = revealed_after(5);
        close_at(&mut milestones, 5);
        let bytes = milestones.state_bytes();
        assert_eq!(bytes, milestones.state_bytes_schema(12));
        let restored = MilestoneProjection::from_state_bytes(&bytes).unwrap();
        assert_eq!(restored, milestones);
        assert_eq!(restored.clean_run, 6);
        assert_eq!(restored.bonus_revealed, Some(at(6)));

        let mut earned = revealed_after(12);
        assert!(earned.reached().contains(&MilestoneKind::FossilFreeYear));
        earned.take_newly_reached();
        let bytes = earned.state_bytes();
        assert_eq!(
            MilestoneProjection::from_state_bytes(&bytes).unwrap(),
            earned
        );
    }

    #[test]
    fn schema_11_state_reads_empty_run_unrevealed() {
        use domain::state::State;
        let milestones = revealed_after(5);
        let bytes = milestones.state_bytes_schema(11);
        assert!(bytes.len() < milestones.state_bytes().len());
        let restored = MilestoneProjection::from_state_bytes_schema(&bytes, 11).unwrap();
        assert_eq!(restored.clean_run, 0);
        assert_eq!(restored.bonus_revealed, None);
        assert_eq!(restored.bonus(), None);
        assert_eq!(restored.reached, milestones.reached);
        assert_eq!(restored.goals, milestones.goals);
        assert_eq!(restored.peak_demand, milestones.peak_demand);
        assert_eq!(restored.connected_cities, milestones.connected_cities);
        assert_eq!(restored.run, milestones.run);
    }

    // Old saves: the run is rebuilt from the monthly reports.
    fn reports_of(
        reports: impl IntoIterator<Item = domain::session::MonthlyReport>,
    ) -> MonthlyReportProjection {
        let mut history = MonthlyReportProjection::default();
        for report in reports {
            history.apply(&DomainEvent::MonthClosed(Box::new(report)), &session());
        }
        history
    }

    /// A projection as a schema 11 save restores it: three goals reached, no run, no reveal.
    fn schema_11_with_three_reached() -> MilestoneProjection {
        use domain::state::State;
        let mut live = revealed_after(5);
        live.take_newly_reached();
        let bytes = live.state_bytes_schema(11);
        let old = MilestoneProjection::from_state_bytes_schema(&bytes, 11).unwrap();
        assert_eq!((old.clean_run, old.bonus_revealed), (0, None));
        assert_eq!(old.reached().len(), 3);
        old
    }

    fn struck_report(index: u32) -> domain::session::MonthlyReport {
        let mut report = report_at(index);
        report.demand = MegawattHours::new(1_000.0);
        report.unserved = MegawattHours::new(100.0);
        report
    }

    #[test]
    fn old_save_with_three_reached_reveals_and_awards_silently() {
        let mut old = schema_11_with_three_reached();
        let history = reports_of((0..12).map(report_at));
        old.upgrade_fossil_free(&history, &session());
        let restored_month = session().clock().month();
        assert_eq!(old.clean_run, 12);
        assert_eq!(old.bonus_revealed, Some(restored_month));
        assert!(matches!(
            old.status(MilestoneKind::FossilFreeYear, restored_month, session().region().calendar()),
            MilestoneStatus::Reached { month } if month == restored_month
        ));
        assert_eq!(old.reached().last(), Some(&MilestoneKind::FossilFreeYear));
        assert!(old.newly_reached.is_empty(), "no notification");
    }

    #[test]
    fn old_save_keeps_pending_notifications_and_shows_an_unfinished_run() {
        let mut old = schema_11_with_three_reached();
        old.newly_reached
            .push(MilestoneKind::FullSeasonWithoutStrike);
        // Eleven clean months behind a struck one: revealed, not awarded.
        let history = reports_of(std::iter::once(struck_report(0)).chain((1..12).map(report_at)));
        old.upgrade_fossil_free(&history, &session());
        assert_eq!(old.clean_run, 11);
        assert!(old.bonus().is_some());
        assert!(!old.reached().contains(&MilestoneKind::FossilFreeYear));
        assert_eq!(old.newly_reached, [MilestoneKind::FullSeasonWithoutStrike]);
    }

    #[test]
    fn old_save_run_stops_at_the_latest_fossil_or_struck_month() {
        let mut old = schema_11_with_three_reached();
        let mut fossil = report_at(7);
        fossil
            .energy_by_kind
            .insert(PlantKind::Ccgt, MegawattHours::new(5.0));
        let history = reports_of(
            (0..7)
                .map(report_at)
                .chain([fossil])
                .chain((8..11).map(report_at))
                .chain([struck_report(11)])
                .chain((12..14).map(report_at)),
        );
        old.upgrade_fossil_free(&history, &session());
        assert_eq!(old.clean_run, 2);
        assert!(!old.reached().contains(&MilestoneKind::FossilFreeYear));
    }

    #[test]
    fn old_save_without_three_reached_stays_hidden_whatever_the_run() {
        let mut old = MilestoneProjection::new(&session());
        let history = reports_of((0..14).map(report_at));
        old.upgrade_fossil_free(&history, &session());
        assert_eq!(old.clean_run, 14);
        assert_eq!(old.bonus(), None);
        assert!(!old.reached().contains(&MilestoneKind::FossilFreeYear));
    }

    #[test]
    fn a_frequency_notice_month_does_not_break_an_old_saves_run() {
        let mut old = schema_11_with_three_reached();
        let mut noticed = report_at(4);
        noticed.frequency_failed = true;
        noticed.frequency_notice = true;
        let history = reports_of(
            (0..4)
                .map(report_at)
                .chain([noticed])
                .chain((5..12).map(report_at)),
        );
        old.upgrade_fossil_free(&history, &session());
        assert_eq!(old.clean_run, 12);
    }

    proptest::proptest! {
        /// `month_struck` agrees with a regulator that judged the same month, including when a
        /// notice earlier in the year has already been spent.
        #[test]
        fn month_struck_matches_the_regulator(
            demand in proptest::prop_oneof![proptest::strategy::Just(0.0), 1.0f64..1.0e6],
            share in proptest::prop_oneof![
                proptest::strategy::Just(0.0),
                proptest::strategy::Just(0.005),
                0.0f64..0.02,
            ],
            out_of_band in 0.0f64..=1.0,
            prior_notice in proptest::bool::ANY,
        ) {
            use domain::regulation::Regulator;
            let session = session();
            let params = session.region().regulator();
            let mut regulator = Regulator::new(params);
            let spent = SimMonth::new(2027, 3).unwrap();
            if prior_notice {
                // A failing month with no notice on record draws the notice and spends it.
                let events = regulator.assess_month(
                    spent,
                    MegawattHours::new(1.0),
                    MegawattHours::new(0.0),
                    Ratio::ONE,
                    params,
                );
                let spent_notice = matches!(
                    events.as_slice(),
                    [DomainEvent::FrequencyNoticeIssued { .. }]
                );
                proptest::prop_assert!(spent_notice);
            }
            let month = SimMonth::new(2027, 6).unwrap();
            let out_of_band = Ratio::new(out_of_band).unwrap();
            let events = regulator.assess_month(
                month,
                MegawattHours::new(demand),
                MegawattHours::new(demand * share),
                out_of_band,
                params,
            );
            let mut report = report(2027, 6, 100.0);
            report.demand = MegawattHours::new(demand);
            report.unserved = MegawattHours::new(demand * share);
            report.frequency_failed = Regulator::fails_frequency(out_of_band, params);
            report.frequency_notice = events
                .iter()
                .any(|event| matches!(event, DomainEvent::FrequencyNoticeIssued { .. }));
            let struck = events
                .iter()
                .any(|event| matches!(event, DomainEvent::StrikeIssued { .. }));
            proptest::prop_assert_eq!(
                month_struck(&report, params.strike_threshold),
                struck,
                "{:?}",
                events
            );
        }
    }

    #[test]
    fn a_strike_free_run_from_last_season_is_no_progress_in_this_one() {
        let mut milestones = legacy_projection();
        // March struck, April and May clean: two months of Spring.
        close(&mut milestones, 2, 100.0);
        strike(&mut milestones);
        close(&mut milestones, 3, 100.0);
        close(&mut milestones, 4, 100.0);
        close(&mut milestones, 5, 100.0);
        let progress = |months| {
            MilestoneStatus::InProgress(MilestoneProgress::StrikeFreeMonths { months, needed: 3 })
        };
        let may = SimMonth::new(2027, 5).unwrap();
        assert_eq!(
            milestones.status(FULL_SEASON, may, session().region().calendar()),
            progress(2),
            "May's review"
        );

        // June is Summer: no Summer month has closed yet.
        let june = SimMonth::new(2027, 6).unwrap();
        assert_eq!(
            milestones.status(FULL_SEASON, june, session().region().calendar()),
            progress(0)
        );

        close(&mut milestones, 6, 100.0);
        assert_eq!(
            milestones.status(FULL_SEASON, june, session().region().calendar()),
            progress(1)
        );
    }
}
