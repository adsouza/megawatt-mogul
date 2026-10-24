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
                    scale,
                ),
                None => (
                    Selection::City(city.id),
                    city.at,
                    city_parts(),
                    city.scale * scale,
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
                                        * c.scale,
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

fn western_solar() -> domain::region::SolarParams {
    use application::RegionCatalog;
    let catalog = infrastructure::region::RonRegionCatalog::new(
        crate::support::shipped_assets().join("regions"),
    );
    catalog
        .load(&domain::ids::RegionId::new("western_interconnection"))
        .unwrap()
        .definition
        .solar()
        .clone()
}

#[test]
fn world_sun_arc_runs_east_to_south_to_west_and_varies_with_the_month() {
    use domain::time::SimMonth;
    let solar = &western_solar();
    for month in [1, 6, 12] {
        let month = SimMonth::new(2027, month).unwrap();
        let (rise, set) = solar.daylight[month.index0()];
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
        let (rise, set) = solar.daylight[month.index0()];
        game::world::sun_position(solar, month, ((rise + set) * 30.0).round() as u32)
    };
    assert!(noon(6).toward_sun.y > noon(12).toward_sun.y);
}

proptest! {
    #[test]
    fn world_sun_arc_is_finite_unit_length_and_dark_outside_daylight(month in 1u8..13, tick in 0u32..=1440) {
        let solar = western_solar();
        let month = domain::time::SimMonth::new(2027, month).unwrap();
        let sun = game::world::sun_position(&solar, month, tick);
        prop_assert!(sun.toward_sun.is_finite());
        prop_assert!((sun.toward_sun.length() - 1.0).abs() < 1e-5);
        prop_assert!((0.0..=1.0).contains(&sun.strength));
        let (rise, set) = solar.daylight[month.index0()];
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
    assert!(light.illuminance > 10000.0 && light.shadow_maps_enabled);
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
fn world_flow_arrows_animate_freeze_hide_and_leave_simulation_and_assets_unchanged() {
    use application::Command;
    use bevy::prelude::*;
    use game::{
        hud::UiAction,
        sim::{Session, SimSpeed},
        world::{FlowArrow, WorldAction, WorldAssets, WorldScene},
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = diorama_app(logs.path());
    crate::support::request(&mut app, "western_interconnection");
    let visible = |app: &mut App| -> Vec<(Entity, Transform)> {
        app.world_mut()
            .query_filtered::<(Entity, &Transform, &Visibility), With<FlowArrow>>()
            .iter(app.world())
            .filter(|(_, _, v)| **v == Visibility::Visible)
            .map(|(e, t, _)| (e, *t))
            .collect()
    };
    assert!(visible(&mut app).is_empty(), "Planning has no arrows");
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
    for (entity, _) in &before {
        let line = app.world().get::<FlowArrow>(*entity).unwrap().line;
        let model = app.world().resource::<WorldScene>().model.as_ref().unwrap();
        assert!(
            model.lines[&line].flow.is_some()
                && model.lines[&line].energized
                && model.lines[&line].building.is_none()
        );
    }
    let checkpoint = app.world().resource::<Session>().0.checkpoint();
    let assets = app.world().resource::<WorldAssets>().counts();
    // Presentation frames move arrows without requesting a simulation tick.
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
    assert_eq!(visible(&mut app), moving, "pause freezes the arrows");
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
        "resource views hide flow arrows"
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
    crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::ReturnToMenu]);
    crate::support::update_n(&mut app, 3);
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<FlowArrow>>()
            .iter(app.world())
            .count(),
        0
    );
}
