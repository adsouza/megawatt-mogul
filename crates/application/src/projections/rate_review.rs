//! The coming annual reviews, estimated on today's state.

use domain::events::DomainEvent;
use domain::session::{GameSession, RateReviewEstimate};

use super::Projection;

/// The latest estimate of the next rate review and reserve review.
///
/// It is recomputed from the session (after the command) whenever something
/// it depends on changes: a month closing, capacity or a city arriving,
/// a capital assessment, or the day phase moving.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RateReviewProjection {
    estimate: Option<RateReviewEstimate>,
}

impl RateReviewProjection {
    /// Seeded with the estimate for the session's initial state.
    pub fn new(session: &GameSession) -> Self {
        Self {
            estimate: Some(session.rate_review_estimate()),
        }
    }

    /// `None` only for a default projection that has not yet seen an event
    /// that refreshes it.
    pub fn estimate(&self) -> Option<&RateReviewEstimate> {
        self.estimate.as_ref()
    }
}

impl Projection for RateReviewProjection {
    fn apply(&mut self, event: &DomainEvent, session: &GameSession) {
        if matches!(
            event,
            DomainEvent::MonthClosed(_)
                | DomainEvent::PlantCommissioned { .. }
                | DomainEvent::CityConnected { .. }
                | DomainEvent::CapitalHeld { .. }
                | DomainEvent::CapitalReleased { .. }
                | DomainEvent::CapitalDisallowed { .. }
                | DomainEvent::PhaseChanged { .. }
        ) {
            self.estimate = Some(session.rate_review_estimate());
        }
    }
}

// Portable state schema 1: explicit field order and enum tags.
domain::state_record!(RateReviewProjection { estimate });

#[cfg(test)]
mod tests {
    use std::sync::{Arc, OnceLock};

    use domain::company::AssetRef;
    use domain::events::DomainEvent;
    use domain::generation::PlantStatus;
    use domain::ids::CellCoord;
    use domain::region::PlantKind;
    use domain::region::fixtures::tiny_region;
    use domain::session::{DayPhase, GameSession, GameSettings, SessionStatus};
    use domain::time::{SimMonth, TICKS_PER_DAY};
    use domain::units::Money;

    use super::*;
    use crate::projections::Projection;
    use crate::projections::test_support::{session, tick_summary};

    /// Plays one month through domain commands: commit every commissioned
    /// dispatchable plant all day on auto-follow, operate the whole day,
    /// finish the review.
    fn play_month(s: &mut GameSession) {
        for view in s.plant_views() {
            if view.kind.is_dispatchable()
                && !matches!(view.status, PlantStatus::UnderConstruction { .. })
            {
                s.start_plant(view.id).unwrap();
                s.set_auto_follow(view.id, true).unwrap();
            }
        }
        s.begin_operating().unwrap();
        for _ in 0..TICKS_PER_DAY {
            s.tick().unwrap();
        }
        assert_eq!(s.phase(), DayPhase::Review);
        s.finish_review().unwrap();
        assert_eq!(s.phase(), DayPhase::Planning);
    }

    fn play_until(s: &mut GameSession, month: SimMonth) {
        while s.clock().month() != month {
            play_month(s);
            assert_eq!(s.status(), SessionStatus::Active);
        }
    }

    /// The tiny game with four peakers ordered in April 2027, played through
    /// real commands. They are commissioned in 2028 and leave the fleet well
    /// above the regulator's reserve margin.
    ///
    /// Returns the session in June 2028 (the excess is new, nothing held yet)
    /// and in February 2029 (December 2028's review held it; the next review
    /// would be its second).
    fn oversupplied_game() -> &'static (GameSession, GameSession) {
        static GAME: OnceLock<(GameSession, GameSession)> = OnceLock::new();
        GAME.get_or_init(|| {
            let mut s = GameSession::new(Arc::new(tiny_region()), 1, GameSettings::default());
            for x in 6..10 {
                s.build_plant(PlantKind::Peaker, CellCoord { x, y: 1 })
                    .unwrap();
            }
            play_until(&mut s, SimMonth::new(2028, 6).unwrap());
            let new_excess = s.clone();
            play_until(&mut s, SimMonth::new(2029, 2).unwrap());
            (new_excess, s)
        })
    }

    #[test]
    fn rate_review_estimate_distinguishes_hold_and_writeoff() {
        let (new_excess, held_before) = oversupplied_game();

        // Capacity commissioned this year and in excess would be held at the
        // coming review, not written off.
        let estimate = new_excess.rate_review_estimate();
        assert!(estimate.reserve_margin > 0.20);
        assert!(estimate.capital_to_hold > Money::ZERO);
        assert_eq!(estimate.capital_at_risk, Money::ZERO);
        assert_eq!(estimate.capital_to_release, Money::ZERO);

        // After the review held it, a second review that still finds it
        // excess writes it off, and nothing new is left to hold.
        assert!(held_before.company().rate_base().held_total() > Money::ZERO);
        let estimate = held_before.rate_review_estimate();
        assert!(estimate.capital_at_risk > Money::ZERO);
        assert_eq!(estimate.capital_to_hold, Money::ZERO);
        assert_eq!(estimate.capital_to_release, Money::ZERO);
        assert!(estimate.capital_at_risk <= held_before.company().rate_base().held_total());
    }

    #[test]
    fn rate_review_estimate_is_close_to_what_the_review_then_does() {
        let (new_excess, _) = oversupplied_game();
        let mut s = new_excess.clone();
        play_until(&mut s, SimMonth::new(2028, 12).unwrap());
        let estimate = s.rate_review_estimate();

        let mut events = Vec::new();
        s.begin_operating().unwrap();
        for _ in 0..TICKS_PER_DAY {
            events.extend(s.tick().unwrap());
        }
        let held: Money = events
            .iter()
            .filter_map(|e| match e {
                DomainEvent::CapitalHeld { amount, .. } => Some(*amount),
                _ => None,
            })
            .sum();
        assert!(held > Money::ZERO);
        // December's own depreciation is the only difference in book value.
        let difference = (held.cents() - estimate.capital_to_hold.cents()).abs();
        assert!(
            difference * 100 < held.cents(),
            "estimate {estimate:?} should be within 1% of the held {held:?}"
        );
    }

    #[test]
    fn rate_review_estimate_is_pure() {
        let (new_excess, held_before) = oversupplied_game();
        // The second state has held portions with a review history to protect.
        assert!(held_before.company().rate_base().held_total() > Money::ZERO);
        for s in [new_excess, held_before] {
            let hash = s.state_hash();
            let history = |s: &GameSession| -> Vec<_> {
                s.company()
                    .rate_base()
                    .assets()
                    .map(|a| {
                        (
                            a.asset,
                            a.held(),
                            a.held_slice().map(|h| h.consecutive_excess_reviews()),
                            a.held_slice().map(|h| h.last_excess_review()),
                        )
                    })
                    .collect()
            };
            let before = history(s);
            let first = s.rate_review_estimate();
            let second = s.rate_review_estimate();
            assert_eq!(first, second);
            assert_eq!(s.state_hash(), hash);
            assert_eq!(history(s), before);
        }
    }

    #[test]
    fn projection_holds_the_estimate_after_a_relevant_event() {
        let s = session();
        let mut projection = RateReviewProjection::default();
        assert_eq!(projection.estimate(), None);
        projection.apply(
            &DomainEvent::PhaseChanged {
                from: DayPhase::Planning,
                to: DayPhase::Operating,
            },
            &s,
        );
        assert_eq!(projection.estimate(), Some(&s.rate_review_estimate()));
    }

    #[test]
    fn projection_refreshes_on_each_triggering_event_and_no_other() {
        let early = &oversupplied_game().0;
        let late = &oversupplied_game().1;
        let triggers = [
            DomainEvent::MonthClosed(Box::new(crate::projections::test_support::report(
                2028, 5, 0.0,
            ))),
            DomainEvent::PlantCommissioned {
                plant: domain::ids::PlantId(1),
                kind: PlantKind::Peaker,
            },
            DomainEvent::CityConnected {
                city: domain::ids::CityId(3),
            },
            DomainEvent::CapitalHeld {
                asset: AssetRef::Plant(domain::ids::PlantId(1)),
                amount: Money::ZERO,
            },
            DomainEvent::CapitalReleased {
                asset: AssetRef::Plant(domain::ids::PlantId(1)),
                amount: Money::ZERO,
            },
            DomainEvent::CapitalDisallowed {
                asset: AssetRef::Plant(domain::ids::PlantId(1)),
                amount: Money::ZERO,
            },
            DomainEvent::PhaseChanged {
                from: DayPhase::Review,
                to: DayPhase::Planning,
            },
        ];
        for trigger in &triggers {
            let mut projection = RateReviewProjection::default();
            projection.apply(trigger, early);
            assert_eq!(
                projection.estimate(),
                Some(&early.rate_review_estimate()),
                "{trigger:?}"
            );
            projection.apply(trigger, late);
            assert_eq!(
                projection.estimate(),
                Some(&late.rate_review_estimate()),
                "{trigger:?}"
            );
        }

        // A tick does not refresh: the old estimate stands.
        let mut projection = RateReviewProjection::new(early);
        let seeded = projection.estimate().cloned();
        projection.apply(&DomainEvent::TickCompleted(tick_summary(0)), late);
        assert_eq!(projection.estimate().cloned(), seeded);
    }
}
