//! Turns the geographic description (polygons and Gaussian blobs) into the
//! domain's cell grid. Pure and deterministic: the same file always gives the
//! same grid.

use domain::ids::CellCoord;
use domain::region::{Cell, TerrainGrid};
use domain::units::Ratio;

use super::dto::{Field, RegionFile};

/// Rasterises `file` onto its grid.
///
/// Each cell is sampled at its centre. A cell is land when its centre lies in
/// some `land` polygon and in no `exclude` polygon (even-odd rule). Each
/// resource layer is `base + sum(value * exp(-d^2 / (2 r^2)))` over its blobs,
/// where `d` is measured in degrees with the longitude difference scaled by
/// `cos(lat)`, then clamped to `[0, 1]`.
pub fn rasterize(file: &RegionFile) -> TerrainGrid {
    let GridDims { width, height } = dims(file);
    let mut cells = Vec::with_capacity(usize::from(width) * usize::from(height));
    for y in 0..height {
        for x in 0..width {
            let (lon, lat) = cell_centre(file, x, y);
            let land = file.land.iter().any(|p| contains(p, lon, lat))
                && !file.exclude.iter().any(|p| contains(p, lon, lat));
            cells.push(Cell {
                land,
                height: sample(&file.height, lon, lat),
                irradiance: sample(&file.irradiance, lon, lat),
                cooling_water: sample(&file.cooling_water, lon, lat),
                hydro_potential: sample(&file.hydro_potential, lon, lat),
            });
        }
    }
    TerrainGrid::new(width, height, cells).expect("one cell was pushed per grid position")
}

/// The cell containing the point, clamped to the grid.
pub fn to_cell(file: &RegionFile, lon: f64, lat: f64) -> CellCoord {
    let GridDims { width, height } = dims(file);
    let b = &file.bounds;
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

fn dims(file: &RegionFile) -> GridDims {
    GridDims {
        width: file.grid.width,
        height: file.grid.height,
    }
}

fn floor_clamped(value: f64, cells: u16) -> u16 {
    let last = f64::from(cells.saturating_sub(1));
    value.floor().clamp(0.0, last) as u16
}

fn cell_centre(file: &RegionFile, x: u16, y: u16) -> (f64, f64) {
    let b = &file.bounds;
    let lon = b.west + (f64::from(x) + 0.5) / f64::from(file.grid.width) * (b.east - b.west);
    let lat = b.north - (f64::from(y) + 0.5) / f64::from(file.grid.height) * (b.north - b.south);
    (lon, lat)
}

/// Even-odd ray casting against an implicitly closed ring.
fn contains(ring: &[(f64, f64)], px: f64, py: f64) -> bool {
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

    #[test]
    fn to_cell_maps_points_and_clamps_to_the_grid() {
        let file = base_file(); // 12 x 8 over lon 0..12, lat 0..8
        let at = |lon, lat| to_cell(&file, lon, lat);
        assert_eq!(at(0.5, 7.5), CellCoord { x: 0, y: 0 });
        assert_eq!(at(7.5, 4.5), CellCoord { x: 7, y: 3 });
        assert_eq!(at(11.99, 0.01), CellCoord { x: 11, y: 7 });
        // Cell centres round-trip.
        let (lon, lat) = cell_centre(&file, 9, 6);
        assert_eq!(at(lon, lat), CellCoord { x: 9, y: 6 });
        // Outside the bounds clamps to the nearest edge cell.
        assert_eq!(at(-50.0, 90.0), CellCoord { x: 0, y: 0 });
        assert_eq!(at(50.0, -90.0), CellCoord { x: 11, y: 7 });
        assert_eq!(at(12.0, 0.0), CellCoord { x: 11, y: 7 });
        assert_eq!(at(f64::NAN, f64::NAN), CellCoord { x: 0, y: 0 });
    }
}
