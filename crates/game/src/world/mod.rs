//! Interactive tabletop diorama. Read-only presentation over simulation queries.
mod terrain;
pub use terrain::{GridTransform, TerrainGeometry, TerrainGeometryError, surface_height};

fn layer_palette(
    region: &domain::region::RegionDefinition,
    layer: crate::hud::MapLayer,
) -> Vec<[u8; 3]> {
    let grid = region.terrain();
    let mut palette = Vec::with_capacity(usize::from(grid.width()) * usize::from(grid.height()));
    for y in 0..grid.height() {
        for x in 0..grid.width() {
            palette.push(crate::hud::cell_color(
                layer,
                region,
                domain::ids::CellCoord { x, y },
            ));
        }
    }
    palette
}

/// The existing resource-layer palette mapped onto the 3D geometry, from one region read.
pub fn region_terrain(
    region: &domain::region::RegionDefinition,
    layer: crate::hud::MapLayer,
) -> Result<TerrainGeometry, TerrainGeometryError> {
    TerrainGeometry::build(region.terrain(), &layer_palette(region, layer))
}

/// Only the vertex colours of [`region_terrain`], in its vertex order. Changing the map layer
/// recolours the existing mesh (`geometry`, built from this region) with this instead of
/// rebuilding it.
pub fn region_terrain_colors(
    geometry: &TerrainGeometry,
    region: &domain::region::RegionDefinition,
    layer: crate::hud::MapLayer,
) -> Result<Vec<[f32; 4]>, TerrainGeometryError> {
    geometry.colors_of(&layer_palette(region, layer))
}

mod camera;
mod electrical;
mod focus;
pub use camera::{CameraPose, OrbitCamera, physical_viewport};

mod flow;
pub use flow::{FlowDirection, FlowPath, FlowPathKey, FlowPulse};
mod lighting;
mod model;
pub use lighting::{SunPosition, sun_position};
pub(crate) use model::line_breaks;
mod overlay;
pub(crate) use overlay::CameraFocus;
mod picking;
pub use model::{
    Part, WorldCity, WorldLine, WorldModel, WorldPlant, city_parts, plant_parts, selection_parts,
};
pub use overlay::{TuningConfig, WorldAction, WorldControls, supply_fraction};
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
