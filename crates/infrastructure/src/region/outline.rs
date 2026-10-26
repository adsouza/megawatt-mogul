//! Geographic presentation data, read from the same bytes as the playable region.
use application::RegionLoadError;
use domain::{ids::RegionId, region::RegionError, units::Currency};

use super::{AtlasDto, Bounds, ParsedRegionFile, catalog::parse_region_bundle, raster::contains};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hemisphere {
    Northern,
    Southern,
    Both,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegionOutline {
    pub areas: Vec<super::dto::AreaDto>,
    pub bounds: Bounds,
    pub land: Vec<Vec<(f64, f64)>>,
    pub exclude: Vec<Vec<(f64, f64)>>,
    pub home: Option<(String, f64, f64)>,
    pub cities: usize,
    pub atlas: Option<AtlasDto>,
    pub frequency_hz: f64,
    pub currency: Currency,
    /// Minimum and maximum civil UTC offsets occurring during the year, in minutes.
    pub zone_span: (i32, i32),
    pub hemisphere: Hemisphere,
}

impl RegionOutline {
    /// Match the playable raster, including its bounds and excluded polygons.
    pub fn contains(&self, lon: f64, lat: f64) -> bool {
        lon >= self.bounds.west
            && lon <= self.bounds.east
            && lat >= self.bounds.south
            && lat <= self.bounds.north
            && self.land.iter().any(|ring| contains(ring, lon, lat))
            && !self.exclude.iter().any(|ring| contains(ring, lon, lat))
    }
}

/// The overview is optional presentation data; a catalog using another file format can still
/// offer its region as a labelled button without a geographic preview.
pub fn region_outline(source: &[u8], id: &RegionId) -> Result<RegionOutline, RegionLoadError> {
    let file = parse_region_bundle(source, id)?;
    let invalid = |field| RegionLoadError::Invalid {
        region: id.clone(),
        error: RegionError::InvalidShape { field },
    };
    let (bounds, land, exclude, frequency_hz) = match &file {
        ParsedRegionFile::Legacy(f) => (&f.bounds, &f.land, &f.exclude, f.frequency.nominal_hz),
        ParsedRegionFile::V4(f) => (&f.bounds, &f.land, &f.exclude, f.frequency.nominal_hz),
    };
    if ![bounds.west, bounds.east, bounds.south, bounds.north]
        .into_iter()
        .all(f64::is_finite)
        || bounds.west >= bounds.east
        || bounds.south >= bounds.north
        || land
            .iter()
            .chain(exclude)
            .flatten()
            .any(|(lon, lat)| !lon.is_finite() || !lat.is_finite())
    {
        return Err(invalid("bounds"));
    }
    if !frequency_hz.is_finite() || frequency_hz <= 0.0 {
        return Err(invalid("frequency.nominal_hz"));
    }
    let hemisphere = if bounds.south >= 0.0 {
        Hemisphere::Northern
    } else if bounds.north <= 0.0 {
        Hemisphere::Southern
    } else {
        Hemisphere::Both
    };
    match file {
        ParsedRegionFile::Legacy(file) => {
            let home = file
                .cities
                .iter()
                .find(|c| c.id == file.start.home_city)
                .map(|c| (c.name_key.clone(), c.lon, c.lat));
            Ok(RegionOutline {
                areas: Vec::new(),
                bounds: file.bounds,
                land: file.land,
                exclude: file.exclude,
                home,
                cities: file.cities.len(),
                atlas: None,
                frequency_hz,
                currency: Currency::Usd,
                zone_span: (0, 0),
                hemisphere,
            })
        }
        ParsedRegionFile::V4(file) => {
            // Optional construction maps must not acquire a geographic click target when the
            // playable loader would reject their IDs, rates, costs or land assignments.
            if file.economic_zones.is_some() {
                super::map_region_v4(file.clone()).map_err(|error| RegionLoadError::Invalid {
                    region: id.clone(),
                    error,
                })?;
            }
            // This reader sees raw DTOs, before domain validation. Validate before doing DST
            // arithmetic (i32::MAX + 60 would otherwise panic in a debug build).
            if file.time_zones.is_empty() {
                return Err(invalid("time_zones"));
            }
            let mut zone_span = (i32::MAX, i32::MIN);
            for zone in &file.time_zones {
                let offset = zone.standard_offset_minutes;
                if !(-720..=840).contains(&offset)
                    || offset % 15 != 0
                    || zone.dst_months.len() != 12
                {
                    return Err(invalid("time_zones.offset"));
                }
                for dst in &zone.dst_months {
                    let civil = offset + if *dst { 60 } else { 0 };
                    zone_span.0 = zone_span.0.min(civil);
                    zone_span.1 = zone_span.1.max(civil);
                }
            }
            let currency = match file.currency {
                super::CurrencyDto::Usd => Currency::Usd,
                super::CurrencyDto::Eur => Currency::Eur,
                super::CurrencyDto::Cny => Currency::Cny,
                super::CurrencyDto::Inr => Currency::Inr,
                super::CurrencyDto::Rub => Currency::Rub,
                super::CurrencyDto::Brl => Currency::Brl,
                super::CurrencyDto::Unknown => return Err(invalid("currency")),
            };
            let home = file
                .cities
                .iter()
                .find(|c| c.id == file.start.home_city)
                .map(|c| (c.name_key.clone(), c.lon, c.lat));
            Ok(RegionOutline {
                areas: file.areas,
                bounds: file.bounds,
                land: file.land,
                exclude: file.exclude,
                home,
                cities: file.cities.len(),
                atlas: file.atlas,
                frequency_hz,
                currency,
                zone_span,
                hemisphere,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::map_region_v4;
    use domain::ids::CellCoord;

    #[test]
    fn outline_rejects_invalid_metadata_without_overflow_or_nonfinite_geometry() {
        let source = include_str!("../../tests/support/minimal_region_v4.ron");
        for bad in [
            source.replace(
                "standard_offset_minutes: -180",
                "standard_offset_minutes: 2147483647",
            ),
            source.replace(
                "standard_offset_minutes: -180",
                "standard_offset_minutes: -2147483648",
            ),
            source.replace("north: 8.0", "north: NaN"),
            source.replace("nominal_hz: 60.0", "nominal_hz: NaN"),
        ] {
            assert!(region_outline(bad.as_bytes(), &RegionId::new("minimal")).is_err());
        }
    }

    #[test]
    fn preview_matches_every_playable_cell_in_the_shipped_regions() {
        use application::RegionCatalog;
        let catalog = super::super::RonRegionCatalog::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/regions"
        ));
        for name in ["eastern_interconnection", "western_interconnection"] {
            let id = RegionId::new(name);
            let source = catalog.load(&id).unwrap().source_bytes;
            let outline = region_outline(&source, &id).unwrap();
            let ParsedRegionFile::V4(file) = parse_region_bundle(&source, &id).unwrap() else {
                panic!("{} is schema 4", id.as_str());
            };
            let region = map_region_v4(file.clone()).unwrap();
            let grid = region.terrain();
            for y in 0..file.grid.height {
                for x in 0..file.grid.width {
                    let lon = file.bounds.west
                        + (f64::from(x) + 0.5) / f64::from(file.grid.width)
                            * (file.bounds.east - file.bounds.west);
                    let lat = file.bounds.north
                        - (f64::from(y) + 0.5) / f64::from(file.grid.height)
                            * (file.bounds.north - file.bounds.south);
                    assert_eq!(
                        outline.contains(lon, lat),
                        grid.get(CellCoord { x, y }).unwrap().land,
                        "{name} ({x}, {y})"
                    );
                }
            }
            assert!(outline.home.is_some());
        }
    }
}
