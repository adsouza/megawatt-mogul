//! Picking uses the world's triangles and block bounds, with forgiving cable hit targets.
use super::{
    GridTransform, TerrainGeometry,
    model::{Part, WorldModel, city_parts, plant_parts},
};
use crate::hud::Selection;
use bevy::prelude::*;

fn triangle(ray: Ray3d, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let ab = b - a;
    let ac = c - a;
    let p = ray.direction.cross(ac);
    let det = ab.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let inv = det.recip();
    let t = ray.origin - a;
    let u = t.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = t.cross(ab);
    let v = ray.direction.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = ac.dot(q) * inv;
    (distance >= 0.0).then_some(distance)
}
pub fn terrain_hit(ray: Ray3d, geometry: &TerrainGeometry) -> Option<f32> {
    geometry
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|i| {
            triangle(
                ray,
                Vec3::from_array(geometry.positions[i[0] as usize]),
                Vec3::from_array(geometry.positions[i[1] as usize]),
                Vec3::from_array(geometry.positions[i[2] as usize]),
            )
        })
        .min_by(f32::total_cmp)
}
fn block_hit(ray: Ray3d, part: Part, at: Vec3, scale: f32) -> Option<f32> {
    let origin = part.rotation.inverse() * ((ray.origin - at) / scale - part.center);
    let direction = part.rotation.inverse() * (*ray.direction / scale);
    let half = part.size * 0.5;
    let mut near = 0.0f32;
    let mut far = f32::INFINITY;
    for axis in 0..3 {
        if direction[axis].abs() < 1e-8 {
            if origin[axis].abs() > half[axis] {
                return None;
            }
        } else {
            let a = (-half[axis] - origin[axis]) / direction[axis];
            let b = (half[axis] - origin[axis]) / direction[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if far < near {
                return None;
            }
        }
    }
    Some(near)
}
/// Visible buildings take priority over cables. Within each class the nearest hit wins;
/// ties use model ID order, with plants before cities. Terrain still occludes all objects.
pub fn pick_world(
    ray: Ray3d,
    world: &WorldModel,
    geometry: &TerrainGeometry,
    model_scale: f32,
) -> Selection {
    hit_world(ray, world, geometry, model_scale).map_or(Selection::None, |hit| hit.selection)
}
pub fn hit_world(
    ray: Ray3d,
    world: &WorldModel,
    _geometry: &TerrainGeometry,
    model_scale: f32,
) -> Option<WorldHit> {
    hit_geometry(ray, world, model_scale, None)
}
pub fn presented_hit(
    ray: Ray3d,
    world: &WorldModel,
    controls: &super::WorldControls,
    tuning: &super::TuningConfig,
) -> Option<WorldHit> {
    hit_geometry(
        ray,
        world,
        tuning.scale(),
        controls.hologram().then_some(tuning),
    )
}
fn hit_geometry(
    ray: Ray3d,
    world: &WorldModel,
    model_scale: f32,
    pillars: Option<&super::TuningConfig>,
) -> Option<WorldHit> {
    let mut nearest = f32::INFINITY;
    let mut selection = Selection::None;
    let mut consider = |distance: Option<f32>, value| {
        if let Some(d) = distance
            && d < nearest
        {
            nearest = d;
            selection = value;
        }
    };
    for p in world.plants.values() {
        for part in plant_parts(p.kind) {
            consider(
                block_hit(ray, part, p.at, model_scale),
                Selection::Plant(p.id),
            );
        }
    }
    if let Some(tuning) = pillars {
        for p in world.plants.values() {
            if let Some(part) = super::pillar_part(p, tuning) {
                consider(
                    block_hit(ray, part, Vec3::ZERO, 1.0),
                    Selection::Plant(p.id),
                );
            }
        }
    }
    for c in world.cities.values() {
        for part in city_parts() {
            consider(
                block_hit(ray, part, c.at, c.scale * model_scale),
                Selection::City(c.id),
            );
        }
    }
    if let Some(d) = terrain_grid_hit(ray, world.region.terrain())
        && d < nearest
    {
        nearest = d;
        let p = ray.get_point(d);
        let grid = world.region.terrain();
        let cell = GridTransform::new(grid.width(), grid.height()).cell_at(p.x, p.z);
        selection = cell.map_or(Selection::None, |cell| {
            // Bare ground around a model belongs to the same site. Only resolve the
            // top surface: hitting a cliff or the underside must not pick through it.
            let on_top =
                super::surface_height(grid, cell).is_some_and(|height| (p.y - height).abs() < 1e-3);
            if on_top && ray.direction.y < 0.0 {
                world
                    .plants
                    .values()
                    .find(|plant| plant.cell == cell)
                    .map(|plant| Selection::Plant(plant.id))
                    .or_else(|| {
                        world
                            .cities
                            .values()
                            .find(|city| city.cell == cell)
                            .map(|city| Selection::City(city.id))
                    })
                    .unwrap_or(Selection::Cell(cell))
            } else {
                Selection::Cell(cell)
            }
        });
    }
    // At junctions a cable's forgiving hit box can sit in front of the building.
    // Prefer the visible building, but leave cables selectable away from its silhouette.
    if !matches!(selection, Selection::Plant(_) | Selection::City(_)) {
        for line in world.lines.values() {
            for part in &line.parts {
                if let Some(d) = block_hit(ray, *part, Vec3::ZERO, 1.0)
                    && d < nearest
                {
                    nearest = d;
                    selection = line.selection;
                }
            }
        }
    }
    nearest.is_finite().then_some(WorldHit {
        distance: nearest,
        selection,
    })
}

fn slab(ray: Ray3d, min: Vec3, max: Vec3) -> Option<(f32, f32)> {
    let mut near = 0.0f32;
    let mut far = f32::INFINITY;
    for axis in 0..3 {
        let direction = ray.direction[axis];
        if direction.abs() < 1e-8 {
            if ray.origin[axis] < min[axis] || ray.origin[axis] > max[axis] {
                return None;
            }
        } else {
            let a = (min[axis] - ray.origin[axis]) / direction;
            let b = (max[axis] - ray.origin[axis]) / direction;
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
    }
    Some((near, far))
}
/// Front-to-back grid traversal over solid terraced columns. This is exactly the surface of
/// TerrainGeometry (tops, exposed skirts, outer walls and bottom), without testing every triangle.
pub fn terrain_grid_hit(ray: Ray3d, grid: &domain::region::TerrainGrid) -> Option<f32> {
    let width = i32::from(grid.width());
    let height = i32::from(grid.height());
    if width == 0 || height == 0 {
        return None;
    }
    let half = Vec2::new(width as f32, height as f32) * 0.5;
    let (start, end) = slab(
        ray,
        Vec3::new(-half.x, super::terrain::BASE, -half.y),
        Vec3::new(half.x, super::terrain::MAX_HEIGHT, half.y),
    )?;
    let at = ray.get_point(start + 0.0001);
    let mut x = ((at.x + half.x).floor() as i32).clamp(0, width - 1);
    let mut z = ((at.z + half.y).floor() as i32).clamp(0, height - 1);
    let boundary = |axis: usize, cell: i32, half: f32| {
        let direction = ray.direction[axis];
        if direction.abs() < 1e-8 {
            f32::INFINITY
        } else {
            (cell as f32 + if direction > 0.0 { 1.0 } else { 0.0 } - half - ray.origin[axis])
                / direction
        }
    };
    for _ in 0..width + height + 2 {
        let next_x = boundary(0, x, half.x);
        let next_z = boundary(2, z, half.y);
        let next = next_x.min(next_z).min(end);
        let top = super::surface_height(
            grid,
            domain::ids::CellCoord {
                x: x as u16,
                y: z as u16,
            },
        )?;
        let min = Vec3::new(x as f32 - half.x, super::terrain::BASE, z as f32 - half.y);
        if let Some((hit, _)) = slab(ray, min, Vec3::new(min.x + 1.0, top, min.z + 1.0))
            && hit <= next + 0.0001
            && hit >= start - 0.0001
        {
            return Some(hit);
        }
        if next >= end {
            return None;
        }
        if next_x <= next_z {
            x += if ray.direction.x > 0.0 { 1 } else { -1 };
        } else {
            z += if ray.direction.z > 0.0 { 1 } else { -1 };
        }
        if x < 0 || x >= width || z < 0 || z >= height {
            return None;
        }
    }
    None
}

#[derive(Debug, Clone, Copy)]
pub struct WorldHit {
    pub distance: f32,
    pub selection: Selection,
}
