//! A rolling window of per-tick grid state, for charts.

use std::collections::{BTreeMap, VecDeque};

use domain::events::DomainEvent;
use domain::region::PlantKind;
use domain::session::GameSession;
use domain::units::{Hertz, Megawatts};

use super::Projection;

/// One day of minute ticks.
pub const TIMELINE_CAPACITY: usize = 1_440;

/// The grid at the end of one tick.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelinePoint {
    pub tick: u32,
    pub demand: Megawatts,
    pub served: Megawatts,
    pub frequency: Hertz,
    pub by_kind: BTreeMap<PlantKind, Megawatts>,
}

/// The last [`TIMELINE_CAPACITY`] ticks, oldest first.
#[derive(Debug, Clone, Default)]
pub struct TimelineProjection {
    points: VecDeque<TimelinePoint>,
}

impl TimelineProjection {
    pub fn points(&self) -> impl Iterator<Item = &TimelinePoint> {
        self.points.iter()
    }
}

impl Projection for TimelineProjection {
    fn apply(&mut self, event: &DomainEvent, _session: &GameSession) {
        if let DomainEvent::TickCompleted(summary) = event {
            if self.points.len() == TIMELINE_CAPACITY {
                self.points.pop_front();
            }
            self.points.push_back(TimelinePoint {
                tick: summary.tick,
                demand: summary.demand,
                served: summary.served,
                frequency: summary.frequency,
                by_kind: summary.by_kind.clone(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projections::test_support::{session, tick_summary};

    #[test]
    fn timeline_keeps_last_1440_points() {
        let session = session();
        let mut timeline = TimelineProjection::default();
        for tick in 0..2_000 {
            timeline.apply(&DomainEvent::TickCompleted(tick_summary(tick)), &session);
        }
        let ticks: Vec<u32> = timeline.points().map(|p| p.tick).collect();
        assert_eq!(ticks.len(), 1_440);
        assert_eq!(ticks.first(), Some(&560));
        assert_eq!(ticks.last(), Some(&1_999));
    }

    #[test]
    fn timeline_point_copies_the_summary_fields() {
        let session = session();
        let mut summary = tick_summary(7);
        summary.demand = Megawatts::new(120.0);
        summary.served = Megawatts::new(110.0);
        summary.frequency = Hertz::new(59.95);
        summary
            .by_kind
            .insert(PlantKind::Coal, Megawatts::new(110.0));
        let mut timeline = TimelineProjection::default();
        timeline.apply(&DomainEvent::TickCompleted(summary.clone()), &session);
        assert_eq!(
            timeline.points().next(),
            Some(&TimelinePoint {
                tick: 7,
                demand: summary.demand,
                served: summary.served,
                frequency: summary.frequency,
                by_kind: summary.by_kind,
            })
        );
    }

    #[test]
    fn timeline_ignores_other_events() {
        let mut timeline = TimelineProjection::default();
        timeline.apply(&DomainEvent::Blackout, &session());
        assert_eq!(timeline.points().count(), 0);
    }
}
