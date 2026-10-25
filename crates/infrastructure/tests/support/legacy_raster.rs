// Frozen test-only raster oracle from 6968f09, before Plan 6.
// Preserve the historical f64 operation order on each target libm.
//! Turns the geographic description (polygons and Gaussian blobs) into the
//! domain's cell grid. Pure and deterministic: the same file always gives the
//! same grid.

use domain::region::{Cell, TerrainGrid};
use domain::units::Ratio;

use infrastructure::region::{Field, RegionFile};

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
                wind_potential: file
                    .wind_potential
                    .as_ref()
                    .map_or(Ratio::ZERO, |f| sample(f, lon, lat)),
                cooling_water: sample(&file.cooling_water, lon, lat),
                hydro_potential: sample(&file.hydro_potential, lon, lat),
            });
        }
    }
    TerrainGrid::new(width, height, cells).expect("one cell was pushed per grid position")
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

fn cell_centre(file: &RegionFile, x: u16, y: u16) -> (f64, f64) {
    let b = &file.bounds;
    let lon = b.west + (f64::from(x) + 0.5) / f64::from(file.grid.width) * (b.east - b.west);
    let lat = b.north - (f64::from(y) + 0.5) / f64::from(file.grid.height) * (b.north - b.south);
    (lon, lat)
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
