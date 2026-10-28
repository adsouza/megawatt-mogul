//! What the month just closed has to teach the player.

use domain::events::DomainEvent;
use domain::region::{PlantKind, RegionDefinition};
use domain::session::{GameSession, IncidentKind, MonthlyReport};
use domain::units::{MegawattHours, Money, Price, Ratio};

use super::Projection;

/// At most this many lessons are shown for a month.
const MAX_LESSONS: usize = 3;

/// Peaker energy above this share of served energy is expensive peaking.
const PEAKER_SHARE_LIMIT: f64 = 0.05;

/// Curtailed solar above this share of served energy is worth a lesson.
const CURTAILED_SHARE_LIMIT: f64 = 0.01;

/// Overgeneration above this share of served energy is worth a lesson.
const OVERGENERATION_SHARE_LIMIT: f64 = 0.02;

/// Start-up costs above this share of operating cost are worth a lesson.
const START_UP_SHARE_LIMIT: f64 = 0.10;

/// `unserved` and `rotating_outage` are summed separately over the month, so
/// their difference carries float noise. Unplanned energy within this share
/// of `unserved` is noise, not protection shedding. Relative, like the
/// report's own `rotating_outage <= unserved` check, and far above the
/// ~1e-13 summation error of a month's ticks.
const UNPLANNED_RELATIVE_TOLERANCE: f64 = 1e-9;

/// One thing the player can learn from a closed month.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Lesson {
    GeothermalSuccess {
        plant: domain::ids::PlantId,
    },
    GeothermalDry {
        plant: domain::ids::PlantId,
        lost: Money,
    },
    GeothermalDecline {
        plant: domain::ids::PlantId,
        threshold: u8,
        first: bool,
    },

    /// Peakers supplied a large share of the month's energy at a high price.
    ExpensivePeaking {
        energy: MegawattHours,
        /// What that energy cost in fuel.
        cost: Money,
        /// The region's CCGT marginal cost, the cheaper mid-merit option.
        alternative: Price,
    },
    /// The fleet could not cover the load, so the grid cut demand on purpose:
    /// `energy` is the planned rotating-outage share of unserved energy and
    /// `peak_share` the largest share ordered. This is the lesson for planned
    /// shortages (build or commit capacity); protection-driven shedding is
    /// [`UnservedEnergy`](Self::UnservedEnergy).
    RotatingOutages {
        energy: MegawattHours,
        peak_share: Ratio,
    },
    /// Protection shed demand beyond any planned outage: `energy` is the
    /// unserved energy that was not a rotating outage. This is the lesson for
    /// protection-driven shedding (keep reserves and ramp); planned shortages
    /// are [`RotatingOutages`](Self::RotatingOutages). `first_incident_tick`
    /// is the first incident that is not a rotating-outage order (zero when
    /// there is none).
    UnservedEnergy {
        energy: MegawattHours,
        first_incident_tick: u32,
    },
    /// Generation outran demand: surplus energy (positive imbalance) and/or
    /// plants tripped by over-frequency protection.
    Overgeneration {
        energy: MegawattHours,
        trips: u32,
    },
    /// The month failed frequency performance; `notice` is true when it drew
    /// a formal notice rather than a strike. In a month that also failed the
    /// unserved-energy test, `notice: false` means the month's one strike was
    /// for unserved energy and frequency was not judged separately, not that
    /// frequency drew a strike of its own: the Plan 2 UI should read the
    /// strike's reason from the `StrikeIssued` event.
    FrequencyPerformance {
        out_of_band: Ratio,
        notice: bool,
    },
    /// Solar was curtailed because there was nowhere for it to go.
    SolarCurtailed {
        energy: MegawattHours,
    },
    RenewableCurtailed {
        energy: MegawattHours,
    },
    /// Starting plants cost a large share of operating cost.
    StartUpCosts {
        total: Money,
        share: Ratio,
    },
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
/// Priority: expensive peaking, rotating outages, unserved energy,
/// overgeneration, frequency performance, curtailed solar, start-up costs,
/// then the rate review.
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

    if report.rotating_outage > MegawattHours::ZERO {
        lessons.push(Lesson::RotatingOutages {
            energy: report.rotating_outage,
            peak_share: report.peak_outage_share,
        });
    }

    // `rotating_outage` is part of `unserved`: only the rest is protection's.
    let unplanned = report.unserved - report.rotating_outage;
    if unplanned.value() > report.unserved.value() * UNPLANNED_RELATIVE_TOLERANCE {
        lessons.push(Lesson::UnservedEnergy {
            energy: unplanned,
            first_incident_tick: report
                .incidents
                .iter()
                .find(|incident| !matches!(incident.kind, IncidentKind::RotatingOutage { .. }))
                .map_or(0, |incident| incident.tick),
        });
    }

    if report.overgeneration.value() > served * OVERGENERATION_SHARE_LIMIT
        || report.over_frequency_trips >= 1
    {
        lessons.push(Lesson::Overgeneration {
            energy: report.overgeneration,
            trips: report.over_frequency_trips,
        });
    }

    if report.frequency_failed {
        lessons.push(Lesson::FrequencyPerformance {
            out_of_band: report.out_of_band_share,
            notice: report.frequency_notice,
        });
    }

    if report.curtailed.value() > served * CURTAILED_SHARE_LIMIT {
        lessons.push(
            if report
                .curtailed_by_kind
                .get(&PlantKind::Wind)
                .is_some_and(|e| e.value() > 0.0)
            {
                Lesson::RenewableCurtailed {
                    energy: report.curtailed,
                }
            } else {
                Lesson::SolarCurtailed {
                    energy: report.curtailed,
                }
            },
        );
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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LessonsProjection {
    latest: Vec<Lesson>,
    pending_geothermal: Vec<Lesson>,
}

impl LessonsProjection {
    /// Empty until the first month closes.
    pub fn latest(&self) -> &[Lesson] {
        &self.latest
    }
}

impl Projection for LessonsProjection {
    fn apply(&mut self, event: &DomainEvent, session: &GameSession) {
        match event {
            DomainEvent::ExplorationSucceeded { plant } => self
                .pending_geothermal
                .push(Lesson::GeothermalSuccess { plant: *plant }),
            DomainEvent::ExplorationDry {
                plant,
                capital_lost,
            } => self.pending_geothermal.push(Lesson::GeothermalDry {
                plant: *plant,
                lost: *capital_lost,
            }),
            DomainEvent::FieldDeclineThreshold {
                plant,
                threshold,
                first,
            } => self.pending_geothermal.push(Lesson::GeothermalDecline {
                plant: *plant,
                threshold: *threshold,
                first: *first,
            }),
            DomainEvent::MonthClosed(report) => {
                self.latest = lessons_for(report, session.region());
                if !self.pending_geothermal.is_empty() {
                    self.latest
                        .retain(|lesson| !matches!(lesson, Lesson::AllClear));
                    let priority = self
                        .latest
                        .iter()
                        .rposition(|lesson| {
                            matches!(
                                lesson,
                                Lesson::UnservedEnergy { .. }
                                    | Lesson::RotatingOutages { .. }
                                    | Lesson::FrequencyPerformance { .. }
                                    | Lesson::Overgeneration { .. }
                            )
                        })
                        .map_or(0, |i| i + 1);
                    self.latest
                        .splice(priority..priority, self.pending_geothermal.drain(..));
                    self.latest.truncate(MAX_LESSONS);
                }
            }
            _ => {}
        }
    }
}

// Portable state schema 1: explicit field order and enum tags.
domain::state_record!(LessonsProjection { latest; since6 { pending_geothermal: Vec::new() } });
impl domain::state::State for Lesson {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::GeothermalSuccess { plant } => {
                domain::state::State::write(&10u8, out);
                domain::state::State::write(plant, out);
            }
            Self::GeothermalDry { plant, lost } => {
                domain::state::State::write(&11u8, out);
                domain::state::State::write(plant, out);
                domain::state::State::write(lost, out);
            }
            Self::GeothermalDecline {
                plant,
                threshold,
                first,
            } => {
                domain::state::State::write(&12u8, out);
                domain::state::State::write(plant, out);
                domain::state::State::write(threshold, out);
                domain::state::State::write(first, out);
            }
            Self::ExpensivePeaking {
                energy,
                cost,
                alternative,
            } => {
                domain::state::State::write(&0u8, out);
                domain::state::State::write(energy, out);
                domain::state::State::write(cost, out);
                domain::state::State::write(alternative, out);
            }
            Self::RotatingOutages { energy, peak_share } => {
                domain::state::State::write(&1u8, out);
                domain::state::State::write(energy, out);
                domain::state::State::write(peak_share, out);
            }
            Self::UnservedEnergy {
                energy,
                first_incident_tick,
            } => {
                domain::state::State::write(&2u8, out);
                domain::state::State::write(energy, out);
                domain::state::State::write(first_incident_tick, out);
            }
            Self::Overgeneration { energy, trips } => {
                domain::state::State::write(&3u8, out);
                domain::state::State::write(energy, out);
                domain::state::State::write(trips, out);
            }
            Self::FrequencyPerformance {
                out_of_band,
                notice,
            } => {
                domain::state::State::write(&4u8, out);
                domain::state::State::write(out_of_band, out);
                domain::state::State::write(notice, out);
            }
            Self::SolarCurtailed { energy } => {
                domain::state::State::write(&5u8, out);
                domain::state::State::write(energy, out);
            }
            Self::StartUpCosts { total, share } => {
                domain::state::State::write(&6u8, out);
                domain::state::State::write(total, out);
                domain::state::State::write(share, out);
            }
            Self::RateReview {
                tariff,
                allowed_return,
            } => {
                domain::state::State::write(&7u8, out);
                domain::state::State::write(tariff, out);
                domain::state::State::write(allowed_return, out);
            }
            Self::RenewableCurtailed { energy } => {
                domain::state::State::write(&9u8, out);
                domain::state::State::write(energy, out);
            }
            Self::AllClear => {
                domain::state::State::write(&8u8, out);
            }
        }
    }
    fn read(input: &mut domain::state::StateReader<'_>) -> Result<Self, domain::state::StateError> {
        Ok(match <u8 as domain::state::State>::read(input)? {
            10 if input.schema >= 6 => Self::GeothermalSuccess {
                plant: domain::state::State::read(input)?,
            },
            11 if input.schema >= 6 => Self::GeothermalDry {
                plant: domain::state::State::read(input)?,
                lost: domain::state::State::read(input)?,
            },
            12 if input.schema >= 6 => Self::GeothermalDecline {
                plant: domain::state::State::read(input)?,
                threshold: domain::state::State::read(input)?,
                first: domain::state::State::read(input)?,
            },
            0 => Self::ExpensivePeaking {
                energy: domain::state::State::read(input)?,
                cost: domain::state::State::read(input)?,
                alternative: domain::state::State::read(input)?,
            },
            1 => Self::RotatingOutages {
                energy: domain::state::State::read(input)?,
                peak_share: domain::state::State::read(input)?,
            },
            2 => Self::UnservedEnergy {
                energy: domain::state::State::read(input)?,
                first_incident_tick: domain::state::State::read(input)?,
            },
            3 => Self::Overgeneration {
                energy: domain::state::State::read(input)?,
                trips: domain::state::State::read(input)?,
            },
            4 => Self::FrequencyPerformance {
                out_of_band: domain::state::State::read(input)?,
                notice: domain::state::State::read(input)?,
            },
            5 => Self::SolarCurtailed {
                energy: domain::state::State::read(input)?,
            },
            6 => Self::StartUpCosts {
                total: domain::state::State::read(input)?,
                share: domain::state::State::read(input)?,
            },
            7 => Self::RateReview {
                tariff: domain::state::State::read(input)?,
                allowed_return: domain::state::State::read(input)?,
            },
            8 => Self::AllClear,
            9 if input.schema >= 2 => Self::RenewableCurtailed {
                energy: domain::state::State::read(input)?,
            },
            _ => return Err(domain::state::StateError::Invalid),
        })
    }
}

#[cfg(test)]
mod tests {
    use domain::region::fixtures::tiny_region;
    use domain::regulation::RateReviewOutcome;
    use domain::session::Incident;
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
    fn overgeneration_lesson_fires_on_surplus_or_trips() {
        let region = tiny_region();

        // Surplus above 2% of the 100,000 MWh served.
        let mut r = clean_report();
        r.overgeneration = mwh(2_500.0);
        assert_eq!(
            lessons_for(&r, &region),
            vec![Lesson::Overgeneration {
                energy: mwh(2_500.0),
                trips: 0,
            }]
        );

        // A single over-frequency trip is enough, whatever the surplus.
        let mut r = clean_report();
        r.over_frequency_trips = 1;
        assert_eq!(
            lessons_for(&r, &region),
            vec![Lesson::Overgeneration {
                energy: mwh(0.0),
                trips: 1,
            }]
        );

        // Exactly 2% and no trips: the threshold is strict.
        let mut r = clean_report();
        r.overgeneration = mwh(2_000.0);
        assert_eq!(lessons_for(&r, &region), vec![Lesson::AllClear]);
    }

    #[test]
    fn frequency_lesson_reports_notice_flag() {
        let region = tiny_region();
        let out_of_band = Ratio::new(0.08).unwrap();

        let mut r = clean_report();
        r.out_of_band_share = out_of_band;
        // A band excursion that did not fail the month teaches nothing.
        assert_eq!(lessons_for(&r, &region), vec![Lesson::AllClear]);

        r.frequency_failed = true;
        r.frequency_notice = true;
        assert_eq!(
            lessons_for(&r, &region),
            vec![Lesson::FrequencyPerformance {
                out_of_band,
                notice: true,
            }]
        );

        r.frequency_notice = false;
        assert_eq!(
            lessons_for(&r, &region),
            vec![Lesson::FrequencyPerformance {
                out_of_band,
                notice: false,
            }]
        );
    }

    #[test]
    fn lesson_priority_places_new_lessons_after_unserved() {
        let region = tiny_region();
        let mut r = everything_report();
        r.overgeneration = mwh(5_000.0);
        r.over_frequency_trips = 2;
        r.out_of_band_share = Ratio::new(0.1).unwrap();
        r.frequency_failed = true;

        // Peaking, unserved, then overgeneration: the cap of three cuts the rest.
        let lessons = lessons_for(&r, &region);
        assert_eq!(lessons.len(), 3);
        assert!(matches!(lessons[0], Lesson::ExpensivePeaking { .. }));
        assert!(matches!(lessons[1], Lesson::UnservedEnergy { .. }));
        assert!(matches!(lessons[2], Lesson::Overgeneration { .. }));

        // Without peaking or unserved energy, the new lessons lead, in order,
        // ahead of curtailment, start-up costs and the rate review.
        r.energy_by_kind.clear();
        r.unserved = MegawattHours::ZERO;
        let lessons = lessons_for(&r, &region);
        assert!(matches!(lessons[0], Lesson::Overgeneration { .. }));
        assert!(matches!(lessons[1], Lesson::FrequencyPerformance { .. }));
        assert!(matches!(lessons[2], Lesson::SolarCurtailed { .. }));
    }

    fn outage_incident(tick: u32, share: f64) -> Incident {
        Incident {
            tick,
            kind: IncidentKind::RotatingOutage {
                share: Ratio::new(share).unwrap(),
            },
        }
    }

    #[test]
    fn rotating_outages_lesson_fires_on_planned_energy() {
        let region = tiny_region();
        let mut r = clean_report();
        // Planned energy is part of `unserved`: all 30 MWh of it is planned.
        r.unserved = mwh(30.0);
        r.rotating_outage = mwh(30.0);
        r.peak_outage_share = Ratio::new(0.25).unwrap();
        assert_eq!(
            lessons_for(&r, &region),
            vec![Lesson::RotatingOutages {
                energy: mwh(30.0),
                peak_share: Ratio::new(0.25).unwrap(),
            }]
        );
    }

    #[test]
    fn unserved_lesson_counts_only_unplanned_energy() {
        let region = tiny_region();
        let mut r = clean_report();
        r.unserved = mwh(50.0);
        r.rotating_outage = mwh(30.0);
        r.peak_outage_share = Ratio::new(0.25).unwrap();
        r.incidents = vec![
            outage_incident(0, 0.25),
            Incident {
                tick: 840,
                kind: IncidentKind::LoadShed {
                    mw: Megawatts::new(50.0),
                },
            },
        ];
        // The unserved lesson reports the 20 MWh protection shed, and its
        // first incident skips the planned outage order at tick 0.
        assert_eq!(
            lessons_for(&r, &region),
            vec![
                Lesson::RotatingOutages {
                    energy: mwh(30.0),
                    peak_share: Ratio::new(0.25).unwrap(),
                },
                Lesson::UnservedEnergy {
                    energy: mwh(20.0),
                    first_incident_tick: 840,
                },
            ]
        );

        // With no unplanned incident recorded, the tick falls back to zero.
        r.incidents = vec![outage_incident(60, 0.25)];
        assert_eq!(
            lessons_for(&r, &region)[1],
            Lesson::UnservedEnergy {
                energy: mwh(20.0),
                first_incident_tick: 0,
            }
        );
    }

    #[test]
    fn an_outage_only_month_has_no_unserved_lesson() {
        let region = tiny_region();
        let mut r = clean_report();
        r.peak_outage_share = Ratio::new(0.1).unwrap();
        r.incidents = vec![outage_incident(0, 0.1)];

        // Exactly equal: nothing was shed by protection.
        r.unserved = mwh(30.0);
        r.rotating_outage = mwh(30.0);
        let lessons = lessons_for(&r, &region);
        assert!(matches!(lessons[..], [Lesson::RotatingOutages { .. }]));

        // Float noise from summing the two figures differently is not shed
        // energy either.
        r.unserved = mwh(0.1 + 0.2);
        r.rotating_outage = mwh(0.3);
        let lessons = lessons_for(&r, &region);
        assert!(matches!(lessons[..], [Lesson::RotatingOutages { .. }]));

        // A real, small unplanned remainder still fires.
        r.unserved = mwh(30.5);
        r.rotating_outage = mwh(30.0);
        let lessons = lessons_for(&r, &region);
        assert!(matches!(
            lessons[..],
            [
                Lesson::RotatingOutages { .. },
                Lesson::UnservedEnergy { .. }
            ]
        ));
    }

    #[test]
    fn lesson_priority_places_outages_before_unserved() {
        let region = tiny_region();
        let mut r = everything_report();
        r.unserved = mwh(80.0);
        r.rotating_outage = mwh(30.0);
        r.peak_outage_share = Ratio::new(0.2).unwrap();

        // Peaking, outages, unserved: the cap of three cuts the rest.
        let lessons = lessons_for(&r, &region);
        assert_eq!(lessons.len(), 3);
        assert!(matches!(lessons[0], Lesson::ExpensivePeaking { .. }));
        assert!(matches!(lessons[1], Lesson::RotatingOutages { .. }));
        assert!(matches!(lessons[2], Lesson::UnservedEnergy { .. }));

        // Without peaking, the outages lead.
        r.energy_by_kind.clear();
        let lessons = lessons_for(&r, &region);
        assert!(matches!(lessons[0], Lesson::RotatingOutages { .. }));
        assert!(matches!(lessons[1], Lesson::UnservedEnergy { .. }));
        assert!(matches!(lessons[2], Lesson::SolarCurtailed { .. }));
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
