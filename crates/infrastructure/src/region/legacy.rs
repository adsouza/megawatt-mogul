//! Frozen compatibility values for archived legacy region schemas.
//! These values never come from current shipped files or apply to schema 4.

pub const A12_STORAGE_ACCREDITATION: [(f64, f64); 4] =
    [(2.0, 0.65), (4.0, 0.85), (6.0, 0.93), (8.0, 0.96)];
pub const A12_SATISFACTION_TARGET: f64 = 90.0;
pub const A12_RETURN_PER_SATISFACTION_POINT: f64 = 0.0005;

/// Apply only to pre-wind archives. New schema sources author every field.
pub(super) fn extend_wind(file: &mut super::RegionFile) -> Result<()> {
    use super::{PlantKindDto, WindExtension};
    let payload = match file.id.as_str() {
        "eastern_interconnection" => include_str!("wind_east_v1.ron"),
        "western_interconnection" => include_str!("wind_west_v1.ron"),
        _ => include_str!("wind_other_v1.ron"),
    };
    if file.wind.is_some()
        || file.wind_potential.is_some()
        || file.catalog.iter().any(|s| {
            s.kind == PlantKindDto::Wind
                || s.battery
                    .as_ref()
                    .is_some_and(|b| b.charge_end_hour.is_some())
        })
    {
        return Err(domain::region::RegionError::MissingField {
            field: "region_schema",
        });
    }
    let extension: WindExtension =
        ron::from_str(payload).expect("frozen wind extension is valid RON");
    file.wind = Some(extension.wind);
    file.wind_potential = Some(extension.potential);
    file.catalog.push(extension.plant);
    for spec in &mut file.catalog {
        if let Some(b) = &mut spec.battery {
            b.charge_end_hour = Some(extension.charge_end_hour);
        }
    }
    Ok(())
}

use super::mapping::array;
use super::{DemandDto, RegionFile};
use domain::ids::CityId;
use domain::region::{
    Calendar, CityLocale, Climate, ClimateId, RegionClock, RegionError, TimeZone, ZoneId,
};
use domain::units::Currency;
use std::collections::BTreeMap;
type Result<T> = std::result::Result<T, RegionError>;

pub(super) struct LegacyLocale {
    pub currency: Currency,
    pub clock: RegionClock,
    pub city_locales: BTreeMap<CityId, CityLocale>,
    pub climates: Vec<Climate>,
    pub calendar: Calendar,
}
/// Frozen locale for all schemas through 3; never applied to schema 4.
pub(super) fn legacy_locale(file: &RegionFile) -> Result<LegacyLocale> {
    Ok(LegacyLocale {
        currency: Currency::Usd,
        clock: RegionClock {
            system_zone: ZoneId(0),
            zones: vec![TimeZone {
                id: ZoneId(0),
                name_key: None,
                standard_offset_minutes: 0,
                dst_months: [false; 12],
            }],
        },
        city_locales: file
            .cities
            .iter()
            .map(|c| {
                (
                    CityId(c.id),
                    CityLocale {
                        zone: ZoneId(0),
                        climate: ClimateId(0),
                    },
                )
            })
            .collect(),
        climates: vec![map_demand(&file.demand)?],
        calendar: Calendar::northern_meteorological(),
    })
}
fn map_demand(demand: &DemandDto) -> Result<Climate> {
    Ok(Climate {
        id: ClimateId(0),
        shapes: vec![
            array(&demand.winter, "demand.winter")?,
            array(&demand.summer, "demand.summer")?,
            array(&demand.shoulder, "demand.shoulder")?,
        ],
        month_shapes: [0, 0, 2, 2, 2, 1, 1, 1, 2, 2, 2, 0],
        monthly: array(&demand.monthly, "demand.monthly")?,
    })
}
