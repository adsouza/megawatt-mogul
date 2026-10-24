//! A heads-up when the net load is about to climb faster than the fleet can
//! follow.

use domain::events::DomainEvent;
use domain::session::GameSession;
use domain::time::{SimDuration, TICKS_PER_DAY};
use domain::units::Megawatts;

use super::Projection;

/// How far ahead the warning looks.
const HORIZON_MINUTES: u32 = 60;

/// The warning is re-evaluated on every tick that is a multiple of this.
const EVALUATION_INTERVAL: u32 = 5;

/// Net load is expected to rise by more than the fleet can ramp up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RampWarning {
    /// The tick the warning was evaluated at.
    pub at_tick: u32,
    pub horizon_minutes: u32,
    /// The expected rise in net load over the horizon.
    pub net_load_change: Megawatts,
    /// How much connected dispatchable output could rise over the horizon.
    pub capability: Megawatts,
}

/// A warning when `net_ahead − net_now` exceeds `capability`.
pub fn ramp_warning(
    at_tick: u32,
    net_now: Megawatts,
    net_ahead: Megawatts,
    capability: Megawatts,
) -> Option<RampWarning> {
    let net_load_change = net_ahead - net_now;
    (net_load_change > capability).then_some(RampWarning {
        at_tick,
        horizon_minutes: HORIZON_MINUTES,
        net_load_change,
        capability,
    })
}

/// The latest ramp warning, refreshed every fifth tick.
///
/// The forecast and fleet are read from the session as it stands after the
/// command, so a command that runs many ticks at once judges the fleet by
/// where it ended up.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RampWarningProjection {
    current: Option<RampWarning>,
}

impl RampWarningProjection {
    /// The warning from the last evaluation; `None` before any evaluation or
    /// when the last one found the fleet able to keep up.
    pub fn current(&self) -> Option<&RampWarning> {
        self.current.as_ref()
    }

    fn evaluate(&mut self, tick: u32, session: &GameSession) {
        let last_tick = TICKS_PER_DAY - 1;
        let net_now = session.forecast_dispatch_net_load(tick);
        let net_ahead = session.forecast_dispatch_net_load((tick + HORIZON_MINUTES).min(last_tick));
        let capability = session
            .power()
            .ramp_capability_up(SimDuration::from_minutes(HORIZON_MINUTES));
        self.current = ramp_warning(tick, net_now, net_ahead, capability);
    }
}

impl Projection for RampWarningProjection {
    fn apply(&mut self, event: &DomainEvent, session: &GameSession) {
        if let DomainEvent::TickCompleted(summary) = event
            && summary.tick.is_multiple_of(EVALUATION_INTERVAL)
        {
            self.evaluate(summary.tick, session);
        }
    }
}

// Portable state schema 1: explicit field order and enum tags.
domain::state_record!(RampWarningProjection { current });
domain::state_record!(RampWarning {
    at_tick,
    horizon_minutes,
    net_load_change,
    capability
});

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use domain::ids::CellCoord;
    use domain::region::PlantKind;
    use domain::region::fixtures::tiny_region_parts;
    use domain::region::{RegionDefinition, StartPlantDef};
    use domain::session::GameSettings;

    use super::*;
    use crate::projections::test_support::{session, tick_summary};

    /// The tiny region with 1,500 MW of solar in its starting fleet: the
    /// net load climbs steeply as the sun sets into the evening peak.
    fn duck_curve_session() -> GameSession {
        let mut parts = tiny_region_parts();
        for (x, y) in [(7, 1), (8, 1), (9, 1), (7, 6), (8, 5)] {
            parts.start.fleet.push(StartPlantDef {
                kind: PlantKind::Solar,
                cell: CellCoord { x, y },
            });
        }
        let region = RegionDefinition::try_from(parts).expect("duck-curve region is valid");
        GameSession::new(Arc::new(region), 1, GameSettings::default())
    }

    fn tick(projection: &mut RampWarningProjection, tick: u32, session: &GameSession) {
        projection.apply(&DomainEvent::TickCompleted(tick_summary(tick)), session);
    }

    #[test]
    fn warns_when_net_load_rise_exceeds_capability() {
        assert_eq!(
            ramp_warning(
                1000,
                Megawatts::new(900.0),
                Megawatts::new(1500.0),
                Megawatts::new(490.0)
            )
            .map(|w| w.net_load_change),
            Some(Megawatts::new(600.0))
        );
        assert!(
            ramp_warning(
                1000,
                Megawatts::new(900.0),
                Megawatts::new(1300.0),
                Megawatts::new(490.0)
            )
            .is_none()
        );
    }

    #[test]
    fn warning_records_its_tick_horizon_and_capability() {
        let warning = ramp_warning(
            1000,
            Megawatts::new(900.0),
            Megawatts::new(1500.0),
            Megawatts::new(490.0),
        );
        assert_eq!(
            warning,
            Some(RampWarning {
                at_tick: 1000,
                horizon_minutes: 60,
                net_load_change: Megawatts::new(600.0),
                capability: Megawatts::new(490.0),
            })
        );
    }

    #[test]
    fn a_rise_equal_to_capability_is_no_warning() {
        assert!(
            ramp_warning(
                0,
                Megawatts::new(900.0),
                Megawatts::new(1390.0),
                Megawatts::new(490.0)
            )
            .is_none()
        );
    }

    #[test]
    fn ramp_projection_updates_every_fifth_tick() {
        let session = duck_curve_session();
        let mut projection = RampWarningProjection::default();
        assert_eq!(projection.current(), None);

        // Ticks that are not multiples of five are never evaluated, even
        // where the duck curve would warn.
        for t in 1_016..=1_019 {
            tick(&mut projection, t, &session);
            assert_eq!(projection.current(), None, "tick {t} is not evaluated");
        }

        // Tick 1020 is 17:00: the sun is setting into the evening peak and
        // the fleet is offline, so it cannot follow.
        tick(&mut projection, 1_020, &session);
        let expected = ramp_warning(
            1_020,
            session.forecast_net_load(1_020),
            session.forecast_net_load(1_080),
            session
                .power()
                .ramp_capability_up(SimDuration::from_minutes(60)),
        );
        assert!(expected.is_some(), "the duck curve should warn at 17:00");
        assert_eq!(projection.current(), expected.as_ref());

        // Between evaluations the warning stands, even where a fresh
        // evaluation would clear it (net load falls after midnight).
        tick(&mut projection, 1_426, &session);
        assert_eq!(projection.current(), expected.as_ref());

        // The next multiple of five re-evaluates and clears it.
        tick(&mut projection, 1_430, &session);
        assert_eq!(projection.current(), None);
    }

    #[test]
    fn the_horizon_is_capped_at_the_last_tick_of_the_day() {
        let session = duck_curve_session();
        let mut projection = RampWarningProjection::default();
        // Tick 1380 looks 60 minutes ahead to 1440, capped at 1439.
        tick(&mut projection, 1_380, &session);
        let expected = ramp_warning(
            1_380,
            session.forecast_net_load(1_380),
            session.forecast_net_load(1_439),
            session
                .power()
                .ramp_capability_up(SimDuration::from_minutes(60)),
        );
        assert_eq!(projection.current(), expected.as_ref());
    }

    #[test]
    fn other_events_do_not_evaluate() {
        let mut projection = RampWarningProjection::default();
        projection.apply(&DomainEvent::Blackout, &session());
        assert_eq!(projection.current(), None);
    }

    #[test]
    fn a_warning_near_midnight_looks_ahead_only_to_the_last_tick() {
        // Hour 0 demand far above hour 23, so the net load jumps as the day wraps.
        let mut parts = tiny_region_parts();
        parts.demand.shoulder[0] = 3.0;
        let region = RegionDefinition::try_from(parts).expect("wrapped-day region is valid");
        let session = GameSession::new(Arc::new(region), 1, GameSettings::default());
        let mut projection = RampWarningProjection::default();
        tick(&mut projection, 1_380, &session);
        let warning = projection.current().expect("the wrap-around jump warns");
        let change = session.forecast_net_load(1_439) - session.forecast_net_load(1_380);
        assert_eq!(warning.net_load_change, change);
        assert_eq!(warning.at_tick, 1_380);
    }
}
