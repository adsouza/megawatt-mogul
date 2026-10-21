//! Region data: RON files parsed into DTOs, rasterised into terrain and mapped
//! through the domain's validating constructors.

mod catalog;
mod dto;
mod mapping;
mod outline;
mod raster;

pub use catalog::{RonRegionCatalog, load_region_bytes};
pub use dto::*;
pub use mapping::map_region;
pub use outline::{RegionOutline, region_outline};
pub use raster::{rasterize, to_cell};
