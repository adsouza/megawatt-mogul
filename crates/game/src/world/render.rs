//! Session-scoped render assets and ID-based reconciliation. Headless-safe; no GPU reads.
use super::{
    Part, TerrainGeometry, TuningConfig, WorldControls, WorldModel, city_parts, plant_parts,
};
use crate::{
    hud::{MapLayer, Selection},
    sim::{Session, SessionGeneration},
};
use bevy::ecs::system::SystemParam;
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use domain::{
    ids::{CityId, LineId, PlantId},
    region::PlantKind,
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Component)]
pub struct WorldEntity;
#[derive(Component)]
pub(super) struct Pillar(PlantId);
#[derive(Component)]
pub(super) struct SelectionMarker;
#[derive(Resource, Default)]
pub struct ViewSync {
    pub plants: BTreeMap<PlantId, Entity>,
    pub cities: BTreeMap<CityId, Entity>,
    pub lines: BTreeMap<LineId, Entity>,
    pillars: BTreeMap<PlantId, Entity>,
    arrows: BTreeMap<LineId, Vec<Entity>>,
}
#[derive(Resource, Default)]
pub struct WorldScene {
    pub model: Option<WorldModel>,
    pub geometry: Option<TerrainGeometry>,
    pub generation: Option<u64>,
    pub revision: u64,
}
#[derive(Resource, Default)]
pub struct WorldAssets {
    meshes: Vec<Handle<Mesh>>,
    materials: Vec<Handle<StandardMaterial>>,
    terrain: Option<Handle<Mesh>>,
    plant_meshes: BTreeMap<PlantKind, Handle<Mesh>>,
    city: Handle<Mesh>,
    cube: Handle<Mesh>,
    ring: Handle<Mesh>,
    arrow: Handle<Mesh>,
    line_meshes: BTreeMap<LineId, Handle<Mesh>>,
    layer: Option<MapLayer>,
}
impl WorldAssets {
    pub fn counts(&self) -> (usize, usize) {
        (
            self.meshes.len() + self.line_meshes.len(),
            self.materials.len(),
        )
    }
}
const TERRAIN: usize = 0;
const ONLINE: usize = 1;
const OFFLINE: usize = 2;
const BUILDING: usize = 3;
const CITY: usize = 4;
const LINE: usize = 5;
const ENERGIZED: usize = 6;
const BEAM: usize = 7;
const SELECTED: usize = 8;
const FLOW: usize = 9;

pub fn mesh_of(geometry: &TerrainGeometry) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, geometry.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, geometry.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, geometry.colors.clone())
    .with_inserted_indices(Indices::U32(geometry.indices.clone()))
}
fn parts_geometry(parts: &[Part]) -> TerrainGeometry {
    let mut geometry = TerrainGeometry::default();
    for part in parts {
        let half = part.size * 0.5;
        for (normal, corners) in [
            (
                Vec3::Y,
                [[-1., 1., -1.], [-1., 1., 1.], [1., 1., 1.], [1., 1., -1.]],
            ),
            (
                Vec3::NEG_Y,
                [
                    [-1., -1., 1.],
                    [-1., -1., -1.],
                    [1., -1., -1.],
                    [1., -1., 1.],
                ],
            ),
            (
                Vec3::X,
                [[1., -1., -1.], [1., 1., -1.], [1., 1., 1.], [1., -1., 1.]],
            ),
            (
                Vec3::NEG_X,
                [
                    [-1., -1., 1.],
                    [-1., 1., 1.],
                    [-1., 1., -1.],
                    [-1., -1., -1.],
                ],
            ),
            (
                Vec3::Z,
                [[1., -1., 1.], [1., 1., 1.], [-1., 1., 1.], [-1., -1., 1.]],
            ),
            (
                Vec3::NEG_Z,
                [
                    [-1., -1., -1.],
                    [-1., 1., -1.],
                    [1., 1., -1.],
                    [1., -1., -1.],
                ],
            ),
        ] {
            let start = geometry.positions.len() as u32;
            for corner in corners {
                geometry.positions.push(
                    (part.center + part.rotation * (Vec3::from_array(corner) * half)).to_array(),
                );
                geometry.normals.push((part.rotation * normal).to_array());
                geometry.colors.push(part.color);
            }
            geometry
                .indices
                .extend([start, start + 1, start + 2, start, start + 2, start + 3]);
        }
    }
    geometry
}
fn parts_mesh(parts: &[Part]) -> Mesh {
    mesh_of(&parts_geometry(parts))
}

fn initialise(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    assets: &mut WorldAssets,
    model: &WorldModel,
    geometry: &TerrainGeometry,
) {
    let mut add = |mesh| {
        let h = meshes.add(mesh);
        assets.meshes.push(h.clone());
        h
    };
    let terrain = add(mesh_of(geometry));
    assets.terrain = Some(terrain.clone());
    for kind in PlantKind::ALL {
        assets
            .plant_meshes
            .insert(kind, add(parts_mesh(&plant_parts(kind))));
    }
    assets.city = add(parts_mesh(&city_parts()));
    assets.cube = add(Cuboid::new(1.0, 1.0, 1.0).into());
    let ring = super::selection_parts();
    assets.ring = add(parts_mesh(&ring));
    assets.arrow = add(parts_mesh(&super::flow::arrow_parts()));
    for color in [
        Color::WHITE,
        Color::WHITE,
        Color::srgb(0.38, 0.40, 0.45),
        Color::srgb(0.95, 0.62, 0.18),
        Color::WHITE,
        Color::srgb(0.75, 0.60, 0.28),
        Color::srgb(0.1, 0.8, 1.0),
        Color::srgb(0.1, 0.7, 1.0),
        Color::srgb(1.0, 0.85, 0.15),
        Color::srgb(1.0, 1.0, 0.6),
    ] {
        assets.materials.push(materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.85,
            ..default()
        }));
    }
    commands.spawn((
        WorldEntity,
        Mesh3d(terrain),
        MeshMaterial3d(assets.materials[TERRAIN].clone()),
    ));
    commands.spawn((
        WorldEntity,
        Mesh3d(assets.ring.clone()),
        MeshMaterial3d(assets.materials[SELECTED].clone()),
        Transform::default(),
        Visibility::Hidden,
        SelectionMarker,
    ));
    let size = model.size();
    commands.spawn((
        WorldEntity,
        Mesh3d(assets.cube.clone()),
        MeshMaterial3d(assets.materials[OFFLINE].clone()),
        Transform::from_xyz(0.0, -0.65, 0.0).with_scale(Vec3::new(
            size.x + 0.7,
            0.75,
            size.y + 0.7,
        )),
    ));
    commands.spawn((
        WorldEntity,
        super::lighting::SunLight,
        DirectionalLight {
            illuminance: 0.0,
            color: Color::srgb(1.0, 0.90, 0.75),
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default(),
        bevy::light::CascadeShadowConfigBuilder {
            maximum_distance: 500.0,
            ..default()
        }
        .build(),
    ));
}

#[derive(SystemParam)]
pub struct RenderStore<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    assets: ResMut<'w, WorldAssets>,
    views: ResMut<'w, ViewSync>,
}
fn clear(
    commands: &mut Commands,
    entities: &Query<Entity, With<WorldEntity>>,
    store: &mut RenderStore,
) {
    for e in entities {
        commands.entity(e).despawn();
    }
    for mesh in store.assets.meshes.drain(..) {
        store.meshes.remove(mesh.id());
    }
    let lines = std::mem::take(&mut store.assets.line_meshes);
    for mesh in lines.into_values() {
        store.meshes.remove(mesh.id());
    }
    for material in store.assets.materials.drain(..) {
        store.materials.remove(material.id());
    }
    *store.assets = default();
    *store.views = default();
}
pub fn reconcile(
    mut commands: Commands,
    session: Option<Res<Session>>,
    generation: Res<SessionGeneration>,
    mut scene: ResMut<WorldScene>,
    mut controls: ResMut<WorldControls>,
    entities: Query<Entity, With<WorldEntity>>,
    mut store: RenderStore,
) {
    let Some(session) = session else {
        if scene.model.is_some() {
            clear(&mut commands, &entities, &mut store);
            *scene = default();
            controls.reset_session();
        }
        return;
    };
    let fresh = scene.generation != Some(generation.0)
        || scene
            .model
            .as_ref()
            .is_none_or(|m| !Arc::ptr_eq(&m.region, session.0.session().region()));
    if !fresh && !session.is_changed() {
        return;
    }
    let model = WorldModel::of(&session.0);
    if fresh {
        clear(&mut commands, &entities, &mut store);
        controls.reset_session();
        match super::region_terrain(&model.region, MapLayer::Terrain) {
            Ok(geometry) => {
                initialise(
                    &mut commands,
                    &mut store.meshes,
                    &mut store.materials,
                    &mut store.assets,
                    &model,
                    &geometry,
                );
                scene.geometry = Some(geometry);
                scene.generation = Some(generation.0);
                store.assets.layer = Some(MapLayer::Terrain);
            }
            Err(error) => {
                tracing::error!(?error, "world terrain failed");
                return;
            }
        }
    }
    controls.enter_phase(model.phase);
    let assets = &mut store.assets;
    let views = &mut store.views;
    for p in model.plants.values() {
        if let std::collections::btree_map::Entry::Vacant(entry) = views.plants.entry(p.id) {
            let root = commands
                .spawn((
                    WorldEntity,
                    Mesh3d(assets.plant_meshes[&p.kind].clone()),
                    MeshMaterial3d(assets.materials[ONLINE].clone()),
                    Transform::from_translation(p.at),
                ))
                .id();
            let pillar = commands
                .spawn((
                    WorldEntity,
                    Pillar(p.id),
                    Mesh3d(assets.cube.clone()),
                    MeshMaterial3d(assets.materials[BEAM].clone()),
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .id();
            entry.insert(root);
            views.pillars.insert(p.id, pillar);
        }
    }
    for c in model.cities.values() {
        views.cities.entry(c.id).or_insert_with(|| {
            commands
                .spawn((
                    WorldEntity,
                    Mesh3d(assets.city.clone()),
                    MeshMaterial3d(assets.materials[CITY].clone()),
                    Transform::from_translation(c.at).with_scale(Vec3::splat(c.scale)),
                ))
                .id()
        });
    }
    for line in model.lines.values() {
        if scene
            .model
            .as_ref()
            .and_then(|m| m.lines.get(&line.id))
            .is_some_and(|old| old.parts != line.parts)
            && let Some(handle) = assets.line_meshes.get(&line.id)
            && let Some(mut mesh) = store.meshes.get_mut(handle)
        {
            *mesh = parts_mesh(&line.parts);
        }
        views.arrows.entry(line.id).or_insert_with(|| {
            let length: f32 = line.parts.iter().map(|p| p.size.z).sum();
            let count = (length / 2.0).ceil().clamp(1.0, 16.0) as u32;
            (0..count)
                .map(|slot| {
                    commands
                        .spawn((
                            WorldEntity,
                            super::FlowArrow {
                                line: line.id,
                                slot,
                                count,
                            },
                            Mesh3d(assets.arrow.clone()),
                            MeshMaterial3d(assets.materials[FLOW].clone()),
                            Transform::default(),
                            Visibility::Hidden,
                            bevy::light::NotShadowCaster,
                        ))
                        .id()
                })
                .collect()
        });
        views.lines.entry(line.id).or_insert_with(|| {
            let h = store.meshes.add(parts_mesh(&line.parts));
            assets.line_meshes.insert(line.id, h.clone());
            commands
                .spawn((
                    WorldEntity,
                    Mesh3d(h),
                    MeshMaterial3d(assets.materials[LINE].clone()),
                ))
                .id()
        });
    }
    // Domain currently never removes commissioned assets, but retain correct identity if it does.
    views.plants.retain(|id, e| {
        if model.plants.contains_key(id) {
            true
        } else {
            commands.entity(*e).despawn();
            false
        }
    });
    views.pillars.retain(|id, e| {
        if model.plants.contains_key(id) {
            true
        } else {
            commands.entity(*e).despawn();
            false
        }
    });
    views.cities.retain(|id, e| {
        if model.cities.contains_key(id) {
            true
        } else {
            commands.entity(*e).despawn();
            false
        }
    });
    views.arrows.retain(|id, arrows| {
        if model.lines.contains_key(id) {
            true
        } else {
            for arrow in arrows {
                commands.entity(*arrow).despawn();
            }
            false
        }
    });
    views.lines.retain(|id, e| {
        if model.lines.contains_key(id) {
            true
        } else {
            commands.entity(*e).despawn();
            if let Some(h) = assets.line_meshes.remove(id) {
                store.meshes.remove(h.id());
            }
            false
        }
    });
    scene.model = Some(model);
    scene.revision = scene.revision.wrapping_add(1);
}

#[derive(SystemParam)]
pub(super) struct VisualInputs<'w> {
    scene: Res<'w, WorldScene>,
    controls: Res<'w, WorldControls>,
    layer: Res<'w, MapLayer>,
    selection: Res<'w, Selection>,
    tuning: Res<'w, TuningConfig>,
    time: Res<'w, Time>,
}
type ObjectFilter = (Without<Pillar>, Without<SelectionMarker>);
type MarkerFilter = (With<SelectionMarker>, Without<Pillar>);
pub fn update_visuals(
    input: VisualInputs,
    mut store: RenderStore,
    mut objects: Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>), ObjectFilter>,
    mut pillars: Query<(&Pillar, &mut Transform, &mut Visibility), Without<SelectionMarker>>,
    mut marker: Query<(&mut Transform, &mut Visibility), MarkerFilter>,
) {
    let VisualInputs {
        scene,
        controls,
        layer,
        selection,
        tuning,
        time,
    } = input;
    let Some(model) = &scene.model else {
        return;
    };
    if store.assets.materials.is_empty() {
        return;
    }
    let holo = controls.hologram();
    let chosen = if controls.overlay && !holo {
        *layer
    } else {
        MapLayer::Terrain
    };
    if store.assets.layer != Some(chosen) {
        if let Ok(g) = super::region_terrain(&model.region, chosen)
            && let Some(h) = &store.assets.terrain
        {
            let h = h.clone();
            if let Some(mut mesh) = store.meshes.get_mut(&h) {
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, g.colors);
            }
        }
        store.assets.layer = Some(chosen);
    }
    let city_light = model.served
        * if model.served < 0.999 {
            0.90 + 0.10 * (time.elapsed_secs() * 5.0).sin()
        } else {
            1.0
        };
    for (i, h) in store.assets.materials.iter().enumerate() {
        if let Some(mut m) = store.materials.get_mut(h) {
            // Preserve face lighting and cast shadows under the supply indicators.
            m.unlit = i == BEAM || i == SELECTED || i == FLOW;
            m.emissive = LinearRgba::BLACK;
            match i {
                TERRAIN => m.base_color = Color::WHITE,
                CITY => {
                    m.base_color = if holo {
                        let brightness = 0.25 + model.served * 0.75;
                        Color::srgb(brightness, brightness, brightness)
                    } else {
                        Color::WHITE
                    };
                    if holo {
                        m.emissive = LinearRgba::rgb(0.1, 0.6, 0.9) * city_light * 0.18;
                    }
                }
                ENERGIZED if holo => {
                    m.emissive = LinearRgba::rgb(0.05, 0.6, 1.0)
                        * (0.6 + 0.15 * (time.elapsed_secs() * 2.0).sin())
                }
                BEAM => {
                    let color = if tuning.beam_color.is_finite() {
                        tuning.beam_color.clamp(Vec3::ZERO, Vec3::splat(5.0))
                    } else {
                        Vec3::new(0.1, 0.7, 1.0)
                    };
                    m.emissive = LinearRgba::rgb(color.x, color.y, color.z) * 2.0;
                }
                FLOW => m.emissive = LinearRgba::rgb(1.0, 1.0, 0.35),
                SELECTED => m.emissive = LinearRgba::rgb(1.0, 0.8, 0.1),
                _ => {}
            }
        }
    }
    let material = |i: usize| MeshMaterial3d(store.assets.materials[i].clone());
    let scale = tuning.scale();
    for p in model.plants.values() {
        if let Some(e) = store.views.plants.get(&p.id)
            && let Ok((mut t, mut m)) = objects.get_mut(*e)
        {
            t.scale = Vec3::splat(scale);
            *m = material(if p.building.is_some() {
                BUILDING
            } else if !p.connected
                || (!p.online && model.phase == domain::session::DayPhase::Operating)
            {
                OFFLINE
            } else {
                ONLINE
            });
        }
    }
    for c in model.cities.values() {
        if let Some(e) = store.views.cities.get(&c.id)
            && let Ok((mut t, mut m)) = objects.get_mut(*e)
        {
            t.scale = Vec3::splat(c.scale * scale);
            *m = material(if c.connected { CITY } else { OFFLINE });
        }
    }
    for l in model.lines.values() {
        if let Some(e) = store.views.lines.get(&l.id)
            && let Ok((_, mut m)) = objects.get_mut(*e)
        {
            *m = material(if l.building.is_some() {
                BUILDING
            } else if holo && l.energized {
                ENERGIZED
            } else {
                LINE
            });
        }
    }
    for (p, mut t, mut visible) in &mut pillars {
        if let Some(p) = model.plants.get(&p.0) {
            if let Some(part) = super::pillar_part(p, &tuning).filter(|_| holo) {
                *visible = Visibility::Visible;
                *t = Transform::from_translation(part.center).with_scale(part.size);
            } else {
                *visible = Visibility::Hidden;
            }
        } else {
            *visible = Visibility::Hidden;
        }
    }
    let selected = match *selection {
        Selection::Plant(id) => model.plants.get(&id).map(|p| p.at),
        Selection::City(id) => model.cities.get(&id).map(|c| c.at),
        Selection::Cell(c) => super::GridTransform::new(
            model.region.terrain().width(),
            model.region.terrain().height(),
        )
        .cell_center(model.region.terrain(), c),
        Selection::LineFrom(node) => model.nodes.get(&node).copied(),
        Selection::Line(id) => model.lines.get(&id).map(|line| line.midpoint),
        Selection::None => None,
    };
    for (mut t, mut v) in &mut marker {
        if let Some(at) = selected {
            let size = match *selection {
                Selection::Cell(_) => 1.0,
                Selection::City(id) => model.cities.get(&id).map_or(scale, |c| c.scale * scale),
                _ => scale,
            };
            *t = Transform::from_translation(at + Vec3::Y * 0.03).with_scale(Vec3::splat(size));
            *v = Visibility::Visible;
        } else {
            *v = Visibility::Hidden;
        }
    }
}
