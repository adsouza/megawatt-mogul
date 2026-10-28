//! Turns the geographic description (polygons and Gaussian blobs) into the
//! domain's cell grid. Pure and deterministic: the same file always gives the
//! same grid.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use application::RegionLoadError;
use domain::ids::{CellCoord, RegionId};
use domain::region::{Cell, RegionError, TerrainGrid};
use domain::units::Ratio;
use serde::Deserialize;

use super::dto::{Bounds, Field, GridSize, IrradianceV4, RegionFile, RegionFileV4};

#[derive(Clone, Copy)]
pub(super) enum Irradiance<'a> {
    Gaussian(&'a Field),
    Cells(&'a [f64]),
}

/// Borrowed geometry shared by both source schemas.
#[derive(Clone, Copy)]
pub(super) struct Geography<'a> {
    pub bounds: &'a Bounds,
    pub grid: GridSize,
    pub land: &'a [Vec<(f64, f64)>],
    pub exclude: &'a [Vec<(f64, f64)>],
    pub height: &'a Field,
    pub irradiance: Irradiance<'a>,
    pub cooling_water: &'a Field,
    pub hydro_potential: &'a Field,
    pub wind_potential: Option<&'a Field>,
    pub geothermal_potential: Option<&'a Field>,
}
/// Rasterises `file` onto its grid.
///
/// Each cell is sampled at its centre. A cell is land when its centre lies in
/// some `land` polygon and in no `exclude` polygon (even-odd rule). Each
/// resource layer is `base + sum(value * exp(-d^2 / (2 r^2)))` over its blobs,
/// where `d` is measured in degrees with the longitude difference scaled by
/// `cos(lat)`, then clamped to `[0, 1]`.
pub fn rasterize(file: &RegionFile) -> TerrainGrid {
    rasterize_geometry(file.geography())
}

pub(super) fn rasterize_geometry(file: Geography<'_>) -> TerrainGrid {
    let GridDims { width, height } = dims(file);
    let land_index = PolygonIndex::new(file.land);
    let exclude_index = PolygonIndex::new(file.exclude);
    let mut cells = Vec::with_capacity(usize::from(width) * usize::from(height));
    for y in 0..height {
        for x in 0..width {
            let (lon, lat) = cell_centre(file, x, y);
            let land = land_index.contains(lon, lat) && !exclude_index.contains(lon, lat);
            cells.push(Cell {
                land,
                height: sample(file.height, lon, lat),
                irradiance: match file.irradiance {
                    Irradiance::Gaussian(field) => sample(field, lon, lat),
                    Irradiance::Cells(values) => {
                        Ratio::new(values[usize::from(y) * usize::from(width) + usize::from(x)])
                            .expect("mapping validates every irradiance cell")
                    }
                },
                wind_potential: file
                    .wind_potential
                    .map_or(Ratio::ZERO, |f| sample(f, lon, lat)),
                geothermal_potential: file
                    .geothermal_potential
                    .map_or(Ratio::ZERO, |f| sample(f, lon, lat)),
                cooling_water: sample(file.cooling_water, lon, lat),
                hydro_potential: sample(file.hydro_potential, lon, lat),
            });
        }
    }
    TerrainGrid::new(width, height, cells).expect("one cell was pushed per grid position")
}

/// The cell containing the point, clamped to the grid.
pub fn to_cell(file: &RegionFile, lon: f64, lat: f64) -> CellCoord {
    to_cell_geometry(file.geography(), lon, lat)
}

pub(super) fn to_cell_geometry(file: Geography<'_>, lon: f64, lat: f64) -> CellCoord {
    let GridDims { width, height } = dims(file);
    let b = file.bounds;
    // A NaN fraction saturates to 0 in the cast, so the result is always in range.
    let fx = (lon - b.west) / (b.east - b.west) * f64::from(width);
    let fy = (b.north - lat) / (b.north - b.south) * f64::from(height);
    CellCoord {
        x: floor_clamped(fx, width),
        y: floor_clamped(fy, height),
    }
}

struct GridDims {
    width: u16,
    height: u16,
}

fn dims(file: Geography<'_>) -> GridDims {
    GridDims {
        width: file.grid.width,
        height: file.grid.height,
    }
}

fn floor_clamped(value: f64, cells: u16) -> u16 {
    let last = f64::from(cells.saturating_sub(1));
    value.floor().clamp(0.0, last) as u16
}

fn cell_centre(file: Geography<'_>, x: u16, y: u16) -> (f64, f64) {
    let b = file.bounds;
    let lon = b.west + (f64::from(x) + 0.5) / f64::from(file.grid.width) * (b.east - b.west);
    let lat = b.north - (f64::from(y) + 0.5) / f64::from(file.grid.height) * (b.north - b.south);
    (lon, lat)
}

/// One economic zone's authored coverage, for offline authoring of `economic_zones.cells`.
/// A point belongs to the zone when it lies in any of `polygons` (even-odd per ring, as land).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZonePolygons {
    pub id: u16,
    pub polygons: Vec<Vec<(f64, f64)>>,
}

/// Rasterises zone polygons into `economic_zones.cells`, row-major from the northwest cell.
///
/// Each cell is sampled at its centre, as land is. A cell that is not playable in the terrain
/// the loader builds from `file` gets 0, as does a playable cell in no zone. A playable cell in
/// exactly one zone gets that zone's id; one in two zones is an `InvalidShape` error.
pub fn economic_zone_cells(
    file: &RegionFileV4,
    zones: &[ZonePolygons],
) -> Result<Vec<u16>, RegionLoadError> {
    let fail = |error| RegionLoadError::Invalid {
        region: RegionId::new(&file.id),
        error,
    };
    let invalid = |field| fail(RegionError::InvalidShape { field });
    super::mapping::check_v4_geography(file).map_err(fail)?;
    let mut ids = BTreeSet::new();
    for zone in zones {
        if zone.id == 0 || !ids.insert(zone.id) {
            return Err(invalid("economic_zones.id"));
        }
        if zone
            .polygons
            .iter()
            .flatten()
            .any(|(lon, lat)| !lon.is_finite() || !lat.is_finite())
        {
            return Err(invalid("economic_zones.polygons"));
        }
    }
    let geography = file.geography();
    let terrain = rasterize_geometry(geography);
    let GridDims { width, height } = dims(geography);
    let mut cells = Vec::with_capacity(usize::from(width) * usize::from(height));
    for y in 0..height {
        for x in 0..width {
            if !terrain
                .get(CellCoord { x, y })
                .is_some_and(|cell| cell.land)
            {
                cells.push(0);
                continue;
            }
            let (lon, lat) = cell_centre(geography, x, y);
            let mut claims = zones
                .iter()
                .filter(|zone| zone.polygons.iter().any(|ring| contains(ring, lon, lat)));
            cells.push(match (claims.next(), claims.next()) {
                (None, _) => 0,
                (Some(zone), None) => zone.id,
                (Some(_), Some(_)) => return Err(invalid("economic_zones.cells")),
            });
        }
    }
    Ok(cells)
}

/// A `(cells: [...])` sidecar (spec §28.11.1): each `header` line as a comment, then the
/// row-major values with a comment before each grid row and at most `per_line` values per line.
/// Values print with `Debug`, which keeps every digit of an `f64`.
pub fn format_cells_sidecar<T: std::fmt::Debug>(
    header: &[String],
    cells: &[T],
    width: u16,
    per_line: usize,
) -> String {
    let mut out = String::new();
    for line in header {
        let _ = writeln!(out, "// {line}");
    }
    out.push_str("(cells: [\n");
    for (y, row) in cells.chunks(usize::from(width.max(1))).enumerate() {
        let _ = writeln!(out, "    // Row {y}");
        for line in row.chunks(per_line.max(1)) {
            out.push_str("    ");
            for value in line {
                let _ = write!(out, "{value:?}, ");
            }
            out.pop();
            out.push('\n');
        }
    }
    out.push_str("])\n");
    out
}

/// The `economic_zones` cells sidecar, with the provenance header `author_economic_zone_cells`
/// writes: the two input file names and the SHA-256 of the zone polygons' source.
pub fn economic_zone_cells_sidecar(
    region_file: &str,
    zones_file: &str,
    zones_source: &[u8],
    cells: &[u16],
    width: u16,
) -> String {
    use sha2::{Digest, Sha256};
    let digest: String = Sha256::digest(zones_source)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let header = [
        format!("Offline author_economic_zone_cells: {region_file} with {zones_file}"),
        format!("Zones SHA256: {digest}"),
        "Cell-centre sampling; non-playable cells and cells in no zone are 0.".to_owned(),
    ];
    format_cells_sidecar(&header, cells, width, 20)
}

/// Even-odd ray casting against an implicitly closed ring.
pub(super) fn contains(ring: &[(f64, f64)], px: f64, py: f64) -> bool {
    let mut inside = false;
    let mut prev = match ring.last() {
        Some(&p) => p,
        None => return false,
    };
    for &(x, y) in ring {
        let (px_prev, py_prev) = prev;
        if (y > py) != (py_prev > py) && px < (px_prev - x) * (py - y) / (py_prev - y) + x {
            inside = !inside;
        }
        prev = (x, y);
    }
    inside
}

fn sample(field: &Field, lon: f64, lat: f64) -> Ratio {
    let cos_lat = lat.to_radians().cos();
    let blobs: f64 = field
        .blobs
        .iter()
        .map(|b| {
            let dx = (lon - b.lon) * cos_lat;
            let dy = lat - b.lat;
            b.value * (-(dx * dx + dy * dy) / (2.0 * b.radius_deg * b.radius_deg)).exp()
        })
        .sum();
    // `mapping` rejects non-finite input; a NaN that slips through reads as 0.
    Ratio::new((field.base + blobs).clamp(0.0, 1.0)).unwrap_or(Ratio::ZERO)
}

impl RegionFile {
    pub(super) fn geography(&self) -> Geography<'_> {
        Geography {
            bounds: &self.bounds,
            grid: self.grid,
            land: &self.land,
            exclude: &self.exclude,
            height: &self.height,
            irradiance: Irradiance::Gaussian(&self.irradiance),
            cooling_water: &self.cooling_water,
            hydro_potential: &self.hydro_potential,
            wind_potential: self.wind_potential.as_ref(),
            geothermal_potential: None,
        }
    }
}

impl RegionFileV4 {
    pub(super) fn geography(&self) -> Geography<'_> {
        Geography {
            bounds: &self.bounds,
            grid: self.grid,
            land: &self.land,
            exclude: &self.exclude,
            height: &self.height,
            irradiance: match &self.irradiance {
                IrradianceV4::Gaussian(field) => Irradiance::Gaussian(field),
                IrradianceV4::Cells(values) => Irradiance::Cells(values),
                // Unresolved: mapping rejects the empty list as `irradiance.cells`.
                IrradianceV4::CellsFile(_) => Irradiance::Cells(&[]),
            },
            cooling_water: &self.cooling_water,
            hydro_potential: &self.hydro_potential,
            wind_potential: Some(&self.wind_potential),
            geothermal_potential: self.geothermal_potential.as_ref(),
        }
    }
}

/// Cache ring extents once; detailed shared area borders otherwise scan every province
/// for every cell, even when the point lies thousands of kilometres away.
type BoundedRing<'a> = (&'a [(f64, f64)], [f64; 4]);
pub(super) struct PolygonIndex<'a> {
    rings: Vec<BoundedRing<'a>>,
}
impl<'a> PolygonIndex<'a> {
    pub(super) fn new(rings: &'a [Vec<(f64, f64)>]) -> Self {
        Self {
            rings: rings
                .iter()
                .map(|r| {
                    let bounds = r.iter().fold(
                        [
                            f64::INFINITY,
                            f64::NEG_INFINITY,
                            f64::INFINITY,
                            f64::NEG_INFINITY,
                        ],
                        |b, (x, y)| [b[0].min(*x), b[1].max(*x), b[2].min(*y), b[3].max(*y)],
                    );
                    (r.as_slice(), bounds)
                })
                .collect(),
        }
    }
    pub(super) fn contains(&self, x: f64, y: f64) -> bool {
        self.rings.iter().any(|(ring, b)| {
            x >= b[0] && x <= b[1] && y >= b[2] && y <= b[3] && contains(ring, x, y)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_file() -> RegionFile {
        ron::de::from_str(include_str!("../../tests/support/minimal_region.ron"))
            .expect("minimal region parses")
    }

    /// A 10 x 10 grid over 0..10 degrees, with one land square from 2 to 8.
    fn square_file() -> RegionFile {
        let mut file = base_file();
        file.bounds.west = 0.0;
        file.bounds.east = 10.0;
        file.bounds.south = 0.0;
        file.bounds.north = 10.0;
        file.grid.width = 10;
        file.grid.height = 10;
        file.land = vec![vec![(2.0, 2.0), (8.0, 2.0), (8.0, 8.0), (2.0, 8.0)]];
        file.exclude = vec![];
        file
    }

    fn land_at(grid: &TerrainGrid, x: u16, y: u16) -> bool {
        grid.get(CellCoord { x, y }).unwrap().land
    }

    #[test]
    fn square_polygon_marks_inner_cells_as_land() {
        let grid = rasterize(&square_file());
        for y in 0..10 {
            for x in 0..10 {
                let inside = (2..8).contains(&x) && (2..8).contains(&y);
                assert_eq!(land_at(&grid, x, y), inside, "cell ({x}, {y})");
            }
        }
    }

    #[test]
    fn exclude_polygon_cuts_a_hole() {
        let mut file = square_file();
        file.exclude = vec![vec![(4.0, 4.0), (6.0, 4.0), (6.0, 6.0), (4.0, 6.0)]];
        let grid = rasterize(&file);
        for y in 0..10 {
            for x in 0..10 {
                let inside = (2..8).contains(&x) && (2..8).contains(&y);
                let hole = (4..6).contains(&x) && (4..6).contains(&y);
                assert_eq!(land_at(&grid, x, y), inside && !hole, "cell ({x}, {y})");
            }
        }
    }

    #[test]
    fn concave_polygons_follow_the_even_odd_rule() {
        let mut file = square_file();
        // A "C" opening to the east: the notch between x = 5 and 8 is water.
        file.land = vec![vec![
            (2.0, 2.0),
            (8.0, 2.0),
            (8.0, 4.0),
            (5.0, 4.0),
            (5.0, 6.0),
            (8.0, 6.0),
            (8.0, 8.0),
            (2.0, 8.0),
        ]];
        let grid = rasterize(&file);
        // Cell centre (6.5, 4.5) -> x = 6, y = 5 (lat = 10 - 5.5) sits in the notch.
        assert!(!land_at(&grid, 6, 5));
        assert!(land_at(&grid, 3, 5));
        assert!(land_at(&grid, 6, 7));
        assert!(land_at(&grid, 6, 2));
    }

    #[test]
    fn blob_peaks_at_its_centre() {
        // The centre of cell (5, 5) is lon 5.5, lat 4.5.
        let mut file = square_file();
        file.irradiance.base = 0.2;
        file.irradiance.blobs = vec![super::super::dto::Blob {
            lon: 5.5,
            lat: 4.5,
            radius_deg: 1.0,
            value: 0.5,
        }];
        let at = CellCoord { x: 5, y: 5 };
        let peak = rasterize(&file).get(at).unwrap().irradiance.value();
        assert!((peak - 0.7).abs() < 1e-9, "peak was {peak}");

        file.irradiance.blobs[0].value = 2.0;
        let clamped = rasterize(&file).get(at).unwrap().irradiance.value();
        assert_eq!(clamped, 1.0);

        // Far from the blob (about 7 degrees) the field falls back to its base.
        let far = rasterize(&file)
            .get(CellCoord { x: 0, y: 9 })
            .unwrap()
            .irradiance
            .value();
        assert!((far - 0.2).abs() < 1e-6, "far was {far}");
    }

    #[test]
    fn blob_distance_scales_longitude_by_cos_latitude() {
        let mut file = base_file();
        file.bounds.west = 0.0;
        file.bounds.east = 10.0;
        file.bounds.south = 55.0;
        file.bounds.north = 65.0;
        file.grid.width = 10;
        file.grid.height = 10;
        // Cell (5, 5): lon 5.5, lat 59.5. Put the blob one degree of longitude west of it.
        file.hydro_potential.base = 0.0;
        file.hydro_potential.blobs = vec![super::super::dto::Blob {
            lon: 4.5,
            lat: 59.5,
            radius_deg: 1.0,
            value: 0.9,
        }];
        let got = rasterize(&file)
            .get(CellCoord { x: 5, y: 5 })
            .unwrap()
            .hydro_potential
            .value();
        let dx = 59.5_f64.to_radians().cos();
        let want = 0.9 * (-(dx * dx) / 2.0).exp();
        assert!((got - want).abs() < 1e-12, "got {got}, want {want}");
        // Without the cosine the falloff would be much steeper.
        assert!(got > 0.9 * (-0.5_f64).exp());
    }

    #[test]
    fn rasterising_is_deterministic() {
        let file = base_file();
        assert_eq!(rasterize(&file), rasterize(&file));
    }

    /// `square_file`'s geography in schema 4: 10 x 10 over 0..10 degrees, land 2..8.
    fn square_file_v4() -> RegionFileV4 {
        let mut file: RegionFileV4 =
            ron::de::from_str(include_str!("../../tests/support/minimal_region_v4.ron"))
                .expect("minimal v4 region parses");
        file.bounds.west = 0.0;
        file.bounds.east = 10.0;
        file.bounds.south = 0.0;
        file.bounds.north = 10.0;
        file.grid.width = 10;
        file.grid.height = 10;
        file.land = vec![vec![(2.0, 2.0), (8.0, 2.0), (8.0, 8.0), (2.0, 8.0)]];
        file.exclude = vec![];
        file
    }

    fn zone(id: u16, west: f64, east: f64, south: f64, north: f64) -> ZonePolygons {
        ZonePolygons {
            id,
            polygons: vec![vec![
                (west, south),
                (east, south),
                (east, north),
                (west, north),
            ]],
        }
    }

    fn zone_at(cells: &[u16], x: usize, y: usize) -> u16 {
        cells[y * 10 + x]
    }

    fn invalid_field(result: Result<Vec<u16>, RegionLoadError>) -> &'static str {
        match result {
            Err(RegionLoadError::Invalid {
                error: RegionError::InvalidShape { field },
                ..
            }) => field,
            other => panic!("expected InvalidShape, got {other:?}"),
        }
    }

    #[test]
    fn zone_cells_follow_cell_centres() {
        // The split at lon 4.6 and lat 5.4 is off the cell boundaries, so only the
        // centre convention decides: x = 4 (centre 4.5) is west, y = 4 (centre 5.5) is north.
        let zones = [
            zone(1, 0.0, 4.6, 5.4, 10.0),
            zone(2, 4.6, 10.0, 5.4, 10.0),
            zone(3, 0.0, 10.0, 0.0, 5.4),
        ];
        let cells = economic_zone_cells(&square_file_v4(), &zones).unwrap();
        assert_eq!(cells.len(), 100);
        for y in 2..8 {
            for x in 2..8 {
                let want = match (x, y) {
                    (_, 5..) => 3,
                    (..=4, _) => 1,
                    _ => 2,
                };
                assert_eq!(zone_at(&cells, x, y), want, "cell ({x}, {y})");
            }
        }
    }

    #[test]
    fn zone_cells_are_zero_off_land_and_outside_every_zone() {
        let mut file = square_file_v4();
        file.exclude = vec![vec![(4.0, 4.0), (6.0, 4.0), (6.0, 6.0), (4.0, 6.0)]];
        // Zone 7 covers the whole map but only land outside the hole may take it. Zone 9 covers
        // only water, and the land strip x = 7 lies in no zone.
        let zones = [
            zone(7, -1.0, 7.0, -1.0, 11.0),
            zone(9, 8.0, 11.0, -1.0, 11.0),
        ];
        let cells = economic_zone_cells(&file, &zones).unwrap();
        let terrain = rasterize_geometry(file.geography());
        for y in 0..10 {
            for x in 0..10 {
                let land = terrain
                    .get(CellCoord {
                        x: x as u16,
                        y: y as u16,
                    })
                    .unwrap()
                    .land;
                let want = if land && x < 7 { 7 } else { 0 };
                assert_eq!(zone_at(&cells, x, y), want, "cell ({x}, {y})");
            }
        }
        assert_eq!(
            zone_at(&cells, 4, 4),
            0,
            "the excluded hole is not playable"
        );
        assert_eq!(zone_at(&cells, 7, 5), 0, "playable but in no zone");
        assert!(
            economic_zone_cells(&file, &[])
                .unwrap()
                .iter()
                .all(|&id| id == 0)
        );
    }

    #[test]
    fn a_cell_claimed_by_two_zones_is_rejected() {
        let file = square_file_v4();
        let overlap = [zone(1, 0.0, 5.0, 0.0, 10.0), zone(2, 4.0, 10.0, 0.0, 10.0)];
        assert_eq!(
            invalid_field(economic_zone_cells(&file, &overlap)),
            "economic_zones.cells"
        );
        // An overlap over water only is harmless: those cells use the regional default.
        let wet = [zone(1, 0.0, 2.0, 0.0, 10.0), zone(2, 1.0, 10.0, 0.0, 10.0)];
        let cells = economic_zone_cells(&file, &wet).unwrap();
        assert_eq!(zone_at(&cells, 1, 5), 0);
        assert_eq!(zone_at(&cells, 2, 5), 2);
        // The reserved default and repeated IDs cannot be authored.
        for zones in [
            vec![zone(0, 0.0, 10.0, 0.0, 10.0)],
            vec![zone(4, 0.0, 3.0, 0.0, 10.0), zone(4, 6.0, 10.0, 0.0, 10.0)],
        ] {
            assert_eq!(
                invalid_field(economic_zone_cells(&file, &zones)),
                "economic_zones.id"
            );
        }
        let mut broken = file.clone();
        broken.grid.width = 0;
        assert_eq!(invalid_field(economic_zone_cells(&broken, &wet)), "grid");
    }

    #[test]
    fn cell_sidecars_have_a_header_a_row_comment_and_wrapped_values() {
        use crate::region::CellsSidecar;
        let cells: Vec<u16> = (0..50).map(|i| i % 3).collect();
        let text = format_cells_sidecar(&["Offline test".to_owned()], &cells, 25, 20);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "// Offline test");
        assert_eq!(lines[1], "(cells: [");
        assert_eq!(lines[2], "    // Row 0");
        assert_eq!(lines[3].matches(',').count(), 20);
        assert_eq!(lines[4].matches(',').count(), 5);
        assert_eq!(lines[5], "    // Row 1");
        assert!(text.ends_with("\n])\n"));
        let parsed: CellsSidecar<u16> = ron::de::from_str(&text).unwrap();
        assert_eq!(parsed.cells, cells);

        let values = [0.1, 1.0 / 3.0, 0.7405953153490916];
        let text = format_cells_sidecar(&[], &values, 3, 5);
        assert!(text.contains("    0.1, 0.3333333333333333, 0.7405953153490916,\n"));
        let parsed: CellsSidecar<f64> = ron::de::from_str(&text).unwrap();
        assert_eq!(parsed.cells, values);
    }

    #[test]
    fn zone_cell_sidecars_record_their_inputs() {
        let text = economic_zone_cells_sidecar("r.ron", "z.ron", b"zones", &[0, 1, 2, 3], 2);
        let lines: Vec<&str> = text.lines().take(4).collect();
        assert_eq!(
            lines,
            [
                "// Offline author_economic_zone_cells: r.ron with z.ron",
                // `printf zones | shasum -a 256`
                "// Zones SHA256: 8c279b58e430cfbe2c032ed02222c60e243a210109aca37212503abf05bfb192",
                "// Cell-centre sampling; non-playable cells and cells in no zone are 0.",
                "(cells: [",
            ]
        );
    }

    #[test]
    fn to_cell_maps_points_and_clamps_to_the_grid() {
        let file = base_file(); // 12 x 8 over lon 0..12, lat 0..8
        let at = |lon, lat| to_cell(&file, lon, lat);
        assert_eq!(at(0.5, 7.5), CellCoord { x: 0, y: 0 });
        assert_eq!(at(7.5, 4.5), CellCoord { x: 7, y: 3 });
        assert_eq!(at(11.99, 0.01), CellCoord { x: 11, y: 7 });
        // Cell centres round-trip.
        let (lon, lat) = cell_centre(file.geography(), 9, 6);
        assert_eq!(at(lon, lat), CellCoord { x: 9, y: 6 });
        // Outside the bounds clamps to the nearest edge cell.
        assert_eq!(at(-50.0, 90.0), CellCoord { x: 0, y: 0 });
        assert_eq!(at(50.0, -90.0), CellCoord { x: 11, y: 7 });
        assert_eq!(at(12.0, 0.0), CellCoord { x: 11, y: 7 });
        assert_eq!(at(f64::NAN, f64::NAN), CellCoord { x: 0, y: 0 });
    }
}
