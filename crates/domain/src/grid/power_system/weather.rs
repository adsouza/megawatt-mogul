//! The day-ahead weather forecast (spec 2026-10-31 §3): drawn when Planning begins, read by
//! every "expected weather" query, and the base the actual day's surprise is added to.

use super::PowerSystem;
use crate::generation::{MAX_CLEARNESS, MIN_CLEARNESS, mean_clearness};
use crate::region::RegionDefinition;
use crate::rng::SimRng;
use crate::time::{SimMonth, TICKS_PER_DAY};

/// The coming day's forecast weather, regional like the weather itself.
///
/// The default is the pre-forecast clear sky with no wind deviation; [`WeatherForecast::neutral`]
/// is the month's mean, which is what a region that knows nothing ahead would forecast.
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherForecast {
    /// The month the forecast was drawn for; `None` before the first draw.
    pub month: Option<SimMonth>,
    /// Forecast clearness, unclamped: the actual day adds the surprise before clamping.
    pub clearness: f64,
    /// The clearness expected given the forecast: the mean of the bounded actual draw for
    /// calibrated regions (A16 forbids an optimistic forecast caused by clipping), else the
    /// clamped forecast. What expected solar uses.
    pub expected_clearness: f64,
    /// The forecast wind deviation from the monthly mean for each minute of the day. Empty
    /// means no deviation.
    pub wind_path: Vec<f64>,
}

impl Default for WeatherForecast {
    fn default() -> Self {
        Self {
            month: None,
            clearness: 1.0,
            expected_clearness: 1.0,
            wind_path: Vec::new(),
        }
    }
}

impl WeatherForecast {
    /// The month's mean weather: what a region with nothing known ahead forecasts.
    pub fn neutral(region: &RegionDefinition, month: SimMonth) -> Self {
        Self {
            month: Some(month),
            clearness: region.solar().clearness_mean[month.index0()],
            expected_clearness: region.expected_clearness(month),
            wind_path: Vec::new(),
        }
    }

    /// The forecast wind deviation from the monthly mean at `tick`.
    pub fn wind_deviation(&self, tick: u32) -> f64 {
        self.wind_path.get(tick as usize).copied().unwrap_or(0.0)
    }
}

crate::state_record!(WeatherForecast {
    month,
    clearness,
    expected_clearness,
    wind_path
});

impl PowerSystem {
    pub fn weather_forecast(&self) -> &WeatherForecast {
        &self.weather_forecast
    }

    pub(crate) fn set_weather_forecast(&mut self, forecast: WeatherForecast) {
        self.weather_forecast = forecast;
    }

    /// Draws the day-ahead forecast for `month`: one normal for clearness, then one for each
    /// minute of the wind path. Every normal is drawn even when its coefficient is zero.
    pub fn draw_weather_forecast(
        &mut self,
        region: &RegionDefinition,
        month: SimMonth,
        rng: &mut SimRng,
    ) {
        let (weather, solar, wind) = (region.weather(), region.solar(), region.wind());
        let clearness = solar.clearness_mean[month.index0()]
            + weather.forecast_sd(solar.clearness_sd) * rng.standard_normal();
        let mut wind_path = Vec::with_capacity(TICKS_PER_DAY as usize);
        let mut deviation = weather.forecast_sd(wind.initial_sd) * rng.standard_normal();
        wind_path.push(deviation);
        let innovation_sd = weather.forecast_sd(wind.innovation_sd);
        for _ in 1..TICKS_PER_DAY {
            deviation = wind.persistence * deviation + innovation_sd * rng.standard_normal();
            wind_path.push(deviation);
        }
        let expected_clearness = if solar.annual_site_clearness {
            mean_clearness(clearness, weather.surprise_sd(solar.clearness_sd))
        } else {
            clearness.clamp(MIN_CLEARNESS, MAX_CLEARNESS)
        };
        self.weather_forecast = WeatherForecast {
            month: Some(month),
            clearness,
            expected_clearness,
            wind_path,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::RegionDefinition;
    use crate::region::fixtures::{tiny_region, tiny_region_parts};
    use crate::rng::SimRng;
    use crate::time::{SimMonth, TICKS_PER_DAY};

    fn april() -> SimMonth {
        SimMonth::new(2027, 4).unwrap()
    }

    fn region_with_share(share: f64) -> RegionDefinition {
        let mut parts = tiny_region_parts();
        parts.weather.surprise_share = share;
        RegionDefinition::try_from(parts).unwrap()
    }

    #[test]
    fn draw_order_is_clearness_then_1440_wind_normals() {
        let region = tiny_region();
        let mut power = super::super::tests::system_with(&region, &[]);
        let (mut rng, mut reference) = (SimRng::from_seed(11), SimRng::from_seed(11));
        power.draw_weather_forecast(&region, april(), &mut rng);

        let (weather, solar, wind) = (region.weather(), region.solar(), region.wind());
        let clearness = solar.clearness_mean[april().index0()]
            + weather.forecast_sd(solar.clearness_sd) * reference.standard_normal();
        let mut path = vec![weather.forecast_sd(wind.initial_sd) * reference.standard_normal()];
        for _ in 1..TICKS_PER_DAY {
            let previous = *path.last().unwrap();
            path.push(
                wind.persistence * previous
                    + weather.forecast_sd(wind.innovation_sd) * reference.standard_normal(),
            );
        }
        assert_eq!(rng, reference);
        let forecast = power.weather_forecast();
        assert_eq!(forecast.month, Some(april()));
        assert_eq!(forecast.clearness, clearness);
        assert_eq!(forecast.wind_path, path);
        assert_eq!(forecast.wind_path.len(), TICKS_PER_DAY as usize);
    }

    #[test]
    fn share_one_gives_a_neutral_mean_forecast() {
        let region = region_with_share(1.0);
        let mut power = super::super::tests::system_with(&region, &[]);
        power.draw_weather_forecast(&region, april(), &mut SimRng::from_seed(3));
        let forecast = power.weather_forecast();
        assert_eq!(
            forecast.clearness,
            region.solar().clearness_mean[april().index0()]
        );
        assert!(forecast.wind_path.iter().all(|f| *f == 0.0));
        assert_eq!(forecast.wind_deviation(17), 0.0);
        assert_eq!(
            WeatherForecast::neutral(&region, april()).clearness,
            forecast.clearness
        );
    }

    #[test]
    fn expected_clearness_is_the_clamped_forecast_without_annual_site_clearness() {
        let region = tiny_region();
        assert!(!region.solar().annual_site_clearness);
        let mut power = super::super::tests::system_with(&region, &[]);
        power.draw_weather_forecast(&region, april(), &mut SimRng::from_seed(5));
        let forecast = power.weather_forecast();
        assert_eq!(
            forecast.expected_clearness,
            forecast.clearness.clamp(
                crate::generation::MIN_CLEARNESS,
                crate::generation::MAX_CLEARNESS
            )
        );
    }
}
