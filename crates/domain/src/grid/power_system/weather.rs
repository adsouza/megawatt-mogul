//! The day-ahead weather forecast (spec 2026-10-31 §3): drawn when Planning begins, read by
//! every "expected weather" query, and the base the actual day's surprise is added to.

use super::PowerSystem;
use crate::generation::{MAX_CLEARNESS, MIN_CLEARNESS, mean_clearness};
use crate::region::RegionDefinition;
use crate::rng::SimRng;
use crate::time::{SimMonth, TICKS_PER_DAY};
use crate::units::Megawatts;

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
    /// The day's actual weather at the start of the day: the forecast plus the surprise. Draws
    /// one normal for clearness, then one for the initial wind.
    pub(super) fn draw_day_weather(
        &mut self,
        region: &RegionDefinition,
        month: SimMonth,
        rng: &mut SimRng,
    ) {
        let (weather, solar, wind) = (region.weather(), region.solar(), region.wind());
        self.clearness = (self.weather_forecast.clearness
            + weather.surprise_sd(solar.clearness_sd) * rng.standard_normal())
        .clamp(MIN_CLEARNESS, MAX_CLEARNESS);
        self.wind_factor = (wind.monthly[month.index0()]
            + self.weather_forecast.wind_deviation(0)
            + weather.surprise_sd(wind.initial_sd) * rng.standard_normal())
        .clamp(0.0, 1.0);
    }

    /// One minute's actual wind factor around the forecast path. Draws one normal.
    pub(super) fn step_day_wind(
        &mut self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
        rng: &mut SimRng,
    ) {
        let (weather, wind) = (region.weather(), region.wind());
        self.wind_factor = crate::generation::wind_step_with_forecast(
            self.wind_factor,
            wind.monthly[month.index0()],
            self.weather_forecast.wind_deviation(tick - 1),
            self.weather_forecast.wind_deviation(tick),
            wind.persistence,
            weather.surprise_sd(wind.innovation_sd),
            rng.standard_normal(),
        );
    }

    /// Connected solar's forecast at `tick`: central, low and high. The band is the clearness
    /// surprise's ±2σ around the forecast clearness, clamped like the actual draw.
    pub fn solar_forecast(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
    ) -> (Megawatts, Megawatts, Megawatts) {
        if !self.island_nodes().is_empty() && !self.island_states().is_empty() {
            return self
                .island_nodes()
                .keys()
                .filter_map(|id| self.scoped_island(*id, region))
                .map(|p| p.solar_forecast(region, month, tick))
                .fold(
                    (Megawatts::ZERO, Megawatts::ZERO, Megawatts::ZERO),
                    |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2),
                );
        }
        let spread = 2.0 * region.weather().surprise_sd(region.solar().clearness_sd);
        let at = |clearness: f64| {
            self.solar_at_clearness(
                region,
                month,
                tick,
                clearness.clamp(MIN_CLEARNESS, MAX_CLEARNESS),
            )
        };
        let forecast = self.weather_forecast.clearness;
        (
            self.expected_solar(region, month, tick),
            at(forecast - spread),
            at(forecast + spread),
        )
    }

    pub fn weather_forecast(&self) -> &WeatherForecast {
        &self.weather_forecast
    }

    pub(crate) fn set_weather_forecast(&mut self, forecast: WeatherForecast) {
        self.weather_forecast = forecast;
    }

    /// Draws the day-ahead forecast for `month`: one normal for clearness, then one for each
    /// minute of the wind path. Every normal is drawn even when its coefficient is zero, except
    /// in a region that knows nothing ahead (`surprise_share == 1`, every pre-forecast region):
    /// its forecast is the month's mean and draws nothing, so saves from before the forecast
    /// existed continue exactly as they would have.
    pub fn draw_weather_forecast(
        &mut self,
        region: &RegionDefinition,
        month: SimMonth,
        rng: &mut SimRng,
    ) {
        let (weather, solar, wind) = (region.weather(), region.solar(), region.wind());
        if weather.surprise_share >= 1.0 {
            self.weather_forecast = WeatherForecast::neutral(region, month);
            return;
        }
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
    fn share_one_gives_a_neutral_mean_forecast_without_drawing() {
        let region = region_with_share(1.0);
        let mut power = super::super::tests::system_with(&region, &[]);
        let mut rng = SimRng::from_seed(3);
        power.draw_weather_forecast(&region, april(), &mut rng);
        assert_eq!(rng, SimRng::from_seed(3));
        let forecast = power.weather_forecast();
        assert_eq!(forecast, &WeatherForecast::neutral(&region, april()));
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

#[cfg(test)]
mod day_tests {
    use crate::generation::{draw_clearness, wind_step};
    use crate::region::RegionDefinition;
    use crate::region::fixtures::tiny_region_parts;
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

    /// A Planning forecast, then the day's weather: clearness and every minute's wind factor.
    fn played_day(region: &RegionDefinition, seed: u64) -> (super::WeatherForecast, f64, Vec<f64>) {
        let mut sys = super::super::tests::system_with(region, &[]);
        let mut rng = SimRng::from_seed(seed);
        sys.draw_weather_forecast(region, april(), &mut rng);
        sys.begin_day(region, april(), &mut rng);
        let mut winds = vec![sys.wind_factor()];
        for tick in 1..TICKS_PER_DAY {
            sys.tick(region, april(), tick, &mut rng);
            winds.push(sys.wind_factor());
        }
        (sys.weather_forecast().clone(), sys.clearness, winds)
    }

    #[test]
    fn share_one_reproduces_todays_day_bit_for_bit() {
        let region = region_with_share(1.0);
        let (_, clearness, winds) = played_day(&region, 21);
        // Today's day, on the same generator: a share-one forecast draws nothing.
        let mut reference = SimRng::from_seed(21);
        let expected_clearness = draw_clearness(region.solar(), april(), &mut reference);
        let w = region.wind();
        let m = w.monthly[april().index0()];
        let mut expected = vec![(m + w.initial_sd * reference.standard_normal()).clamp(0.0, 1.0)];
        for _ in 1..TICKS_PER_DAY {
            reference.standard_normal(); // the minute's demand noise draw
            let previous = *expected.last().unwrap();
            expected.push(wind_step(
                previous,
                m,
                w.persistence,
                w.innovation_sd,
                reference.standard_normal(),
            ));
        }
        assert_eq!(clearness.to_bits(), expected_clearness.to_bits());
        assert_eq!(winds, expected);
    }

    #[test]
    fn share_zero_makes_the_day_the_forecast() {
        let region = region_with_share(0.0);
        let (forecast, clearness, winds) = played_day(&region, 22);
        assert_eq!(
            clearness,
            forecast.clearness.clamp(
                crate::generation::MIN_CLEARNESS,
                crate::generation::MAX_CLEARNESS
            )
        );
        let m = region.wind().monthly[april().index0()];
        for (tick, wind) in winds.iter().enumerate() {
            let expected = (m + forecast.wind_deviation(tick as u32)).clamp(0.0, 1.0);
            assert!(
                (wind - expected).abs() < 1e-12,
                "tick {tick}: {wind} vs {expected}"
            );
        }
    }
}

#[cfg(test)]
mod reader_tests {
    use super::super::tests::{connected, system_with};
    use crate::ids::CellCoord;
    use crate::region::PlantKind;
    use crate::region::fixtures::tiny_region;
    use crate::rng::SimRng;
    use crate::time::SimMonth;

    fn april() -> SimMonth {
        SimMonth::new(2027, 4).unwrap()
    }

    #[test]
    fn planning_wind_forecast_follows_the_forecast_path() {
        let region = tiny_region();
        let mut sys = system_with(&region, &[]);
        sys.draw_weather_forecast(&region, april(), &mut SimRng::from_seed(13));
        let w = region.wind();
        let m = w.monthly[april().index0()];
        let band = 2.0 * region.weather().surprise_sd(w.initial_sd);
        for tick in [0, 30, 700, 1439] {
            let central = (m + sys.weather_forecast().wind_deviation(tick)).clamp(0.0, 1.0);
            let (got, low, high) = sys.wind_factor_forecast(&region, april(), tick);
            assert_eq!(got, central, "tick {tick}");
            assert_eq!(low, (central - band).max(0.0));
            assert_eq!(high, (central + band).min(1.0));
        }
    }

    #[test]
    fn solar_band_brackets_the_central_forecast() {
        let region = tiny_region();
        let mut sys = system_with(
            &region,
            &[connected(PlantKind::Solar, CellCoord { x: 5, y: 5 })],
        );
        sys.draw_weather_forecast(&region, april(), &mut SimRng::from_seed(14));
        let mut widest = 0.0;
        for hour in 0..24 {
            let tick = hour * 60 + 30;
            let (central, low, high) = sys.solar_forecast(&region, april(), tick);
            assert_eq!(central, sys.expected_solar(&region, april(), tick));
            assert!(low <= central && central <= high, "hour {hour}");
            widest = f64::max(widest, (high - low).value());
        }
        assert!(widest > 0.0);
    }
}
