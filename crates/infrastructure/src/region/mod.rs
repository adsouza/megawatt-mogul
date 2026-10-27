//! Region data: RON files and their sidecars, bundled, parsed into DTOs,
//! rasterised into terrain and mapped through the domain's validating constructors.

mod bundle;
mod bundled;
pub use bundled::BundleRegionCatalog;
mod catalog;
mod dto;
pub mod legacy;
mod mapping;
mod outline;
mod raster;

pub use bundle::{RegionBundle, decode_region_bundle, encode_region_bundle};
pub use catalog::{RonRegionCatalog, load_region_bytes, parse_region_bundle};
pub use dto::*;
pub use mapping::{map_region, map_region_v4};
pub use outline::{Hemisphere, RegionOutline, region_outline};
pub use raster::{
    ZonePolygons, economic_zone_cells, economic_zone_cells_sidecar, format_cells_sidecar,
    rasterize, to_cell,
};

pub const CURRENT_REGION_SCHEMA: u32 = 4;
