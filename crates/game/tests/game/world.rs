//! Plan 3's CPU geometry contract: no window, renderer or simulation mutation.
use bevy::math::Vec3;
use domain::{
    ids::CellCoord,
    region::{Cell, TerrainGrid},
    units::Ratio,
};
use game::world::{GridTransform, TerrainGeometry, surface_height};
use proptest::prelude::*;

fn grid(width: u16, height: u16, heights: &[f64]) -> TerrainGrid {
    let cells = heights
        .iter()
        .map(|height| Cell {
            land: *height >= 0.0,
            height: Ratio::new(height.max(0.0)).unwrap(),
            irradiance: Ratio::ZERO,
            cooling_water: Ratio::ZERO,
            hydro_potential: Ratio::ZERO,
        })
        .collect();
    TerrainGrid::new(width, height, cells).unwrap()
}
fn valid(mesh: &TerrainGeometry) {
    assert_eq!(mesh.positions.len(), mesh.normals.len());
    assert_eq!(mesh.positions.len(), mesh.colors.len());
    assert_eq!(mesh.indices.len() % 3, 0);
    for (positions, normals) in mesh.positions.iter().zip(&mesh.normals) {
        assert!(positions.iter().all(|v| v.is_finite()));
        assert!((Vec3::from_array(*normals).length() - 1.0).abs() < 1e-6);
    }
    for face in mesh.indices.as_chunks::<3>().0 {
        let p: Vec<Vec3> = face
            .iter()
            .map(|index| Vec3::from_array(mesh.positions[*index as usize]))
            .collect();
        let normal = (p[1] - p[0]).cross(p[2] - p[0]);
        // Adjacent shipped cells can differ by a single f32 ULP: a tiny vertical skirt is
        // still a real face. Reject zero area rather than imposing an arbitrary visual scale.
        assert!(normal.is_finite() && normal.length_squared() > 0.0, "{p:?}");
        assert!(
            normal
                .normalize()
                .dot(Vec3::from_array(mesh.normals[face[0] as usize]))
                > 0.999
        );
    }
}

#[test]
fn world_flat_terrain_has_complete_tops_a_closed_base_and_consistent_normals() {
    let terrain = grid(2, 2, &[0.0, 0.0, 0.0, 0.0]);
    let mesh = TerrainGeometry::build(&terrain, &[[64, 128, 70]; 4]).unwrap();
    valid(&mesh);
    // Four cell tops, eight outer side quads, one bottom. No internal coincident walls.
    assert_eq!(mesh.indices.len(), 13 * 6);
    let grid = GridTransform::new(2, 2);
    assert_eq!(grid.cell_at(-1.0, -1.0), Some(CellCoord { x: 0, y: 0 }));
    assert_eq!(grid.cell_at(1.0, 0.0), None);
    assert_eq!(grid.cell_at(0.0, 1.0), None);
    assert_eq!(grid.cell_at(f32::NAN, 0.0), None);
}
#[test]
fn world_height_and_water_are_shared_by_meshes_and_object_placement() {
    let terrain = grid(2, 1, &[-1.0, 1.0]);
    let mesh = TerrainGeometry::build(&terrain, &[[28, 52, 96], [150, 130, 100]]).unwrap();
    valid(&mesh);
    let transform = GridTransform::new(2, 1);
    for x in 0..2 {
        let cell = CellCoord { x, y: 0 };
        let center = transform.cell_center(&terrain, cell).unwrap();
        assert_eq!(transform.cell_at(center.x, center.z), Some(cell));
        assert_eq!(center.y, surface_height(&terrain, cell).unwrap());
        assert!(mesh.positions.iter().any(|p| p[1] == center.y));
    }
    assert_eq!(
        surface_height(&terrain, CellCoord { x: 0, y: 0 }),
        Some(0.0)
    );
    assert!(surface_height(&terrain, CellCoord { x: 1, y: 0 }).unwrap() > 1.0);
    assert!(TerrainGeometry::build(&terrain, &[]).is_err());
    assert!(GridTransform::new(0, 1).cell_at(0.0, 0.0).is_none());
    let empty = TerrainGrid::new(0, 0, vec![]).unwrap();
    assert!(
        TerrainGeometry::build(&empty, &[])
            .unwrap()
            .positions
            .is_empty()
    );
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn world_geometry_is_finite_complete_and_deterministic(width in 1u16..9,height in 1u16..9, values in prop::collection::vec(0u16..1001,64)) {
        let elevations:Vec<_> = values[..usize::from(width)*usize::from(height)].iter().map(|v|f64::from(*v)/1000.0).collect();
        let terrain = grid(width,height,&elevations);
        let colors = vec![[70,170,90];elevations.len()];
        let mesh = TerrainGeometry::build(&terrain,&colors).unwrap(); valid(&mesh);
        prop_assert_eq!(&mesh,&TerrainGeometry::build(&terrain,&colors).unwrap());
        let coordinates = GridTransform::new(width,height);
        for y in 0..height { for x in 0..width {
            let cell = CellCoord{x,y}; let at = coordinates.cell_center(&terrain,cell).unwrap();
            prop_assert_eq!(coordinates.cell_at(at.x,at.z),Some(cell));
            let tops = mesh.normals.iter().filter(|n|**n==[0.0,1.0,0.0]).count();
            prop_assert_eq!(tops,usize::from(width)*usize::from(height)*4);
        }}
    }
}

#[test]
fn world_both_shipped_regions_build_with_existing_resource_layers_without_changing_geometry() {
    use application::RegionCatalog;
    use domain::ids::RegionId;
    let catalog = infrastructure::region::RonRegionCatalog::new(
        crate::support::shipped_assets().join("regions"),
    );
    for id in ["eastern_interconnection", "western_interconnection"] {
        let region = catalog.load(&RegionId::new(id)).unwrap();
        let terrain =
            game::world::region_terrain(&region.definition, game::hud::MapLayer::Terrain).unwrap();
        valid(&terrain);
        let irradiance =
            game::world::region_terrain(&region.definition, game::hud::MapLayer::Irradiance)
                .unwrap();
        assert_eq!(terrain.positions, irradiance.positions);
        assert_eq!(terrain.normals, irradiance.normals);
        assert_eq!(terrain.indices, irradiance.indices);
        assert_ne!(terrain.colors, irradiance.colors);
    }
}

#[test]
fn world_viewport_uses_egui_scale_clips_and_rejects_zero() {
    use bevy_egui::egui::{Rect, pos2};
    use game::world::physical_viewport;
    let r = Rect::from_min_max(pos2(-1.0, 10.2), pos2(300.3, 210.8));
    let v = physical_viewport(r, 1.25, bevy::prelude::UVec2::new(350, 250)).unwrap();
    assert_eq!(v.physical_position, bevy::prelude::UVec2::new(0, 13));
    assert_eq!(v.physical_size, bevy::prelude::UVec2::new(350, 237));
    assert!(physical_viewport(r, 0.0, bevy::prelude::UVec2::splat(500)).is_none());
    assert!(physical_viewport(r, 1.0, bevy::prelude::UVec2::ZERO).is_none());
    assert!(physical_viewport(Rect::NOTHING, 1.0, bevy::prelude::UVec2::splat(500)).is_none());
}

#[test]
fn world_camera_fits_both_regions_and_projects_ray_roundtrips() {
    use bevy::prelude::*;
    use game::world::OrbitCamera;
    for size in [Vec2::new(100.0, 60.0), Vec2::new(110.0, 70.0)] {
        for aspect in [0.7, 1.6, 3.0] {
            let pose = OrbitCamera::default().pose(size, aspect);
            for x in [-size.x / 2.0, size.x / 2.0] {
                for z in [-size.y / 2.0, size.y / 2.0] {
                    let point = Vec3::new(x, 2.2, z);
                    let ndc = pose.project(point).unwrap();
                    assert!(ndc.x.abs() < 1.0 && ndc.y.abs() < 1.0, "{ndc:?}");
                    let ray = pose.ray(ndc.truncate()).unwrap();
                    assert!(ray.direction.cross(point - ray.origin).length() < 0.002);
                }
            }
        }
    }
}

proptest! {
    #[test]
    fn world_extreme_camera_inputs_stay_finite(dx in -1e6f32..1e6, dy in -1e6f32..1e6, zoom in -1e6f32..1e6) {
        use game::world::OrbitCamera;
        use bevy::prelude::*;
        let mut orbit = OrbitCamera::default();
        for _ in 0..20 { orbit.orbit(Vec2::new(dx,dy)); orbit.pan(Vec2::new(dx,dy),Vec2::new(100.0,60.0),400.0); orbit.zoom(zoom); }
        let pose = orbit.pose(Vec2::new(100.0,60.0),1.0);
        prop_assert!(pose.eye.is_finite() && pose.target.is_finite());
        prop_assert!(pose.eye.y > 3.0);
        prop_assert!(orbit.zoom >= 0.08 && orbit.zoom <= 3.0);
    }
}

#[test]
fn world_camera_can_zoom_close_enough_to_inspect_cell_sized_models() {
    let size = bevy::math::Vec2::new(100.0, 60.0);
    let mut camera = game::world::OrbitCamera {
        zoom: 0.35,
        ..Default::default()
    };
    let before = camera.pose(size, 1.0);
    camera.zoom(2000.0);
    assert_eq!(camera.zoom, 0.08);
    let close = camera.pose(size, 1.0);
    assert!(close.eye.distance(close.target) < before.eye.distance(before.target) / 3.0);
}

#[test]
fn world_city_and_plant_models_leave_a_margin_inside_their_cell() {
    use domain::{ids::RegionId, region::PlantKind, session::GameSettings};
    use game::world::{TuningConfig, WorldModel, city_parts, plant_parts};
    let fits = |parts: &[game::world::Part], scale: f32| {
        for part in parts {
            for x in [-0.5, 0.5] {
                for y in [-0.5, 0.5] {
                    for z in [-0.5, 0.5] {
                        let corner = (part.center
                            + part.rotation * (part.size * Vec3::new(x, y, z)))
                            * scale;
                        assert!(
                            corner.x.abs() <= 0.40001 && corner.z.abs() <= 0.40001,
                            "model crosses the cell margin: {corner:?}"
                        );
                    }
                }
            }
        }
    };
    // Check defaults and the largest allowed developer scale, including invalid input fallback.
    for raw in [1.0, 2.0, 100.0, f32::NAN, f32::INFINITY] {
        let tuning = TuningConfig {
            model_scale: raw,
            ..Default::default()
        };
        for kind in PlantKind::ALL {
            fits(&plant_parts(kind), tuning.scale());
        }
    }
    let catalog = infrastructure::region::RonRegionCatalog::new(
        crate::support::shipped_assets().join("regions"),
    );
    for id in ["eastern_interconnection", "western_interconnection"] {
        let game = application::Game::start_new(
            &catalog,
            &RegionId::new(id),
            7,
            GameSettings::default(),
            Box::new(application::InMemoryCommandLog::default()),
        )
        .unwrap();
        let checkpoint = game.checkpoint();
        let model = WorldModel::of(&game);
        for city in model.cities.values() {
            fits(&city_parts(), city.scale);
        }
        assert_eq!(game.checkpoint(), checkpoint);
    }
}

proptest! {
    #[test]
    fn world_model_footprints_stay_bounded_at_any_visual_scale(requested in any::<f32>(), city_size in 0.6f32..=1.0) {
        use game::world::{TuningConfig, city_parts, plant_parts, selection_parts};
        let scale = TuningConfig { model_scale: requested, ..Default::default() }.scale();
        for (parts, factor, radius) in domain::region::PlantKind::ALL.into_iter()
            .map(|kind| (plant_parts(kind), scale, 0.4))
            .chain([(city_parts(), city_size * scale, 0.4), (selection_parts().to_vec(), scale, 0.45)]) {
            for part in parts {
                for x in [-0.5, 0.5] { for y in [-0.5, 0.5] { for z in [-0.5, 0.5] {
                    let corner = (part.center + part.rotation * (part.size * Vec3::new(x, y, z))) * factor;
                    prop_assert!(corner.is_finite());
                    prop_assert!(corner.x.abs() <= radius + 1e-5 && corner.z.abs() <= radius + 1e-5);
                } } }
            }
        }
    }
}

#[test]
fn world_model_preserves_identity_demand_sizes_and_simulation_checkpoint() {
    use domain::region::PlantKind;
    use game::world::{WorldModel, plant_parts};
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = crate::support::hud_app(logs.path());
    crate::support::request(&mut app, "eastern_interconnection");
    let game = &app.world().resource::<game::sim::Session>().0;
    let checkpoint = game.checkpoint();
    let model = WorldModel::of(game);
    assert_eq!(model.plants.len(), game.session().plant_views().len());
    assert_eq!(model.cities.len(), game.session().city_views().len());
    assert_eq!(model.lines.len(), game.session().line_views().len());
    assert!(
        model
            .cities
            .values()
            .all(|c| c.scale.is_finite() && c.scale > 0.0)
    );
    let shapes: Vec<_> = PlantKind::ALL.into_iter().map(plant_parts).collect();
    for (i, shape) in shapes.iter().enumerate() {
        for other in &shapes[i + 1..] {
            assert_ne!(shape, other);
        }
    }
    assert_eq!(game.checkpoint(), checkpoint);
}

#[test]
fn world_picking_uses_elevated_terrain_and_object_bounds_and_rejects_sky() {
    use bevy::prelude::*;
    use game::hud::{MapLayer, Selection};
    use game::world::{OrbitCamera, WorldModel, pick_world, region_terrain};
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = crate::support::hud_app(logs.path());
    crate::support::request(&mut app, "western_interconnection");
    let game = &app.world().resource::<game::sim::Session>().0;
    let mut model = WorldModel::of(game);
    model.cities.clear();
    model.lines.clear();
    let geometry = region_terrain(&model.region, MapLayer::Terrain).unwrap();
    let pose = OrbitCamera::default().pose(model.size(), 1.6);
    for plant in model.plants.values() {
        let point = plant.at + Vec3::Y * 0.4;
        let ray = pose.ray(pose.project(point).unwrap().truncate()).unwrap();
        assert_eq!(
            pick_world(ray, &model, &geometry, 1.0),
            Selection::Plant(plant.id)
        );
    }
    let ray = Ray3d::new(Vec3::new(0.0, 20.0, 0.0), Dir3::Y);
    assert_eq!(pick_world(ray, &model, &geometry, 1.0), Selection::None);
}

fn diorama_app(logs: &std::path::Path) -> bevy::prelude::App {
    let mut app = crate::support::hud_app(logs);
    app.add_plugins(game::world::WorldViewPlugin);
    app
}

#[test]
fn world_rendered_meshes_and_picking_share_the_fitted_footprints() {
    use bevy::prelude::*;
    use game::{
        hud::Selection,
        sim::Session,
        world::{TuningConfig, ViewSync, WorldControls, WorldScene, presented_hit},
    };
    let logs = tempfile::TempDir::new().unwrap();
    for region in ["eastern_interconnection", "western_interconnection"] {
        let mut app = diorama_app(&logs.path().join(region));
        crate::support::request(&mut app, region);
        let checkpoint = app.world().resource::<Session>().0.checkpoint();
        for requested in [0.4, 1.0, 2.0] {
            app.world_mut().resource_mut::<TuningConfig>().model_scale = requested;
            app.update();
            let world = app.world();
            let model = world.resource::<WorldScene>().model.as_ref().unwrap();
            let views = world.resource::<ViewSync>();
            for (entity, at) in views
                .plants
                .iter()
                .map(|(id, entity)| (entity, model.plants[id].at))
                .chain(
                    views
                        .cities
                        .iter()
                        .map(|(id, entity)| (entity, model.cities[id].at)),
                )
            {
                let transform = world.get::<Transform>(*entity).unwrap();
                let mesh = world
                    .resource::<Assets<Mesh>>()
                    .get(&world.get::<Mesh3d>(*entity).unwrap().0)
                    .unwrap();
                for position in mesh
                    .attribute(Mesh::ATTRIBUTE_POSITION)
                    .unwrap()
                    .as_float3()
                    .unwrap()
                {
                    let relative = transform.transform_point(Vec3::from_array(*position)) - at;
                    assert!(
                        relative.x.abs() <= 0.40001 && relative.z.abs() <= 0.40001,
                        "rendered vertex crosses cell margin: {relative:?}"
                    );
                }
            }
            // The old oversized hit boxes must not intercept clicks in the clear cell margin.
            let mut isolated = model.clone();
            isolated.cities.clear();
            isolated.lines.clear();
            let plant = isolated.plants.values().next().unwrap().clone();
            isolated.plants.retain(|id, _| *id == plant.id);
            for kind in domain::region::PlantKind::ALL {
                isolated.plants.get_mut(&plant.id).unwrap().kind = kind;
                let ray = Ray3d::new(plant.at + Vec3::new(0.45, 10.0, 0.0), Dir3::NEG_Y);
                assert_eq!(
                    presented_hit(
                        ray,
                        &isolated,
                        &WorldControls::default(),
                        world.resource::<TuningConfig>()
                    )
                    .unwrap()
                    .selection,
                    Selection::Cell(plant.cell)
                );
            }
            assert_eq!(world.resource::<Session>().0.checkpoint(), checkpoint);
        }
    }
}

#[test]
fn world_reconciliation_reuses_assets_and_destroys_everything_on_leave_and_reload() {
    use bevy::prelude::*;
    use game::sim::{ReturnToMenu, Session};
    use game::world::{ViewSync, WorldAssets, WorldEntity, WorldScene};
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = diorama_app(logs.path());
    for region in [
        "eastern_interconnection",
        "western_interconnection",
        "western_interconnection",
    ] {
        crate::support::request(&mut app, region);
        let before = app.world().resource::<WorldAssets>().counts();
        let plants = app.world().resource::<ViewSync>().plants.clone();
        assert_eq!(
            plants.len(),
            app.world()
                .resource::<Session>()
                .0
                .session()
                .plant_views()
                .len()
        );
        let checkpoint = app.world().resource::<Session>().0.checkpoint();
        crate::support::update_n(&mut app, 50);
        assert_eq!(app.world().resource::<WorldAssets>().counts(), before);
        assert_eq!(app.world().resource::<ViewSync>().plants, plants);
        assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
        assert!(app.world().resource::<WorldScene>().model.is_some());
        app.world_mut().write_message(ReturnToMenu);
        crate::support::update_n(&mut app, 3);
        assert!(app.world().resource::<WorldScene>().model.is_none());
        assert_eq!(app.world().resource::<WorldAssets>().counts(), (0, 0));
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<WorldEntity>>()
                .iter(app.world())
                .count(),
            0
        );
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 0);
        assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), 0);
    }
}

#[test]
fn world_real_pass_projects_selection_blocks_dialogs_and_drags_without_selecting() {
    use bevy::prelude::*;
    use bevy_egui::egui;
    use game::{
        hud::{Prompt, Selection, UiAction},
        world::{MapSurface, WorldControls, WorldScene},
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = diorama_app(logs.path());
    crate::support::request(&mut app, "eastern_interconnection");
    let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
    pass.settled(&mut app, 1280.0, 800.0);
    let surface = app.world().resource::<MapSurface>();
    let rect = surface.rect.unwrap();
    let pose = surface.pose.unwrap();
    let scene = app.world().resource::<WorldScene>();
    let world = scene.model.as_ref().unwrap();
    let (id, ndc) = world
        .cities
        .values()
        .filter(|c| c.connected)
        .find_map(|c| {
            game::world::city_parts().iter().find_map(|part| {
                let roof = part.center + part.rotation * (Vec3::Y * part.size.y / 2.0);
                let ndc = pose.project(c.at + roof * c.scale)?;
                let ray = pose.ray(ndc.truncate())?;
                (game::world::pick_world(ray, world, scene.geometry.as_ref().unwrap(), 1.0)
                    == Selection::City(c.id))
                .then_some((c.id, ndc))
            })
        })
        .expect("a connected city's visible roof");
    let at = egui::pos2(
        rect.left() + (ndc.x + 1.0) * 0.5 * rect.width(),
        rect.top() + (1.0 - ndc.y) * 0.5 * rect.height(),
    );
    pass.click_at(&mut app, 1280.0, 800.0, at);
    assert_eq!(*app.world().resource::<Selection>(), Selection::City(id));
    let before = app.world().resource::<WorldControls>().orbit;
    pass.drag(
        &mut app,
        1280.0,
        800.0,
        rect.center(),
        rect.center() + egui::vec2(30.0, 20.0),
    );
    assert_ne!(app.world().resource::<WorldControls>().orbit, before);
    assert_eq!(*app.world().resource::<Selection>(), Selection::City(id));
    crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::Prompt(Some(Prompt::LeaveGame))]);
    pass.settled(&mut app, 1280.0, 800.0);
    let before = app.world().resource::<WorldControls>().orbit;
    pass.drag(&mut app, 1280.0, 800.0, at, at + egui::vec2(20.0, 10.0));
    assert_eq!(app.world().resource::<WorldControls>().orbit, before);
}

#[test]
fn world_overlay_defaults_follow_domain_phase_and_do_not_touch_simulation() {
    use domain::session::DayPhase;
    use game::world::{WorldAction, WorldControls};
    let mut controls = WorldControls::default();
    controls.enter_phase(DayPhase::Planning);
    assert!(!controls.hologram());
    controls.apply(WorldAction::ToggleOverlay);
    assert!(!controls.overlay);
    controls.enter_phase(DayPhase::Planning);
    assert!(!controls.overlay);
    controls.enter_phase(DayPhase::Operating);
    assert!(controls.hologram());
    controls.apply(WorldAction::ToggleOverlay);
    assert!(!controls.hologram());
    controls.enter_phase(DayPhase::Review);
    assert!(controls.overlay && !controls.hologram());
}

proptest! {
    #[test]
    fn world_labels_never_escape_viewport(x in -1000f32..2000.0,y in -1000f32..2000.0,w in 1f32..300.0,h in 1f32..120.0) {
        use bevy_egui::egui;
        let rect=egui::Rect::from_min_size(egui::pos2(100.0,120.0),egui::vec2(320.0,200.0));
        let label=game::world::label_rect(rect,egui::pos2(x,y),egui::vec2(w,h)).unwrap();
        prop_assert!(rect.expand(0.001).contains_rect(label));
    }
}

#[test]
fn world_camera_keeps_a_full_window_transparent_hud_and_a_separate_hdr_view() {
    use bevy::{
        camera::{CameraOutputMode, Hdr},
        prelude::*,
    };
    use bevy_egui::{EguiGlobalSettings, PrimaryEguiContext};
    use game::world::{CameraPlugin, DioramaCamera};
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = crate::support::headless_app(&crate::support::shipped_assets(), logs.path());
    app.add_plugins((game::hud::HudPlugin, game::world::WorldViewPlugin));
    app.init_resource::<EguiGlobalSettings>()
        .add_plugins(CameraPlugin);
    app.update();
    let ui = app
        .world_mut()
        .query_filtered::<&Camera, With<PrimaryEguiContext>>()
        .single(app.world())
        .unwrap();
    assert!(ui.viewport.is_none());
    assert!(matches!(ui.clear_color,ClearColorConfig::Custom(c) if c==Color::NONE));
    assert!(matches!(
        ui.output_mode,
        CameraOutputMode::Write {
            blend_state: Some(_),
            clear_color: ClearColorConfig::None
        }
    ));
    let camera = app
        .world_mut()
        .query_filtered::<(Entity, &Camera), With<DioramaCamera>>()
        .single(app.world())
        .unwrap();
    assert!(!camera.1.is_active);
    assert!(app.world().get::<Hdr>(camera.0).is_some());
}

proptest! {
    #[test]
    fn world_copper_plate_supply_is_uniform_and_bounded(outage in 0f64..1.0,shed in 0f64..1.0) {
        use domain::grid::GridMode;
        let served=game::world::supply_fraction(GridMode::Normal,outage,shed);
        prop_assert!((served as f64-(1.0-outage)*(1.0-shed)).abs()<1e-6);
        prop_assert!((0.0..=1.0).contains(&served));
        prop_assert_eq!(game::world::supply_fraction(GridMode::Blackout,outage,shed),0.0);
    }
}

#[test]
fn world_fixed_script_preserves_checkpoints_reports_and_construction_identity() {
    use application::Command;
    use bevy::prelude::*;
    use domain::{ids::CellCoord, region::PlantKind};
    use game::{
        hud::UiAction,
        sim::Session,
        world::{ViewSync, WorldAction, WorldAssets, WorldScene},
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut view = diorama_app(&logs.path().join("view"));
    let mut reference = crate::support::hud_app(&logs.path().join("reference"));
    crate::support::request(&mut view, "western_interconnection");
    crate::support::request(&mut reference, "western_interconnection");
    let s = view.world().resource::<Session>().0.session();
    let region = s.region();
    let plant_build_months = region.spec(PlantKind::Peaker).build_months;
    let cell = (0..region.terrain().height())
        .flat_map(|y| (0..region.terrain().width()).map(move |x| CellCoord { x, y }))
        .find(|c| {
            region.site_check(PlantKind::Peaker, *c).is_ok()
                && s.plant_views().iter().all(|p| p.cell != *c)
        })
        .unwrap();
    for command in [
        Command::BuildPlant {
            kind: PlantKind::Peaker,
            cell,
        },
        Command::BeginOperating,
        Command::AdvanceTicks { n: 60 },
        Command::AdvanceTicks { n: 1380 },
        Command::FinishReview,
    ] {
        for app in [&mut view, &mut reference] {
            crate::hud_smoke::dispatch_in(app, vec![UiAction::Command(command.clone())]);
            app.update();
        }
        crate::hud_smoke::dispatch_in(
            &mut view,
            vec![
                UiAction::World(WorldAction::ToggleOverlay),
                UiAction::World(WorldAction::Orbit(Vec2::new(250.0, -400.0))),
                UiAction::World(WorldAction::Zoom(180.0)),
            ],
        );
        assert_eq!(
            view.world().resource::<Session>().0.checkpoint(),
            reference.world().resource::<Session>().0.checkpoint()
        );
        assert_eq!(
            view.world()
                .resource::<Session>()
                .0
                .projections()
                .monthly_reports
                .latest(),
            reference
                .world()
                .resource::<Session>()
                .0
                .projections()
                .monthly_reports
                .latest()
        );
    }
    let model = view
        .world()
        .resource::<WorldScene>()
        .model
        .as_ref()
        .unwrap();
    let plant = model.plants.values().find(|p| p.cell == cell).unwrap();
    assert_eq!(plant.building, Some(plant_build_months - 1));
    assert!(
        view.world()
            .resource::<ViewSync>()
            .plants
            .contains_key(&plant.id)
    );
    let ordered = plant.id;
    let entity = view.world().resource::<ViewSync>().plants[&ordered];
    let count = view.world().resource::<WorldAssets>().counts();
    view.update();
    assert_eq!(view.world().resource::<WorldAssets>().counts(), count);
    for _ in 0..11 {
        for command in [
            Command::BeginOperating,
            Command::AdvanceTicks { n: 1440 },
            Command::FinishReview,
        ] {
            for app in [&mut view, &mut reference] {
                crate::hud_smoke::dispatch_in(app, vec![UiAction::Command(command.clone())]);
                app.update();
            }
        }
        assert_eq!(
            view.world().resource::<Session>().0.checkpoint(),
            reference.world().resource::<Session>().0.checkpoint()
        );
        assert_eq!(view.world().resource::<WorldAssets>().counts(), count);
    }
    assert_eq!(view.world().resource::<ViewSync>().plants[&ordered], entity);
    assert_eq!(
        view.world()
            .resource::<WorldScene>()
            .model
            .as_ref()
            .unwrap()
            .plants[&ordered]
            .building,
        None
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn world_accelerated_terrain_rays_match_the_rendered_triangles(x in -0.99f32..0.99,z in -0.99f32..0.99,values in prop::collection::vec(0u16..1001,64)) {
        use bevy::prelude::*;
        let elevation:Vec<_>=values.iter().map(|v|f64::from(*v)/1000.0).collect();
        let grid=grid(8,8,&elevation);let geometry=TerrainGeometry::build(&grid,&[[70,90,50];64]).unwrap();
        let pose=game::world::OrbitCamera::default().pose(Vec2::splat(8.0),1.6);
        let ray=pose.ray(Vec2::new(x,z)).unwrap();
        let slow=game::world::terrain_hit(ray,&geometry);let fast=game::world::terrain_grid_hit(ray,&grid);
        match (slow,fast) {(Some(a),Some(b))=>prop_assert!((a-b).abs()<0.001,"{a} {b}"),(None,None)=>{},other=>prop_assert!(false,"{other:?}")}
    }
}

#[test]
fn world_real_pass_blocks_legends_panels_and_review_and_checks_all_layers_contrast() {
    use application::Command;
    use bevy_egui::egui;
    use game::{
        hud::{MapLayer, Selection, UiAction},
        sim::Session,
        world::{MapSurface, WorldAssets, WorldControls},
    };
    let logs = tempfile::TempDir::new().unwrap();
    for region in ["eastern_interconnection", "western_interconnection"] {
        let mut app = diorama_app(&logs.path().join(region));
        crate::support::request(&mut app, region);
        let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
        for zoom in [1.0, 1.25] {
            pass.ctx().set_zoom_factor(zoom);
            for (width, height) in [(1024.0, 640.0), (1280.0, 800.0)] {
                // bevy_egui supplies logical points: physical window / actual pixels-per-point.
                let (width, height) = (width / zoom, height / zoom);
                pass.settled(&mut app, width, height);
                let rect = app.world().resource::<MapSurface>().rect.unwrap();
                assert!(rect.is_positive());
                assert!(
                    rect.height() > 100.0,
                    "map controls left too little terrain: {rect:?}"
                );
                let before = app.world().resource::<WorldControls>().orbit;
                pass.run_with(
                    &mut app,
                    width,
                    height,
                    vec![
                        egui::Event::PointerMoved(egui::pos2(50.0, 150.0)),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            phase: egui::TouchPhase::Move,
                            delta: egui::vec2(0.0, 100.0),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
                assert_eq!(
                    app.world().resource::<WorldControls>().orbit,
                    before,
                    "panel wheel"
                );
                pass.run_with(
                    &mut app,
                    width,
                    height,
                    vec![egui::Event::PointerMoved(rect.center())],
                );
                assert_eq!(
                    app.world().resource::<WorldControls>().orbit,
                    before,
                    "panel scroll inertia must not zoom the map"
                );
                crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::ShowLegend(true)]);
                pass.settled(&mut app, width, height);
                let legend = app.world().resource::<MapSurface>().legend.unwrap();
                assert!(
                    app.world()
                        .resource::<MapSurface>()
                        .rect
                        .unwrap()
                        .expand(0.1)
                        .contains_rect(legend),
                    "legend must fit its map viewport: {legend:?}"
                );
                let selected = *app.world().resource::<Selection>();
                pass.click_at(&mut app, width, height, legend.center());
                assert_eq!(*app.world().resource::<Selection>(), selected);
                pass.drag(
                    &mut app,
                    width,
                    height,
                    legend.center(),
                    legend.center() + egui::vec2(10.0, 10.0),
                );
                assert_eq!(
                    app.world().resource::<WorldControls>().orbit,
                    before,
                    "legend drag"
                );
                let output = pass.paint_with(&mut app, width, height, vec![]);
                assert!(crate::contrast::check_paint(output) > 0);
                crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::ShowLegend(false)]);
            }
        }
        let count = app.world().resource::<WorldAssets>().counts();
        for layer in MapLayer::all() {
            crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::SetLayer(layer)]);
            app.update();
            assert_eq!(app.world().resource::<WorldAssets>().counts(), count);
        }
        crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::Command(Command::BeginOperating)]);
        app.update();
        assert!(app.world().resource::<WorldControls>().hologram());
        crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::SetLayer(MapLayer::Irradiance)]);
        assert!(!app.world().resource::<WorldControls>().hologram());
        crate::hud_smoke::dispatch_in(
            &mut app,
            vec![UiAction::World(game::world::WorldAction::ControlRoom)],
        );
        assert!(app.world().resource::<WorldControls>().hologram());
        pass.settled(&mut app, 1280.0, 800.0);
        let output = pass.paint_with(&mut app, 1280.0, 800.0, vec![]);
        assert!(crate::contrast::check_paint(output) > 0);
        crate::hud_smoke::dispatch_in(
            &mut app,
            vec![UiAction::Command(Command::AdvanceTicks { n: 1440 })],
        );
        app.update();
        assert_eq!(
            app.world().resource::<Session>().0.session().phase(),
            domain::session::DayPhase::Review
        );
        pass.settled(&mut app, 1280.0, 800.0);
        let before = app.world().resource::<WorldControls>().orbit;
        let rect = app.world().resource::<MapSurface>().rect.unwrap();
        pass.drag(
            &mut app,
            1280.0,
            800.0,
            rect.left_top() + egui::vec2(20.0, 20.0),
            rect.left_top() + egui::vec2(40.0, 40.0),
        );
        assert_eq!(
            app.world().resource::<WorldControls>().orbit,
            before,
            "Review backdrop"
        );
        let output = pass.paint_with(&mut app, 1280.0, 800.0, vec![]);
        assert!(crate::contrast::check_paint(output) > 0);
        assert!(
            app.world()
                .resource::<game::l10n::Localizer>()
                .problems()
                .is_empty()
        );
    }
}

#[cfg(feature = "dev")]
#[test]
fn world_dev_inspector_registers_types_without_default_bevy_features_and_preserves_session() {
    use bevy_egui::egui;
    use game::{
        sim::Session,
        world::{DevToolsPlugin, TuningConfig},
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = diorama_app(logs.path());
    app.add_plugins(DevToolsPlugin);
    crate::support::request(&mut app, "western_interconnection");
    let before = app.world().resource::<Session>().0.checkpoint();
    let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
    pass.settled(&mut app, 1280.0, 800.0);
    pass.run_with(
        &mut app,
        1280.0,
        800.0,
        vec![egui::Event::Key {
            key: egui::Key::F12,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let widgets = pass.settled(&mut app, 1280.0, 800.0);
    assert!(widgets.iter().any(|w| w.text == "Diorama inspector"));
    assert!(app.world().contains_resource::<TuningConfig>());
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), before);
}

#[test]
fn world_visible_output_pillars_are_pickable_and_hidden_pillars_are_not() {
    use bevy::prelude::*;
    use game::{
        hud::Selection,
        sim::Session,
        world::{TuningConfig, WorldControls, WorldModel, pillar_part, presented_hit},
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = diorama_app(logs.path());
    crate::support::request(&mut app, "eastern_interconnection");
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(application::Command::BeginOperating)
        .unwrap();
    let mut model = WorldModel::of(&app.world().resource::<Session>().0);
    let plant = model
        .plants
        .values()
        .find(|p| p.connected && p.output > 0.5)
        .unwrap()
        .clone();
    model.plants.retain(|id, _| *id == plant.id);
    model.cities.clear();
    model.lines.clear();
    let tuning = TuningConfig::default();
    let part = pillar_part(&plant, &tuning).unwrap();
    let ray = Ray3d::new(part.center + Vec3::X * 10.0, Dir3::NEG_X);
    let mut controls = WorldControls::default();
    controls.enter_phase(domain::session::DayPhase::Operating);
    assert_eq!(
        presented_hit(ray, &model, &controls, &tuning)
            .unwrap()
            .selection,
        Selection::Plant(plant.id)
    );
    controls.overlay = false;
    assert!(presented_hit(ray, &model, &controls, &tuning).is_none());
}

#[test]
fn world_does_not_break_the_boot_error_screen_when_localization_is_unavailable() {
    let files = tempfile::TempDir::new().unwrap();
    let mut app = crate::support::headless_app(files.path(), &files.path().join("logs"));
    app.add_plugins((game::hud::HudPlugin, game::world::WorldViewPlugin));
    crate::support::update_n(&mut app, 3);
    assert!(app.world().contains_resource::<game::state::BootError>());
    let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(widgets.iter().any(|w| w.text.contains("could not start")));
}

#[test]
fn world_compact_layer_selector_changes_layer_without_picking_through_its_popup() {
    use bevy_egui::egui::accesskit::Role;
    use game::hud::{MapLayer, Selection};
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = diorama_app(logs.path());
    crate::support::request(&mut app, "western_interconnection");
    let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
    pass.ctx().set_zoom_factor(1.25);
    let widgets = pass.settled(&mut app, 819.2, 512.0);
    let combo = widgets
        .iter()
        .find(|w| w.role == Role::ComboBox)
        .expect("narrow map has a layer selector");
    pass.click_at(&mut app, 819.2, 512.0, combo.rect.center());
    let widgets = pass.settled(&mut app, 819.2, 512.0);
    let sunshine = crate::support::egui_harness::button(&widgets, "Sunshine");
    for label in ["Suitability: Gas plant (CCGT)", "Suitability: Gas peaker"] {
        assert!(
            !widgets.iter().any(|w| w.text == label),
            "removed layer {label}"
        );
    }
    for label in [
        "Suitability: Nuclear plant",
        "Suitability: Coal plant",
        "Suitability: Solar farm",
        "Suitability: Hydro plant",
    ] {
        crate::support::egui_harness::button(&widgets, label);
    }
    pass.click_at(&mut app, 819.2, 512.0, sunshine.rect.center());
    assert_eq!(*app.world().resource::<MapLayer>(), MapLayer::Irradiance);
    assert_eq!(*app.world().resource::<Selection>(), Selection::None);
    let widgets = pass.settled(&mut app, 819.2, 512.0);
    assert!(widgets.iter().any(|w| w.text == "Solar potential"));
    let viewport = app
        .world()
        .resource::<game::world::MapSurface>()
        .rect
        .unwrap();
    assert!(viewport.height() > 100.0);
    let scale = widgets
        .iter()
        .find(|w| w.text == "Solar potential")
        .unwrap();
    assert!(
        scale.rect.bottom() <= viewport.top(),
        "key stays outside the map"
    );
    crate::hud_smoke::dispatch_in(
        &mut app,
        vec![game::hud::UiAction::World(
            game::world::WorldAction::ToggleOverlay,
        )],
    );
    let widgets = pass.settled(&mut app, 819.2, 512.0);
    assert!(!widgets.iter().any(|w| w.text == "Solar potential"));
}

#[test]
fn world_secondary_and_middle_drags_pan_without_selecting() {
    use bevy_egui::egui;
    use game::{
        hud::Selection,
        world::{MapSurface, WorldControls},
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = diorama_app(logs.path());
    crate::support::request(&mut app, "eastern_interconnection");
    let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
    pass.settled(&mut app, 1280.0, 800.0);
    for button in [egui::PointerButton::Secondary, egui::PointerButton::Middle] {
        let at = app.world().resource::<MapSurface>().rect.unwrap().center();
        let end = at + egui::vec2(40.0, 20.0);
        let before = app.world().resource::<WorldControls>().orbit.target;
        pass.run_with(
            &mut app,
            1280.0,
            800.0,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        pass.run_with(
            &mut app,
            1280.0,
            800.0,
            vec![egui::Event::PointerMoved(end)],
        );
        pass.run_with(
            &mut app,
            1280.0,
            800.0,
            vec![egui::Event::PointerButton {
                pos: end,
                button,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_ne!(
            app.world().resource::<WorldControls>().orbit.target,
            before,
            "{button:?} drag pans"
        );
        assert_eq!(*app.world().resource::<Selection>(), Selection::None);
    }
}

#[test]
fn world_operating_overlay_preserves_lit_terrain_and_building_shading() {
    use application::Command;
    use bevy::prelude::*;
    use game::{
        hud::UiAction,
        sim::Session,
        world::{TuningConfig, ViewSync, WorldAction, WorldAssets, WorldEntity},
    };
    let logs = tempfile::TempDir::new().unwrap();
    for region in ["eastern_interconnection", "western_interconnection"] {
        let mut app = diorama_app(&logs.path().join(region));
        crate::support::request(&mut app, region);
        let terrain = app
            .world_mut()
            .query_filtered::<(Entity, &Mesh3d), With<WorldEntity>>()
            .iter(app.world())
            .max_by_key(|(_, mesh)| {
                app.world()
                    .resource::<Assets<Mesh>>()
                    .get(&mesh.0)
                    .unwrap()
                    .count_vertices()
            })
            .unwrap()
            .0;
        let material = |world: &World, entity| {
            let handle = &world
                .get::<MeshMaterial3d<StandardMaterial>>(entity)
                .unwrap()
                .0;
            world
                .resource::<Assets<StandardMaterial>>()
                .get(handle)
                .unwrap()
                .clone()
        };
        let terrain_colour = material(app.world(), terrain).base_color;
        let counts = app.world().resource::<WorldAssets>().counts();
        crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::Command(Command::BeginOperating)]);
        app.update();
        let checkpoint = app.world().resource::<Session>().0.checkpoint();
        let views = app.world().resource::<ViewSync>();
        let buildings: Vec<_> = views
            .plants
            .values()
            .chain(views.cities.values())
            .copied()
            .collect();
        for entity in buildings {
            assert!(
                !material(app.world(), entity).unlit,
                "Operating must retain face lighting and shadows"
            );
        }
        let ground = material(app.world(), terrain);
        assert!(!ground.unlit);
        assert_eq!(
            ground.base_color, terrain_colour,
            "The supply overlay must not blacken the terrain"
        );
        let sunlight = app.world().resource::<TuningConfig>().sunlight();
        for light in app
            .world_mut()
            .query::<&DirectionalLight>()
            .iter(app.world())
        {
            assert_eq!(light.illuminance, sunlight);
        }
        for _ in 0..2 {
            crate::hud_smoke::dispatch_in(
                &mut app,
                vec![UiAction::World(WorldAction::ToggleOverlay)],
            );
            app.update();
            assert_eq!(app.world().resource::<WorldAssets>().counts(), counts);
            assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
            assert_eq!(material(app.world(), terrain).base_color, terrain_colour);
        }
    }
}
