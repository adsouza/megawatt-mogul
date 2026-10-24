//! Shared regional wind availability, independent of farm count and daylight.

use crate::units::{Megawatts, Ratio};

pub fn wind_step(
    previous: f64,
    mean: f64,
    persistence: f64,
    innovation_sd: f64,
    innovation: f64,
) -> f64 {
    (mean + persistence * (previous - mean) + innovation_sd * innovation).clamp(0.0, 1.0)
}

pub fn wind_available(capacity: Megawatts, potential: Ratio, factor: f64) -> Megawatts {
    capacity * (potential.value() * factor.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{Megawatts, Ratio};

    #[test]
    fn wind_output_scales_with_site_and_resource_at_any_hour() {
        assert_eq!(
            wind_available(Megawatts::new(300.0), Ratio::new(0.8).unwrap(), 0.5),
            Megawatts::new(120.0)
        );
        assert_eq!(
            wind_available(Megawatts::new(300.0), Ratio::ZERO, 1.0),
            Megawatts::ZERO
        );
    }

    proptest::proptest! {
        #[test]
        fn wind_transition_is_bounded(previous in 0.0..=1.0f64, innovation in -100.0..100.0f64) {
            let next = wind_step(previous, 0.6, 0.995, 0.015, innovation);
            proptest::prop_assert!((0.0..=1.0).contains(&next));
            proptest::prop_assert!((next - (0.6 + 0.995 * (previous - 0.6) + 0.015 * innovation).clamp(0.0, 1.0)).abs() < 1e-12);
        }
    }
}
