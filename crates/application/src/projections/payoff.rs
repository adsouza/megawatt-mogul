//! Hypothetical full-utilization cash flow, independent of dispatch and saved state.

use domain::{
    region::{GeothermalParams, PlantKind, PlantSpec},
    units::{MegawattHours, Money, Price},
};

/// Constant-price comparison using average months in a 365-day year. Balances are plotted in
/// cents as floats; economic inputs and monthly cash remain typed, rounded money values.
#[derive(Debug, Clone, PartialEq)]
pub struct PlantPayoff {
    pub construction: Vec<Money>,
    pub monthly_revenue: Money,
    pub monthly_fuel: Money,
    pub monthly_upkeep: Money,
    pub monthly_net: Money,
    /// Fractional months since ordering, including construction. None when net income <= 0.
    pub payback_months: Option<f64>,
}

impl PlantPayoff {
    pub fn balance_at(&self, month: f64) -> f64 {
        let build = self.construction.len() as f64;
        let paid = self
            .construction
            .iter()
            .enumerate()
            .map(|(i, payment)| payment.cents() as f64 * (month - i as f64).clamp(0.0, 1.0))
            .sum::<f64>();
        -paid + (month - build).max(0.0) * self.monthly_net.cents() as f64
    }
}

/// Full-utilization generator comparison; storage has no generation payoff.
pub fn plant_payoff(
    spec: &PlantSpec,
    tariff: Price,
    geothermal: Option<&GeothermalParams>,
) -> Option<PlantPayoff> {
    if spec.kind == PlantKind::Battery {
        return None;
    }
    let construction = if let Some(params) =
        geothermal.filter(|_| spec.kind == PlantKind::Geothermal)
    {
        let early = params.exploration_capital(spec.capital_cost);
        let mut payments = early.split_even(params.exploration_months);
        payments.extend(
            (spec.capital_cost - early).split_even(spec.build_months - params.exploration_months),
        );
        payments
    } else {
        spec.capital_cost.split_even(spec.build_months)
    };
    let monthly_output = MegawattHours::new(spec.capacity.value() * 24.0 * 365.0 / 12.0);
    let monthly_revenue = tariff.cost_of(monthly_output, 1);
    let monthly_fuel = spec.marginal_cost.cost_of(monthly_output, 1);
    let monthly_net = monthly_revenue - monthly_fuel - spec.fixed_om_per_month;
    let payback_months = (monthly_net.cents() > 0).then(|| {
        f64::from(spec.build_months) + spec.capital_cost.cents() as f64 / monthly_net.cents() as f64
    });
    Some(PlantPayoff {
        construction,
        monthly_revenue,
        monthly_fuel,
        monthly_upkeep: spec.fixed_om_per_month,
        monthly_net,
        payback_months,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{
        region::{PlantKind, fixtures::tiny_region},
        units::{Money, Price},
    };

    #[test]
    fn payoff_includes_build_time_fuel_and_monthly_upkeep() {
        let region = tiny_region();
        let mut spec = region.spec(PlantKind::Ccgt).clone();
        spec.capital_cost = Money::from_cents(100_000_003);
        spec.build_months = 3;
        let tariff = Price::per_mwh(spec.marginal_cost.value() + 20.0);
        let payoff = plant_payoff(&spec, tariff, None).unwrap();
        let energy = spec.capacity.value() * 24.0 * 365.0 / 12.0;
        let expected = (energy * tariff.value() * 100.0).round_ties_even() as i64
            - (energy * spec.marginal_cost.value() * 100.0).round_ties_even() as i64
            - spec.fixed_om_per_month.cents();
        assert_eq!(payoff.monthly_net.cents(), expected);
        assert_eq!(payoff.construction, spec.capital_cost.split_even(3));
        assert_eq!(payoff.balance_at(0.0), 0.0);
        assert_eq!(payoff.balance_at(3.0), -100_000_003.0);
        assert_eq!(payoff.balance_at(4.0), -100_000_003.0 + expected as f64);
        let recovery = payoff.payback_months.unwrap();
        assert!((recovery - (3.0 + 100_000_003.0 / expected as f64)).abs() < 1e-9);
        assert!(payoff.balance_at(recovery).abs() < 1e-5);
    }

    #[test]
    fn unprofitable_generation_never_repays_capital() {
        let region = tiny_region();
        for kind in [PlantKind::Ccgt, PlantKind::Peaker] {
            let spec = region.spec(kind);
            let payoff = plant_payoff(spec, spec.marginal_cost, None).unwrap();
            assert!(payoff.monthly_net.is_negative());
            assert_eq!(payoff.payback_months, None);
            assert!(
                payoff.balance_at(f64::from(spec.build_months) + 12.0)
                    < -spec.capital_cost.cents() as f64
            );
        }
        let battery = plant_payoff(region.spec(PlantKind::Battery), Price::per_mwh(100.0), None);
        assert_eq!(battery, None);
    }

    #[test]
    fn geothermal_spends_exploration_before_the_rest_of_construction() {
        let region = tiny_region();
        let spec = region.spec(PlantKind::Geothermal);
        let params = region.geothermal().unwrap();
        let payoff = plant_payoff(spec, Price::per_mwh(100.0), Some(params)).unwrap();
        let n = params.exploration_months as usize;
        let early = payoff.construction[..n]
            .iter()
            .map(|m| m.cents())
            .sum::<i64>();
        assert_eq!(early, params.exploration_capital(spec.capital_cost).cents());
        assert_ne!(payoff.construction[n - 1], payoff.construction[n]);
        assert_eq!(
            payoff.construction.iter().map(|m| m.cents()).sum::<i64>(),
            spec.capital_cost.cents()
        );
    }

    #[test]
    fn zero_operating_margin_has_no_break_even_and_stays_at_capital_spent() {
        let region = tiny_region();
        let mut spec = region.spec(PlantKind::Solar).clone();
        spec.fixed_om_per_month = Money::ZERO;
        let payoff = plant_payoff(&spec, spec.marginal_cost, None).unwrap();
        assert_eq!(payoff.monthly_net, Money::ZERO);
        assert_eq!(payoff.payback_months, None);
        assert_eq!(payoff.balance_at(1200.0), -spec.capital_cost.cents() as f64);
    }

    proptest::proptest! {
        #[test]
        fn construction_spends_exact_capital_before_any_income(
            cents in 0i64..1_000_000_000_000, months in 1u32..120,
        ) {
            let region = tiny_region();
            let mut spec = region.spec(PlantKind::Solar).clone();
            spec.capital_cost = Money::from_cents(cents);
            spec.build_months = months;
            let payoff = plant_payoff(&spec, Price::per_mwh(100.0), None).unwrap();
            proptest::prop_assert_eq!(payoff.construction.iter().map(|m| m.cents()).sum::<i64>(), cents);
            proptest::prop_assert_eq!(payoff.balance_at(f64::from(months)), -cents as f64);
            for pair in (0..=months).map(|m| payoff.balance_at(f64::from(m))).collect::<Vec<_>>().windows(2) {
                proptest::prop_assert!(pair[1] <= pair[0]);
            }
        }
    }
}
