//! Bounded valley filling / peak shaving at minute resolution. No RNG or live state.
use crate::units::{MegawattHours, Megawatts};

pub(crate) fn fill_minutes(
    residual: &[Megawatts],
    caps: &[Megawatts],
    energy: MegawattHours,
    charging: bool,
) -> Vec<Megawatts> {
    assert_eq!(residual.len(), caps.len());
    let room: f64 = caps.iter().map(|p| p.value()).sum::<f64>() / 60.0;
    let budget = energy.value().max(0.0).min(room);
    if budget == 0.0 {
        return vec![Megawatts::ZERO; caps.len()];
    }
    let max_cap = caps.iter().map(|p| p.value()).fold(0.0, f64::max);
    let mut lo = residual
        .iter()
        .map(|p| p.value())
        .fold(f64::INFINITY, f64::min)
        - max_cap;
    let mut hi = residual
        .iter()
        .map(|p| p.value())
        .fold(f64::NEG_INFINITY, f64::max)
        + max_cap;
    let allocation = |level: f64, r: Megawatts, cap: Megawatts| {
        (if charging {
            level - r.value()
        } else {
            r.value() - level
        })
        .clamp(0.0, cap.value())
    };
    for _ in 0..64 {
        let level = lo + (hi - lo) / 2.0;
        let used: f64 = residual
            .iter()
            .zip(caps)
            .map(|(&r, &cap)| allocation(level, r, cap))
            .sum::<f64>()
            / 60.0;
        if (used > budget) == charging {
            hi = level;
        } else {
            lo = level;
        }
    }
    let level = if charging { lo } else { hi };
    // A final sequential cap protects conservation against accumulated bisection roundoff.
    let mut remaining = budget * 60.0;
    residual
        .iter()
        .zip(caps)
        .map(|(&r, &cap)| {
            let power = allocation(level, r, cap).min(remaining).max(0.0);
            remaining = (remaining - power).max(0.0);
            Megawatts::new(power)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    #[test]
    fn morning_valley_and_evening_peak_are_preferred() {
        let load = [100.0, 300.0, 200.0].map(Megawatts::new);
        let caps = [Megawatts::new(100.0); 3];
        let charge = fill_minutes(&load, &caps, MegawattHours::new(1.0), true);
        assert!((charge[0].value() - 60.0).abs() < 1e-9);
        assert_eq!(charge[1], Megawatts::ZERO);
        let discharge = fill_minutes(&load, &caps, MegawattHours::new(1.0), false);
        assert!((discharge[1].value() - 60.0).abs() < 1e-9);
    }
    proptest! {
        #[test]
        fn fills_obey_budget_and_bounds(load in prop::collection::vec(-1000.0f64..1000.0, 1..100), budget in 0.0f64..300.0, charging in any::<bool>()) {
            let residual: Vec<_> = load.iter().copied().map(Megawatts::new).collect();
            let caps = vec![Megawatts::new(200.0); load.len()];
            let fill=fill_minutes(&residual,&caps,MegawattHours::new(budget),charging);
            let used=fill.iter().map(|p|p.value()).sum::<f64>()/60.0;
            prop_assert!(used <= budget+1e-8);
            prop_assert!((used-budget.min(load.len() as f64*200.0/60.0)).abs()<1e-7);
            prop_assert!(fill.iter().all(|p| (0.0..=200.0).contains(&p.value())));
        }
    }
}
