//! One-off achievements, for the player's sense of progress.

use domain::events::DomainEvent;
use domain::session::GameSession;
use domain::time::{Season, SimMonth};
use domain::units::Megawatts;

use super::Projection;

/// An achievement the player can earn once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MilestoneKind {
    /// A month's peak demand reached this many gigawatts.
    ServePeak { gigawatts: u32 },
    /// Three consecutive closed months of one season, none with a strike.
    FullSeasonWithoutStrike,
    /// This many cities are connected.
    ConnectCities { count: u32 },
}

/// Every milestone, in the order they are checked and reported.
pub const MILESTONES: [MilestoneKind; 3] = [
    MilestoneKind::ServePeak { gigawatts: 50 },
    MilestoneKind::FullSeasonWithoutStrike,
    MilestoneKind::ConnectCities { count: 10 },
];

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
}

/// Closed months in a row of one season needed for a strike-free season.
const SEASON_MONTHS: u32 = 3;

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
        };
        projection.observe_connected_cities(session);
        projection
    }

    /// Milestones reached so far, in the order they were reached.
    pub fn reached(&self) -> Vec<MilestoneKind> {
        self.reached.iter().map(|(kind, _)| *kind).collect()
    }

    /// When `kind` was reached, or how far the player is from it in `now`,
    /// the month being played (or reviewed).
    pub fn status(&self, kind: MilestoneKind, now: SimMonth) -> MilestoneStatus {
        if let Some((_, month)) = self.reached.iter().find(|(reached, _)| *reached == kind) {
            return MilestoneStatus::Reached { month: *month };
        }
        MilestoneStatus::InProgress(match kind {
            MilestoneKind::ServePeak { gigawatts } => MilestoneProgress::Peak {
                best: self.peak_demand,
                goal: Megawatts::new(f64::from(gigawatts) * MW_PER_GW),
            },
            MilestoneKind::FullSeasonWithoutStrike => MilestoneProgress::StrikeFreeMonths {
                months: match self.run {
                    // A run of last season's months is no progress in this one.
                    Some(run) if !self.strike_pending && run.season == now.season() => run.months,
                    _ => 0,
                },
                needed: SEASON_MONTHS,
            },
            MilestoneKind::ConnectCities { count } => MilestoneProgress::Cities {
                connected: self.connected_cities,
                needed: count,
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
        );
    }

    fn set_connected_cities(&mut self, count: u32, month: SimMonth) {
        self.connected_cities = count;
        self.check(month);
    }

    fn close_month(&mut self, month: SimMonth, peak: Megawatts) {
        let season = month.season();
        self.peak_demand = self.peak_demand.max(peak);
        self.run = if std::mem::take(&mut self.strike_pending) {
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
        self.check(month);
    }

    /// Records every milestone whose condition now holds and that is not yet
    /// reached, in [`MILESTONES`] order, as reached in `month`.
    fn check(&mut self, month: SimMonth) {
        for kind in MILESTONES {
            if !self.reached.iter().any(|(reached, _)| *reached == kind) && self.is_met(kind) {
                self.reached.push((kind, month));
                self.newly_reached.push(kind);
            }
        }
    }

    fn is_met(&self, kind: MilestoneKind) -> bool {
        match kind {
            MilestoneKind::ServePeak { gigawatts } => {
                self.peak_demand.value() >= f64::from(gigawatts) * MW_PER_GW
            }
            MilestoneKind::FullSeasonWithoutStrike => {
                self.run.is_some_and(|run| run.months >= SEASON_MONTHS)
            }
            MilestoneKind::ConnectCities { count } => self.connected_cities >= count,
        }
    }
}

impl Projection for MilestoneProjection {
    fn apply(&mut self, event: &DomainEvent, session: &GameSession) {
        match event {
            DomainEvent::MonthClosed(report) => {
                self.close_month(report.month, report.peak_demand);
            }
            DomainEvent::StrikeIssued { .. } => self.strike_pending = true,
            DomainEvent::CityConnected { .. } => self.observe_connected_cities(session),
            _ => {}
        }
    }
}

// Portable state schema 1: explicit field order and enum tags.
domain::state_record!(MilestoneProjection {
    reached,
    newly_reached,
    peak_demand,
    connected_cities,
    strike_pending,
    run
});
impl domain::state::State for MilestoneKind {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::ServePeak { gigawatts } => {
                domain::state::State::write(&0u8, out);
                domain::state::State::write(gigawatts, out);
            }
            Self::FullSeasonWithoutStrike => {
                domain::state::State::write(&1u8, out);
            }
            Self::ConnectCities { count } => {
                domain::state::State::write(&2u8, out);
                domain::state::State::write(count, out);
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
        let mut milestones = MilestoneProjection::new(&session());
        close(&mut milestones, 1, 50_000.0);
        close(&mut milestones, 2, 50_000.0);
        assert_eq!(milestones.reached(), [PEAK_50]);
        assert_eq!(milestones.take_newly_reached(), vec![PEAK_50]);
        assert_eq!(milestones.take_newly_reached(), vec![]);
    }

    #[test]
    fn peak_below_threshold_does_not_count() {
        let mut milestones = MilestoneProjection::new(&session());
        close(&mut milestones, 1, 49_999.0);
        assert_eq!(milestones.reached(), []);
    }

    #[test]
    fn full_season_requires_three_strike_free_months_of_one_season() {
        let mut clean = MilestoneProjection::new(&session());
        for month in [3, 4, 5] {
            close(&mut clean, month, 100.0);
        }
        assert_eq!(clean.reached(), [FULL_SEASON]);

        // A strike issued while April closes taints April.
        let mut struck = MilestoneProjection::new(&session());
        close(&mut struck, 3, 100.0);
        strike(&mut struck);
        close(&mut struck, 4, 100.0);
        close(&mut struck, 5, 100.0);
        assert_eq!(struck.reached(), []);
    }

    #[test]
    fn full_season_milestone_ignores_notices() {
        let mut milestones = MilestoneProjection::new(&session());
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
            let mut milestones = MilestoneProjection::new(&session());
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
        let mut milestones = MilestoneProjection::new(&session());
        for month in [4, 5, 6, 7] {
            close(&mut milestones, month, 100.0);
        }
        assert_eq!(milestones.reached(), []);
        close(&mut milestones, 8, 100.0);
        assert_eq!(milestones.reached(), [FULL_SEASON]);
    }

    #[test]
    fn a_strike_resets_the_run_and_a_later_clean_season_still_counts() {
        let mut milestones = MilestoneProjection::new(&session());
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
        let mut milestones = MilestoneProjection::new(&session());
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
        let mut milestones = MilestoneProjection::new(&session());
        let month = SimMonth::new(2027, 3).unwrap();
        milestones.set_connected_cities(9, month);
        assert_eq!(milestones.reached(), []);
        milestones.set_connected_cities(10, month);
        milestones.set_connected_cities(11, month);
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
        let mut milestones = MilestoneProjection::new(&session());
        let cities = MilestoneKind::ConnectCities { count: 10 };
        assert_eq!(
            milestones.status(PEAK_50, now),
            MilestoneStatus::InProgress(MilestoneProgress::Peak {
                best: Megawatts::new(0.0),
                goal: Megawatts::new(50_000.0),
            })
        );
        assert_eq!(
            milestones.status(FULL_SEASON, now),
            MilestoneStatus::InProgress(MilestoneProgress::StrikeFreeMonths {
                months: 0,
                needed: 3,
            })
        );
        assert_eq!(
            milestones.status(cities, now),
            MilestoneStatus::InProgress(MilestoneProgress::Cities {
                connected: 2,
                needed: 10,
            }),
            "the tiny region starts with two cities"
        );

        close(&mut milestones, 3, 30_000.0);
        close(&mut milestones, 4, 20_000.0);
        assert_eq!(
            milestones.status(PEAK_50, now),
            MilestoneStatus::InProgress(MilestoneProgress::Peak {
                best: Megawatts::new(30_000.0),
                goal: Megawatts::new(50_000.0),
            }),
            "the best peak so far, not the last"
        );
        assert_eq!(
            milestones.status(FULL_SEASON, now),
            MilestoneStatus::InProgress(MilestoneProgress::StrikeFreeMonths {
                months: 2,
                needed: 3,
            })
        );
        strike(&mut milestones);
        assert_eq!(
            milestones.status(FULL_SEASON, now),
            MilestoneStatus::InProgress(MilestoneProgress::StrikeFreeMonths {
                months: 0,
                needed: 3,
            }),
            "a strike this month breaks the run"
        );

        close(&mut milestones, 6, 55_000.0);
        assert_eq!(
            milestones.status(PEAK_50, now),
            MilestoneStatus::Reached {
                month: SimMonth::new(2027, 6).unwrap()
            }
        );
        milestones.set_connected_cities(10, start);
        assert_eq!(
            milestones.status(cities, now),
            MilestoneStatus::Reached { month: start }
        );
    }

    #[test]
    fn a_strike_free_run_from_last_season_is_no_progress_in_this_one() {
        let mut milestones = MilestoneProjection::new(&session());
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
            milestones.status(FULL_SEASON, may),
            progress(2),
            "May's review"
        );

        // June is Summer: no Summer month has closed yet.
        let june = SimMonth::new(2027, 6).unwrap();
        assert_eq!(milestones.status(FULL_SEASON, june), progress(0));

        close(&mut milestones, 6, 100.0);
        assert_eq!(milestones.status(FULL_SEASON, june), progress(1));
    }
}
