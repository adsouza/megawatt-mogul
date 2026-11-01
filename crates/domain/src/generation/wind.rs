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

/// One minute of the actual wind factor around the day-ahead forecast path (spec 2026-10-31
/// §3.1). The surprise is carried as the post-clamp residual `previous − mean − forecast_previous`,
/// so the state is clamped every step exactly as [`wind_step`] clamps it; with a zero forecast
/// this is [`wind_step`] bit for bit.
pub fn wind_step_with_forecast(
    previous: f64,
    mean: f64,
    forecast_previous: f64,
    forecast_now: f64,
    persistence: f64,
    surprise_sd: f64,
    innovation: f64,
) -> f64 {
    (mean
        + forecast_now
        + persistence * (previous - mean - forecast_previous)
        + surprise_sd * innovation)
        .clamp(0.0, 1.0)
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
        fn with_a_zero_forecast_the_step_equals_todays_wind_step(
            previous in 0.0..=1.0f64,
            mean in 0.0..=1.0f64,
            innovation in -5.0..5.0f64,
        ) {
            proptest::prop_assert_eq!(
                wind_step_with_forecast(previous, mean, 0.0, 0.0, 0.995, 0.015, innovation)
                    .to_bits(),
                wind_step(previous, mean, 0.995, 0.015, innovation).to_bits()
            );
        }

        #[test]
        fn the_surprise_is_carried_as_the_post_clamp_residual(
            previous in 0.0..=1.0f64,
            forecast_previous in -0.3..0.3f64,
            forecast_now in -0.3..0.3f64,
            innovation in -5.0..5.0f64,
        ) {
            let next = wind_step_with_forecast(
                previous, 0.6, forecast_previous, forecast_now, 0.995, 0.0075, innovation,
            );
            let residual = previous - 0.6 - forecast_previous;
            let expected =
                (0.6 + forecast_now + 0.995 * residual + 0.0075 * innovation).clamp(0.0, 1.0);
            proptest::prop_assert!((next - expected).abs() < 1e-12);
        }

        #[test]
        fn wind_transition_is_bounded(previous in 0.0..=1.0f64, innovation in -100.0..100.0f64) {
            let next = wind_step(previous, 0.6, 0.995, 0.015, innovation);
            proptest::prop_assert!((0.0..=1.0).contains(&next));
            proptest::prop_assert!((next - (0.6 + 0.995 * (previous - 0.6) + 0.015 * innovation).clamp(0.0, 1.0)).abs() < 1e-12);
        }
    }
}
