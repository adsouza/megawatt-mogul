//! Customer satisfaction: how happy customers are with reliability and price.

use crate::region::SatisfactionParams;
use crate::units::{Price, Ratio};

/// Where satisfaction is heading, on a `0.0..=100.0` scale:
/// `100 × rel × price`, where
/// - `rel = clamp((r − floor) / (target − floor), 0, 1)`, and
/// - `price = clamp(1 − sensitivity × max(0, tariff / reference − 1), 0, 1)`.
///
/// Degenerate data is handled without dividing by zero: a target at or below
/// the floor makes `rel` a step at the target, and a non-positive reference
/// tariff applies no price penalty.
pub fn satisfaction_target(
    reliability: Ratio,
    tariff: Price,
    reference: Price,
    reliability_target: Ratio,
    p: &SatisfactionParams,
) -> f64 {
    let (r, floor, target) = (
        reliability.value(),
        p.reliability_floor.value(),
        reliability_target.value(),
    );
    let rel = if target > floor {
        ((r - floor) / (target - floor)).clamp(0.0, 1.0)
    } else if r >= target {
        1.0
    } else {
        0.0
    };
    let markup = if reference.value() > 0.0 {
        (tariff.value() / reference.value() - 1.0).max(0.0)
    } else {
        0.0
    };
    let price = (1.0 - p.price_sensitivity * markup).clamp(0.0, 1.0);
    100.0 * rel * price
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> SatisfactionParams {
        SatisfactionParams {
            starting: 70.0,
            reliability_floor: Ratio::new(0.95).unwrap(),
            price_sensitivity: 2.0,
            smoothing: Ratio::new(0.2).unwrap(),
        }
    }

    fn r(v: f64) -> Ratio {
        Ratio::new(v).unwrap()
    }

    #[test]
    fn satisfaction_target_is_100_at_target_and_reference() {
        let t = satisfaction_target(
            r(0.999),
            Price::per_mwh(100.0),
            Price::per_mwh(100.0),
            r(0.999),
            &params(),
        );
        assert_eq!(t, 100.0);
    }

    #[test]
    fn target_falls_with_reliability_and_markup() {
        let p = params();
        let reference = Price::per_mwh(100.0);
        let at = |rel: f64, tariff: f64| {
            satisfaction_target(r(rel), Price::per_mwh(tariff), reference, r(0.999), &p)
        };
        assert_eq!(at(0.90, 100.0), 0.0, "below the floor");
        assert_eq!(
            at(1.0, 80.0),
            100.0,
            "cheap tariffs are not rewarded above 100"
        );
        assert!((at(0.999, 110.0) - 80.0).abs() < 1e-9);
        assert_eq!(at(0.999, 200.0), 0.0, "price factor clamps at zero");
        assert!((at(0.9745, 100.0) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn degenerate_parameters_do_not_divide_by_zero() {
        let p = params();
        let price = Price::per_mwh(100.0);
        // Target equal to the floor: a step at the target.
        assert_eq!(
            satisfaction_target(r(0.95), price, price, r(0.95), &p),
            100.0
        );
        assert_eq!(satisfaction_target(r(0.94), price, price, r(0.95), &p), 0.0);
        // A non-positive reference tariff applies no price penalty.
        let zero = Price::per_mwh(0.0);
        assert_eq!(
            satisfaction_target(r(0.999), price, zero, r(0.999), &p),
            100.0
        );
    }
}
