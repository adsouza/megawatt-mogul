//! Geographic presentation data, read from the same bytes as the playable region.
use application::RegionLoadError;
use domain::ids::RegionId;

use super::{AtlasDto, Bounds, ParsedRegionFile, catalog::parse_region_file, raster::contains};

#[derive(Debug, Clone, PartialEq)]
pub struct RegionOutline {
    pub bounds: Bounds,
    pub land: Vec<Vec<(f64, f64)>>,
    pub exclude: Vec<Vec<(f64, f64)>>,
    pub home: Option<(String, f64, f64)>,
    pub cities: usize,
    pub atlas: Option<AtlasDto>,
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
    let file = parse_region_file(source, id)?;
    match file {
        ParsedRegionFile::Legacy(file) => {
            let home = file
                .cities
                .iter()
                .find(|c| c.id == file.start.home_city)
                .map(|c| (c.name_key.clone(), c.lon, c.lat));
            Ok(RegionOutline {
                bounds: file.bounds,
                land: file.land,
                exclude: file.exclude,
                home,
                cities: file.cities.len(),
                atlas: None,
            })
        }
        ParsedRegionFile::V4(file) => {
            let home = file
                .cities
                .iter()
                .find(|c| c.id == file.start.home_city)
                .map(|c| (c.name_key.clone(), c.lon, c.lat));
            Ok(RegionOutline {
                bounds: file.bounds,
                land: file.land,
                exclude: file.exclude,
                home,
                cities: file.cities.len(),
                atlas: file.atlas,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::{RegionFile, rasterize};
    use domain::ids::CellCoord;

    #[test]
    fn preview_matches_every_playable_cell_in_the_shipped_regions() {
        for (id, source) in [
            (
                "eastern_interconnection",
                include_bytes!("../../../../assets/regions/eastern_interconnection.ron").as_slice(),
            ),
            (
                "western_interconnection",
                include_bytes!("../../../../assets/regions/western_interconnection.ron").as_slice(),
            ),
        ] {
            let outline = region_outline(source, &RegionId::new(id)).unwrap();
            let file: RegionFile = ron::de::from_bytes(source).unwrap();
            let grid = rasterize(&file);
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
                        "{id} ({x}, {y})"
                    );
                }
            }
            assert!(outline.home.is_some());
        }
    }
}
