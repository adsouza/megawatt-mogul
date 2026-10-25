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
pub fn sun_factor(daylight: &[(f64, f64); 12], month: SimMonth, tick: u32) -> f64 {
    let (rise, set) = daylight[month.index0()];
    let t = f64::from(tick) / f64::from(TICKS_PER_HOUR);
    if t <= rise || t >= set {
        return 0.0;
    }
    (std::f64::consts::PI * (t - rise) / (set - rise)).sin()
}

/// Solar direction, with azimuth clockwise from north in `[0, 360)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SunAngles {
    pub elevation_deg: f64,
    pub azimuth_deg: f64,
}

/// Declination on the representative 15th of a non-leap month.
pub fn declination_deg(month: SimMonth) -> f64 {
    const MONTH_START: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let n = MONTH_START[month.index0()] + 15;
    -23.44 * (std::f64::consts::TAU * f64::from(n + 10) / 365.0).cos()
}

fn solar_components(lat: f64, lon: f64, utc_minutes: f64, month: SimMonth) -> (f64, f64, f64) {
    let (sin_lat, cos_lat) = lat.to_radians().sin_cos();
    let (sin_dec, cos_dec) = declination_deg(month).to_radians().sin_cos();
    let h = (15.0 * (utc_minutes / 60.0 + lon / 15.0 - 12.0)).to_radians();
    let (sin_h, cos_h) = h.sin_cos();
    let up = sin_lat * sin_dec + cos_lat * cos_dec * cos_h;
    let east = -cos_dec * sin_h;
    let north = cos_lat * sin_dec - sin_lat * cos_dec * cos_h;
    (east, north, up)
}

/// Geometric sun direction, without refraction or an equation of time.
/// The east/north vector avoids tangent singularities at the poles and zenith.
pub fn sun_angles(lat: f64, lon: f64, utc_minutes: f64, month: SimMonth) -> SunAngles {
    let (east, north, up) = solar_components(lat, lon, utc_minutes, month);
    SunAngles {
        elevation_deg: up.clamp(-1.0, 1.0).asin().to_degrees(),
        azimuth_deg: east.atan2(north).to_degrees().rem_euclid(360.0),
    }
}

pub(crate) fn astronomical_sun(lat: f64, lon: f64, utc_minutes: f64, month: SimMonth) -> f64 {
    solar_components(lat, lon, utc_minutes, month)
        .2
        .clamp(0.0, 1.0)
}

/// Annual mean daily minute integral, weighted by non-leap month lengths.
/// Only region construction calls this, once per row and once at the equator.
pub(crate) fn annual_sun_integral(lat: f64) -> f64 {
    const DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let (sin_lat, cos_lat) = lat.to_radians().sin_cos();
    DAYS.iter()
        .enumerate()
        .map(|(m, &days)| {
            let month = SimMonth::new(2027, m as u8 + 1).expect("non-leap representative month");
            let (sin_dec, cos_dec) = declination_deg(month).to_radians().sin_cos();
            let daily: f64 = (0..1440)
                .map(|tick| {
                    let h = (15.0 * (f64::from(tick) / 60.0 - 12.0)).to_radians();
                    (sin_lat * sin_dec + cos_lat * cos_dec * h.cos()).clamp(0.0, 1.0)
                })
                .sum();
            daily * f64::from(days)
        })
        .sum::<f64>()
        / 365.0
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

    fn month(m: u8) -> SimMonth {
        SimMonth::new(2027, m).unwrap()
    }
    fn noon(lon: f64) -> f64 {
        720.0 - lon * 4.0
    }
    fn sun_at(lat: f64, lon: f64, m: u8, utc: f64) -> f64 {
        sun_angles(lat, lon, utc, month(m))
            .elevation_deg
            .to_radians()
            .sin()
            .max(0.0)
    }

    #[test]
    fn quito_equinox_noon_is_near_overhead() {
        assert!(sun_angles(-0.18, -78.47, noon(-78.47), month(3)).elevation_deg > 85.0);
    }
    #[test]
    fn murmansk_has_polar_night_and_midnight_sun() {
        assert!((0..1440).all(|m| sun_at(68.97, 33.08, 12, f64::from(m)) == 0.0));
        assert!(sun_at(68.97, 33.08, 6, -33.08 * 4.0) > 0.0);
    }
    #[test]
    fn sao_paulo_december_days_are_longer_than_june() {
        let daylight = |month| {
            (0..1440)
                .filter(|&m| sun_at(-23.55, -46.63, month, f64::from(m)) > 0.0)
                .count()
        };
        assert!(daylight(12) > daylight(6) + 120);
    }
    #[test]
    fn southern_noon_sun_is_in_the_north() {
        let a = sun_angles(-23.55, -46.63, noon(-46.63), month(1));
        assert!(a.azimuth_deg < 20.0 || a.azimuth_deg > 340.0);
        let morning = sun_angles(45.0, 0.0, 540.0, month(3));
        let evening = sun_angles(45.0, 0.0, 900.0, month(3));
        assert!((90.0..180.0).contains(&morning.azimuth_deg));
        assert!((180.0..270.0).contains(&evening.azimuth_deg));
    }
    #[test]
    fn eastern_sites_rise_earlier_on_the_system_clock() {
        let r = crate::region::fixtures::astronomical_region();
        let rise = |cell| (0..1440).find(|&t| r.sun(cell, month(3), t) > 0.0).unwrap();
        assert!(
            rise(crate::ids::CellCoord { x: 10, y: 3 })
                < rise(crate::ids::CellCoord { x: 1, y: 3 })
        );
    }
    #[test]
    fn daylight_table_regions_are_unchanged() {
        let r = tiny_region();
        let crate::region::SolarModel::DaylightTable(table) = r.solar().model else {
            panic!("legacy fixture")
        };
        for m in 1..=12 {
            for tick in 0..1440 {
                let (rise, set) = table[usize::from(m - 1)];
                let hour = f64::from(tick) / 60.0;
                let old = if hour <= rise || hour >= set {
                    0.0
                } else {
                    (std::f64::consts::PI * (hour - rise) / (set - rise)).sin()
                };
                for cell in [
                    crate::ids::CellCoord { x: 0, y: 0 },
                    crate::ids::CellCoord { x: 11, y: 7 },
                ] {
                    assert_eq!(r.sun(cell, month(m), tick).to_bits(), old.to_bits());
                    assert_eq!(
                        r.solar_potential(cell),
                        r.terrain().get(cell).unwrap().irradiance
                    );
                }
            }
            assert_eq!(r.sun_at_home(month(m), 720), None);
        }
    }
    #[test]
    fn computed_home_sun_uses_system_offset_and_dst() {
        let mut p = crate::region::fixtures::tiny_region_parts();
        p.solar.model = crate::region::SolarModel::Astronomical;
        p.clock.zones[0].standard_offset_minutes = 180;
        p.clock.zones[0].dst_months[5] = true;
        let r = crate::region::RegionDefinition::try_from(p).unwrap();
        let home = r.city(r.start().home_city).unwrap().cell;
        let (lat, lon) = r.cell_lat_lon(home);
        for (m, offset) in [(3, 180.0), (6, 240.0)] {
            let angles = sun_angles(lat, lon, 720.0 - offset, month(m));
            assert_eq!(r.sun_at_home(month(m), 720), Some(angles));
            assert!(
                (r.sun(home, month(m), 720) - angles.elevation_deg.to_radians().sin().max(0.0))
                    .abs()
                    < 1e-14
            );
        }
    }
    #[test]
    fn annual_solar_potential_uses_latitude_on_an_absolute_scale() {
        let mut p = crate::region::fixtures::tiny_region_parts();
        p.solar.model = crate::region::SolarModel::Astronomical;
        // The first row centre is 60 N; irradiance is 0.8 in the fixture.
        p.geo.north = 60.5;
        p.geo.south = 52.5;
        let r = crate::region::RegionDefinition::try_from(p).unwrap();
        // Independent continuous-hour-angle integration gives a 60 N/equator
        // annual ratio of 0.57035813; minute sampling differs by under 1e-6.
        let potential = r.solar_potential(crate::ids::CellCoord { x: 1, y: 0 });
        assert!((potential.value() - 0.45628650).abs() < 1e-6);
        assert_eq!(
            potential,
            r.solar_potential(crate::ids::CellCoord { x: 11, y: 0 })
        );
        assert!(r.solar_potential(crate::ids::CellCoord { x: 1, y: 7 }) > potential);
    }

    #[test]
    fn sun_angles_are_finite_at_poles_and_zenith() {
        for m in 1..=12 {
            for lat in [-90.0, 90.0, declination_deg(month(m))] {
                let a = sun_angles(lat, 0.0, 720.0, month(m));
                assert!(a.elevation_deg.is_finite() && a.azimuth_deg.is_finite());
            }
        }
    }
    proptest! {
        #[test]
        fn astronomical_sun_and_angles_are_bounded(lat in -90.0f64..=90.0, lon in -180.0f64..=180.0, m in 1u8..13, tick in any::<u32>()) {
            let a = sun_angles(lat,lon,f64::from(tick),month(m));
            prop_assert!(a.elevation_deg.is_finite());
            prop_assert!((-90.0..=90.0).contains(&a.elevation_deg));
            prop_assert!((0.0..360.0).contains(&a.azimuth_deg));
            let s = astronomical_sun(lat,lon,f64::from(tick),month(m));
            prop_assert!(s.is_finite() && (0.0..=1.0).contains(&s));
            for singular_lat in [-90.0, 90.0, declination_deg(month(m))] {
                let singular = sun_angles(singular_lat, 0.0, 720.0, month(m));
                prop_assert!(singular.elevation_deg.is_finite() && singular.azimuth_deg.is_finite());
            }
        }
        #[test]
        fn solar_potential_is_bounded_and_monotone_in_irradiance(a in 0.0f64..=1.0, b in 0.0f64..=1.0, y in 0u16..8) {
            let mut p = crate::region::fixtures::tiny_region_parts();
            p.solar.model = crate::region::SolarModel::Astronomical;
            let mut cells = (0..8).flat_map(|y| { let terrain = &p.terrain; (0..12).map(move |x| terrain.get(crate::ids::CellCoord{x,y}).unwrap().clone()) }).collect::<Vec<_>>();
            cells[usize::from(y)*12+1].irradiance = Ratio::new(a.min(b)).unwrap();
            cells[usize::from(y)*12+2].irradiance = Ratio::new(a.max(b)).unwrap();
            p.terrain = crate::region::TerrainGrid::new(12,8,cells).unwrap();
            let r = crate::region::RegionDefinition::try_from(p).unwrap();
            let low = r.solar_potential(crate::ids::CellCoord{x:1,y});
            let high = r.solar_potential(crate::ids::CellCoord{x:2,y});
            prop_assert!((0.0..=1.0).contains(&low.value()));
            prop_assert!((0.0..=1.0).contains(&high.value()));
            prop_assert!(low <= high);
        }
    }

    #[test]
    fn sun_is_zero_at_night_and_one_at_solar_noon() {
        let r = tiny_region();
        let apr = SimMonth::new(2027, 4).unwrap();
        // April daylight is 6.3 h to 19.1 h: one hour before sunrise is tick 318,
        // solar noon (12.7 h) is tick 762.
        assert_eq!(r.sun(crate::ids::CellCoord { x: 1, y: 1 }, apr, 318), 0.0);
        assert!((r.sun(crate::ids::CellCoord { x: 1, y: 1 }, apr, 762) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn sun_is_zero_after_sunset_and_positive_in_daylight() {
        let r = tiny_region();
        let apr = SimMonth::new(2027, 4).unwrap();
        assert_eq!(
            r.sun(crate::ids::CellCoord { x: 1, y: 1 }, apr, 20 * 60),
            0.0
        );
        assert_eq!(r.sun(crate::ids::CellCoord { x: 1, y: 1 }, apr, 0), 0.0);
        assert!(r.sun(crate::ids::CellCoord { x: 1, y: 1 }, apr, 9 * 60) > 0.0);
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
