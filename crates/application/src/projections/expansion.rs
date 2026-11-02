//! How long the player has run the grid since first reaching beyond the
//! region's starter territory, so a later reader can tell a played region.

use domain::events::DomainEvent;
use domain::session::GameSession;

use super::Projection;

/// Month closes after the first expansion that make a region "played".
pub const PLAYED_MONTHS: u8 = 12;

/// Counts month closes after the player first connects a city that is not
/// one of the region's starter cities.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExpansionProjection {
    /// `None` until the first non-starter city connects; then the closes
    /// since, saturating at [`PLAYED_MONTHS`].
    closes_since_expansion: Option<u8>,
}

impl ExpansionProjection {
    /// Seeds the count from the session: a game that already serves a city
    /// beyond its starter territory has expanded, with no closes counted yet.
    pub fn new(session: &GameSession) -> Self {
        let starters = &session.region().start().connected_cities;
        let expanded = session
            .city_views()
            .iter()
            .any(|city| city.connected && !starters.contains(&city.id));
        Self {
            closes_since_expansion: expanded.then_some(0),
        }
    }

    /// Whether at least [`PLAYED_MONTHS`] months have closed since the first
    /// expansion.
    pub fn played(&self) -> bool {
        self.closes_since_expansion
            .is_some_and(|closes| closes >= PLAYED_MONTHS)
    }
}

impl Projection for ExpansionProjection {
    fn apply(&mut self, event: &DomainEvent, session: &GameSession) {
        match event {
            DomainEvent::CityConnected { city }
                if self.closes_since_expansion.is_none()
                    && !session.region().start().connected_cities.contains(city) =>
            {
                self.closes_since_expansion = Some(0);
            }
            DomainEvent::MonthClosed(_) => {
                if let Some(closes) = self.closes_since_expansion {
                    self.closes_since_expansion = Some(closes.saturating_add(1).min(PLAYED_MONTHS));
                }
            }
            _ => {}
        }
    }
}

// Portable state schema 8.
domain::state_record!(ExpansionProjection {
    closes_since_expansion
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projections::Projections;
    use crate::projections::test_support::{report, session};
    use domain::ids::CityId;
    use domain::state::State;
    use proptest::prelude::*;

    /// A starter city of the `tiny_region` fixture.
    const STARTER: CityId = CityId(1);
    /// A `tiny_region` city outside the starter territory.
    const EXTRA: CityId = CityId(3);

    fn connect(projection: &mut ExpansionProjection, city: CityId) {
        projection.apply(&DomainEvent::CityConnected { city }, &session());
    }

    fn close(projection: &mut ExpansionProjection, months: usize) {
        let event = DomainEvent::MonthClosed(Box::new(report(2027, 1, 100.0)));
        for _ in 0..months {
            projection.apply(&event, &session());
        }
    }

    #[test]
    fn starts_unplayed_without_extra_city() {
        let projection = ExpansionProjection::new(&session());
        assert_eq!(projection.closes_since_expansion, None);
        assert!(!projection.played());
    }

    #[test]
    fn counts_closes_after_first_extra_city() {
        let mut projection = ExpansionProjection::new(&session());
        connect(&mut projection, EXTRA);
        close(&mut projection, usize::from(PLAYED_MONTHS) - 1);
        assert!(!projection.played());
        close(&mut projection, 1);
        assert!(projection.played());
    }

    #[test]
    fn starter_city_connections_do_not_count() {
        let mut projection = ExpansionProjection::new(&session());
        connect(&mut projection, STARTER);
        close(&mut projection, 20);
        assert!(!projection.played());
    }

    #[test]
    fn closes_before_expansion_do_not_count() {
        let mut projection = ExpansionProjection::new(&session());
        close(&mut projection, 30);
        connect(&mut projection, EXTRA);
        close(&mut projection, usize::from(PLAYED_MONTHS) - 1);
        assert!(!projection.played());
    }

    #[test]
    fn a_later_extra_connection_does_not_restart_the_count() {
        let mut projection = ExpansionProjection::new(&session());
        connect(&mut projection, EXTRA);
        close(&mut projection, 5);
        connect(&mut projection, EXTRA);
        close(&mut projection, usize::from(PLAYED_MONTHS) - 5);
        assert!(projection.played());
    }

    #[derive(Debug, Clone, Copy)]
    enum Step {
        StarterConnect,
        ExtraConnect,
        Close,
    }

    fn step() -> impl Strategy<Value = Step> {
        prop_oneof![
            1 => Just(Step::StarterConnect),
            1 => Just(Step::ExtraConnect),
            4 => Just(Step::Close),
        ]
    }

    proptest! {
        #[test]
        fn played_iff_twelve_closes_follow_an_extra_connection(
            steps in prop::collection::vec(step(), 0..80)
        ) {
            let mut projection = ExpansionProjection::new(&session());
            for step in &steps {
                match step {
                    Step::StarterConnect => connect(&mut projection, STARTER),
                    Step::ExtraConnect => connect(&mut projection, EXTRA),
                    Step::Close => close(&mut projection, 1),
                }
            }
            let closes_after = steps
                .iter()
                .skip_while(|step| !matches!(step, Step::ExtraConnect))
                .filter(|step| matches!(step, Step::Close))
                .count();
            prop_assert_eq!(
                projection.played(),
                closes_after >= usize::from(PLAYED_MONTHS)
            );
        }
    }

    #[test]
    fn state_bytes_round_trip_at_schema_8() {
        for closes_since_expansion in [None, Some(0), Some(5), Some(PLAYED_MONTHS)] {
            let projection = ExpansionProjection {
                closes_since_expansion,
            };
            let bytes = projection.state_bytes_schema(8);
            assert_eq!(
                ExpansionProjection::from_state_bytes_schema(&bytes, 8),
                Ok(projection)
            );
        }

        let mut projections = Projections::new(&session());
        projections.expansion.closes_since_expansion = Some(3);
        let current = projections.state_bytes_schema(8);
        // Schema 8 predates the frozen regional milestone targets (schema 10): they read back as
        // the legacy goals until the restore upgrades them from the session, as `Game` does.
        let mut restored = Projections::from_state_bytes_schema(&current, 8).unwrap();
        restored.milestones.upgrade_targets(&session());
        assert_eq!(restored, projections);
        // Schema 7 never carried the projection: it reads back as the default.
        let old = projections.state_bytes_schema(7);
        assert_eq!(old.len() + 2, current.len());
        assert_eq!(
            Projections::from_state_bytes_schema(&old, 7)
                .unwrap()
                .expansion,
            ExpansionProjection::default()
        );
    }
}
