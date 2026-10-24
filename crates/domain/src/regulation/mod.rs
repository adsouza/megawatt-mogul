//! The regulator: licence strikes for unreliable service and poor frequency
//! performance (a notice comes first), and the annual performance-based rate
//! review.

pub mod rate_review;

pub use rate_review::{
    RateReviewInputs, RateReviewOutcome, compute_rate_review, reserve_excess,
    satisfaction_adjustment,
};

use crate::events::DomainEvent;
use crate::region::RegulatorParams;
use crate::time::SimMonth;
use crate::units::{MegawattHours, Megawatts, Money, Price, Ratio};

/// Months a frequency notice protects the company from a frequency strike.
const NOTICE_WINDOW_MONTHS: i32 = 12;

/// Which performance test a strike was issued for. A month failing both
/// tests is struck for [`UnservedEnergy`](Self::UnservedEnergy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrikeReason {
    UnservedEnergy,
    FrequencyPerformance,
}

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
    last_frequency_notice: Option<SimMonth>,
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
            last_frequency_notice: None,
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

    /// The month of the last formal frequency-performance notice, if any.
    pub fn last_frequency_notice(&self) -> Option<SimMonth> {
        self.last_frequency_notice
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

    /// Whether a month with this out-of-band share fails frequency
    /// performance: strictly more than the allowed share of Normal-mode ticks.
    pub fn fails_frequency(out_of_band: Ratio, p: &RegulatorParams) -> bool {
        out_of_band > p.frequency_violation_share
    }

    /// Judges a month in `month` against the two performance tests (spec
    /// §14.4); a revoked licence is never judged again.
    ///
    /// - Failing unserved energy (`unserved / demand` above the threshold)
    ///   earns one strike for [`StrikeReason::UnservedEnergy`], whether or not
    ///   frequency also failed, and leaves the notice untouched.
    /// - Failing only frequency performance (`out_of_band` above the allowed
    ///   share) draws a formal notice and no strike when none was issued in
    ///   the last 12 months; otherwise it earns a
    ///   [`StrikeReason::FrequencyPerformance`] strike.
    /// - A month passing both tests is clean: clean months expire strikes. A
    ///   notice month or a strike month restarts the clean streak.
    ///
    /// Enough strikes revoke the licence (once, permanently).
    pub fn assess_month(
        &mut self,
        month: SimMonth,
        demand: MegawattHours,
        unserved: MegawattHours,
        out_of_band: Ratio,
        p: &RegulatorParams,
    ) -> Vec<DomainEvent> {
        if self.licence == Licence::Revoked {
            return Vec::new();
        }
        let mut events = Vec::new();
        let unserved_failed =
            demand.value() > 0.0 && unserved.value() / demand.value() > p.strike_threshold.value();
        if unserved_failed {
            self.issue_strike(StrikeReason::UnservedEnergy, &mut events);
        } else if Self::fails_frequency(out_of_band, p) {
            if self.notice_due(month) {
                self.last_frequency_notice = Some(month);
                self.clean_months = 0;
                events.push(DomainEvent::FrequencyNoticeIssued { month });
            } else {
                self.issue_strike(StrikeReason::FrequencyPerformance, &mut events);
            }
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

    /// A notice is due when none has been issued, or the last was at least a
    /// year before `month`.
    fn notice_due(&self, month: SimMonth) -> bool {
        self.last_frequency_notice
            .is_none_or(|last| month.months_since(last) >= NOTICE_WINDOW_MONTHS)
    }

    fn issue_strike(&mut self, reason: StrikeReason, events: &mut Vec<DomainEvent>) {
        self.strikes = self.strikes.saturating_add(1);
        self.clean_months = 0;
        events.push(DomainEvent::StrikeIssued {
            strikes: self.strikes,
            reason,
        });
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

// Portable state schema 1: explicit field order and enum tags.
crate::state_record!(Regulator {
    tariff,
    allowed_return,
    strikes,
    clean_months,
    licence,
    tally,
    last_cost_per_mwh,
    last_frequency_notice
});
impl crate::state::State for Licence {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Active => {
                crate::state::State::write(&0u8, out);
            }
            Self::Revoked => {
                crate::state::State::write(&1u8, out);
            }
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(match <u8 as crate::state::State>::read(input)? {
            0 => Self::Active,
            1 => Self::Revoked,
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}
crate::state_record!(YearTally {
    demand,
    served,
    operating_cost,
    peak_demand,
    firm_added,
    months
});
impl crate::state::State for StrikeReason {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::UnservedEnergy => {
                crate::state::State::write(&0u8, out);
            }
            Self::FrequencyPerformance => {
                crate::state::State::write(&1u8, out);
            }
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(match <u8 as crate::state::State>::read(input)? {
            0 => Self::UnservedEnergy,
            1 => Self::FrequencyPerformance,
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::fixtures::tiny_region;
    use crate::time::SimMonth;

    const DEMAND: f64 = 1000.0;

    fn month(year: i32, month: u8) -> SimMonth {
        SimMonth::new(year, month).unwrap()
    }

    /// A share just above the violation limit.
    fn out_of_band(p: &RegulatorParams) -> Ratio {
        Ratio::new(p.frequency_violation_share.value() + 0.01).unwrap()
    }

    /// Fails the unserved-energy test only.
    fn bad(r: &mut Regulator, p: &RegulatorParams) -> Vec<DomainEvent> {
        r.assess_month(
            month(2027, 1),
            MegawattHours::new(DEMAND),
            MegawattHours::new(DEMAND * 0.01),
            Ratio::ZERO,
            p,
        )
    }

    /// Passes both tests.
    fn clean(r: &mut Regulator, p: &RegulatorParams) -> Vec<DomainEvent> {
        r.assess_month(
            month(2027, 1),
            MegawattHours::new(DEMAND),
            MegawattHours::ZERO,
            Ratio::ZERO,
            p,
        )
    }

    /// Fails the frequency test only, in `at`.
    fn noisy(r: &mut Regulator, at: SimMonth, p: &RegulatorParams) -> Vec<DomainEvent> {
        r.assess_month(
            at,
            MegawattHours::new(DEMAND),
            MegawattHours::ZERO,
            out_of_band(p),
            p,
        )
    }

    fn unserved_strike(strikes: u8) -> DomainEvent {
        DomainEvent::StrikeIssued {
            strikes,
            reason: StrikeReason::UnservedEnergy,
        }
    }

    fn frequency_strike(strikes: u8) -> DomainEvent {
        DomainEvent::StrikeIssued {
            strikes,
            reason: StrikeReason::FrequencyPerformance,
        }
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
                unserved_strike(1),
                unserved_strike(2),
                unserved_strike(3),
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
        assert_eq!(bad(&mut r, p), vec![unserved_strike(1)]);
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
            r.assess_month(
                month(2027, 1),
                MegawattHours::new(DEMAND),
                at,
                Ratio::ZERO,
                p
            )
            .is_empty()
        );
        assert_eq!(r.strikes(), 0);
    }

    #[test]
    fn no_strike_without_demand() {
        let region = tiny_region();
        let p = region.regulator();
        let mut r = Regulator::new(p);
        let events = r.assess_month(
            month(2027, 1),
            MegawattHours::ZERO,
            MegawattHours::new(5.0),
            Ratio::ZERO,
            p,
        );
        assert!(events.is_empty());
        assert_eq!(r.strikes(), 0);
    }

    #[test]
    fn first_frequency_only_failure_draws_a_notice_not_a_strike() {
        let region = tiny_region();
        let p = region.regulator();
        let mut r = Regulator::new(p);
        assert_eq!(r.last_frequency_notice(), None);
        let at = month(2027, 3);
        assert_eq!(
            noisy(&mut r, at, p),
            vec![DomainEvent::FrequencyNoticeIssued { month: at }]
        );
        assert_eq!(r.strikes(), 0);
        assert_eq!(r.last_frequency_notice(), Some(at));
        assert_eq!(r.licence(), Licence::Active);
    }

    #[test]
    fn the_violation_share_is_exclusive() {
        let region = tiny_region();
        let p = region.regulator();
        let mut r = Regulator::new(p);
        let events = r.assess_month(
            month(2027, 3),
            MegawattHours::new(DEMAND),
            MegawattHours::ZERO,
            p.frequency_violation_share,
            p,
        );
        assert!(events.is_empty());
        assert_eq!(r.last_frequency_notice(), None);
    }

    #[test]
    fn second_frequency_failure_within_twelve_months_strikes() {
        let region = tiny_region();
        let p = region.regulator();
        let mut r = Regulator::new(p);
        noisy(&mut r, month(2027, 3), p);
        // 11 months after the notice is still inside the window.
        assert_eq!(noisy(&mut r, month(2028, 2), p), vec![frequency_strike(1)]);
        assert_eq!(r.strikes(), 1);
        // The strike did not renew the notice.
        assert_eq!(r.last_frequency_notice(), Some(month(2027, 3)));
    }

    #[test]
    fn a_notice_is_due_again_after_twelve_months() {
        let region = tiny_region();
        let p = region.regulator();
        let mut r = Regulator::new(p);
        noisy(&mut r, month(2027, 3), p);
        let again = month(2028, 3);
        assert_eq!(
            noisy(&mut r, again, p),
            vec![DomainEvent::FrequencyNoticeIssued { month: again }]
        );
        assert_eq!(r.strikes(), 0);
        assert_eq!(r.last_frequency_notice(), Some(again));
    }

    #[test]
    fn both_failing_gives_one_unserved_strike_and_keeps_the_notice() {
        let region = tiny_region();
        let p = region.regulator();
        let mut r = Regulator::new(p);
        let events = r.assess_month(
            month(2027, 3),
            MegawattHours::new(DEMAND),
            MegawattHours::new(DEMAND * 0.01),
            out_of_band(p),
            p,
        );
        assert_eq!(events, vec![unserved_strike(1)]);
        assert_eq!(r.strikes(), 1);
        // The notice is not used up: the next frequency-only failure draws it.
        assert_eq!(r.last_frequency_notice(), None);
        let at = month(2027, 4);
        assert_eq!(
            noisy(&mut r, at, p),
            vec![DomainEvent::FrequencyNoticeIssued { month: at }]
        );
        assert_eq!(r.strikes(), 1);
    }

    #[test]
    fn notice_month_resets_the_clean_streak() {
        let region = tiny_region();
        let p = region.regulator();
        let mut r = Regulator::new(p);
        bad(&mut r, p);
        clean(&mut r, p);
        clean(&mut r, p);
        noisy(&mut r, month(2027, 3), p);
        // Two clean months since the notice are not enough to expire the strike.
        assert!(clean(&mut r, p).is_empty());
        assert!(clean(&mut r, p).is_empty());
        assert_eq!(
            clean(&mut r, p),
            vec![DomainEvent::StrikeExpired { strikes: 0 }]
        );
    }

    #[test]
    fn a_frequency_strike_month_is_not_clean_for_expiry() {
        let region = tiny_region();
        let p = region.regulator();
        let mut r = Regulator::new(p);
        noisy(&mut r, month(2027, 1), p); // notice
        bad(&mut r, p); // strike 1
        clean(&mut r, p);
        clean(&mut r, p);
        assert_eq!(noisy(&mut r, month(2027, 4), p), vec![frequency_strike(2)]);
        // The streak restarted at the frequency strike.
        assert!(clean(&mut r, p).is_empty());
        assert!(clean(&mut r, p).is_empty());
        assert_eq!(
            clean(&mut r, p),
            vec![DomainEvent::StrikeExpired { strikes: 1 }]
        );
    }

    #[test]
    fn three_strikes_from_any_mix_revoke() {
        let region = tiny_region();
        let p = region.regulator();
        let mut r = Regulator::new(p);
        let mut events = bad(&mut r, p);
        events.extend(noisy(&mut r, month(2027, 3), p)); // notice
        events.extend(noisy(&mut r, month(2027, 4), p)); // strike 2
        events.extend(noisy(&mut r, month(2027, 5), p)); // strike 3
        assert_eq!(
            events,
            vec![
                unserved_strike(1),
                DomainEvent::FrequencyNoticeIssued {
                    month: month(2027, 3)
                },
                frequency_strike(2),
                frequency_strike(3),
                DomainEvent::LicenseRevoked,
            ]
        );
        assert_eq!(r.licence(), Licence::Revoked);
        // Revoked is final: nothing more is judged.
        assert!(noisy(&mut r, month(2027, 6), p).is_empty());
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
        assert_eq!(r.last_frequency_notice(), None);
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
