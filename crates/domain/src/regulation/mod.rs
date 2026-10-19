//! The regulator: licence strikes for unreliable service and the annual
//! performance-based rate review.

pub mod rate_review;

pub use rate_review::{RateReviewInputs, RateReviewOutcome, compute_rate_review, reserve_excess};

use crate::events::DomainEvent;
use crate::region::RegulatorParams;
use crate::units::{MegawattHours, Megawatts, Money, Price, Ratio};

/// Whether the company may still operate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Licence {
    Active,
    /// Permanent: a revoked licence is never reinstated.
    Revoked,
}

/// What the regulator has seen since the last rate review.
#[derive(Debug, Clone, PartialEq)]
pub struct YearTally {
    pub demand: MegawattHours,
    pub served: MegawattHours,
    pub operating_cost: Money,
    /// Highest monthly peak demand.
    pub peak_demand: Megawatts,
    /// Firm capacity commissioned since the last review.
    pub firm_added: Megawatts,
    /// Months recorded since the last review, 0 to 12.
    pub months: u8,
}

impl Default for YearTally {
    fn default() -> Self {
        Self {
            demand: MegawattHours::ZERO,
            served: MegawattHours::ZERO,
            operating_cost: Money::ZERO,
            peak_demand: Megawatts::ZERO,
            firm_added: Megawatts::ZERO,
            months: 0,
        }
    }
}

/// The regulator's state: tariff, allowed return, strikes and licence.
#[derive(Debug, Clone, PartialEq)]
pub struct Regulator {
    tariff: Price,
    allowed_return: Ratio,
    strikes: u8,
    clean_months: u8,
    licence: Licence,
    tally: YearTally,
    last_cost_per_mwh: Option<f64>,
}

impl Regulator {
    pub fn new(p: &RegulatorParams) -> Self {
        Self {
            tariff: p.starting_tariff,
            allowed_return: p.base_return,
            strikes: 0,
            clean_months: 0,
            licence: Licence::Active,
            tally: YearTally::default(),
            last_cost_per_mwh: None,
        }
    }

    pub fn tariff(&self) -> Price {
        self.tariff
    }

    pub fn allowed_return(&self) -> Ratio {
        self.allowed_return
    }

    pub fn strikes(&self) -> u8 {
        self.strikes
    }

    pub fn licence(&self) -> Licence {
        self.licence
    }

    pub fn tally(&self) -> &YearTally {
        &self.tally
    }

    /// Operating cost per MWh served at the last review, if any energy was served.
    pub fn last_cost_per_mwh(&self) -> Option<f64> {
        self.last_cost_per_mwh
    }

    /// Adds one month to the year's tally; the peak is the maximum seen and
    /// the month count saturates at twelve.
    pub fn record_month(
        &mut self,
        demand: MegawattHours,
        served: MegawattHours,
        operating_cost: Money,
        peak: Megawatts,
    ) {
        self.tally.demand += demand;
        self.tally.served += served;
        self.tally.operating_cost += operating_cost;
        self.tally.peak_demand = self.tally.peak_demand.max(peak);
        self.tally.months = self.tally.months.saturating_add(1).min(12);
    }

    /// Notes firm capacity commissioned this year.
    pub fn record_firm_added(&mut self, firm: Megawatts) {
        self.tally.firm_added += firm;
    }

    /// Judges a month's reliability. A month with `unserved / demand` above
    /// the threshold earns a strike; clean months expire strikes; enough
    /// strikes revoke the licence (once, permanently).
    pub fn assess_month_reliability(
        &mut self,
        demand: MegawattHours,
        unserved: MegawattHours,
        p: &RegulatorParams,
    ) -> Vec<DomainEvent> {
        if self.licence == Licence::Revoked {
            return Vec::new();
        }
        let mut events = Vec::new();
        let over =
            demand.value() > 0.0 && unserved.value() / demand.value() > p.strike_threshold.value();
        if over {
            self.strikes = self.strikes.saturating_add(1);
            self.clean_months = 0;
            events.push(DomainEvent::StrikeIssued {
                strikes: self.strikes,
            });
        } else {
            self.clean_months = self.clean_months.saturating_add(1);
            if self.clean_months >= p.clean_months_to_expire && self.strikes > 0 {
                self.strikes -= 1;
                self.clean_months = 0;
                events.push(DomainEvent::StrikeExpired {
                    strikes: self.strikes,
                });
            }
        }
        if self.strikes >= p.strikes_to_revoke {
            self.licence = Licence::Revoked;
            events.push(DomainEvent::LicenseRevoked);
        }
        events
    }

    /// Adopts a rate review's decision and starts a fresh year.
    pub fn apply_rate_review(&mut self, outcome: &RateReviewOutcome) -> Vec<DomainEvent> {
        self.tariff = outcome.tariff;
        self.allowed_return = outcome.allowed_return;
        if outcome.cost_per_mwh.is_some() {
            self.last_cost_per_mwh = outcome.cost_per_mwh;
        }
        self.tally = YearTally::default();
        vec![DomainEvent::RateReviewCompleted {
            tariff: outcome.tariff,
            allowed_return: outcome.allowed_return,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::fixtures::tiny_region;

    const DEMAND: f64 = 1000.0;

    fn bad(r: &mut Regulator, p: &RegulatorParams) -> Vec<DomainEvent> {
        r.assess_month_reliability(
            MegawattHours::new(DEMAND),
            MegawattHours::new(DEMAND * 0.01),
            p,
        )
    }

    fn clean(r: &mut Regulator, p: &RegulatorParams) -> Vec<DomainEvent> {
        r.assess_month_reliability(MegawattHours::new(DEMAND), MegawattHours::ZERO, p)
    }

    #[test]
    fn third_strike_revokes_licence() {
        let region = tiny_region();
        let p = region.regulator();
        let mut r = Regulator::new(p);
        let events: Vec<_> = (0..3).flat_map(|_| bad(&mut r, p)).collect();
        assert_eq!(
            events,
            vec![
                DomainEvent::StrikeIssued { strikes: 1 },
                DomainEvent::StrikeIssued { strikes: 2 },
                DomainEvent::StrikeIssued { strikes: 3 },
                DomainEvent::LicenseRevoked,
            ]
        );
        assert_eq!(r.licence(), Licence::Revoked);
        // revocation is permanent and reported once
        assert!(bad(&mut r, p).is_empty());
        for _ in 0..10 {
            assert!(clean(&mut r, p).is_empty());
        }
        assert_eq!(r.strikes(), 3);
        assert_eq!(r.licence(), Licence::Revoked);
    }

    #[test]
    fn strike_expires_after_three_clean_months() {
        let region = tiny_region();
        let p = region.regulator();
        let mut r = Regulator::new(p);
        assert_eq!(
            bad(&mut r, p),
            vec![DomainEvent::StrikeIssued { strikes: 1 }]
        );
        assert!(clean(&mut r, p).is_empty());
        assert!(clean(&mut r, p).is_empty());
        assert_eq!(
            clean(&mut r, p),
            vec![DomainEvent::StrikeExpired { strikes: 0 }]
        );
        assert_eq!(r.strikes(), 0);
    }

    #[test]
    fn a_bad_month_restarts_the_clean_streak() {
        let region = tiny_region();
        let p = region.regulator();
        let mut r = Regulator::new(p);
        bad(&mut r, p);
        clean(&mut r, p);
        clean(&mut r, p);
        bad(&mut r, p);
        assert!(clean(&mut r, p).is_empty());
        assert!(clean(&mut r, p).is_empty());
        assert_eq!(
            clean(&mut r, p),
            vec![DomainEvent::StrikeExpired { strikes: 1 }]
        );
    }

    #[test]
    fn clean_months_without_strikes_emit_nothing() {
        let region = tiny_region();
        let p = region.regulator();
        let mut r = Regulator::new(p);
        for _ in 0..12 {
            assert!(clean(&mut r, p).is_empty());
        }
        assert_eq!(r.strikes(), 0);
    }

    #[test]
    fn threshold_is_exclusive() {
        let region = tiny_region();
        let p = region.regulator();
        let mut r = Regulator::new(p);
        let at = MegawattHours::new(DEMAND * p.strike_threshold.value());
        assert!(
            r.assess_month_reliability(MegawattHours::new(DEMAND), at, p)
                .is_empty()
        );
        assert_eq!(r.strikes(), 0);
    }

    #[test]
    fn no_strike_without_demand() {
        let region = tiny_region();
        let p = region.regulator();
        let mut r = Regulator::new(p);
        let events = r.assess_month_reliability(MegawattHours::ZERO, MegawattHours::new(5.0), p);
        assert!(events.is_empty());
        assert_eq!(r.strikes(), 0);
    }

    #[test]
    fn tally_accumulates_and_keeps_peak_maximum() {
        let region = tiny_region();
        let mut r = Regulator::new(region.regulator());
        r.record_month(
            MegawattHours::new(100.0),
            MegawattHours::new(90.0),
            Money::from_dollars(10),
            Megawatts::new(50.0),
        );
        r.record_month(
            MegawattHours::new(200.0),
            MegawattHours::new(200.0),
            Money::from_dollars(20),
            Megawatts::new(40.0),
        );
        r.record_firm_added(Megawatts::new(30.0));
        r.record_firm_added(Megawatts::new(5.0));
        let t = r.tally();
        assert_eq!(t.demand, MegawattHours::new(300.0));
        assert_eq!(t.served, MegawattHours::new(290.0));
        assert_eq!(t.operating_cost, Money::from_dollars(30));
        assert_eq!(t.peak_demand, Megawatts::new(50.0));
        assert_eq!(t.firm_added, Megawatts::new(35.0));
        assert_eq!(t.months, 2);
    }

    #[test]
    fn the_months_tally_saturates_at_twelve() {
        let region = tiny_region();
        let mut r = Regulator::new(region.regulator());
        for _ in 0..15 {
            r.record_month(
                MegawattHours::new(1.0),
                MegawattHours::new(1.0),
                Money::ZERO,
                Megawatts::new(1.0),
            );
        }
        assert_eq!(r.tally().months, 12);
    }

    #[test]
    fn new_regulator_starts_at_region_values() {
        let region = tiny_region();
        let p = region.regulator();
        let r = Regulator::new(p);
        assert_eq!(r.tariff(), p.starting_tariff);
        assert_eq!(r.allowed_return(), p.base_return);
        assert_eq!(r.licence(), Licence::Active);
        assert_eq!(r.last_cost_per_mwh(), None);
        assert_eq!(r.tally(), &YearTally::default());
    }

    #[test]
    fn apply_rate_review_adopts_outcome_and_resets_tally() {
        let region = tiny_region();
        let mut r = Regulator::new(region.regulator());
        r.record_month(
            MegawattHours::new(100.0),
            MegawattHours::new(100.0),
            Money::from_dollars(10),
            Megawatts::new(50.0),
        );
        let outcome = RateReviewOutcome {
            tariff: Price::per_mwh(101.5),
            allowed_return: region.regulator().max_return,
            revenue_requirement: Money::from_dollars(1),
            cost_per_mwh: Some(42.0),
        };
        assert_eq!(r.tally().months, 1);
        let events = r.apply_rate_review(&outcome);
        assert_eq!(
            events,
            vec![DomainEvent::RateReviewCompleted {
                tariff: outcome.tariff,
                allowed_return: outcome.allowed_return,
            }]
        );
        assert_eq!(r.tariff(), outcome.tariff);
        assert_eq!(r.allowed_return(), outcome.allowed_return);
        assert_eq!(r.last_cost_per_mwh(), Some(42.0));
        assert_eq!(r.tally(), &YearTally::default());

        // a review with no served energy keeps the previous cost per MWh
        let idle = RateReviewOutcome {
            cost_per_mwh: None,
            ..outcome
        };
        r.apply_rate_review(&idle);
        assert_eq!(r.last_cost_per_mwh(), Some(42.0));
    }
}
