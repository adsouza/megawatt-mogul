//! Offline authoring only: rasterise economic-zone polygons into an economic-zone cells sidecar.
//! Usage: author_economic_zone_cells <region.ron> <zones.ron>
//! `<zones.ron>` is `(zones: [(id: 1, polygons: [[(lon, lat), ...]])])`, normally
//! `crates/infrastructure/authoring/<region>_economic_zones.ron`. Inputs are read-only; stdout is
//! the `(cells: [...])` sidecar the region's `economic_zones.cells_file` names (spec §28.11.1),
//! normally `assets/regions/<region>/economic_zone_cells.ron`. The region's geography includes
//! its geography sidecars; its current zone cells are ignored, so a stale or missing cells
//! sidecar never blocks regenerating it.
use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;

use domain::ids::RegionId;
use infrastructure::region::{
    ParsedRegionFile, RegionFileV4, RonRegionCatalog, ZonePolygons, economic_zone_cells,
    economic_zone_cells_sidecar,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ZoneAuthoring {
    zones: Vec<ZonePolygons>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: author_economic_zone_cells <region.ron> <zones.ron>".into());
    }
    let region_path = Path::new(&args[0]);
    let mut region: RegionFileV4 = ron::from_str(&std::fs::read_to_string(region_path)?)?;
    if let Some(zones) = &mut region.economic_zones {
        zones.cells_file = None;
        zones.cells.clear();
    }
    let catalog = RonRegionCatalog::new(region_path.parent().unwrap_or(Path::new(".")));
    let ParsedRegionFile::V4(region) =
        catalog.resolve(&RegionId::new(&region.id), ParsedRegionFile::V4(region))?
    else {
        return Err("a schema-4 region resolved to another schema".into());
    };
    let zones_source = std::fs::read(&args[1])?;
    let authoring: ZoneAuthoring = ron::from_str(std::str::from_utf8(&zones_source)?)?;
    let cells = economic_zone_cells(&region, &authoring.zones)?;

    let mut counts = BTreeMap::<u16, usize>::new();
    for id in &cells {
        *counts.entry(*id).or_default() += 1;
    }
    for zone in &authoring.zones {
        if !counts.contains_key(&zone.id) {
            eprintln!("warning: zone {} claims no playable cell", zone.id);
        }
    }
    eprintln!(
        "{}: {} cells; cells per zone (0 = regional default or not playable): {counts:?}",
        region.id,
        cells.len()
    );
    let name = |path: &str| {
        Path::new(path)
            .file_name()
            .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned())
    };
    let output = economic_zone_cells_sidecar(
        &name(&args[0]),
        &name(&args[1]),
        &zones_source,
        &cells,
        region.grid.width,
    );
    std::io::stdout().write_all(output.as_bytes())?;
    Ok(())
}
