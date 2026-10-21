//! Plan 3's CPU-side world geometry. Rendering/camera integration follows separately.
mod terrain;
pub use terrain::{GridTransform, TerrainGeometry, TerrainGeometryError, surface_height};

/// The existing resource-layer palette mapped onto the 3D geometry, from one region read.
pub fn region_terrain(
    region: &domain::region::RegionDefinition,
    layer: crate::hud::MapLayer,
) -> Result<TerrainGeometry, TerrainGeometryError> {
    let grid = region.terrain();
    let palette: Vec<_> = (0..grid.height())
        .flat_map(|y| {
            (0..grid.width()).map(move |x| {
                crate::hud::cell_color(layer, region, domain::ids::CellCoord { x, y })
            })
        })
        .collect();
    TerrainGeometry::build(grid, &palette)
}
