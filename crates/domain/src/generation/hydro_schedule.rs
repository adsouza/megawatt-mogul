//! Water-filling allocation of a hydro plant's daily budget across hours.
//!
//! Spec §12.3: pour the budget onto the forecast residual load like water into
//! a basin, so it lands on the highest hours first. Pure and deterministic: no
//! RNG, no state, and a fixed number of bisection steps.

use crate::units::{MegawattHours, Megawatts};

/// Bisection steps used to find the water level. Fixed, so the result is
/// deterministic. 100 halvings exhaust f64 precision for any realistic range.
const BISECTION_STEPS: u32 = 100;

/// One plant's allocation over the 24 hours of a day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HydroFill {
    /// The water level `λ`. `None` when nothing is allocated (no budget, no
    /// eligible hour, unusable capacity) or when the water spills because the
    /// plant cannot usefully run harder, so there is no level.
    pub level: Option<Megawatts>,
    /// MW allocated to each hour. Ineligible hours are 0.
    pub hourly: [Megawatts; 24],
}

impl HydroFill {
    fn empty() -> Self {
        Self {
            level: None,
            hourly: [Megawatts::ZERO; 24],
        }
    }
}

/// Spreads `budget` over the `eligible` hours of `residual`, at most `capacity`
/// MW in any hour.
///
/// Hour `h` gets `clamp(residual_h - λ, 0, capacity)`, with `λ` chosen so the
/// allocations sum to `budget` (each hour lasts 1 h). The sum may fall short of
/// `budget` by bisection error (never above it), and the result is not rescaled.
/// If the budget exceeds what the plant can usefully run
/// (`Σ min(capacity, max(0, residual_h))`), each eligible hour gets exactly
/// that and the rest spills, with `level: None`.
///
/// Total over its inputs: an eligible hour whose residual is not finite is
/// treated as ineligible, and a non-finite or non-positive budget or capacity
/// allocates nothing.
pub fn water_fill(
    residual: &[Megawatts; 24],
    eligible: &[bool; 24],
    capacity: Megawatts,
    budget: MegawattHours,
) -> HydroFill {
    let cap = capacity.value();
    let budget = budget.value();
    if !(cap.is_finite() && cap > 0.0 && budget.is_finite() && budget > 0.0) {
        return HydroFill::empty();
    }

    let usable = |h: usize| eligible[h] && residual[h].value().is_finite();
    let res = |h: usize| residual[h].value();
    let hours = || (0..24).filter(|&h| usable(h));

    let Some(min_res) = hours().map(res).reduce(f64::min) else {
        return HydroFill::empty();
    };
    let max_res = hours().map(res).fold(f64::NEG_INFINITY, f64::max);

    let mut fill = HydroFill::empty();
    let spillable: f64 = hours().map(|h| cap.min(res(h).max(0.0))).sum();
    if budget >= spillable {
        for h in hours() {
            fill.hourly[h] = Megawatts::new(cap.min(res(h).max(0.0)));
        }
        return fill;
    }

    let poured = |level: f64| -> f64 {
        hours()
            .map(|h| (res(h) - level).clamp(0.0, cap))
            .sum::<f64>()
    };
    // At `lo` every eligible hour is full (poured >= budget); at `hi` none is.
    let (mut lo, mut hi) = (min_res - cap, max_res);
    for _ in 0..BISECTION_STEPS {
        let mid = lo + (hi - lo) / 2.0;
        if poured(mid) > budget {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    // `hi` always pours at most the budget.
    for h in hours() {
        fill.hourly[h] = Megawatts::new((res(h) - hi).clamp(0.0, cap));
    }
    fill.level = Some(Megawatts::new(hi));
    fill
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::array::uniform24;
    use proptest::prelude::*;

    const TOL: f64 = 1e-6;

    fn mw(values: &[f64; 24]) -> [Megawatts; 24] {
        values.map(Megawatts::new)
    }

    fn run(res: &[f64; 24], mask: &[bool; 24], cap: f64, budget: f64) -> HydroFill {
        water_fill(
            &mw(res),
            mask,
            Megawatts::new(cap),
            MegawattHours::new(budget),
        )
    }

    fn total(fill: &HydroFill) -> f64 {
        fill.hourly.iter().map(|m| m.value()).sum()
    }

    fn usable(res: &[f64; 24], mask: &[bool; 24], cap: f64) -> f64 {
        (0..24)
            .filter(|&h| mask[h])
            .map(|h| cap.min(res[h].max(0.0)))
            .sum()
    }

    #[test]
    fn fills_the_peak_first() {
        let mut res = [1000.0; 24];
        for r in &mut res[17..=20] {
            *r = 1500.0;
        }
        let fill = run(&res, &[true; 24], 800.0, 1200.0);
        let level = fill.level.expect("water level").value();
        assert!((level - 1200.0).abs() < 1e-6, "level {level}");
        for h in 0..24 {
            let want = if (17..=20).contains(&h) { 300.0 } else { 0.0 };
            assert!(
                (fill.hourly[h].value() - want).abs() < 1e-6,
                "hour {h}: {}",
                fill.hourly[h].value()
            );
        }
    }

    #[test]
    fn caps_at_capacity_and_spills_excess_water() {
        let fill = run(&[500.0; 24], &[true; 24], 100.0, 10_000.0);
        assert_eq!(fill.level, None);
        assert!(fill.hourly.iter().all(|m| m.value() == 100.0));
    }

    #[test]
    fn spill_allocates_nothing_to_negative_residual_hours() {
        let mut res = [500.0; 24];
        res[3] = -50.0;
        let fill = run(&res, &[true; 24], 100.0, 10_000.0);
        assert_eq!(fill.level, None);
        assert_eq!(fill.hourly[3].value(), 0.0);
        assert_eq!(fill.hourly[4].value(), 100.0);
    }

    #[test]
    fn ineligible_hours_get_nothing() {
        let mut mask = [false; 24];
        for m in &mut mask[..12] {
            *m = true;
        }
        let fill = run(&[800.0; 24], &mask, 300.0, 1200.0);
        assert!(fill.level.is_some());
        for h in 12..24 {
            assert_eq!(fill.hourly[h].value(), 0.0);
        }
        assert!((total(&fill) - 1200.0).abs() < TOL * 1200.0);
        // Equal residuals share the water equally.
        assert!((fill.hourly[0].value() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn no_budget_or_no_eligible_hours_gives_no_level() {
        let res = [900.0; 24];
        for fill in [
            run(&res, &[true; 24], 500.0, 0.0),
            run(&res, &[true; 24], 500.0, -5.0),
            run(&res, &[false; 24], 500.0, 1000.0),
            run(&res, &[true; 24], 0.0, 1000.0),
        ] {
            assert_eq!(fill.level, None);
            assert!(fill.hourly.iter().all(|m| m.value() == 0.0));
        }
    }

    #[test]
    fn non_finite_inputs_yield_zeros_not_panics() {
        let mut res = mw(&[900.0; 24]);
        res[5] = Megawatts::unchecked(f64::NAN);
        res[6] = Megawatts::unchecked(f64::INFINITY);
        let fill = water_fill(
            &res,
            &[true; 24],
            Megawatts::new(500.0),
            MegawattHours::new(1000.0),
        );
        assert_eq!(fill.hourly[5].value(), 0.0);
        assert_eq!(fill.hourly[6].value(), 0.0);
        assert!(fill.hourly.iter().all(|m| m.value().is_finite()));
        assert!((total(&fill) - 1000.0).abs() < TOL * 1000.0);

        for (cap, budget) in [
            (Megawatts::unchecked(f64::NAN), 1000.0),
            (Megawatts::unchecked(f64::INFINITY), 1000.0),
            (Megawatts::new(500.0), f64::NAN),
            (Megawatts::new(500.0), f64::INFINITY),
        ] {
            let fill = water_fill(
                &mw(&[900.0; 24]),
                &[true; 24],
                cap,
                MegawattHours::new(budget),
            );
            assert_eq!(fill.level, None);
            assert!(fill.hourly.iter().all(|m| m.value() == 0.0));
        }
    }

    proptest! {
        #[test]
        fn conserves_water(
            res in uniform24(0.0f64..5000.0),
            cap in 1.0f64..2000.0,
            budget in 0.0f64..40_000.0,
            mask in any::<[bool; 24]>(),
        ) {
            let fill = run(&res, &mask, cap, budget);
            let sum = total(&fill);
            prop_assert!(sum <= budget + TOL * budget, "sum {sum} > budget {budget}");
            let want = budget.min(usable(&res, &mask, cap));
            prop_assert!((sum - want).abs() <= TOL * budget.max(1.0), "sum {sum} want {want}");
        }

        #[test]
        fn bounded(
            res in uniform24(0.0f64..5000.0),
            cap in 1.0f64..2000.0,
            budget in 0.0f64..40_000.0,
            mask in any::<[bool; 24]>(),
        ) {
            let fill = run(&res, &mask, cap, budget);
            for (h, (m, eligible)) in fill.hourly.iter().zip(mask).enumerate() {
                let a = m.value();
                prop_assert!((0.0..=cap).contains(&a), "hour {h}: {a}");
                if !eligible {
                    prop_assert_eq!(a, 0.0);
                }
            }
        }

        #[test]
        fn water_filling_optimal(
            res in uniform24(0.0f64..5000.0),
            cap in 1.0f64..2000.0,
            budget in 0.0f64..40_000.0,
            mask in any::<[bool; 24]>(),
        ) {
            let fill = run(&res, &mask, cap, budget);
            let eps = 1e-9;
            for i in (0..24).filter(|&i| mask[i]) {
                for j in (0..24).filter(|&j| mask[j]) {
                    let (ai, aj) = (fill.hourly[i].value(), fill.hourly[j].value());
                    if ai < cap - eps && aj > eps {
                        prop_assert!(res[i] - ai <= res[j] - aj + TOL, "i={i} j={j}");
                    }
                }
            }
        }

        #[test]
        fn monotone(
            res in uniform24(0.0f64..5000.0),
            cap in 1.0f64..2000.0,
            budget in 0.0f64..40_000.0,
            mask in any::<[bool; 24]>(),
        ) {
            let fill = run(&res, &mask, cap, budget);
            for i in (0..24).filter(|&i| mask[i]) {
                for j in (0..24).filter(|&j| mask[j]) {
                    if res[i] >= res[j] {
                        prop_assert!(
                            fill.hourly[i].value() >= fill.hourly[j].value() - 1e-9,
                            "i={i} j={j}"
                        );
                    }
                }
            }
        }

        #[test]
        fn deterministic(
            res in uniform24(0.0f64..5000.0),
            cap in 1.0f64..2000.0,
            budget in 0.0f64..40_000.0,
            mask in any::<[bool; 24]>(),
        ) {
            prop_assert_eq!(run(&res, &mask, cap, budget), run(&res, &mask, cap, budget));
        }
    }
}
