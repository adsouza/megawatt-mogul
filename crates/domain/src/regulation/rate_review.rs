//! The annual performance-based rate review: a pure function from the year's
//! tally to a new tariff and allowed return.

use crate::region::RegulatorParams;
use crate::regulation::YearTally;
use crate::units::{Megawatts, Money, Price, Ratio};

/// Everything the regulator weighs at a rate review.
#[derive(Debug, Clone, PartialEq)]
pub struct RateReviewInputs {
    pub tally: YearTally,
    pub depreciation: Money,
    /// Rate base excluding held and disallowed capital.
    pub eligible_rate_base: Money,
    pub firm_capacity: Megawatts,
    pub projected_peak: Megawatts,
    pub previous_tariff: Price,
    pub previous_cost_per_mwh: Option<f64>,
}

/// The regulator's decision.
#[derive(Debug, Clone, PartialEq)]
pub struct RateReviewOutcome {
    pub tariff: Price,
    pub allowed_return: Ratio,
    pub revenue_requirement: Money,
    pub cost_per_mwh: Option<f64>,
}

/// Firm capacity beyond what the regulator considers prudent:
/// `max(0, firm − max(actual, projected) × (1 + margin))`.
///
/// This is a capacity amount, not a write-off fraction; the rate base
/// allocates it to held portions and new capital.
pub fn reserve_excess(
    firm: Megawatts,
    actual_peak: Megawatts,
    projected_peak: Megawatts,
    reserve_margin: Ratio,
) -> Megawatts {
    let review_peak = actual_peak.max(projected_peak);
    let prudent = review_peak * (1.0 + reserve_margin.value());
    (firm - prudent).max(Megawatts::ZERO)
}

/// Reviews the year. Pure and total: zero or negative energy never divides by
/// zero, and the allowed return always lands inside the regulator's band.
///
/// - `reliability = served / demand` (the target when nothing was demanded);
/// - `cost_per_mwh = operating cost / served` (`None` when nothing was served);
/// - `return = base + per_reliability × (reliability − target)
///   + per_cost_improvement × (prev − cur) / prev`, the last term only when
///   both costs exist, clamped to `[min, max]`;
/// - `revenue requirement = operating cost + depreciation + return × eligible
///   rate base × min(months, 12) ÷ 12`, where `months` (0 to 12) is how much of the
///   year the tally covers: a short first period earns a proportionate return
///   and zero months earn none;
/// - `tariff = revenue requirement / served` (the previous tariff when nothing was served).
pub fn compute_rate_review(i: &RateReviewInputs, p: &RegulatorParams) -> RateReviewOutcome {
    let served = i.tally.served.value();
    let demand = i.tally.demand.value();
    let operating_dollars = i.tally.operating_cost.cents() as f64 / 100.0;

    let reliability = if demand > 0.0 {
        served / demand
    } else {
        p.reliability_target.value()
    };
    let cost_per_mwh = (served > 0.0).then(|| operating_dollars / served);

    let mut raw = p.base_return.value()
        + p.return_per_reliability * (reliability - p.reliability_target.value());
    if let (Some(prev), Some(cur)) = (i.previous_cost_per_mwh, cost_per_mwh)
        && prev > 0.0
    {
        raw += p.return_per_cost_improvement * (prev - cur) / prev;
    }
    let allowed_return = clamp_return(raw, p);

    // Only the tallied part of the year earns return: the other terms already
    // cover just those months. `months / 12` is exactly 1 for a full year,
    // and a tally can never cover more than one.
    let year_fraction = f64::from(i.tally.months.min(12)) / 12.0;
    let return_cents =
        (allowed_return.value() * i.eligible_rate_base.cents() as f64 * year_fraction)
            .round_ties_even() as i64;
    let revenue_requirement = Money::from_cents(
        i.tally
            .operating_cost
            .cents()
            .saturating_add(i.depreciation.cents())
            .saturating_add(return_cents),
    );

    let tariff = if served > 0.0 {
        Price::per_mwh(revenue_requirement.cents() as f64 / 100.0 / served)
    } else {
        i.previous_tariff
    };

    RateReviewOutcome {
        tariff,
        allowed_return,
        revenue_requirement,
        cost_per_mwh,
    }
}

/// Clamps into `[min, max]`; NaN falls back to the base return.
fn clamp_return(raw: f64, p: &RegulatorParams) -> Ratio {
    let raw = if raw.is_nan() {
        p.base_return.value()
    } else {
        raw
    };
    let clamped = raw.max(p.min_return.value()).min(p.max_return.value());
    Ratio::new(clamped).unwrap_or(p.base_return)
}

// Portable state schema 1: explicit field order and enum tags.
crate::state_record!(RateReviewOutcome {
    tariff,
    allowed_return,
    revenue_requirement,
    cost_per_mwh
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::fixtures::tiny_region;
    use crate::units::MegawattHours;
    use proptest::prelude::*;

    fn tally(demand: f64, served: f64, cost_cents: i64) -> YearTally {
        YearTally {
            demand: MegawattHours::new(demand),
            served: MegawattHours::new(served),
            operating_cost: Money::from_cents(cost_cents),
            peak_demand: Megawatts::new(100.0),
            firm_added: Megawatts::ZERO,
            months: 12,
        }
    }

    fn inputs(served: f64, cost_cents: i64, rate_base_cents: i64) -> RateReviewInputs {
        RateReviewInputs {
            tally: tally(served, served, cost_cents),
            depreciation: Money::ZERO,
            eligible_rate_base: Money::from_cents(rate_base_cents),
            firm_capacity: Megawatts::new(200.0),
            projected_peak: Megawatts::new(100.0),
            previous_tariff: Price::per_mwh(95.0),
            previous_cost_per_mwh: None,
        }
    }

    proptest! {
        #[test]
        fn tariff_recovers_revenue_requirement(served in 1.0f64..1e9, cost in 0i64..1e14 as i64, rb in 0i64..1e14 as i64) {
            let o = compute_rate_review(&inputs(served, cost, rb), tiny_region().regulator());
            prop_assert!((o.tariff.value() * served - o.revenue_requirement.cents() as f64 / 100.0).abs() < 0.01);
        }

        #[test]
        fn allowed_return_stays_in_band(served in 0.0f64..1e9, demand in 0.0f64..1e9, prev in proptest::option::of(1.0f64..500.0)) {
            let region = tiny_region();
            let p = region.regulator();
            let mut i = inputs(served, 1_000_000, 1_000_000_000);
            i.tally = tally(demand, served, 1_000_000);
            i.previous_cost_per_mwh = prev;
            let o = compute_rate_review(&i, p);
            prop_assert!(o.allowed_return.value() >= p.min_return.value());
            prop_assert!(o.allowed_return.value() <= p.max_return.value());
        }

        #[test]
        fn reserve_excess_uses_larger_peak(firm in 0.0f64..1e5, actual in 0.0f64..1e5, projected in 0.0f64..1e5) {
            let margin = tiny_region().regulator().reserve_margin;
            let e = reserve_excess(
                Megawatts::new(firm),
                Megawatts::new(actual),
                Megawatts::new(projected),
                margin,
            );
            let expected = (firm - actual.max(projected) * (1.0 + margin.value())).max(0.0);
            prop_assert!(e.value() >= 0.0);
            prop_assert!((e.value() - expected).abs() < 1e-9);
        }
    }

    /// The revenue requirement of `i` over `months` tallied months.
    fn requirement_over(i: &RateReviewInputs, months: u8) -> (RateReviewOutcome, Money) {
        let mut i = i.clone();
        i.tally.months = months;
        let outcome = compute_rate_review(&i, tiny_region().regulator());
        let non_return = i.tally.operating_cost + i.depreciation;
        (outcome.clone(), outcome.revenue_requirement - non_return)
    }

    proptest! {
        #[test]
        fn return_term_is_linear_in_months(months in 1u8..=12, rb in 0i64..1e12 as i64) {
            let mut i = inputs(1000.0, 7_000_000, rb);
            i.depreciation = Money::from_cents(123_456);
            let (outcome, term) = requirement_over(&i, months);
            let (_, full_year) = requirement_over(&i, 12);
            let exact = outcome.allowed_return.value() * rb as f64 * f64::from(months) / 12.0;
            prop_assert!((term.cents() as f64 - exact).abs() <= 1.0);
            prop_assert!((term.cents() as f64 - full_year.cents() as f64 * f64::from(months) / 12.0).abs() <= 1.0);
        }
    }

    #[test]
    fn full_year_return_is_unchanged() {
        let region = tiny_region();
        let mut i = inputs(1000.0, 7_000_000, 123_456_789);
        i.depreciation = Money::from_cents(555_555);
        i.tally.months = 12;
        let o = compute_rate_review(&i, region.regulator());
        // The pre-proration formula: the whole year's return, rounded once.
        let whole_year = (o.allowed_return.value() * i.eligible_rate_base.cents() as f64)
            .round_ties_even() as i64;
        assert_eq!(
            o.revenue_requirement,
            Money::from_cents(7_000_000 + 555_555 + whole_year)
        );
    }

    #[test]
    fn a_partial_year_earns_a_proportionate_return() {
        let region = tiny_region();
        let mut i = inputs(1000.0, 0, 120_000_000);
        i.tally.months = 9;
        let o = compute_rate_review(&i, region.regulator());
        let expected = (o.allowed_return.value() * 120_000_000.0 * 0.75).round_ties_even() as i64;
        assert_eq!(o.revenue_requirement, Money::from_cents(expected));
    }

    #[test]
    fn more_than_twelve_months_earn_one_year_of_return() {
        let i = inputs(1000.0, 7_000_000, 123_456_789);
        assert_eq!(requirement_over(&i, 13), requirement_over(&i, 12));
    }

    #[test]
    fn zero_months_earns_no_return() {
        let region = tiny_region();
        let mut i = inputs(1000.0, 7_000_000, 123_456_789);
        i.depreciation = Money::from_cents(555_555);
        i.tally.months = 0;
        let o = compute_rate_review(&i, region.regulator());
        assert_eq!(o.revenue_requirement, Money::from_cents(7_555_555));
    }

    #[test]
    fn zero_served_energy_keeps_previous_tariff() {
        let region = tiny_region();
        let mut i = inputs(0.0, 500_000, 1_000_000);
        i.tally = tally(0.0, 0.0, 500_000);
        i.previous_cost_per_mwh = Some(40.0);
        let o = compute_rate_review(&i, region.regulator());
        assert_eq!(o.tariff, i.previous_tariff);
        assert_eq!(o.cost_per_mwh, None);
        // no demand: reliability sits at target, so the return is the base return
        assert_eq!(o.allowed_return, region.regulator().base_return);
    }

    #[test]
    fn held_capital_is_excluded_from_allowed_return() {
        let region = tiny_region();
        let p = region.regulator();
        let total = inputs(1000.0, 0, 100_000_000);
        let eligible = inputs(1000.0, 0, 60_000_000);
        let a = compute_rate_review(&total, p);
        let b = compute_rate_review(&eligible, p);
        // reliability 1.0 vs target 0.999 pushes the return to the cap; no prior cost, so no cost term
        let r = b.allowed_return.value();
        assert_eq!(
            b.revenue_requirement,
            Money::from_cents((r * 60_000_000.0).round_ties_even() as i64)
        );
        assert!(b.revenue_requirement < a.revenue_requirement);
    }

    #[test]
    fn return_follows_reliability_and_cost_improvement() {
        let region = tiny_region();
        let p = region.regulator();
        // reliability 0.9995: 0.09 + 2.0 × 0.0005 = 0.091
        let mut i = inputs(1000.0, 10_000_000, 0);
        i.tally = tally(1000.0 / 0.9995, 1000.0, 10_000_000);
        let o = compute_rate_review(&i, p);
        assert!((o.allowed_return.value() - 0.091).abs() < 1e-9);
        // cost 100 → 90 per MWh is a 10% improvement: + 0.05 × 0.1 = 0.005
        i.tally = tally(1000.0, 1000.0, 9_000_000);
        i.previous_cost_per_mwh = Some(100.0);
        let o = compute_rate_review(&i, p);
        assert_eq!(o.cost_per_mwh, Some(90.0));
        assert!((o.allowed_return.value() - (0.09 + 2.0 * 0.001 + 0.005)).abs() < 1e-9);
    }

    #[test]
    fn poor_reliability_clamps_to_minimum_return() {
        let region = tiny_region();
        let mut i = inputs(900.0, 0, 1_000_000);
        i.tally = tally(1000.0, 900.0, 0);
        let o = compute_rate_review(&i, region.regulator());
        assert_eq!(o.allowed_return, region.regulator().min_return);
    }

    #[test]
    fn rate_review_survives_extreme_inputs() {
        let region = tiny_region();
        let mut i = inputs(1e9, i64::MAX, i64::MAX);
        i.depreciation = Money::from_cents(i64::MAX);
        i.previous_cost_per_mwh = Some(f64::MIN_POSITIVE);
        let o = compute_rate_review(&i, region.regulator());
        assert!(o.allowed_return.value() <= region.regulator().max_return.value());
        let mut j = inputs(-5.0, 0, 0);
        j.tally = tally(-1.0, -5.0, 0);
        assert_eq!(
            compute_rate_review(&j, region.regulator()).tariff,
            j.previous_tariff
        );
    }

    #[test]
    fn no_demand_means_target_reliability_even_if_energy_was_served() {
        let region = tiny_region();
        let mut i = inputs(100.0, 1_000_000, 1_000_000);
        i.tally = tally(0.0, 100.0, 1_000_000);
        let o = compute_rate_review(&i, region.regulator());
        assert_eq!(o.allowed_return, region.regulator().base_return);
    }

    #[test]
    fn a_zero_previous_cost_contributes_no_improvement_term() {
        let region = tiny_region();
        let mut i = inputs(1000.0, 10_000_000, 0);
        i.tally = tally(1000.0, 1000.0, 10_000_000);
        let without = compute_rate_review(&i, region.regulator());
        i.previous_cost_per_mwh = Some(0.0);
        let with_zero = compute_rate_review(&i, region.regulator());
        assert_eq!(with_zero.allowed_return, without.allowed_return);
        assert_eq!(with_zero.allowed_return.value(), 0.09 + 2.0 * 0.001);
    }
}
