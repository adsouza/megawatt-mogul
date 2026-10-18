//! What the month just closed has to teach the player.

use domain::events::DomainEvent;
use domain::region::{PlantKind, RegionDefinition};
use domain::session::{GameSession, MonthlyReport};
use domain::units::{MegawattHours, Money, Price, Ratio};

use super::Projection;

/// At most this many lessons are shown for a month.
const MAX_LESSONS: usize = 3;

/// Peaker energy above this share of served energy is expensive peaking.
const PEAKER_SHARE_LIMIT: f64 = 0.05;

/// Curtailed solar above this share of served energy is worth a lesson.
const CURTAILED_SHARE_LIMIT: f64 = 0.01;

/// Start-up costs above this share of operating cost are worth a lesson.
const START_UP_SHARE_LIMIT: f64 = 0.10;

/// One thing the player can learn from a closed month.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Lesson {
    /// Peakers supplied a large share of the month's energy at a high price.
    ExpensivePeaking {
        energy: MegawattHours,
        /// What that energy cost in fuel.
        cost: Money,
        /// The region's CCGT marginal cost, the cheaper mid-merit option.
        alternative: Price,
    },
    /// Some demand went unserved; `first_incident_tick` is when protection
    /// first acted (zero when no incident was recorded).
    UnservedEnergy {
        energy: MegawattHours,
        first_incident_tick: u32,
    },
    /// Solar was curtailed because there was nowhere for it to go.
    SolarCurtailed { energy: MegawattHours },
    /// Starting plants cost a large share of operating cost.
    StartUpCosts { total: Money, share: Ratio },
    /// The annual rate review happened.
    RateReview {
        tariff: Price,
        allowed_return: Ratio,
    },
    /// Nothing went noticeably wrong.
    AllClear,
}

/// The lessons of `report`, most important first, at most three. A month
/// with nothing to learn yields `[AllClear]`.
///
/// Priority: expensive peaking, unserved energy, curtailed solar, start-up
/// costs, then the rate review.
pub fn lessons_for(report: &MonthlyReport, region: &RegionDefinition) -> Vec<Lesson> {
    let served = report.served.value();
    let mut lessons = Vec::new();

    let peaker_energy = report
        .energy_by_kind
        .get(&PlantKind::Peaker)
        .copied()
        .unwrap_or(MegawattHours::ZERO);
    if peaker_energy.value() > served * PEAKER_SHARE_LIMIT {
        lessons.push(Lesson::ExpensivePeaking {
            energy: peaker_energy,
            cost: region
                .spec(PlantKind::Peaker)
                .marginal_cost
                .cost_of(peaker_energy, 1),
            alternative: region.spec(PlantKind::Ccgt).marginal_cost,
        });
    }

    if report.unserved > MegawattHours::ZERO {
        lessons.push(Lesson::UnservedEnergy {
            energy: report.unserved,
            first_incident_tick: report.incidents.first().map_or(0, |incident| incident.tick),
        });
    }

    if report.curtailed.value() > served * CURTAILED_SHARE_LIMIT {
        lessons.push(Lesson::SolarCurtailed {
            energy: report.curtailed,
        });
    }

    let start_up = magnitude(report.start_up);
    let operating = magnitude(report.fuel) + start_up + magnitude(report.fixed_om);
    if start_up.cents() as f64 > operating.cents() as f64 * START_UP_SHARE_LIMIT {
        let share = start_up.cents() as f64 / operating.cents() as f64;
        lessons.push(Lesson::StartUpCosts {
            total: start_up,
            share: Ratio::new(share.clamp(0.0, 1.0)).unwrap_or(Ratio::ONE),
        });
    }

    if let Some(review) = &report.rate_review {
        lessons.push(Lesson::RateReview {
            tariff: review.tariff,
            allowed_return: review.allowed_return,
        });
    }

    lessons.truncate(MAX_LESSONS);
    if lessons.is_empty() {
        lessons.push(Lesson::AllClear);
    }
    lessons
}

/// Report money is signed like the ledger (costs negative).
fn magnitude(money: Money) -> Money {
    if money.is_negative() { -money } else { money }
}

/// The lessons of the latest closed month.
#[derive(Debug, Clone, Default)]
pub struct LessonsProjection {
    latest: Vec<Lesson>,
}

impl LessonsProjection {
    /// Empty until the first month closes.
    pub fn latest(&self) -> &[Lesson] {
        &self.latest
    }
}

impl Projection for LessonsProjection {
    fn apply(&mut self, event: &DomainEvent, session: &GameSession) {
        if let DomainEvent::MonthClosed(report) = event {
            self.latest = lessons_for(report, session.region());
        }
    }
}

#[cfg(test)]
mod tests {
    use domain::region::fixtures::tiny_region;
    use domain::regulation::RateReviewOutcome;
    use domain::session::{Incident, IncidentKind};
    use domain::units::Megawatts;

    use super::*;
    use crate::projections::test_support::{report, session};

    fn mwh(value: f64) -> MegawattHours {
        MegawattHours::new(value)
    }

    fn dollars(amount: i64) -> Money {
        Money::from_dollars(amount)
    }

    fn outcome() -> RateReviewOutcome {
        RateReviewOutcome {
            tariff: Price::per_mwh(101.0),
            allowed_return: Ratio::new(0.09).unwrap(),
            revenue_requirement: dollars(1_000_000),
            cost_per_mwh: Some(60.0),
        }
    }

    /// A clean month: plenty served, nothing curtailed, no start-ups.
    fn clean_report() -> MonthlyReport {
        let mut r = report(2027, 6, 1_000.0);
        r.served = mwh(100_000.0);
        r.fuel = -dollars(3_000_000);
        r.fixed_om = -dollars(1_000_000);
        r
    }

    /// A report that triggers every rule at once.
    fn everything_report() -> MonthlyReport {
        let mut r = clean_report();
        r.energy_by_kind.insert(PlantKind::Peaker, mwh(10_000.0));
        r.unserved = mwh(50.0);
        r.incidents = vec![
            Incident {
                tick: 840,
                kind: IncidentKind::LoadShed {
                    mw: Megawatts::new(50.0),
                },
            },
            Incident {
                tick: 900,
                kind: IncidentKind::Blackout,
            },
        ];
        r.curtailed = mwh(2_000.0);
        r.start_up = -dollars(2_000_000);
        r.rate_review = Some(outcome());
        r
    }

    #[test]
    fn lessons_follow_priority_and_cap_at_three() {
        let region = tiny_region();
        let lessons = lessons_for(&everything_report(), &region);
        assert_eq!(lessons.len(), 3);
        assert!(matches!(lessons[0], Lesson::ExpensivePeaking { .. }));
        assert!(matches!(lessons[1], Lesson::UnservedEnergy { .. }));
        assert!(matches!(lessons[2], Lesson::SolarCurtailed { .. }));
    }

    #[test]
    fn clean_month_is_all_clear() {
        let region = tiny_region();
        assert_eq!(
            lessons_for(&clean_report(), &region),
            vec![Lesson::AllClear]
        );
    }

    #[test]
    fn expensive_peaking_prices_the_energy_against_ccgt() {
        let region = tiny_region();
        let mut r = clean_report();
        r.energy_by_kind.insert(PlantKind::Peaker, mwh(6_000.0));
        assert_eq!(
            lessons_for(&r, &region),
            vec![Lesson::ExpensivePeaking {
                energy: mwh(6_000.0),
                // 6,000 MWh at the fixture's $150/MWh peaker.
                cost: dollars(900_000),
                // The fixture's $45/MWh CCGT.
                alternative: Price::per_mwh(45.0),
            }]
        );
    }

    #[test]
    fn unserved_energy_reports_the_first_incident_tick() {
        let region = tiny_region();
        let lessons = lessons_for(&everything_report(), &region);
        assert_eq!(
            lessons[1],
            Lesson::UnservedEnergy {
                energy: mwh(50.0),
                first_incident_tick: 840,
            }
        );

        let mut r = clean_report();
        r.unserved = mwh(1.0);
        assert_eq!(
            lessons_for(&r, &region),
            vec![Lesson::UnservedEnergy {
                energy: mwh(1.0),
                first_incident_tick: 0,
            }]
        );
    }

    #[test]
    fn start_up_costs_use_magnitudes_of_signed_report_money() {
        let region = tiny_region();
        let mut r = clean_report();
        // 1M of 5M operating cost is 20%.
        r.start_up = -dollars(1_000_000);
        r.fuel = -dollars(3_000_000);
        r.fixed_om = -dollars(1_000_000);
        assert_eq!(
            lessons_for(&r, &region),
            vec![Lesson::StartUpCosts {
                total: dollars(1_000_000),
                share: Ratio::new(0.2).unwrap(),
            }]
        );
    }

    #[test]
    fn rate_review_lesson_carries_the_new_tariff_and_return() {
        let region = tiny_region();
        let mut r = clean_report();
        r.rate_review = Some(outcome());
        assert_eq!(
            lessons_for(&r, &region),
            vec![Lesson::RateReview {
                tariff: Price::per_mwh(101.0),
                allowed_return: Ratio::new(0.09).unwrap(),
            }]
        );
    }

    #[test]
    fn thresholds_are_strict() {
        let region = tiny_region();
        let mut r = clean_report();
        // Exactly 5% peaker, 1% curtailed and 10% start-up: none triggers.
        r.energy_by_kind.insert(PlantKind::Peaker, mwh(5_000.0));
        r.curtailed = mwh(1_000.0);
        r.start_up = -dollars(500_000);
        r.fuel = -dollars(3_000_000);
        r.fixed_om = -dollars(1_500_000);
        assert_eq!(lessons_for(&r, &region), vec![Lesson::AllClear]);
    }

    #[test]
    fn a_month_with_no_operating_cost_has_no_start_up_lesson() {
        let region = tiny_region();
        let mut r = clean_report();
        r.fuel = Money::ZERO;
        r.fixed_om = Money::ZERO;
        assert_eq!(lessons_for(&r, &region), vec![Lesson::AllClear]);
    }

    #[test]
    fn projection_holds_the_latest_months_lessons() {
        let session = session();
        let mut projection = LessonsProjection::default();
        assert!(projection.latest().is_empty());

        projection.apply(&DomainEvent::Blackout, &session);
        assert!(projection.latest().is_empty());

        projection.apply(
            &DomainEvent::MonthClosed(Box::new(everything_report())),
            &session,
        );
        assert_eq!(projection.latest().len(), 3);

        projection.apply(
            &DomainEvent::MonthClosed(Box::new(clean_report())),
            &session,
        );
        assert_eq!(projection.latest(), [Lesson::AllClear]);
    }
}
