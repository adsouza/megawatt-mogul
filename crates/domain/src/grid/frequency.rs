//! The frequency model: deviation dynamics and the band a deviation falls in.

use crate::region::FrequencyParams;
use crate::units::{Hertz, Megawatts};

/// Smallest system size used as the divisor, so an empty system cannot divide by zero.
const MIN_SYSTEM_SIZE_MW: f64 = 1.0;

/// One-tick update of the frequency deviation from nominal.
///
/// A positive imbalance (generation above served load) raises frequency, a
/// negative one lowers it, and damping pulls the deviation back toward zero.
pub fn next_deviation(
    dev: Hertz,
    imbalance: Megawatts,
    system_size: Megawatts,
    p: &FrequencyParams,
) -> Hertz {
    let size = system_size.max(Megawatts::new(MIN_SYSTEM_SIZE_MW)).value();
    let d = dev.value();
    Hertz::new(d + p.k_inertia * imbalance.value() / size - p.k_damping * d)
}

/// Where a deviation sits relative to the protection thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrequencyBand {
    Normal,
    Low,
    Shedding,
    High,
}

pub fn band(dev: Hertz, p: &FrequencyParams) -> FrequencyBand {
    if dev > p.curtail_above {
        FrequencyBand::High
    } else if dev < p.shed_below {
        FrequencyBand::Shedding
    } else if dev < -p.normal_band {
        FrequencyBand::Low
    } else {
        FrequencyBand::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::fixtures::tiny_region;

    #[test]
    fn deviation_converges_to_inertia_over_damping() {
        let p = tiny_region().frequency().clone();
        let mut d = Hertz::ZERO;
        for _ in 0..500 {
            d = next_deviation(d, Megawatts::new(-20.0), Megawatts::new(1000.0), &p);
        }
        assert!((d.value() - (5.0 * -0.02 / 0.2)).abs() < 1e-6);
    }

    #[test]
    fn tiny_system_size_is_floored_at_one_megawatt() {
        let p = tiny_region().frequency().clone();
        let d = next_deviation(Hertz::ZERO, Megawatts::new(0.5), Megawatts::ZERO, &p);
        assert!((d.value() - 5.0 * 0.5).abs() < 1e-12);
    }

    #[test]
    fn bands_follow_thresholds_with_high_taking_precedence() {
        let p = tiny_region().frequency().clone();
        let b = |v: f64| band(Hertz::new(v), &p);
        assert_eq!(b(0.0), FrequencyBand::Normal);
        assert_eq!(b(-0.05), FrequencyBand::Normal);
        assert_eq!(b(-0.2), FrequencyBand::Low);
        assert_eq!(b(-0.5), FrequencyBand::Low);
        assert_eq!(b(-0.6), FrequencyBand::Shedding);
        assert_eq!(b(0.5), FrequencyBand::Normal);
        assert_eq!(b(0.6), FrequencyBand::High);
    }
}
