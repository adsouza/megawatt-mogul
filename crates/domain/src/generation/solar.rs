//! Solar availability: the sun's daily arc and day-to-day cloud cover.

use crate::region::SolarParams;
use crate::rng::SimRng;
use crate::time::{SimMonth, TICKS_PER_HOUR};
use crate::units::{Megawatts, Ratio};

const MIN_CLEARNESS: f64 = 0.2;
const MAX_CLEARNESS: f64 = 1.0;

/// How high the sun is, `0.0..=1.0`: a half sine between sunrise and sunset,
/// zero at night. Time of day is the start of the tick's minute, `tick / 60`
/// hours.
pub fn sun_factor(p: &SolarParams, month: SimMonth, tick: u32) -> f64 {
    let (rise, set) = p.daylight[month.index0()];
    let t = f64::from(tick) / f64::from(TICKS_PER_HOUR);
    if t <= rise || t >= set {
        return 0.0;
    }
    (std::f64::consts::PI * (t - rise) / (set - rise)).sin()
}

/// One day's clearness (cloud cover), clamped to `0.2..=1.0`. Draws exactly one
/// normal.
pub fn draw_clearness(p: &SolarParams, month: SimMonth, rng: &mut SimRng) -> f64 {
    rng.normal(p.clearness_mean[month.index0()], p.clearness_sd)
        .clamp(MIN_CLEARNESS, MAX_CLEARNESS)
}

/// Output a solar plant can deliver: capacity times the site's irradiance,
/// sun height and clearness. Never negative.
pub fn solar_available(
    capacity: Megawatts,
    irradiance: Ratio,
    sun: f64,
    clearness: f64,
) -> Megawatts {
    (capacity * (irradiance.value() * sun * clearness)).max(Megawatts::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::fixtures::tiny_region;
    use proptest::prelude::*;

    #[test]
    fn sun_is_zero_at_night_and_one_at_solar_noon() {
        let r = tiny_region();
        let apr = SimMonth::new(2027, 4).unwrap();
        // April daylight is 6.3 h to 19.1 h: one hour before sunrise is tick 318,
        // solar noon (12.7 h) is tick 762.
        assert_eq!(sun_factor(r.solar(), apr, 318), 0.0);
        assert!((sun_factor(r.solar(), apr, 762) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn sun_is_zero_after_sunset_and_positive_in_daylight() {
        let r = tiny_region();
        let apr = SimMonth::new(2027, 4).unwrap();
        assert_eq!(sun_factor(r.solar(), apr, 20 * 60), 0.0);
        assert_eq!(sun_factor(r.solar(), apr, 0), 0.0);
        assert!(sun_factor(r.solar(), apr, 9 * 60) > 0.0);
    }

    proptest! {
        #[test]
        fn clearness_is_clamped(seed in any::<u64>()) {
            let c = draw_clearness(
                tiny_region().solar(),
                SimMonth::new(2027, 6).unwrap(),
                &mut SimRng::from_seed(seed),
            );
            prop_assert!((0.2..=1.0).contains(&c));
        }
    }

    #[test]
    fn clearness_draws_exactly_one_normal() {
        let r = tiny_region();
        let mut drawn = SimRng::from_seed(3);
        let mut reference = SimRng::from_seed(3);
        draw_clearness(r.solar(), SimMonth::new(2027, 6).unwrap(), &mut drawn);
        reference.standard_normal();
        assert_eq!(drawn, reference);
    }

    #[test]
    fn available_is_the_product_of_its_factors() {
        let out = solar_available(Megawatts::new(300.0), Ratio::new(0.8).unwrap(), 0.5, 0.75);
        assert!((out.value() - 300.0 * 0.8 * 0.5 * 0.75).abs() < 1e-9);
    }

    #[test]
    fn available_is_zero_at_night() {
        let out = solar_available(Megawatts::new(300.0), Ratio::ONE, 0.0, 1.0);
        assert_eq!(out, Megawatts::ZERO);
    }

    #[test]
    fn available_is_clamped_non_negative() {
        let out = solar_available(Megawatts::new(300.0), Ratio::ONE, -0.1, 1.0);
        assert_eq!(out, Megawatts::ZERO);
    }
}
