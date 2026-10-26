//! Civil-time demand shapes and the region's authored seasonal calendar.
use super::{ClimateId, RegionDefinition, RegionDefinitionParts, RegionError, check_shape};
use crate::ids::CityId;
use crate::time::{Season, SimMonth, TICKS_PER_HOUR};
use crate::units::Ratio;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub struct Climate {
    pub id: ClimateId,
    pub shapes: Vec<[f64; 24]>,
    pub month_shapes: [usize; 12],
    pub monthly: [f64; 12],
}
impl Climate {
    /// Interpolate adjacent local hours, preserving legacy floating-point order.
    pub fn factor(&self, month: SimMonth, local_minute: u32) -> f64 {
        let hourly = &self.shapes[self.month_shapes[month.index0()]];
        let hour = (local_minute / TICKS_PER_HOUR % 24) as usize;
        let next = (hour + 1) % 24;
        let fraction = f64::from(local_minute % TICKS_PER_HOUR) / f64::from(TICKS_PER_HOUR);
        let interpolated = hourly[hour] + (hourly[next] - hourly[hour]) * fraction;
        interpolated * self.monthly[month.index0()]
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct DemandNoiseParams {
    pub forecast_error: Ratio,
    pub error_persistence: f64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calendar {
    pub seasons: [Season; 12],
}
impl Calendar {
    pub fn northern_meteorological() -> Self {
        Self {
            seasons: std::array::from_fn(|i| {
                SimMonth::new(2027, i as u8 + 1)
                    .expect("valid calendar month")
                    .season()
            }),
        }
    }
    pub fn season(&self, month: SimMonth) -> Season {
        self.seasons[month.index0()]
    }
    /// Length of the maximal cyclic run containing this month (at most twelve).
    pub fn run_length(&self, month: SimMonth) -> u8 {
        let i = month.index0();
        let mut length = 1;
        while length < 12 && self.seasons[(i + length) % 12] == self.seasons[i] {
            length += 1;
        }
        let mut back = 1;
        while length < 12 && self.seasons[(i + 12 - back) % 12] == self.seasons[i] {
            length += 1;
            back += 1;
        }
        length as u8
    }
}
impl RegionDefinition {
    pub fn load_factor(&self, city: CityId, month: SimMonth, tick: u32) -> f64 {
        let id = self
            .city_locales
            .get(&city)
            .expect("city belongs to the region")
            .climate;
        self.climates
            .iter()
            .find(|c| c.id == id)
            .expect("validated city climate")
            .factor(month, self.local_minute(city, month, tick))
    }
    pub fn demand_noise(&self) -> &DemandNoiseParams {
        &self.demand_noise
    }
    pub fn season(&self, month: SimMonth) -> Season {
        self.calendar.season(month)
    }
    pub fn calendar(&self) -> &Calendar {
        &self.calendar
    }
}
pub(super) fn validate(parts: &RegionDefinitionParts) -> Result<(), RegionError> {
    let mut ids = BTreeSet::new();
    for climate in &parts.climates {
        check_shape(ids.insert(climate.id), "climates.id")?;
        check_shape(
            (1..=4).contains(&climate.shapes.len())
                && climate
                    .shapes
                    .iter()
                    .flatten()
                    .all(|v| v.is_finite() && *v >= 0.0),
            "climates.shapes",
        )?;
        check_shape(
            climate
                .month_shapes
                .iter()
                .all(|i| *i < climate.shapes.len()),
            "climates.month_shapes",
        )?;
        check_shape(
            climate.monthly.iter().all(|v| v.is_finite() && *v >= 0.0),
            "climates.monthly",
        )?;
    }
    check_shape(
        parts
            .city_locales
            .values()
            .all(|locale| ids.contains(&locale.climate)),
        "cities.climate",
    )?;
    check_shape(
        (0.0..1.0).contains(&parts.demand_noise.error_persistence),
        "demand.error_persistence",
    )?;
    check_shape(
        (0.0..=1.0).contains(&parts.demand_noise.forecast_error.value()),
        "demand.forecast_error",
    )
}
