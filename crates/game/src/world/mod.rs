//! Interactive tabletop diorama. Read-only presentation over simulation queries.
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

mod camera;
pub use camera::{CameraPose, OrbitCamera, physical_viewport};

mod flow;
pub use flow::{FlowArrow, FlowDirection};
mod lighting;
mod model;
pub use lighting::{SunPosition, sun_position};
mod overlay;
mod picking;
pub use model::{
    Part, WorldCity, WorldLine, WorldModel, WorldPlant, city_parts, plant_parts, selection_parts,
};
pub use overlay::{TuningConfig, WorldAction, WorldControls, pillar_part, supply_fraction};
pub use picking::{WorldHit, hit_world, pick_world, presented_hit, terrain_grid_hit, terrain_hit};

mod plugins;
mod render;
mod ui;
mod viewport_render;
pub use plugins::{CameraPlugin, DioramaCamera, OverlayPlugin, WorldViewPlugin};
pub use render::{ViewSync, WorldAssets, WorldEntity, WorldScene};
pub use ui::{
    MapSurface, ToolbarModel, draw_surface, label_rect, mark_blocked, screen_ndc, toolbar_model,
};

#[cfg(feature = "dev")]
mod dev;
#[cfg(feature = "dev")]
pub use dev::DevToolsPlugin;
