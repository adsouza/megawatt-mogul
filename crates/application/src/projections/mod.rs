//! The read side of CQRS: projections fold the domain events a command
//! produced into query-friendly views.
//!
//! Projections are pure functions of the event stream (plus the session, for
//! facts an event only hints at), so they are deterministic and rebuildable.

mod lessons;
mod milestones;
mod monthly_report;
mod plant_history;
mod ramp_warning;
mod rate_review;
mod timeline;

use domain::events::DomainEvent;
use domain::session::GameSession;

pub use lessons::{Lesson, LessonsProjection, lessons_for};
pub use milestones::{MILESTONES, MilestoneKind, MilestoneProjection};
pub use monthly_report::MonthlyReportProjection;
pub use plant_history::{PlantHistory, PlantHistoryProjection};
pub use ramp_warning::{RampWarning, RampWarningProjection, ramp_warning};
pub use rate_review::RateReviewProjection;
pub use timeline::{TIMELINE_CAPACITY, TimelinePoint, TimelineProjection};

/// One read model, updated one event at a time.
///
/// `session` is the state *after* the whole command ran, so a projection may
/// read it for facts an event only signals (for example, the connected cities
/// when `CityConnected` arrives).
pub trait Projection {
    fn apply(&mut self, event: &DomainEvent, session: &GameSession);
}

/// Every read model of a running game.
#[derive(Debug, Clone)]
pub struct Projections {
    pub timeline: TimelineProjection,
    pub monthly_reports: MonthlyReportProjection,
    pub plant_history: PlantHistoryProjection,
    pub milestones: MilestoneProjection,
    pub ramp_warnings: RampWarningProjection,
    pub rate_review: RateReviewProjection,
    pub lessons: LessonsProjection,
}

impl Projections {
    /// Empty projections seeded from the session's initial state.
    pub fn new(session: &GameSession) -> Self {
        Self {
            timeline: TimelineProjection::default(),
            monthly_reports: MonthlyReportProjection::default(),
            plant_history: PlantHistoryProjection::default(),
            milestones: MilestoneProjection::new(session),
            ramp_warnings: RampWarningProjection::default(),
            rate_review: RateReviewProjection::new(session),
            lessons: LessonsProjection::default(),
        }
    }

    /// Applies each event, in order, to each projection in a fixed order:
    /// timeline, monthly reports, plant history, milestones, ramp warnings,
    /// rate review, lessons.
    pub fn apply_all(&mut self, events: &[DomainEvent], session: &GameSession) {
        for event in events {
            self.timeline.apply(event, session);
            self.monthly_reports.apply(event, session);
            self.plant_history.apply(event, session);
            self.milestones.apply(event, session);
            self.ramp_warnings.apply(event, session);
            self.rate_review.apply(event, session);
            self.lessons.apply(event, session);
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use domain::grid::{GridMode, TickSummary};
    use domain::region::fixtures::tiny_region;
    use domain::session::{GameSession, GameSettings, MonthlyReport};
    use domain::time::SimMonth;
    use domain::units::{Hertz, MegawattHours, Megawatts, Money, Ratio};

    pub fn session() -> GameSession {
        GameSession::new(Arc::new(tiny_region()), 1, GameSettings::default())
    }

    pub fn tick_summary(tick: u32) -> TickSummary {
        TickSummary {
            tick,
            demand: Megawatts::new(100.0),
            served: Megawatts::new(100.0),
            unserved: Megawatts::new(0.0),
            generation: Megawatts::new(100.0),
            curtailed: Megawatts::new(0.0),
            imbalance: Megawatts::new(0.0),
            by_kind: BTreeMap::new(),
            by_plant: BTreeMap::new(),
            frequency: Hertz::new(60.0),
            mode: GridMode::Normal,
            shed: Ratio::ZERO,
        }
    }

    pub fn report(year: i32, month: u8, peak_mw: f64) -> MonthlyReport {
        MonthlyReport {
            month: SimMonth::new(year, month).unwrap(),
            days: 30,
            revenue: Money::ZERO,
            fuel: Money::ZERO,
            start_up: Money::ZERO,
            fixed_om: Money::ZERO,
            construction: Money::ZERO,
            net: Money::ZERO,
            energy_by_kind: BTreeMap::new(),
            merit_order: Vec::new(),
            demand: MegawattHours::new(0.0),
            served: MegawattHours::new(0.0),
            unserved: MegawattHours::new(0.0),
            curtailed: MegawattHours::new(0.0),
            imbalance: MegawattHours::new(0.0),
            overgeneration: MegawattHours::new(0.0),
            peak_demand: Megawatts::new(peak_mw),
            reliability: Ratio::ONE,
            incidents: Vec::new(),
            over_frequency_trips: 0,
            out_of_band_share: Ratio::ZERO,
            frequency_failed: false,
            frequency_notice: false,
            rate_review: None,
        }
    }
}
