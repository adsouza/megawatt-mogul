//! Offline authoring only: rasterise economic-zone polygons into an `economic_zones.cells` block.
//! Usage: author_economic_zone_cells <region.ron> <zones.ron>
//! `<zones.ron>` is `(zones: [(id: 1, polygons: [[(lon, lat), ...]])])`, normally
//! `crates/infrastructure/authoring/<region>_economic_zones.ron`. Inputs are read-only; stdout is
//! reviewed before it replaces the `cells` member of the region's `economic_zones` block.
use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;

use infrastructure::region::{
    RegionFileV4, ZonePolygons, economic_zone_cells, format_economic_zone_cells,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

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
    let region: RegionFileV4 = ron::from_str(&std::fs::read_to_string(&args[0])?)?;
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
    let digest: String = Sha256::digest(&zones_source)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut output = format!(
        "        // Offline author_economic_zone_cells: {} with {}\n        // Zones SHA256: {digest}\n        // Cell-centre sampling; non-playable cells and cells in no zone are 0.\n",
        name(&args[0]),
        name(&args[1]),
    );
    output.push_str(&format_economic_zone_cells(&cells, region.grid.width));
    std::io::stdout().write_all(output.as_bytes())?;
    Ok(())
}
