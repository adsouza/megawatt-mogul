//! Demand: daily and seasonal load shapes, correlated forecast error and the
//! noise-free forecast the player sees.

mod forecast;

pub use forecast::{ForecastPoint, hourly_forecast};

use crate::ids::{CityId, NodeId};
use crate::region::{CityDef, DemandShape};
use crate::rng::SimRng;
use crate::time::{Season, SimMonth, TICKS_PER_HOUR};
use crate::units::Megawatts;

const HOURS_PER_DAY: u32 = 24;

/// A city's demand attached to a transmission node.
#[derive(Debug, Clone, PartialEq)]
pub struct DemandCenter {
    def: CityDef,
    node: NodeId,
}

impl DemandCenter {
    pub fn new(def: CityDef, node: NodeId) -> Self {
        Self { def, node }
    }

    pub fn id(&self) -> CityId {
        self.def.id
    }

    pub fn node(&self) -> NodeId {
        self.node
    }

    pub fn def(&self) -> &CityDef {
        &self.def
    }
}

/// Demand multiplier at `tick` of a day in `month`: the season's hourly shape,
/// linearly interpolated between hour `h` and `h + 1` (hour 23 wraps to hour 0),
/// times the month's multiplier.
pub fn shape_factor(shape: &DemandShape, month: SimMonth, tick: u32) -> f64 {
    let hourly = match month.season() {
        Season::Winter => &shape.winter,
        Season::Summer => &shape.summer,
        Season::Spring | Season::Autumn => &shape.shoulder,
    };
    let hour = (tick / TICKS_PER_HOUR % HOURS_PER_DAY) as usize;
    let next = (hour + 1) % HOURS_PER_DAY as usize;
    let fraction = f64::from(tick % TICKS_PER_HOUR) / f64::from(TICKS_PER_HOUR);
    let interpolated = hourly[hour] + (hourly[next] - hourly[hour]) * fraction;
    interpolated * shape.monthly[month.index0()]
}

/// Slowly varying demand forecast error, as a fraction of demand.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DemandNoise {
    pub error: f64,
}

impl DemandNoise {
    /// Advances the AR(1) process `e <- phi * e + sigma * sqrt(1 - phi^2) * N(0, 1)`
    /// and returns the new error. Draws exactly one standard normal per call, so
    /// the RNG stream position is independent of the state.
    pub fn step(&mut self, shape: &DemandShape, rng: &mut SimRng) -> f64 {
        let phi = shape.error_persistence;
        let sigma = shape.forecast_error.value();
        self.error = phi * self.error + sigma * (1.0 - phi * phi).sqrt() * rng.standard_normal();
        self.error
    }
}

/// Total expected demand of `cities` at `tick`, without noise. Sums only the
/// cities it is given; the caller decides which ones are connected.
pub fn forecast_demand<'a>(
    cities: impl IntoIterator<Item = &'a DemandCenter>,
    shape: &DemandShape,
    month: SimMonth,
    tick: u32,
) -> Megawatts {
    let factor = shape_factor(shape, month, tick);
    cities
        .into_iter()
        .map(|city| city.def.base_load * factor)
        .sum()
}

// Portable state schema 1: explicit field order and enum tags.
crate::state_record!(DemandCenter { def, node });
crate::state_record!(DemandNoise { error });

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::fixtures::tiny_region;

    fn april() -> SimMonth {
        SimMonth::new(2027, 4).unwrap()
    }

    fn centers() -> Vec<DemandCenter> {
        tiny_region()
            .cities()
            .iter()
            .enumerate()
            .map(|(i, def)| DemandCenter::new(def.clone(), NodeId(i as u32)))
            .collect()
    }

    #[test]
    fn shape_at_hour_start_is_table_value() {
        let r = tiny_region();
        assert!((shape_factor(r.demand(), april(), 0) - 0.70 * 0.90).abs() < 1e-12);
    }

    #[test]
    fn shape_interpolates_half_hour() {
        let r = tiny_region();
        let expected = (0.70 + 0.66) / 2.0 * 0.90;
        assert!((shape_factor(r.demand(), april(), 30) - expected).abs() < 1e-12);
    }

    #[test]
    fn shape_wraps_hour_23_to_0() {
        let r = tiny_region();
        let expected = (0.73 + 0.70) / 2.0 * 0.90;
        assert!((shape_factor(r.demand(), april(), 1410) - expected).abs() < 1e-12);
    }

    #[test]
    fn shape_uses_the_season_array() {
        let r = tiny_region();
        let jan = SimMonth::new(2027, 1).unwrap();
        let jul = SimMonth::new(2027, 7).unwrap();
        // Hour 7 is inside the winter window (x1.1) but outside the summer one.
        let winter = shape_factor(r.demand(), jan, 7 * 60);
        let summer = shape_factor(r.demand(), jul, 7 * 60);
        assert!((winter - 0.85 * 1.1 * 1.10).abs() < 1e-12);
        assert!((summer - 0.85 * 1.15).abs() < 1e-12);
    }

    #[test]
    fn ar1_noise_is_stationary_with_sigma() {
        let r = tiny_region();
        let mut n = DemandNoise::default();
        let mut rng = SimRng::from_seed(9);
        let xs: Vec<f64> = (0..200_000).map(|_| n.step(r.demand(), &mut rng)).collect();
        let sd = (xs.iter().map(|x| x * x).sum::<f64>() / xs.len() as f64).sqrt();
        assert!((sd - 0.03).abs() < 0.003);
    }

    #[test]
    fn noise_step_draws_exactly_one_normal() {
        let r = tiny_region();
        let mut n = DemandNoise { error: 0.02 };
        let mut stepped = SimRng::from_seed(4);
        let mut reference = SimRng::from_seed(4);
        n.step(r.demand(), &mut stepped);
        reference.standard_normal();
        assert_eq!(stepped, reference);
    }

    #[test]
    fn forecast_sums_the_cities_it_is_given() {
        let r = tiny_region();
        let cities = centers();
        let factor = shape_factor(r.demand(), april(), 600);
        let all = forecast_demand(&cities, r.demand(), april(), 600);
        assert!((all.value() - 1300.0 * factor).abs() < 1e-9);
        let one = forecast_demand(&cities[..1], r.demand(), april(), 600);
        assert!((one.value() - 600.0 * factor).abs() < 1e-9);
        let none = forecast_demand(&cities[..0], r.demand(), april(), 600);
        assert_eq!(none, Megawatts::ZERO);
    }

    #[test]
    fn demand_center_exposes_its_parts() {
        let def = tiny_region().cities()[0].clone();
        let c = DemandCenter::new(def.clone(), NodeId(7));
        assert_eq!(c.id(), def.id);
        assert_eq!(c.node(), NodeId(7));
        assert_eq!(c.def(), &def);
    }

    #[test]
    fn noise_step_is_persistence_times_error_plus_scaled_innovation() {
        let r = tiny_region();
        let shape = r.demand();
        let (phi, sigma) = (shape.error_persistence, shape.forecast_error.value());
        for seed in 0..10 {
            let mut shadow = SimRng::from_seed(seed);
            let z = shadow.standard_normal();
            let mut n = DemandNoise { error: 0.02 };
            let got = n.step(shape, &mut SimRng::from_seed(seed));
            let expected = phi * 0.02 + sigma * (1.0 - phi * phi).sqrt() * z;
            assert!((got - expected).abs() < 1e-15, "seed {seed}");
            assert_eq!(n.error, got);
        }
    }
}
