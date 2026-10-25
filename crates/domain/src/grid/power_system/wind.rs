//! Shared, pure variable-generation forecasts and schema-1 activation.
use super::*;

impl PowerSystem {
    pub fn wind_factor(&self) -> f64 {
        self.wind_factor
    }

    pub fn wind_factor_forecast(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
    ) -> (f64, f64, f64) {
        let w = region.wind();
        let mean = w.monthly[month.index0()];
        let factor = if self.wind_month == Some(month) {
            let horizon = tick.saturating_sub(self.wind_tick.unwrap_or(0));
            mean + w.persistence.powf(f64::from(horizon)) * (self.wind_factor - mean)
        } else {
            mean
        };
        (
            factor,
            (factor - w.band.value()).max(0.0),
            (factor + w.band.value()).min(1.0),
        )
    }

    pub fn wind_forecast(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
    ) -> (Megawatts, Megawatts, Megawatts) {
        let (factor, low, high) = self.wind_factor_forecast(region, month, tick);
        let potential: Megawatts = self
            .connected_plants()
            .filter(|p| p.kind() == PlantKind::Wind)
            .map(|p| p.spec().capacity * cell_ratio(region, p.cell(), |c| c.wind_potential).value())
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
