//! A rolling window of per-tick grid state, for charts.

use std::collections::{BTreeMap, VecDeque};

use domain::demand::ForecastPoint;
use domain::events::DomainEvent;
use domain::generation::BatteryFlow;
use domain::grid::TickSummary;
use domain::ids::PlantId;
use domain::region::PlantKind;
use domain::session::{DayPhase, GameSession};
use domain::time::SimMonth;
use domain::units::{Hertz, Megawatts, Price};

use super::Projection;

/// One day of minute ticks.
pub const TIMELINE_CAPACITY: usize = 1_440;

/// The grid at the end of one tick.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelinePoint {
    pub tick: u32,
    pub demand: Megawatts,
    pub batteries: BTreeMap<PlantId, BatteryFlow>,
    pub served: Megawatts,
    pub frequency: Hertz,
    pub by_kind: BTreeMap<PlantKind, Megawatts>,
    /// The system marginal cost: the highest marginal cost among connected
    /// plants producing more than zero this tick, or `None` when nothing
    /// produces. Under a regulated tariff nobody is paid this per tick (§6.1);
    /// it is the "price" the merit order teaches.
    pub marginal_cost: Option<Price>,
}

/// The last [`TIMELINE_CAPACITY`] ticks, oldest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimelineProjection {
    pub(super) points: VecDeque<TimelinePoint>,
    pub(super) forecast: Option<[ForecastPoint; 24]>,
    pub(super) completed: Vec<CompletedDay>,
}

/// The played day representing a closed month, with its original demand forecast.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletedDay {
    pub month: SimMonth,
    pub points: Vec<TimelinePoint>,
    pub forecast: [ForecastPoint; 24],
}

impl TimelineProjection {
    pub fn points(&self) -> impl Iterator<Item = &TimelinePoint> {
        self.points.iter()
    }

    pub fn completed_days(&self) -> &[CompletedDay] {
        &self.completed
    }
}

/// The highest marginal cost among the plants producing in `summary`. A plant
/// the session does not know contributes nothing.
fn system_marginal_cost(summary: &TickSummary, session: &GameSession) -> Option<Price> {
    summary
        .by_plant
        .iter()
        .filter(|(_, output)| **output > Megawatts::ZERO)
        .filter_map(|(id, _)| session.power().plant(*id))
        .filter(|p| p.kind() != PlantKind::Battery)
        .map(|plant| plant.spec().marginal_cost)
        .max_by(|a, b| a.value().total_cmp(&b.value()))
}

impl Projection for TimelineProjection {
    fn apply(&mut self, event: &DomainEvent, session: &GameSession) {
        if let DomainEvent::PhaseChanged {
            to: DayPhase::Operating,
            ..
        } = event
        {
            // Capture before month-close commissioning or later months change the forecast.
            self.forecast = Some(session.hourly_forecast());
        }
        if let DomainEvent::MonthClosed(report) = event
            && let Some(forecast) = self.forecast.take()
        {
            self.completed.push(CompletedDay {
                month: report.month,
                points: self.points.iter().cloned().collect(),
                forecast,
            });
        }
        if let DomainEvent::TickCompleted(summary) = event {
            if self.points.len() == TIMELINE_CAPACITY {
                self.points.pop_front();
            }
            self.points.push_back(TimelinePoint {
                tick: summary.tick,
                demand: summary.demand,
                batteries: summary.batteries.clone(),
                served: summary.served,
                frequency: summary.frequency,
                by_kind: summary.by_kind.clone(),
                marginal_cost: system_marginal_cost(summary, session),
            });
        }
    }
}

// Portable state schema 1: explicit field order and enum tags.
domain::state_record!(TimelineProjection {
    points,
    forecast,
    completed
});
domain::state_record!(TimelinePoint {
    tick,
    demand,
    served,
    frequency,
    by_kind,
    marginal_cost,
    batteries
});
domain::state_record!(CompletedDay {
    month,
    points,
    forecast
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projections::test_support::{session, tick_summary};
    use domain::ids::PlantId;

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
                batteries: BTreeMap::new(),
                tick: 7,
                demand: summary.demand,
                served: summary.served,
                frequency: summary.frequency,
                by_kind: summary.by_kind,
                marginal_cost: None,
            })
        );
    }

    fn plant_of(session: &GameSession, kind: PlantKind) -> PlantId {
        session
            .plant_views()
            .iter()
            .find(|p| p.kind == kind)
            .expect("plant of kind")
            .id
    }

    fn marginal_cost_of(session: &GameSession, kind: PlantKind) -> Price {
        session.region().spec(kind).marginal_cost
    }

    fn point_with(session: &GameSession, output: &[(PlantId, f64)]) -> TimelinePoint {
        let mut summary = tick_summary(0);
        summary.by_plant = output
            .iter()
            .map(|&(plant, mw)| (plant, Megawatts::new(mw)))
            .collect();
        let mut timeline = TimelineProjection::default();
        timeline.apply(&DomainEvent::TickCompleted(summary), session);
        timeline.points().next().cloned().unwrap()
    }

    #[test]
    fn timeline_marginal_cost_is_the_most_expensive_producer() {
        let session = session();
        let coal = plant_of(&session, PlantKind::Coal);
        let ccgt = plant_of(&session, PlantKind::Ccgt);
        let peaker = plant_of(&session, PlantKind::Peaker);
        let cost = |kind| marginal_cost_of(&session, kind);
        assert!(cost(PlantKind::Coal) < cost(PlantKind::Ccgt));
        assert!(cost(PlantKind::Ccgt) < cost(PlantKind::Peaker));

        // The peaker is connected but idle, so it does not set the price.
        let point = point_with(&session, &[(coal, 300.0), (ccgt, 80.0), (peaker, 0.0)]);
        assert_eq!(point.marginal_cost, Some(cost(PlantKind::Ccgt)));

        // Once it produces, it is the marginal plant, whatever the others do.
        let point = point_with(&session, &[(coal, 300.0), (ccgt, 80.0), (peaker, 0.5)]);
        assert_eq!(point.marginal_cost, Some(cost(PlantKind::Peaker)));
        let point = point_with(&session, &[(peaker, 5.0)]);
        assert_eq!(point.marginal_cost, Some(cost(PlantKind::Peaker)));

        // Nothing producing means no price.
        assert_eq!(point_with(&session, &[]).marginal_cost, None);
        let idle = point_with(&session, &[(coal, 0.0), (peaker, 0.0)]);
        assert_eq!(idle.marginal_cost, None);
    }

    #[test]
    fn timeline_ignores_other_events() {
        let mut timeline = TimelineProjection::default();
        timeline.apply(&DomainEvent::Blackout, &session());
        assert_eq!(timeline.points().count(), 0);
    }
}
