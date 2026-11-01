//! Shared, pure variable-generation forecasts and schema-1 activation.
use super::*;

impl PowerSystem {
    pub fn wind_factor(&self) -> f64 {
        self.wind_factor
    }

    /// The wind factor forecast at `tick`: central, low and high. In Planning it is the
    /// day-ahead forecast path; while the day runs, the current surprise decays toward that
    /// path. The band is the surprise's ±2σ (spec 2026-10-31 §3.3).
    pub fn wind_factor_forecast(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
    ) -> (f64, f64, f64) {
        let actual = (self.wind_month == Some(month))
            .then(|| (self.wind_factor, self.wind_tick.unwrap_or(0)));
        wind_forecast_from(&self.weather_forecast, region, month, tick, actual)
    }

    pub fn wind_forecast(
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
                .map(|p| p.wind_forecast(region, month, tick))
                .fold(
                    (Megawatts::ZERO, Megawatts::ZERO, Megawatts::ZERO),
                    |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2),
                );
        }
        let (factor, low, high) = self.wind_factor_forecast(region, month, tick);
        let potential: Megawatts = self
            .connected_plants()
            .filter(|p| p.kind() == PlantKind::Wind)
            .map(|p| {
                p.available_capacity() * cell_ratio(region, p.cell(), |c| c.wind_potential).value()
            })
            .sum();
        (potential * factor, potential * low, potential * high)
    }

    pub(super) fn withheld_by_kind(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
        withheld: Megawatts,
    ) -> BTreeMap<PlantKind, Megawatts> {
        let mut by_kind = BTreeMap::new();
        for plant in self.connected_plants().filter(|p| is_curtailable(p)) {
            *by_kind.entry(plant.kind()).or_insert(Megawatts::ZERO) +=
                self.available(region, plant, month, tick);
        }
        let total: Megawatts = by_kind.values().copied().sum();
        let fraction = if total > Megawatts::ZERO {
            (withheld / total).clamp(0.0, 1.0)
        } else {
            0.0
        };
        for mw in by_kind.values_mut() {
            *mw = *mw * fraction;
        }
        by_kind
    }

    pub fn expected_variable(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
    ) -> Megawatts {
        self.expected_solar(region, month, tick) + self.wind_forecast(region, month, tick).0
    }

    pub(super) fn wind_supply_forecast(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
    ) -> Vec<SupplyForecastPoint> {
        // Projection supports an empty storage fleet too, retaining minute-level wind changes.
        self.project_storage_supply(
            region,
            month,
            0,
            TICKS_PER_DAY,
            true,
            &mut BatteryScheduleView::default(),
        )
    }

    pub(super) fn has_wind(&self) -> bool {
        self.connected_plants().any(|p| p.kind() == PlantKind::Wind)
    }

    /// The foreseen fall in connected wind output from `tick` to the end of its
    /// hour, by the central forecast, and the ticks it takes. Above its mean the
    /// forecast always drifts down, so callers weigh the size of the fall
    /// against what the fleet can replace in that time, never its sign alone.
    pub(super) fn wind_fall(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
    ) -> (Megawatts, u32) {
        let end = ((tick / TICKS_PER_HOUR + 1) * TICKS_PER_HOUR).min(TICKS_PER_DAY - 1);
        let fall =
            self.wind_forecast(region, month, tick).0 - self.wind_forecast(region, month, end).0;
        (fall.max(Megawatts::ZERO), end.saturating_sub(tick))
    }

    pub(crate) fn wind_state_invalid(&self, month: SimMonth, tick: u32) -> bool {
        self.wind_tick.is_some_and(|sample| {
            sample >= TICKS_PER_DAY || (self.wind_month == Some(month) && sample > tick)
        }) || self.wind_month.is_some_and(|m| m > month)
            || self.wind_tick.is_some() != self.wind_month.is_some()
    }

    pub(crate) fn upgrade_schema1(
        &mut self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: Option<u32>,
    ) {
        self.wind_factor = region.wind().monthly[month.index0()];
        self.wind_tick = tick;
        self.wind_month = tick.map(|_| month);
        for plant in self.plants.values_mut() {
            plant.upgrade_schema1(region);
        }
        self.wind_needs_replan = true;
    }
}

/// The wind factor forecast shared by the whole system and each island's view: the forecast
/// path, plus the decaying surprise `actual` (the factor and the minute it was observed) when
/// the day is running.
pub(super) fn wind_forecast_from(
    forecast: &WeatherForecast,
    region: &RegionDefinition,
    month: SimMonth,
    tick: u32,
    actual: Option<(f64, u32)>,
) -> (f64, f64, f64) {
    let w = region.wind();
    let mean = w.monthly[month.index0()];
    let path = mean + forecast.wind_deviation(tick);
    let factor = match actual {
        Some((factor, observed)) => {
            let horizon = tick.saturating_sub(observed);
            let surprise = factor - mean - forecast.wind_deviation(observed);
            path + w.persistence.powf(f64::from(horizon)) * surprise
        }
        None => path,
    }
    .clamp(0.0, 1.0);
    let band = 2.0 * region.weather().surprise_sd(w.initial_sd);
    (factor, (factor - band).max(0.0), (factor + band).min(1.0))
}
