//! Plan 3's CPU geometry contract: no window, renderer or simulation mutation.
use crate::support::usd_style;
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
            wind_potential: Ratio::ZERO,
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
fn world_effects_button_names_the_action_it_will_take() {
    let l10n = crate::support::shipped_l10n();
    for enabled in [true, false] {
        let model = game::world::toolbar_model(&l10n, true, enabled, false, false);
        assert_eq!(
            model.effects.label,
            if enabled {
                "Disable effects"
            } else {
                "Enable effects"
            }
        );
    }
}

#[test]
fn world_operating_hint_describes_visible_changes_in_plain_language() {
    let l10n = crate::support::shipped_l10n();
    let model = game::world::toolbar_model(&l10n, true, true, true, true);
    for explanation in ["Taller cities", "Fewer lit windows", "Moving dots"] {
        assert!(model.supply.contains(explanation), "{explanation}");
    }
    assert!(!model.supply.contains("gauges"));
    assert!(!model.supply_help.contains("gauge"));
    assert!(
        model
            .supply_help
            .contains("All connected cities receive the same percentage")
    );
    assert!(
        model
            .supply_help
            .contains("do not measure how much power a line carries")
    );
    assert!(l10n.problems().is_empty());
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

proptest! {
    #[test]
    fn world_pointer_zoom_preserves_projection_until_the_pan_boundary(
        yaw in 0f32..std::f32::consts::TAU, elevation in 0.3f32..1.42,
        zoom in 0.08f32..3.0, delta in -2000f32..2000.0, aspect in 0.5f32..3.0,
        x in -45f32..45.0, z in -25f32..25.0, height in 0f32..6.0,
    ) {
        use bevy::math::Vec2;
        let size = Vec2::new(100.0, 60.0);
        let anchor = Vec3::new(x, height, z);
        let mut camera = game::world::OrbitCamera { yaw, elevation, zoom, ..Default::default() };
        let projected = camera.pose(size, aspect).project(anchor);
        prop_assume!(projected.is_some());
        let before = projected.unwrap();
        prop_assume!(before.x.abs() <= 1.0 && before.y.abs() <= 1.0);
        camera.zoom_at(delta, anchor, size, aspect);
        let pose = camera.pose(size, aspect);
        prop_assert!(pose.eye.is_finite() && camera.target.is_finite());
        prop_assert!(camera.target.abs().cmple(size * 0.55).all());
        prop_assert!((0.08..=3.0).contains(&camera.zoom));
        if camera.target.abs().cmplt(size * 0.55 - Vec2::splat(0.001)).all() {
            let after = pose.project(anchor).unwrap();
            prop_assert!((after.truncate() - before.truncate()).length() < 0.0002, "{before:?} -> {after:?}");
        }
        // More wheel events at a distance limit must not slowly pan the camera.
        camera.zoom_at(2000.0, anchor, size, aspect);
        let at_limit = camera;
        camera.zoom_at(2000.0, anchor, size, aspect);
        prop_assert_eq!(camera, at_limit);
    }
}

#[test]
fn world_pointer_zoom_handles_invalid_anchors_and_extreme_input() {
    use bevy::math::Vec2;
    let size = Vec2::new(100.0, 60.0);
    for anchor in [Vec3::NAN, Vec3::splat(f32::INFINITY)] {
        let mut camera = game::world::OrbitCamera::default();
        let mut centred = camera;
        centred.zoom(200.0);
        camera.zoom_at(200.0, anchor, size, 1.6);
        assert_eq!(camera, centred);
    }
    for delta in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut camera = game::world::OrbitCamera::default();
        camera.zoom_at(delta, Vec3::new(25.0, 2.0, 10.0), size, 1.6);
        assert_eq!(camera, game::world::OrbitCamera::default());
    }
}

#[test]
fn world_city_and_plant_models_leave_a_margin_inside_their_cell() {
    use domain::{ids::RegionId, region::PlantKind, session::GameSettings};
    use game::world::{TuningConfig, WorldModel, city_parts, plant_parts};
    let fits = |parts: &[game::world::Part], scale: Vec3| {
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
            fits(&plant_parts(kind), Vec3::splat(tuning.scale()));
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
            fits(&city_parts(), city.visual_scale(1.0));
        }
        assert_eq!(game.checkpoint(), checkpoint);
    }
}

proptest! {
    #[test]
    fn world_model_footprints_stay_bounded_at_any_visual_scale(requested in any::<f32>(), city_height in 0.0f32..=10.0) {
        use game::world::{TuningConfig, city_parts, plant_parts, selection_parts};
        let scale = TuningConfig { model_scale: requested, ..Default::default() }.scale();
        for (parts, factor, radius) in domain::region::PlantKind::ALL.into_iter()
            .map(|kind| (plant_parts(kind), Vec3::splat(scale), 0.4))
            .chain([(city_parts(), Vec3::new(1.0, city_height, 1.0) * scale, 0.4), (selection_parts().to_vec(), Vec3::splat(scale), 0.45)]) {
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
fn world_city_heights_are_proportional_to_demand_in_planning_and_operation() {
    use application::Command;
    use bevy::prelude::*;
    use game::{
        sim::Session,
        world::{ViewSync, WorldScene},
    };
    for region in ["eastern_interconnection", "western_interconnection"] {
        let logs = tempfile::TempDir::new().unwrap();
        let mut app = diorama_app(logs.path());
        crate::support::request(&mut app, region);
        let mut height_per_mw = None;
        for advance in [None, Some(0), Some(60), Some(720)] {
            if let Some(ticks) = advance {
                {
                    let mut session = app.world_mut().resource_mut::<Session>();
                    if ticks == 0 {
                        session.0.execute(Command::BeginOperating).unwrap();
                    }
                    session
                        .0
                        .execute(Command::AdvanceTicks { n: ticks })
                        .unwrap();
                }
                app.update();
            }
            let world = app.world();
            let session = &world.resource::<Session>().0;
            let model = world.resource::<WorldScene>().model.as_ref().unwrap();
            let views = world.resource::<ViewSync>();
            for city in model.cities.values() {
                let detail = session.session().city_detail(city.id).unwrap();
                let expected = if advance.is_none() {
                    detail.base_load
                } else {
                    detail.demand_now.unwrap_or_else(|| {
                        detail.base_load
                            * model.region.load_factor(city.id, model.month, model.tick)
                    })
                };
                let demand = expected.value() as f32;
                let scale = world
                    .get::<Transform>(views.cities[&city.id])
                    .unwrap()
                    .scale;
                assert_eq!(scale.x, 1.0, "demand changes height, not footprint");
                assert_eq!(scale.z, 1.0);
                let ratio = *height_per_mw.get_or_insert(scale.y / demand);
                assert!(
                    (scale.y / demand - ratio).abs() < 1e-6,
                    "city heights must share one scale across cities and times"
                );
            }
        }
    }
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

#[test]
fn world_ground_in_an_occupied_cell_selects_its_building() {
    use bevy::prelude::*;
    use game::{
        hud::Selection,
        world::{TuningConfig, WorldControls, WorldModel, presented_hit},
    };
    for region in ["eastern_interconnection", "western_interconnection"] {
        let logs = tempfile::TempDir::new().unwrap();
        let mut app = crate::support::hud_app(logs.path());
        crate::support::request(&mut app, region);
        let mut model = WorldModel::of(&app.world().resource::<game::sim::Session>().0);
        model.lines.clear();
        for (at, expected) in model
            .plants
            .values()
            .map(|p| (p.at, Selection::Plant(p.id)))
            .chain(model.cities.values().map(|c| (c.at, Selection::City(c.id))))
        {
            // All four bare corners lie outside the fitted 0.8-cell model footprint.
            for x in [-0.45, 0.45] {
                for z in [-0.45, 0.45] {
                    let ray = Ray3d::new(at + Vec3::new(x, 10.0, z), Dir3::NEG_Y);
                    assert_eq!(
                        presented_hit(
                            ray,
                            &model,
                            &WorldControls::default(),
                            &TuningConfig::default()
                        )
                        .unwrap()
                        .selection,
                        expected
                    );
                }
            }
        }
    }
}

#[test]
fn world_real_click_on_site_ground_opens_the_building_instead_of_plant_construction() {
    use bevy::prelude::*;
    use bevy_egui::egui;
    use game::{
        hud::{PlantSection, Selection, build_model},
        l10n::Localizer,
        sim::Session,
        world::{MapSurface, OrbitCamera, WorldControls, WorldScene},
    };
    for region in ["eastern_interconnection", "western_interconnection"] {
        let logs = tempfile::TempDir::new().unwrap();
        let mut app = diorama_app(logs.path());
        crate::support::request(&mut app, region);
        let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
        let model = app.world().resource::<WorldScene>().model.as_ref().unwrap();
        let plant = model.plants.values().next().unwrap();
        let city = model.cities.values().next().unwrap();
        let sites = [
            (plant.at, Selection::Plant(plant.id)),
            (city.at, Selection::City(city.id)),
        ];
        let checkpoint = app.world().resource::<Session>().0.checkpoint();
        for (at, expected) in sites {
            app.world_mut().resource_mut::<WorldControls>().orbit = OrbitCamera {
                target: at.xz(),
                zoom: 0.3,
                elevation: 1.42,
                ..Default::default()
            };
            pass.settled(&mut app, 1280.0, 800.0);
            let surface = app.world().resource::<MapSurface>();
            let rect = surface.rect.unwrap();
            let ndc = surface
                .pose
                .unwrap()
                .project(at + Vec3::new(0.45, 0.0, 0.45))
                .unwrap();
            let pointer = egui::pos2(
                rect.left() + (ndc.x + 1.0) * rect.width() / 2.0,
                rect.top() + (1.0 - ndc.y) * rect.height() / 2.0,
            );
            pass.click_at(&mut app, 1280.0, 800.0, pointer);
            assert_eq!(*app.world().resource::<Selection>(), expected);
            let world = app.world();
            let model = build_model(
                &world.resource::<Session>().0,
                world.resource::<Localizer>(),
                expected,
                &usd_style(),
            )
            .unwrap();
            assert!(matches!(model.plant, PlantSection::PickCell { .. }));
            assert_eq!(world.resource::<Session>().0.checkpoint(), checkpoint);
        }
    }
}

#[test]
fn world_picking_prefers_buildings_at_crowded_line_junctions() {
    use bevy::prelude::*;
    use game::{
        hud::Selection,
        world::{
            Part, TuningConfig, WorldControls, WorldModel, city_parts, plant_parts, presented_hit,
        },
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = crate::support::hud_app(logs.path());
    crate::support::request(&mut app, "eastern_interconnection");
    let game = &app.world().resource::<game::sim::Session>().0;
    let original = WorldModel::of(game);
    let plant = original.plants.values().next().unwrap();
    let city = original.cities.values().next().unwrap();
    for scale in [0.4, 1.0] {
        let tuning = TuningConfig {
            model_scale: scale,
            ..default()
        };
        let controls = WorldControls::default();
        for kind in domain::region::PlantKind::ALL
            .into_iter()
            .map(Some)
            .chain([None])
        {
            let (selection, at, parts, size) = match kind {
                Some(kind) => (
                    Selection::Plant(plant.id),
                    plant.at,
                    plant_parts(kind),
                    Vec3::splat(scale),
                ),
                None => (
                    Selection::City(city.id),
                    city.at,
                    city_parts(),
                    city.visual_scale(scale),
                ),
            };
            let mut model = original.clone();
            model
                .plants
                .retain(|id, _| selection == Selection::Plant(*id));
            model
                .cities
                .retain(|id, _| selection == Selection::City(*id));
            if let Some(kind) = kind {
                model.plants.get_mut(&plant.id).unwrap().kind = kind;
            }
            let roof = parts
                .iter()
                .max_by(|a, b| a.bounds().1.y.total_cmp(&b.bounds().1.y))
                .unwrap();
            let point = at + (roof.center + Vec3::Y * roof.size.y * 0.5) * size;
            let ray = Ray3d::new(point + Vec3::Y * 3.0, Dir3::NEG_Y);
            model.lines.clear();
            let building = presented_hit(ray, &model, &controls, &tuning).unwrap();
            assert_eq!(building.selection, selection);
            let mut line = original.lines.values().next().unwrap().clone();
            line.parts = (0..8)
                .map(|i| Part {
                    rotation: Quat::from_rotation_y(i as f32 * std::f32::consts::PI / 8.0),
                    ..Part::block(point + Vec3::Y * 0.5, Vec3::new(0.1, 0.1, 2.0), [1.0; 4])
                })
                .collect();
            let line_selection = line.selection;
            model.lines.insert(line.id, line);
            let hit = presented_hit(ray, &model, &controls, &tuning).unwrap();
            assert_eq!(hit.selection, selection, "cables intercepted {selection:?}");
            assert_eq!(hit.distance, building.distance);
            // The same cables remain selectable where no building is under the pointer.
            model.plants.clear();
            model.cities.clear();
            assert_eq!(
                presented_hit(ray, &model, &controls, &tuning)
                    .unwrap()
                    .selection,
                line_selection
            );
            // Prioritizing a building must still respect terrain in front of it.
            assert!(matches!(
                presented_hit(
                    Ray3d::new(at - Vec3::Y, Dir3::Y),
                    &original,
                    &controls,
                    &tuning
                )
                .unwrap()
                .selection,
                Selection::Cell(_)
            ));
        }
    }
}

fn diorama_app(logs: &std::path::Path) -> bevy::prelude::App {
    let mut app = crate::support::hud_app(logs);
    app.add_plugins(game::world::WorldViewPlugin);
    app
}

#[test]
fn world_power_lines_stay_fine_when_zoomed_in() {
    use bevy::prelude::*;
    use bevy_egui::egui;
    use game::world::{MapSurface, OrbitCamera, ViewSync, WorldAssets, WorldScene};
    for region in ["eastern_interconnection", "western_interconnection"] {
        let logs = tempfile::TempDir::new().unwrap();
        let mut app = diorama_app(logs.path());
        crate::support::request(&mut app, region);
        let checkpoint = app.world().resource::<game::sim::Session>().0.checkpoint();
        let counts = app.world().resource::<WorldAssets>().counts();
        for height in [600.0, 1200.0] {
            for zoom in [1.0, 0.3, 0.08, 1.0] {
                let model = app.world().resource::<WorldScene>().model.as_ref().unwrap();
                let pose = OrbitCamera {
                    zoom,
                    target: model.lines.values().next().unwrap().midpoint.xz(),
                    ..default()
                }
                .pose(model.size(), 1.6);
                let mut surface = app.world_mut().resource_mut::<MapSurface>();
                surface.pose = Some(pose);
                surface.rect = Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(height * 1.6, height),
                ));
                app.update();
                let world = app.world();
                let model = world.resource::<WorldScene>().model.as_ref().unwrap();
                let mut checked = 0;
                for (id, entity) in &world.resource::<ViewSync>().lines {
                    let mesh = world
                        .resource::<Assets<Mesh>>()
                        .get(&world.get::<Mesh3d>(*entity).unwrap().0)
                        .unwrap();
                    let vertices = mesh
                        .attribute(Mesh::ATTRIBUTE_POSITION)
                        .unwrap()
                        .as_float3()
                        .unwrap();
                    for (part, vertices) in model.lines[id]
                        .parts
                        .iter()
                        .zip(vertices.as_chunks::<24>().0)
                    {
                        let Some(center) = pose.project(part.center) else {
                            continue;
                        };
                        if center.x.abs() > 0.9 || center.y.abs() > 0.9 {
                            continue;
                        }
                        // Compare corners at the same end: measure the cable's cross section,
                        // excluding its length along the terrain route.
                        for a in vertices {
                            for b in vertices {
                                let a = Vec3::from_array(*a);
                                let b = Vec3::from_array(*b);
                                if (part.rotation.inverse() * (a - b)).z.abs() > 1e-4 {
                                    continue;
                                }
                                let (Some(a), Some(b)) = (pose.project(a), pose.project(b)) else {
                                    continue;
                                };
                                let pixels =
                                    ((a - b).truncate() * Vec2::new(height * 1.6, height) * 0.5)
                                        .length();
                                assert!(
                                    pixels <= 2.8,
                                    "thick cable: {pixels} points at zoom {zoom}, height {height}"
                                );
                            }
                        }
                        checked += 1;
                    }
                }
                assert!(checked > 0);
                assert_eq!(world.resource::<WorldAssets>().counts(), counts);
                assert_eq!(
                    world.resource::<game::sim::Session>().0.checkpoint(),
                    checkpoint
                );
            }
        }
    }
}

#[test]
fn world_rendered_models_fit_their_cells_and_bare_margins_select_their_occupants() {
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
            // The whole occupied site selects its plant, including the bare cell margin.
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
                    Selection::Plant(plant.id)
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
fn world_wheel_keeps_the_point_under_the_pointer_fixed_for_cities_and_plants() {
    use bevy_egui::egui;
    use game::{
        hud::{Selection, UiAction},
        sim::Session,
        world::{MapSurface, TuningConfig, WorldAction, WorldControls, WorldScene, presented_hit},
    };
    let logs = tempfile::TempDir::new().unwrap();
    for region in ["eastern_interconnection", "western_interconnection"] {
        let mut app = diorama_app(&logs.path().join(region));
        crate::support::request(&mut app, region);
        let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
        for operating in [false, true] {
            if operating {
                crate::hud_smoke::dispatch_in(
                    &mut app,
                    vec![UiAction::Command(application::Command::BeginOperating)],
                );
                app.update();
            }
            for city in [true, false] {
                crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::World(WorldAction::Reset)]);
                pass.settled(&mut app, 1280.0, 800.0);
                let surface = app.world().resource::<MapSurface>();
                let rect = surface.rect.unwrap();
                let pose = surface.pose.unwrap();
                let model = app.world().resource::<WorldScene>().model.as_ref().unwrap();
                let controls = app.world().resource::<WorldControls>();
                let tuning = app.world().resource::<TuningConfig>();
                let candidates: Vec<_> = if city {
                    model
                        .cities
                        .values()
                        .flat_map(|c| {
                            game::world::city_parts().into_iter().map(move |part| {
                                (
                                    Selection::City(c.id),
                                    c.at + (part.center
                                        + part.rotation * (Vec3::Y * part.size.y / 2.0))
                                        * c.visual_scale(tuning.scale()),
                                )
                            })
                        })
                        .collect()
                } else {
                    model
                        .plants
                        .values()
                        .flat_map(|p| {
                            game::world::plant_parts(p.kind)
                                .into_iter()
                                .map(move |part| {
                                    (
                                        Selection::Plant(p.id),
                                        p.at + part.center
                                            + part.rotation * (Vec3::Y * part.size.y / 2.0),
                                    )
                                })
                        })
                        .collect()
                };
                let (ndc, anchor) = candidates
                    .into_iter()
                    .find_map(|(selection, at)| {
                        let ndc = pose.project(at)?.truncate();
                        if ndc.x.abs() > 0.9 || ndc.y.abs() > 0.9 || ndc.length() < 0.1 {
                            return None;
                        }
                        let ray = pose.ray(ndc)?;
                        let hit = presented_hit(ray, model, controls, tuning)?;
                        (hit.selection == selection).then_some((ndc, ray.get_point(hit.distance)))
                    })
                    .expect("an off-centre visible city/plant");
                let pointer = egui::pos2(
                    rect.left() + (ndc.x + 1.0) * 0.5 * rect.width(),
                    rect.top() + (1.0 - ndc.y) * 0.5 * rect.height(),
                );
                let checkpoint = app.world().resource::<Session>().0.checkpoint();
                let before = controls.orbit;
                for delta in [200.0, -200.0] {
                    pass.run_with(
                        &mut app,
                        1280.0,
                        800.0,
                        vec![
                            egui::Event::PointerMoved(pointer),
                            egui::Event::MouseWheel {
                                unit: egui::MouseWheelUnit::Point,
                                phase: egui::TouchPhase::Move,
                                delta: egui::vec2(0.0, delta),
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    );
                    if delta > 0.0 {
                        assert!(app.world().resource::<WorldControls>().orbit.zoom < before.zoom);
                    }
                    let surface = app.world().resource::<MapSurface>();
                    let after = surface.pose.unwrap().project(anchor).unwrap().truncate();
                    let pixel_drift =
                        (after - ndc) * bevy::math::Vec2::new(rect.width(), rect.height()) / 2.0;
                    assert!(
                        pixel_drift.length() < 0.05,
                        "{region}, city={city}, operating={operating}: drift {pixel_drift:?}"
                    );
                }
                let after = app.world().resource::<WorldControls>().orbit;
                assert!((after.zoom - before.zoom).abs() < 1e-5);
                assert!((after.target - before.target).length() < 0.002);
                assert_eq!(*app.world().resource::<Selection>(), Selection::None);
                assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
            }
        }
    }
}

#[test]
fn world_wheel_over_empty_background_zooms_without_panning() {
    use bevy_egui::egui;
    use game::world::{MapSurface, TuningConfig, WorldControls, WorldScene, presented_hit};
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = diorama_app(logs.path());
    crate::support::request(&mut app, "western_interconnection");
    let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
    pass.settled(&mut app, 1280.0, 800.0);
    let surface = app.world().resource::<MapSurface>();
    let rect = surface.rect.unwrap();
    let pose = surface.pose.unwrap();
    let controls = app.world().resource::<WorldControls>();
    let scene = app.world().resource::<WorldScene>();
    let tuning = app.world().resource::<TuningConfig>();
    let ndc = [(-0.95, 0.95), (0.95, 0.95), (-0.95, -0.95), (0.95, -0.95)]
        .into_iter()
        .map(|(x, y)| bevy::math::Vec2::new(x, y))
        .find(|ndc| {
            presented_hit(
                pose.ray(*ndc).unwrap(),
                scene.model.as_ref().unwrap(),
                controls,
                tuning,
            )
            .is_none()
        })
        .expect("empty background around the diorama");
    let pointer = egui::pos2(
        rect.left() + (ndc.x + 1.0) * rect.width() / 2.0,
        rect.top() + (1.0 - ndc.y) * rect.height() / 2.0,
    );
    let mut expected = controls.orbit;
    expected.zoom(100.0);
    pass.run_with(
        &mut app,
        1280.0,
        800.0,
        vec![
            egui::Event::PointerMoved(pointer),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: egui::vec2(0.0, 100.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert_eq!(app.world().resource::<WorldControls>().orbit, expected);
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
                let ndc = pose.project(c.at + roof * c.visual_scale(1.0))?;
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
    pass.run_with(
        &mut app,
        1280.0,
        800.0,
        vec![egui::Event::ModifiersChanged(egui::Modifiers::SHIFT)],
    );
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
        camera::{CameraOutputMode, Hdr, visibility::RenderLayers},
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
        .query_filtered::<Entity, With<PrimaryEguiContext>>()
        .single(app.world())
        .unwrap();
    let camera = app.world().get::<Camera>(ui).unwrap();
    assert_eq!(app.world().get::<Msaa>(ui), Some(&Msaa::Off));
    // Only the separate 3D camera participates in Bevy's shadow-map preparation.
    assert!(app.world().get::<Camera2d>(ui).is_some());
    assert!(app.world().get::<Camera3d>(ui).is_none());
    assert_eq!(
        app.world().get::<RenderLayers>(ui),
        Some(&RenderLayers::none())
    );
    assert!(camera.viewport.is_none());
    assert!(matches!(camera.clear_color,ClearColorConfig::Custom(c) if c==Color::NONE));
    assert!(matches!(
        camera.output_mode,
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

#[test]
fn world_camera_is_removed_in_review_and_recreated_on_continue() {
    use bevy::prelude::*;
    use bevy_egui::{EguiGlobalSettings, egui};
    use game::{
        hud::UiAction,
        world::{CameraPlugin, DioramaCamera, MapSurface, OrbitCamera, WorldScene},
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = crate::support::headless_app(&crate::support::shipped_assets(), logs.path());
    app.init_resource::<EguiGlobalSettings>().add_plugins((
        game::hud::HudPlugin,
        game::world::WorldViewPlugin,
        CameraPlugin,
    ));
    crate::support::update_n(&mut app, 3);
    app.world_mut().spawn((
        Window {
            resolution: (1280, 720).into(),
            ..default()
        },
        bevy::window::PrimaryWindow,
    ));
    crate::support::request(&mut app, "eastern_interconnection");
    let size = app
        .world()
        .resource::<WorldScene>()
        .model
        .as_ref()
        .unwrap()
        .size();
    {
        let mut surface = app.world_mut().resource_mut::<MapSurface>();
        surface.rect = Some(egui::Rect::from_min_size(
            egui::pos2(260.0, 65.0),
            egui::vec2(720.0, 394.0),
        ));
        surface.scale = 1.0;
        surface.pose = Some(OrbitCamera::default().pose(size, 720.0 / 394.0));
    }
    app.update();
    let old = app
        .world_mut()
        .query_filtered::<Entity, With<DioramaCamera>>()
        .single(app.world())
        .unwrap();
    assert!(app.world().get::<Camera>(old).unwrap().is_active);
    crate::hud_smoke::dispatch_in(
        &mut app,
        vec![
            UiAction::Command(application::Command::BeginOperating),
            UiAction::Command(application::Command::AdvanceTicks { n: 1440 }),
        ],
    );
    app.update();
    assert_eq!(
        app.world()
            .resource::<WorldScene>()
            .model
            .as_ref()
            .unwrap()
            .phase,
        domain::session::DayPhase::Review
    );
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<DioramaCamera>>()
            .iter(app.world())
            .count(),
        0
    );
    crate::hud_smoke::dispatch_in(
        &mut app,
        vec![UiAction::Command(application::Command::FinishReview)],
    );
    app.update();
    let next = app
        .world_mut()
        .query_filtered::<Entity, With<DioramaCamera>>()
        .single(app.world())
        .unwrap();
    assert_ne!(next, old);
    assert!(app.world().get::<Camera>(next).unwrap().is_active);
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
                pass.run_with(
                    &mut app,
                    width,
                    height,
                    vec![egui::Event::ModifiersChanged(egui::Modifiers::SHIFT)],
                );
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
                    "Shift + legend drag"
                );
                pass.run_with(
                    &mut app,
                    width,
                    height,
                    vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE)],
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
        assert!(app.world().resource::<MapSurface>().rect.is_none());
        assert!(app.world().resource::<MapSurface>().pose.is_none());
        pass.drag(
            &mut app,
            1280.0,
            800.0,
            egui::pos2(4.0, 100.0),
            egui::pos2(8.0, 140.0),
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
fn world_plants_remain_pickable_without_output_gauges() {
    use bevy::prelude::*;
    use game::{
        hud::Selection,
        sim::Session,
        world::{TuningConfig, WorldAction, WorldAssets, WorldControls, WorldScene, presented_hit},
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = diorama_app(logs.path());
    crate::support::request(&mut app, "eastern_interconnection");
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(application::Command::BeginOperating)
        .unwrap();
    app.update();
    let assets = app.world().resource::<WorldAssets>().counts();
    let checkpoint = app.world().resource::<Session>().0.checkpoint();
    let scene = app.world().resource::<WorldScene>();
    let mut model = scene.model.as_ref().unwrap().clone();
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
    let controls = app.world().resource::<WorldControls>();
    // The old beam above the roof must neither render nor intercept a click.
    let ray = Ray3d::new(plant.at + Vec3::new(10.0, 3.0, 0.0), Dir3::NEG_X);
    assert!(presented_hit(ray, &model, controls, &tuning).is_none());
    // The bare margin remains selectable via the occupied ground cell.
    let ray = Ray3d::new(plant.at + Vec3::new(0.42, 10.0, 0.42), Dir3::NEG_Y);
    assert_eq!(
        presented_hit(ray, &model, controls, &tuning)
            .unwrap()
            .selection,
        Selection::Plant(plant.id)
    );
    crate::hud_smoke::dispatch_in(
        &mut app,
        vec![game::hud::UiAction::World(WorldAction::ToggleOverlay)],
    );
    app.update();
    crate::hud_smoke::dispatch_in(
        &mut app,
        vec![game::hud::UiAction::World(WorldAction::ControlRoom)],
    );
    app.update();
    assert_eq!(app.world().resource::<WorldAssets>().counts(), assets);
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
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
    for label in [
        "Suitability: Gas plant (CCGT)",
        "Suitability: Gas peaker",
        "Suitability: Battery storage",
        "Suitability: Solar farm",
        "Suitability: Wind farm",
    ] {
        assert!(
            !widgets.iter().any(|w| w.text == label),
            "removed layer {label}"
        );
    }
    for label in [
        "Suitability: Nuclear plant",
        "Suitability: Coal plant",
        "Suitability: Hydro plant",
        "Wind potential",
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
fn world_secondary_middle_and_shift_drags_pan_without_orbiting_or_selecting() {
    use bevy_egui::egui;
    use game::{
        hud::Selection,
        sim::Session,
        world::{MapSurface, WorldControls},
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = diorama_app(logs.path());
    crate::support::request(&mut app, "eastern_interconnection");
    let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
    pass.settled(&mut app, 1280.0, 800.0);
    for (button, modifiers) in [
        (egui::PointerButton::Secondary, egui::Modifiers::NONE),
        (egui::PointerButton::Middle, egui::Modifiers::NONE),
        (egui::PointerButton::Primary, egui::Modifiers::SHIFT),
    ] {
        let run = |app: &mut bevy::prelude::App, mut events: Vec<egui::Event>| {
            events.insert(0, egui::Event::ModifiersChanged(modifiers));
            pass.run_with(app, 1280.0, 800.0, events);
        };
        let at = app.world().resource::<MapSurface>().rect.unwrap().center();
        let end = at + egui::vec2(40.0, 20.0);
        let before = app.world().resource::<WorldControls>().orbit;
        let checkpoint = app.world().resource::<Session>().0.checkpoint();
        run(
            &mut app,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button,
                    pressed: true,
                    modifiers,
                },
            ],
        );
        run(&mut app, vec![egui::Event::PointerMoved(end)]);
        run(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: end,
                button,
                pressed: false,
                modifiers,
            }],
        );
        let after = app.world().resource::<WorldControls>().orbit;
        assert_ne!(
            after.target, before.target,
            "{button:?}, {modifiers:?} drag pans"
        );
        assert_eq!(
            (after.yaw, after.elevation, after.zoom),
            (before.yaw, before.elevation, before.zoom)
        );
        assert_eq!(*app.world().resource::<Selection>(), Selection::None);
        assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
    }
    // Releasing Shift restores the ordinary primary-drag orbit gesture.
    pass.run_with(
        &mut app,
        1280.0,
        800.0,
        vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE)],
    );
    let at = app.world().resource::<MapSurface>().rect.unwrap().center();
    let before = app.world().resource::<WorldControls>().orbit;
    pass.drag(&mut app, 1280.0, 800.0, at, at + egui::vec2(30.0, 20.0));
    let after = app.world().resource::<WorldControls>().orbit;
    assert_eq!(after.target, before.target);
    assert_ne!((after.yaw, after.elevation), (before.yaw, before.elevation));
}

fn legacy_western_solar() -> std::sync::Arc<domain::region::RegionDefinition> {
    infrastructure::region::load_region_bytes(
        include_bytes!("../../../infrastructure/tests/support/global-foundations/western_interconnection-v3.ron").to_vec(),
        &domain::ids::RegionId::new("western_interconnection"),
    ).unwrap().definition
}

#[test]
fn world_legacy_sun_arc_runs_east_to_south_to_west_and_varies_with_the_month() {
    use domain::time::SimMonth;
    let solar = &legacy_western_solar();
    for month in [1, 6, 12] {
        let month = SimMonth::new(2027, month).unwrap();
        let domain::region::SolarModel::DaylightTable(table) = &solar.solar().model else {
            unreachable!()
        };
        let (rise, set) = table[month.index0()];
        let at = |fraction: f64| {
            game::world::sun_position(
                solar,
                month,
                ((rise + (set - rise) * fraction) * 60.0).round() as u32,
            )
        };
        let (morning, noon, evening) = (at(0.25), at(0.5), at(0.75));
        assert!(morning.toward_sun.x > 0.0);
        assert!(evening.toward_sun.x < 0.0);
        assert!(noon.toward_sun.x.abs() < 0.01 && noon.toward_sun.z > 0.0);
        assert!(noon.toward_sun.y > morning.toward_sun.y);
        assert!(noon.strength > 0.99);
        assert_eq!(game::world::sun_position(solar, month, 0).strength, 0.0);
        assert_eq!(game::world::sun_position(solar, month, 1440).strength, 0.0);
    }
    let noon = |m| {
        let month = SimMonth::new(2027, m).unwrap();
        let domain::region::SolarModel::DaylightTable(table) = &solar.solar().model else {
            unreachable!()
        };
        let (rise, set) = table[month.index0()];
        game::world::sun_position(solar, month, ((rise + set) * 30.0).round() as u32)
    };
    assert!(noon(6).toward_sun.y > noon(12).toward_sun.y);
}

proptest! {
    #[test]
    fn world_legacy_sun_arc_is_finite_unit_length_and_dark_outside_daylight(month in 1u8..13, tick in 0u32..=1440) {
        let solar = legacy_western_solar();
        let month = domain::time::SimMonth::new(2027, month).unwrap();
        let sun = game::world::sun_position(&solar, month, tick);
        prop_assert!(sun.toward_sun.is_finite());
        prop_assert!((sun.toward_sun.length() - 1.0).abs() < 1e-5);
        prop_assert!((0.0..=1.0).contains(&sun.strength));
        let domain::region::SolarModel::DaylightTable(table) = &solar.solar().model else { unreachable!() };
        let (rise, set) = table[month.index0()];
        let hour = f64::from(tick) / 60.0;
        if hour <= rise || hour >= set { prop_assert_eq!(sun.strength, 0.0); }
        else { prop_assert!(sun.strength > 0.0 && sun.toward_sun.y > 0.0); }
    }
}

#[test]
fn world_sunlight_changes_direction_with_the_operating_clock() {
    use application::Command;
    use bevy::prelude::*;
    use game::{hud::UiAction, sim::Session};
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = diorama_app(logs.path());
    app.insert_resource(GlobalAmbientLight::default());
    crate::support::request(&mut app, "western_interconnection");
    crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::Command(Command::BeginOperating)]);
    app.update();
    let direction = |app: &mut App| {
        *app.world_mut()
            .query_filtered::<&Transform, With<DirectionalLight>>()
            .single(app.world())
            .unwrap()
            .forward()
    };
    crate::hud_smoke::dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::AdvanceTicks { n: 9 * 60 })],
    );
    app.update();
    let morning = direction(&mut app);
    crate::hud_smoke::dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::AdvanceTicks { n: 6 * 60 })],
    );
    app.update();
    let afternoon = direction(&mut app);
    assert!(
        morning.x < 0.0,
        "morning sunlight travels west: {morning:?}"
    );
    assert!(
        afternoon.x > 0.0,
        "afternoon sunlight travels east: {afternoon:?}"
    );
    let checkpoint = app.world().resource::<Session>().0.checkpoint();
    crate::hud_smoke::dispatch_in(
        &mut app,
        vec![UiAction::SetSpeed(game::sim::SimSpeed::Paused)],
    );
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs(2),
    ));
    app.update();
    assert_eq!(direction(&mut app), afternoon, "pausing freezes the sun");
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
    crate::hud_smoke::dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::AdvanceTicks { n: 9 * 60 })],
    );
    app.update();
    let light = app
        .world_mut()
        .query::<&DirectionalLight>()
        .single(app.world())
        .unwrap();
    assert_eq!(light.illuminance, 0.0);
    assert!(!light.shadow_maps_enabled);
    assert!(app.world().resource::<GlobalAmbientLight>().brightness >= 2000.0);
    crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::Command(Command::FinishReview)]);
    app.update();
    assert!(
        direction(&mut app).z < 0.0,
        "Planning previews the sun in the south"
    );
    let light = app
        .world_mut()
        .query::<&DirectionalLight>()
        .single(app.world())
        .unwrap();
    // Independent spherical calculation for the February 15 preview: Seattle's
    // cell centre is (47.8 N, 122.170833333 W), PT=-480, solar noon rounds
    // to 12:09. delta=-23.44*cos(2*pi*56/365); H=0.079166667 degrees.
    // Direct east/up/south components give the following vector and sin(elevation).
    assert!((light.illuminance - 6751.719).abs() < 0.1);
    assert!(light.shadow_maps_enabled);
    let toward_sun = -direction(&mut app);
    assert!(toward_sun.abs_diff_eq(Vec3::new(-0.00134429, 0.48226565, 0.876024), 1e-5));
}

#[test]
fn world_operating_overlay_preserves_lit_terrain_and_building_shading() {
    use application::Command;
    use bevy::prelude::*;
    use game::{
        hud::UiAction,
        sim::Session,
        world::{ViewSync, WorldAction, WorldAssets, WorldEntity},
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
        for light in app
            .world_mut()
            .query::<&DirectionalLight>()
            .iter(app.world())
        {
            assert_eq!(light.illuminance, 0.0, "the day begins at midnight");
            assert!(!light.shadow_maps_enabled);
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

#[test]
fn world_city_windows_follow_supply_without_flattening_or_glowing_walls() {
    use bevy::prelude::*;
    use game::{
        sim::Session,
        world::{ViewSync, WorldAssets, WorldControls, WorldScene},
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = diorama_app(logs.path());
    crate::support::request(&mut app, "eastern_interconnection");
    let windows = app.world().resource::<ViewSync>().city_windows.clone();
    assert!(!windows.is_empty());
    assert!(
        windows
            .values()
            .flatten()
            .all(|e| *app.world().get::<Visibility>(*e).unwrap() == Visibility::Hidden)
    );
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(application::Command::BeginOperating)
        .unwrap();
    app.update();
    let checkpoint = app.world().resource::<Session>().0.checkpoint();
    let assets = app.world().resource::<WorldAssets>().counts();
    let model = app
        .world()
        .resource::<WorldScene>()
        .model
        .as_ref()
        .unwrap()
        .clone();
    for served in [1.0, 0.5, 0.0] {
        app.world_mut()
            .resource_mut::<WorldScene>()
            .model
            .as_mut()
            .unwrap()
            .served = served;
        // Effects off must retain readable window colours without relying on bloom.
        app.world_mut().resource_mut::<WorldControls>().effects = false;
        app.update();
        for (city, entities) in &windows {
            let c = &model.cities[city];
            let mut lit = 0;
            for entity in entities {
                let world = app.world();
                let material = world
                    .resource::<Assets<StandardMaterial>>()
                    .get(
                        &world
                            .get::<MeshMaterial3d<StandardMaterial>>(*entity)
                            .unwrap()
                            .0,
                    )
                    .unwrap();
                let visibility = *world.get::<Visibility>(*entity).unwrap();
                assert_eq!(visibility == Visibility::Visible, c.connected);
                assert!(
                    material.unlit,
                    "windows must remain visible without sunlight"
                );
                assert_eq!(
                    *world.get::<Transform>(*entity).unwrap(),
                    Transform::from_translation(c.at).with_scale(c.visual_scale(1.0))
                );
                if material.base_color.to_linear().red > 0.1 {
                    lit += 1;
                    let colour = material.base_color.to_srgba();
                    assert!(colour.red > colour.blue * 2.0, "windows glow warm yellow");
                }
            }
            assert_eq!(lit as f32 / entities.len() as f32, served);
            let world = app.world();
            let wall = world.resource::<ViewSync>().cities[city];
            let material = world
                .resource::<Assets<StandardMaterial>>()
                .get(
                    &world
                        .get::<MeshMaterial3d<StandardMaterial>>(wall)
                        .unwrap()
                        .0,
                )
                .unwrap();
            assert!(!material.unlit);
            assert_eq!(material.emissive, LinearRgba::BLACK, "only windows glow");
        }
    }
    app.world_mut().resource_mut::<WorldControls>().overlay = false;
    app.update();
    assert!(
        windows
            .values()
            .flatten()
            .all(|e| *app.world().get::<Visibility>(*e).unwrap() == Visibility::Hidden)
    );
    assert_eq!(app.world().resource::<WorldAssets>().counts(), assets);
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
}

#[test]
fn world_overlapping_lines_animate_only_one_net_direction() {
    use bevy::prelude::*;
    use game::{
        sim::{Session, SimSpeed},
        world::{FlowPulse, WorldScene},
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = diorama_app(logs.path());
    crate::support::request(&mut app, "western_interconnection");
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(application::Command::BeginOperating)
        .unwrap();
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(application::Command::AdvanceTicks { n: 60 })
        .unwrap();
    app.update();
    let model = app.world().resource::<WorldScene>().model.as_ref().unwrap();
    let seattle = model
        .cities
        .values()
        .find(|c| c.name_key == "city-seattle")
        .unwrap()
        .at
        .xz();
    let hydro = model
        .plants
        .values()
        .find(|p| p.kind == domain::region::PlantKind::Hydro)
        .unwrap()
        .at
        .xz();
    let length = seattle.distance(hydro);
    let along = (hydro - seattle).normalize();
    let positions = |app: &mut App| -> std::collections::BTreeMap<Entity, Vec2> {
        app.world_mut()
            .query_filtered::<(Entity, &Transform, &Visibility), With<FlowPulse>>()
            .iter(app.world())
            .filter(|(_, _, v)| **v == Visibility::Visible)
            .map(|(e, t, _)| (e, t.translation.xz()))
            .collect()
    };
    let before = positions(&mut app);
    app.insert_resource(SimSpeed::X1);
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(50),
    ));
    app.update();
    let mut directions = std::collections::BTreeSet::new();
    for (entity, after) in positions(&mut app) {
        let Some(before) = before.get(&entity) else {
            continue;
        };
        let offset = *before - seattle;
        let progress = offset.dot(along);
        let movement = (after - *before).dot(along);
        if progress > 0.3
            && progress < length - 0.3
            && (offset - along * progress).length() < 0.01
            && movement.abs() > 1e-4
            && movement.abs() < 0.2
        {
            directions.insert(movement > 0.0);
        }
    }
    assert_eq!(
        directions.len(),
        1,
        "shared Seattle–Grand Coulee stretch has counter-flowing dots: {directions:?}"
    );
    let checkpoint = app.world().resource::<Session>().0.checkpoint();
    for path in app
        .world_mut()
        .resource_mut::<WorldScene>()
        .model
        .as_mut()
        .unwrap()
        .flow_paths
        .values_mut()
    {
        path.net_flow = 0.0;
    }
    app.update();
    assert!(
        positions(&mut app).is_empty(),
        "zero net transfer hides all dots even on energized lines"
    );
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
}

#[test]
fn world_flow_pulses_animate_freeze_hide_and_leave_simulation_and_assets_unchanged() {
    use application::Command;
    use bevy::prelude::*;
    use game::{
        hud::UiAction,
        sim::{Session, SimSpeed},
        world::{FlowPulse, WorldAction, WorldAssets, WorldScene},
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = diorama_app(logs.path());
    crate::support::request(&mut app, "western_interconnection");
    let visible = |app: &mut App| -> Vec<(Entity, Transform)> {
        app.world_mut()
            .query_filtered::<(Entity, &Transform, &Visibility), With<FlowPulse>>()
            .iter(app.world())
            .filter(|(_, _, v)| **v == Visibility::Visible)
            .map(|(e, t, _)| (e, *t))
            .collect()
    };
    assert!(visible(&mut app).is_empty(), "Planning has no pulses");
    for handle in app
        .world_mut()
        .query_filtered::<&MeshMaterial3d<StandardMaterial>, With<FlowPulse>>()
        .iter(app.world())
    {
        let material = app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(&handle.0)
            .unwrap();
        let blue = material.base_color.to_srgba();
        assert!(
            material.base_color.to_linear().blue > 1.0,
            "unlit flow dots need HDR base colour for glow; emissive is ignored"
        );
        assert!(
            blue.blue > 0.9 && blue.blue > 4.0 * blue.red && blue.green > blue.blue * 0.7,
            "flow dots must use the lines' bright cyan blue: {blue:?}"
        );
        assert!(
            material.unlit,
            "flow dots must stay bright without sunlight"
        );
    }
    crate::hud_smoke::dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::BeginOperating),
            UiAction::Command(Command::AdvanceTicks { n: 60 }),
        ],
    );
    app.update();
    let before = visible(&mut app);
    assert!(!before.is_empty(), "generating plants supply cities");
    let world = app.world();
    // Submarine routes preserve their dark vertex colours instead of using the inland
    // energized material. Compare the cyan dot against an inland line's deeper blue.
    let model = world.resource::<WorldScene>().model.as_ref().unwrap();
    let pulse = before
        .iter()
        .map(|(entity, _)| *entity)
        .find(|entity| {
            let key = world.get::<FlowPulse>(*entity).unwrap().path;
            !model.lines[&model.flow_paths[&key].line].submarine
        })
        .expect("an energized inland route");
    let key = world.get::<FlowPulse>(pulse).unwrap().path;
    let line_id = world
        .resource::<WorldScene>()
        .model
        .as_ref()
        .unwrap()
        .flow_paths[&key]
        .line;
    let line = world.resource::<game::world::ViewSync>().lines[&line_id];
    let colour = |entity| {
        world
            .resource::<Assets<StandardMaterial>>()
            .get(
                &world
                    .get::<MeshMaterial3d<StandardMaterial>>(entity)
                    .unwrap()
                    .0,
            )
            .unwrap()
            .base_color
            .to_linear()
    };
    let dot_colour = colour(pulse);
    let line_colour = colour(line);
    assert!(
        line_colour.blue > line_colour.green * 3.0,
        "energized lines use the dots' deeper blue"
    );
    assert!(
        dot_colour.green / dot_colour.blue > line_colour.green / line_colour.blue,
        "moving dots are cyan against deeper blue lines"
    );
    for (entity, _) in &before {
        let path = app.world().get::<FlowPulse>(*entity).unwrap().path;
        let model = app.world().resource::<WorldScene>().model.as_ref().unwrap();
        assert!(model.flow_paths[&path].net_flow.abs() > 1e-9 && model.flow_paths[&path].energized);
    }
    let checkpoint = app.world().resource::<Session>().0.checkpoint();
    let assets = app.world().resource::<WorldAssets>().counts();
    // Presentation frames move pulses without requesting a simulation tick.
    app.insert_resource(SimSpeed::X1);
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(100),
    ));
    app.update();
    let moving = visible(&mut app);
    assert_ne!(moving, before);
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
    app.insert_resource(SimSpeed::Paused);
    app.update();
    assert_eq!(visible(&mut app), moving, "pause freezes the pulses");
    crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::World(WorldAction::ToggleOverlay)]);
    app.update();
    assert!(visible(&mut app).is_empty());
    crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::World(WorldAction::ControlRoom)]);
    app.update();
    assert_eq!(visible(&mut app), moving);
    crate::hud_smoke::dispatch_in(
        &mut app,
        vec![UiAction::SetLayer(game::hud::MapLayer::Terrain)],
    );
    app.update();
    assert!(
        visible(&mut app).is_empty(),
        "resource views hide flow pulses"
    );
    crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::World(WorldAction::ControlRoom)]);
    app.update();
    assert_eq!(visible(&mut app), moving);
    assert_eq!(app.world().resource::<WorldAssets>().counts(), assets);
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
    // Remove generation using normal commands; AGC must not immediately restart it.
    let plants: Vec<_> = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .plant_views()
        .into_iter()
        .filter(|p| p.output.value() > 0.0)
        .map(|p| p.id)
        .collect();
    let actions = plants
        .into_iter()
        .flat_map(|plant| {
            [
                UiAction::Command(Command::SetAutoFollow { plant, on: false }),
                UiAction::Command(Command::StopPlant { plant }),
            ]
        })
        .chain([UiAction::Command(Command::AdvanceTicks { n: 60 })])
        .collect();
    crate::hud_smoke::dispatch_in(&mut app, actions);
    app.update();
    assert_eq!(
        app.world()
            .resource::<Session>()
            .0
            .session()
            .grid_snapshot()
            .mode,
        domain::grid::GridMode::Blackout
    );
    assert!(
        visible(&mut app).is_empty(),
        "a blackout carries no illustrative power"
    );
    let session = app.world().resource::<Session>().0.session();
    let model = app.world().resource::<WorldScene>().model.as_ref().unwrap();
    for city in model.cities.values().filter(|c| c.connected) {
        let detail = session.city_detail(city.id).unwrap();
        assert_eq!(detail.served_now.unwrap().value(), 0.0);
        assert!(detail.demand_now.unwrap().value() > 0.0);
        assert!(
            city.scale > 0.0,
            "a blackout dims cities without flattening demand"
        );
    }
    crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::ReturnToMenu]);
    crate::support::update_n(&mut app, 3);
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<FlowPulse>>()
            .iter(app.world())
            .count(),
        0
    );
}

#[test]
fn world_computed_sun_points_north_in_the_south_and_is_finite_overhead() {
    use domain::region::{GeoBounds, RegionDefinition, SolarModel, fixtures};
    let month = domain::time::SimMonth::new(2027, 1).unwrap();
    let mut parts = fixtures::tiny_region_parts();
    parts.solar.model = SolarModel::Astronomical;
    parts.geo = GeoBounds {
        west: -1.0,
        east: 1.0,
        south: -25.0,
        north: -24.0,
    };
    parts.clock.zones[0].standard_offset_minutes = 0;
    let region = RegionDefinition::try_from(parts).unwrap();
    let sun = game::world::sun_position(&region, month, 720);
    assert!(sun.toward_sun.z < 0.0);
    assert!(sun.toward_sun.y > 0.9);
    assert!(sun.toward_sun.is_finite());
    assert!((sun.toward_sun.length() - 1.0).abs() < 1e-5);
    assert!((sun.strength - sun.toward_sun.y).abs() < 1e-6);

    let mut parts = fixtures::tiny_region_parts();
    parts.solar.model = SolarModel::Astronomical;
    let home = parts
        .cities
        .iter()
        .find(|c| c.id == parts.start.home_city)
        .unwrap()
        .cell;
    let dec = domain::generation::declination_deg(month);
    parts.geo = GeoBounds {
        west: -f64::from(home.x) - 0.5,
        east: 12.0 - f64::from(home.x) - 0.5,
        north: dec + f64::from(home.y) + 0.5,
        south: dec + f64::from(home.y) + 0.5 - 8.0,
    };
    parts.clock.zones[0].standard_offset_minutes = 0;
    let region = RegionDefinition::try_from(parts).unwrap();
    let overhead = game::world::sun_position(&region, month, 720);
    assert!(overhead.toward_sun.is_finite());
    assert!(overhead.toward_sun.y > 0.9999);
    assert!(overhead.strength > 0.9999);
}

// Plan 7 frame budgets at the largest grid a region may ship (256 x 128).
mod max_grid {
    use application::{
        Game, InMemoryCommandLog, LoadedRegion, RegionCatalog, RegionFingerprint, RegionListing,
        RegionLoadError,
    };
    use bevy::prelude::*;
    use domain::{
        ids::{CellCoord, RegionId},
        region::fixtures,
        session::GameSettings,
    };
    use game::{
        hud::{MapLayer, Selection},
        sim::Session,
        world::{
            MapSurface, OrbitCamera, TerrainGeometry, WorldModel, WorldScene, pick_world,
            region_terrain, region_terrain_colors, surface_height,
        },
    };
    use std::{sync::Arc, time::Instant};

    struct MaxGridCatalog;
    impl RegionCatalog for MaxGridCatalog {
        fn list(&self) -> Result<Vec<RegionListing>, RegionLoadError> {
            Ok(Vec::new())
        }
        fn load(&self, _: &RegionId) -> Result<LoadedRegion, RegionLoadError> {
            Ok(LoadedRegion {
                definition: Arc::new(fixtures::max_grid_region()),
                source_bytes: Vec::new(),
                fingerprint: RegionFingerprint::new([0; 32]),
            })
        }
    }

    fn max_grid_game() -> Game {
        Game::start_new(
            &MaxGridCatalog,
            &RegionId::new("max-grid"),
            7,
            GameSettings::default(),
            Box::new(InMemoryCommandLog::default()),
        )
        .unwrap()
    }

    #[test]
    fn the_max_grid_fixture_is_the_largest_shippable_region() {
        let region = fixtures::max_grid_region();
        let terrain = region.terrain();
        assert_eq!((terrain.width(), terrain.height()), (256, 128));
        assert_eq!(region.cities().len(), 40);
        assert!(matches!(
            region.solar().model,
            domain::region::SolarModel::Astronomical
        ));
        // Varied relief: many distinct heights, so the owned-wall rule has real work to do.
        let heights: std::collections::BTreeSet<u64> = (0..terrain.height())
            .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
            .filter_map(|at| terrain.get(at))
            .map(|cell| cell.height.value().to_bits())
            .collect();
        assert!(heights.len() > 1_000, "{} distinct heights", heights.len());
    }

    #[test]
    fn terrain_mesh_is_built_once_per_region() {
        let logs = tempfile::TempDir::new().unwrap();
        let mut app = super::diorama_app(logs.path());
        app.world_mut().insert_resource(Session(max_grid_game()));
        app.update();
        let region = Arc::clone(app.world().resource::<Session>().0.session().region());
        let builds = |app: &App| app.world().resource::<WorldScene>().terrain_builds;
        assert_eq!(builds(&app), 1, "the region's mesh is built on load");
        let size = app
            .world()
            .resource::<WorldScene>()
            .model
            .as_ref()
            .unwrap()
            .size();
        let layers: Vec<MapLayer> = MapLayer::all().collect();
        for frame in 0..30 {
            let t = frame as f32 / 30.0;
            let pose = OrbitCamera {
                yaw: t * 2.0,
                zoom: 1.0 - 0.5 * t,
                target: Vec2::new(t * 40.0 - 20.0, 10.0 - t * 20.0),
                ..default()
            }
            .pose(size, 1.6);
            app.world_mut().resource_mut::<MapSurface>().pose = Some(pose);
            // Changing the overlay recolours the mesh; it must never rebuild its geometry.
            if frame % 5 == 0 {
                *app.world_mut().resource_mut::<MapLayer>() = layers[(frame / 5) % layers.len()];
            }
            app.update();
            if frame % 5 == 0 {
                // The recoloured mesh is the picture a fresh build would draw for that layer.
                let layer = *app.world().resource::<MapLayer>();
                let expected = region_terrain(&region, layer).unwrap();
                let world = app.world_mut();
                let mesh = world
                    .query::<&Mesh3d>()
                    .iter(world)
                    .filter_map(|handle| world.resource::<Assets<Mesh>>().get(&handle.0))
                    .find(|mesh| mesh.count_vertices() == expected.positions.len())
                    .expect("the terrain mesh is in the scene");
                let Some(bevy::mesh::VertexAttributeValues::Float32x4(colors)) =
                    mesh.attribute(Mesh::ATTRIBUTE_COLOR)
                else {
                    panic!("the terrain mesh has RGBA vertex colours");
                };
                assert_eq!(colors, &expected.colors, "{layer:?}");
            }
        }
        assert_eq!(
            builds(&app),
            1,
            "camera motion and overlay changes reuse the region's mesh"
        );
    }

    #[test]
    fn max_grid_mesh_stays_within_vertex_bound() {
        let region = fixtures::max_grid_region();
        let terrain = region.terrain();
        let (width, height) = (i32::from(terrain.width()), i32::from(terrain.height()));
        let cells = (width * height) as usize;
        let top = |x: i32, y: i32| {
            surface_height(
                terrain,
                CellCoord {
                    x: x as u16,
                    y: y as u16,
                },
            )
        };
        // The owned-wall rule: only the higher cell of a pair owns the face, and the grid's
        // outer edge stands on the base, so every edge cell owns its outer face.
        let mut walls = 0usize;
        for y in 0..height {
            for x in 0..width {
                let h = top(x, y).unwrap();
                for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                    let inside = nx >= 0 && ny >= 0 && nx < width && ny < height;
                    let low = if inside { top(nx, ny).unwrap() } else { -0.25 };
                    walls += usize::from(h > low);
                }
            }
        }
        assert!(walls > cells, "relief leaves only {walls} wall faces");
        let palette = vec![[90, 120, 70]; cells];
        let mesh = TerrainGeometry::build(terrain, &palette).unwrap();
        // The bound is 4 x cells + 4 x wall faces; the closed base adds one more quad.
        let bound = 4 * cells + 4 * walls;
        assert!(mesh.positions.len() <= bound + 4);
        assert_eq!(mesh.positions.len(), bound + 4);
        assert_eq!(mesh.normals.len(), mesh.positions.len());
        assert_eq!(mesh.colors.len(), mesh.positions.len());
        assert_eq!(mesh.indices.len(), mesh.positions.len() / 4 * 6);
        assert!(
            mesh.indices
                .iter()
                .all(|&i| (i as usize) < mesh.positions.len())
        );
    }

    #[test]
    fn recolouring_replays_the_walls_a_build_gave_each_cell() {
        let region = fixtures::max_grid_region();
        let terrain = region.terrain();
        let cells = usize::from(terrain.width()) * usize::from(terrain.height());
        let palette = |salt: usize| -> Vec<[u8; 3]> {
            (0..cells)
                .map(|i| [(i * 7 + salt) as u8, (i * 13) as u8, (i / 3 + salt) as u8])
                .collect()
        };
        let mesh = TerrainGeometry::build(terrain, &palette(0)).unwrap();
        // The same palette recolours to exactly what a build gave it ...
        assert_eq!(mesh.colors_of(&palette(0)).unwrap(), mesh.colors);
        // ... and another palette to exactly what a fresh build would, geometry untouched.
        let other = TerrainGeometry::build(terrain, &palette(5)).unwrap();
        assert_eq!(mesh.colors_of(&palette(5)).unwrap(), other.colors);
        assert_eq!(mesh.positions, other.positions);
        assert!(matches!(
            mesh.colors_of(&palette(0)[1..]),
            Err(game::world::TerrainGeometryError::PaletteLength { .. })
        ));
    }

    fn median(mut samples: Vec<f64>) -> f64 {
        samples.sort_by(f64::total_cmp);
        samples[samples.len() / 2]
    }

    fn millis(mut work: impl FnMut()) -> f64 {
        let start = Instant::now();
        work();
        start.elapsed().as_secs_f64() * 1e3
    }

    /// Run on demand with the workspace crates optimised, `--release` or, to spare the disk,
    /// `CARGO_PROFILE_DEV_OPT_LEVEL=3 CARGO_PROFILE_DEV_DEBUG=0 cargo nextest run -p game
    /// --run-ignored ignored-only -E 'test(max_grid_frame_budgets)'`. The budgets are for an
    /// optimised build on the development Mac; the default test profile compiles the workspace
    /// crates unoptimised, several times slower, and misses the overlay and picker budgets.
    #[test]
    #[ignore = "timing budget; run with optimised workspace crates"]
    fn max_grid_frame_budgets() {
        const RUNS: usize = 100;
        let region = fixtures::max_grid_region();
        let terrain = region.terrain();
        let cells = usize::from(terrain.width()) * usize::from(terrain.height());
        let palette = vec![[90, 120, 70]; cells];

        // Once per region load.
        let build = median(
            (0..RUNS)
                .map(|_| {
                    millis(|| {
                        std::hint::black_box(TerrainGeometry::build(terrain, &palette).unwrap());
                    })
                })
                .collect(),
        );

        // Per click or hover: rays from a spread of screen points, hits and misses alike.
        let game = max_grid_game();
        let model = WorldModel::of(&game);
        let geometry = region_terrain(&model.region, MapLayer::Terrain).unwrap();
        let pose = OrbitCamera::default().pose(model.size(), 1.6);
        let rays: Vec<Ray3d> = (0..RUNS)
            .map(|i| {
                let (u, v) = ((i % 10) as f32 / 6.0 - 0.75, (i / 10) as f32 / 6.0 - 0.75);
                pose.ray(Vec2::new(u, v)).unwrap()
            })
            .collect();
        let mut picked = 0;
        let pick = median(
            rays.iter()
                .map(|ray| {
                    millis(|| {
                        let hit = pick_world(*ray, &model, &geometry, 1.0);
                        picked += usize::from(hit != Selection::None);
                        std::hint::black_box(hit);
                    })
                })
                .collect(),
        );
        assert!(
            picked > RUNS / 2,
            "only {picked} of {RUNS} rays land on the grid"
        );

        // Switching the map layer recolours the terrain mesh in place: the slowest layer's
        // median counts.
        let overlay = MapLayer::all()
            .map(|layer| {
                median(
                    (0..RUNS)
                        .map(|_| {
                            millis(|| {
                                std::hint::black_box(
                                    region_terrain_colors(&geometry, &region, layer).unwrap(),
                                );
                            })
                        })
                        .collect(),
                )
            })
            .fold(0.0, f64::max);

        // One paint of the region picker (globe, every shipped outline, cards) at the smallest
        // window: layout and tessellation, on a context that has already built its fonts.
        let picker = picker_paint_median(RUNS);

        eprintln!(
            "max grid medians over {RUNS} runs: build {build:.3} ms (budget 250), pick {pick:.4} ms \
             (1), overlay refresh {overlay:.3} ms (4), picker paint {picker:.3} ms (4)"
        );
        assert!(build <= 250.0, "TerrainGeometry::build median {build} ms");
        assert!(pick <= 1.0, "pick_world median {pick} ms");
        assert!(overlay <= 4.0, "overlay refresh median {overlay} ms");
        assert!(picker <= 4.0, "region picker paint median {picker} ms");
    }

    fn picker_paint_median(runs: usize) -> f64 {
        use crate::support::egui_harness::central;
        use bevy_egui::egui;
        use game::hud::{
            MenuForm, RegionAtlas, RegionListings, draw_region_select, region_select_model,
        };
        let catalog = infrastructure::region::RonRegionCatalog::new(
            crate::support::shipped_assets().join("regions"),
        );
        let listings = RegionListings(catalog.list());
        let l10n = crate::support::shipped_l10n();
        let model = region_select_model(&listings, Some(42), &l10n)
            .with_atlas(&RegionAtlas::load(&catalog, &listings), &l10n);
        assert_eq!(
            model.regions.len(),
            crate::support::SHIPPED_REGIONS.len(),
            "every shipped region is drawn"
        );
        let minimum = game::window::MIN_WINDOW_SIZE;
        let ctx = egui::Context::default();
        let mut form = MenuForm::new(42);
        let mut paint = || {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    minimum.to_array().into(),
                )),
                ..egui::RawInput::default()
            };
            let output = ctx.run_ui(input, |ui| {
                central(ui, |ui| draw_region_select(ui, &model, &mut form));
            });
            let meshes = ctx.tessellate(output.shapes.clone(), output.pixels_per_point);
            std::hint::black_box(meshes);
            output.drop_without_applying_deltas();
        };
        // Fonts and the globe's cached artwork exist after the first frames.
        for _ in 0..3 {
            paint();
        }
        median((0..runs).map(|_| millis(&mut paint)).collect())
    }
}
