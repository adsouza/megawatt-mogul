//! The player-facing 24-hour demand forecast with an uncertainty band.

use super::{DemandCenter, forecast_demand};
use crate::region::DemandShape;
use crate::time::{SimMonth, TICKS_PER_HOUR};
use crate::units::Megawatts;

/// Forecast demand for one hour of the day. `low` and `high` are the mean
/// `-/+ 2 sigma` of the forecast error.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForecastPoint {
    pub hour: u8,
    pub mean: Megawatts,
    pub low: Megawatts,
    pub high: Megawatts,
}

/// Forecast for each hour of a day in `month`. The mean is taken at minute 30 of
/// the hour. Sums only the cities it is given.
pub fn hourly_forecast<'a>(
    cities: impl IntoIterator<Item = &'a DemandCenter>,
    shape: &DemandShape,
    month: SimMonth,
) -> [ForecastPoint; 24] {
    let cities: Vec<&DemandCenter> = cities.into_iter().collect();
    let band = 2.0 * shape.forecast_error.value();
    std::array::from_fn(|h| {
        let hour = h as u8;
        let tick = u32::from(hour) * TICKS_PER_HOUR + TICKS_PER_HOUR / 2;
        let mean = forecast_demand(cities.iter().copied(), shape, month, tick);
        ForecastPoint {
            hour,
            mean,
            low: mean * (1.0 - band),
            high: mean * (1.0 + band),
        }
    })
}

// Portable state schema 1: explicit field order and enum tags.
crate::state_record!(ForecastPoint {
    hour,
    mean,
    low,
    high
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demand::shape_factor;
    use crate::ids::NodeId;
    use crate::region::fixtures::tiny_region;

    fn centers() -> Vec<DemandCenter> {
        tiny_region()
            .cities()
            .iter()
            .map(|def| DemandCenter::new(def.clone(), NodeId(1)))
            .collect()
    }

    #[test]
    fn forecast_band_brackets_mean() {
        let r = tiny_region();
        let month = SimMonth::new(2027, 4).unwrap();
        let points = hourly_forecast(&centers(), r.demand(), month);
        for p in points {
            assert!(p.low < p.mean && p.mean < p.high);
        }
    }

    #[test]
    fn forecast_covers_each_hour_at_minute_30_with_two_sigma_band() {
        let r = tiny_region();
        let month = SimMonth::new(2027, 4).unwrap();
        let points = hourly_forecast(&centers(), r.demand(), month);
        for (h, p) in points.iter().enumerate() {
            assert_eq!(usize::from(p.hour), h);
            let mean = 1300.0 * shape_factor(r.demand(), month, h as u32 * 60 + 30);
            assert!((p.mean.value() - mean).abs() < 1e-9);
            assert!((p.low.value() - mean * 0.94).abs() < 1e-9);
            assert!((p.high.value() - mean * 1.06).abs() < 1e-9);
        }
    }
}
