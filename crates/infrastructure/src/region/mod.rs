//! Region data: RON files parsed into DTOs, rasterised into terrain and mapped
//! through the domain's validating constructors.

mod catalog;
mod dto;
pub mod legacy;
mod mapping;
mod outline;
mod raster;

pub use catalog::{RonRegionCatalog, load_region_bytes};
pub use dto::*;
pub use mapping::{map_region, map_region_v4};
pub use outline::{RegionOutline, region_outline};
pub use raster::{rasterize, to_cell};

pub const CURRENT_REGION_SCHEMA: u32 = 4;
