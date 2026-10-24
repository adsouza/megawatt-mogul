//! Frozen A12 compatibility defaults for region archives predating schema 2.
//! These values never come from current shipped files; schema 2 must author its own tuning.

pub const A12_STORAGE_ACCREDITATION: [(f64, f64); 4] =
    [(2.0, 0.65), (4.0, 0.85), (6.0, 0.93), (8.0, 0.96)];
pub const A12_SATISFACTION_TARGET: f64 = 90.0;
pub const A12_RETURN_PER_SATISFACTION_POINT: f64 = 0.0005;

/// Apply only to pre-wind archives. New schema sources author every field.
pub(super) fn extend_wind(file: &mut super::RegionFile) -> Result<(), domain::region::RegionError> {
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
