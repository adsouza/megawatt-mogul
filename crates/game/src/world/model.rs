//! Immutable presentation model, read exclusively from public simulation queries.
use super::GridTransform;
use crate::hud::Selection;
use application::Game;
use bevy::prelude::*;
use domain::{
    generation::PlantStatus,
    grid::LineStatus,
    ids::{CellCoord, CityId, LineId, NodeId, PlantId},
    region::{PlantKind, RegionDefinition},
    session::DayPhase,
    time::SimMonth,
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Part {
    pub center: Vec3,
    pub size: Vec3,
    pub rotation: Quat,
    pub color: [f32; 4],
}
impl Part {
    pub fn block(center: Vec3, size: Vec3, color: [f32; 4]) -> Self {
        Self {
            center,
            size,
            rotation: Quat::IDENTITY,
            color,
        }
    }
    /// Exact axis-aligned bounds, including rotation, shared by model fitting and annotations.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let half = ((self.rotation * Vec3::X).abs() * self.size.x
            + (self.rotation * Vec3::Y).abs() * self.size.y
            + (self.rotation * Vec3::Z).abs() * self.size.z)
            / 2.0;
        (self.center - half, self.center + half)
    }
}

/// One grid cell is one world unit. Leave a tenth of a cell clear on each side.
const MODEL_WIDTH: f32 = 0.8;

fn fit_to_cell(mut parts: Vec<Part>) -> Vec<Part> {
    let radius = parts.iter().fold(0.0_f32, |radius, part| {
        let (min, max) = part.bounds();
        radius
            .max(min.x.abs())
            .max(max.x.abs())
            .max(min.z.abs())
            .max(max.z.abs())
    });
    if radius > 0.0 {
        let scale = MODEL_WIDTH / (2.0 * radius);
        for part in &mut parts {
            part.center *= scale;
            part.size *= scale;
        }
    }
    parts
}

pub(super) fn parts_height(parts: &[Part]) -> f32 {
    parts.iter().map(|p| p.bounds().1.y).fold(0.0, f32::max)
}

/// The selection outline also stays inside the selected cell.
pub fn selection_parts() -> [Part; 4] {
    let width = MODEL_WIDTH + 0.1;
    let thickness = 0.03;
    let edge = (width - thickness) / 2.0;
    let horizontal = |z| {
        Part::block(
            Vec3::new(0.0, 0.0, z),
            Vec3::new(width, 0.025, thickness),
            [1.0; 4],
        )
    };
    let vertical = |x| {
        Part::block(
            Vec3::new(x, 0.0, 0.0),
            Vec3::new(thickness, 0.025, width),
            [1.0; 4],
        )
    };
    [
        horizontal(-edge),
        horizontal(edge),
        vertical(-edge),
        vertical(edge),
    ]
}
/// Each kind has a distinct silhouette, composed of hard-edged low-poly blocks.
pub fn plant_parts(kind: PlantKind) -> Vec<Part> {
    let rgb = crate::hud::kind_color(kind);
    let color = Color::srgb_u8(rgb[0], rgb[1], rgb[2])
        .to_linear()
        .to_f32_array();
    let dark = [0.065, 0.085, 0.12, 1.0];
    let pale = [0.7, 0.66, 0.56, 1.0];
    let block = |x, y, z, w, h, d, c| Part::block(Vec3::new(x, y, z), Vec3::new(w, h, d), c);
    let mut parts = vec![block(0.0, 0.08, 0.0, 2.4, 0.16, 1.8, dark)];
    match kind {
        PlantKind::Nuclear => {
            parts.push(block(-0.55, 0.7, 0.0, 0.7, 1.2, 1.1, color));
            for x in [0.2, 0.85] {
                parts.push(block(x, 1.05, 0.0, 0.5, 1.9, 0.7, pale));
                parts.push(block(x, 2.05, 0.0, 0.65, 0.15, 0.8, color));
            }
        }
        PlantKind::Coal => {
            parts.push(block(-0.3, 0.6, 0.0, 1.5, 1.0, 1.3, color));
            for z in [-0.4, 0.4] {
                parts.push(block(0.7, 1.3, z, 0.28, 2.3, 0.28, pale));
            }
        }
        PlantKind::Ccgt => {
            for z in [-0.45, 0.45] {
                parts.push(block(-0.2, 0.5, z, 1.7, 0.7, 0.55, color));
            }
            parts.push(block(0.85, 0.95, 0.0, 0.35, 1.6, 0.4, pale));
        }
        PlantKind::Peaker => {
            parts.push(block(-0.3, 0.4, 0.0, 1.1, 0.55, 1.1, color));
            parts.push(block(0.65, 0.7, 0.0, 0.45, 1.1, 0.45, pale));
        }
        PlantKind::Solar => {
            for z in [-0.5, 0.0, 0.5] {
                parts.push(block(0.0, 0.35, z, 2.1, 0.18, 0.35, color));
            }
        }
        PlantKind::Battery => {
            for x in [-0.72, 0.0, 0.72] {
                parts.push(block(x, 0.6, 0.0, 0.55, 0.95, 1.25, color));
                parts.push(block(x, 1.12, 0.0, 0.4, 0.12, 1.0, pale));
                for z in [-0.4, 0.0, 0.4] {
                    parts.push(block(x, 0.7, z, 0.57, 0.08, 0.08, dark));
                }
            }
        }
        PlantKind::Hydro => {
            parts.push(block(0.0, 0.7, 0.0, 2.3, 1.2, 0.45, pale));
            for x in [-0.8, 0.0, 0.8] {
                parts.push(block(x, 0.45, 0.5, 0.5, 0.6, 0.5, color));
            }
        }
    }
    fit_to_cell(parts)
}
pub fn city_parts() -> Vec<Part> {
    let color = [0.65, 0.53, 0.32, 1.0];
    fit_to_cell(
        [
            (-0.55, -0.45, 0.7),
            (0.05, -0.5, 1.3),
            (0.6, -0.35, 0.85),
            (-0.5, 0.35, 1.0),
            (0.2, 0.4, 0.65),
            (0.7, 0.45, 0.45),
        ]
        .into_iter()
        .map(|(x, z, h)| Part::block(Vec3::new(x, h / 2.0, z), Vec3::new(0.42, h, 0.48), color))
        .collect(),
    )
}
#[derive(Debug, Clone, PartialEq)]
pub struct WorldPlant {
    pub id: PlantId,
    pub kind: PlantKind,
    pub cell: CellCoord,
    pub at: Vec3,
    pub connected: bool,
    pub building: Option<u32>,
    pub online: bool,
    pub output: f32,
}
#[derive(Debug, Clone, PartialEq)]
pub struct WorldCity {
    pub id: CityId,
    pub cell: CellCoord,
    pub at: Vec3,
    pub scale: f32,
    pub connected: bool,
    pub name_key: String,
}
#[derive(Debug, Clone, PartialEq)]
pub struct WorldLine {
    pub id: LineId,
    pub selection: Selection,
    pub parts: Vec<Part>,
    pub building: Option<u32>,
    pub energized: bool,
    pub flow: Option<super::FlowDirection>,
    pub midpoint: Vec3,
}
#[derive(Debug, Clone)]
pub struct WorldModel {
    pub region: Arc<RegionDefinition>,
    pub phase: DayPhase,
    pub month: SimMonth,
    pub tick: u32,
    pub served: f32,
    pub plants: BTreeMap<PlantId, WorldPlant>,
    pub cities: BTreeMap<CityId, WorldCity>,
    pub lines: BTreeMap<LineId, WorldLine>,
    pub nodes: BTreeMap<NodeId, Vec3>,
}
impl WorldModel {
    pub fn of(game: &Game) -> Self {
        let s = game.session();
        let region = Arc::clone(s.region());
        let terrain = region.terrain();
        let coordinates = GridTransform::new(terrain.width(), terrain.height());
        let grid = s.grid_snapshot();
        let served = super::supply_fraction(grid.mode, grid.outage.value(), grid.shed.value());
        let plants: BTreeMap<_, _> = s
            .plant_views()
            .into_iter()
            .filter_map(|p| {
                Some((
                    p.id,
                    WorldPlant {
                        id: p.id,
                        kind: p.kind,
                        cell: p.cell,
                        at: coordinates.cell_center(terrain, p.cell)?,
                        connected: p.connected,
                        building: match p.status {
                            PlantStatus::UnderConstruction { months_left } => Some(months_left),
                            _ => None,
                        },
                        online: !matches!(
                            p.status,
                            PlantStatus::Offline | PlantStatus::UnderConstruction { .. }
                        ),
                        output: (p.output.value() / region.spec(p.kind).capacity.value())
                            .clamp(0.0, 1.0) as f32,
                    },
                ))
            })
            .collect();
        let views = s.city_views();
        let max_load = views
            .iter()
            .filter_map(|c| s.city_detail(c.id))
            .map(|d| d.base_load.value())
            .fold(1.0, f64::max);
        let cities: BTreeMap<_, _> = views
            .into_iter()
            .filter_map(|c| {
                Some((
                    c.id,
                    WorldCity {
                        id: c.id,
                        cell: c.cell,
                        at: coordinates.cell_center(terrain, c.cell)?,
                        scale: (s.city_detail(c.id)?.base_load.value() / max_load).sqrt() as f32
                            * 0.4
                            + 0.6,
                        connected: c.connected,
                        name_key: c.name_key,
                    },
                ))
            })
            .collect();
        let nodes: BTreeMap<_, _> = s
            .plant_views()
            .into_iter()
            .filter_map(|p| {
                Some((
                    p.node,
                    (coordinates.cell_center(terrain, p.cell)?, p.connected),
                ))
            })
            .chain(s.city_views().into_iter().filter_map(|c| {
                Some((
                    c.node,
                    (coordinates.cell_center(terrain, c.cell)?, c.connected),
                ))
            }))
            .collect();
        let energized = grid.mode == domain::grid::GridMode::Normal
            && plants.values().any(|p| p.connected && p.output > 0.0);
        let line_views = s.line_views();
        let sources = s
            .plant_views()
            .into_iter()
            .filter(|p| p.connected && p.output.value() > 0.0)
            .map(|p| (p.node, p.output.value()))
            .collect();
        let sinks = s
            .city_views()
            .into_iter()
            .filter(|c| c.connected)
            .filter_map(|c| Some((c.node, s.city_detail(c.id)?.served_now?.value())))
            .chain(
                s.plant_views()
                    .into_iter()
                    .filter(|p| p.connected)
                    .filter_map(|p| {
                        let charge = p.battery?.flow.charge.value();
                        (charge > 0.0).then_some((p.node, charge))
                    }),
            )
            .collect();
        let flows = if s.phase() == DayPhase::Operating && energized {
            super::flow::route_flows(&line_views, &sources, &sinks)
        } else {
            BTreeMap::new()
        };
        let lines = line_views
            .into_iter()
            .filter_map(|l| {
                let (from, fc) = nodes.get(&l.from)?;
                let (to, tc) = nodes.get(&l.to)?;
                let steps = ((to.xz() - from.xz()).length() * 2.0).ceil().max(1.0) as u32;
                let mut parts = Vec::new();
                let mut previous: Option<Vec3> = None;
                for i in 0..steps {
                    if matches!(l.status, LineStatus::UnderConstruction { .. }) && i % 2 == 1 {
                        previous = None;
                        continue;
                    }
                    let a = from.lerp(*to, i as f32 / steps as f32);
                    let b = from.lerp(*to, (i + 1) as f32 / steps as f32);
                    let height = |p: Vec3| {
                        coordinates
                            .cell_at(p.x, p.z)
                            .and_then(|c| super::surface_height(terrain, c))
                            .unwrap_or(p.y)
                    };
                    let h = height(a).max(height(b)) + 0.18;
                    let a = Vec3::new(a.x, h, a.z);
                    let b = Vec3::new(b.x, h, b.z);
                    if let Some(end) = previous
                        && (end.y - a.y).abs() > 0.0001
                    {
                        let delta = a - end;
                        parts.push(Part {
                            center: (a + end) / 2.0,
                            size: Vec3::new(0.10, 0.10, delta.length() + 0.06),
                            rotation: Quat::from_rotation_arc(Vec3::Z, delta.normalize()),
                            color: [1.0; 4],
                        });
                    }
                    previous = Some(b);
                    let delta = b - a;
                    parts.push(Part {
                        center: (a + b) / 2.0,
                        size: Vec3::new(0.10, 0.10, delta.length() + 0.06),
                        rotation: delta.try_normalize().map_or(Quat::IDENTITY, |direction| {
                            Quat::from_rotation_arc(Vec3::Z, direction)
                        }),
                        color: [1.0; 4],
                    });
                }
                Some((
                    l.id,
                    WorldLine {
                        id: l.id,
                        selection: Selection::Line(l.id),
                        parts,
                        building: match l.status {
                            LineStatus::UnderConstruction { months_left } => Some(months_left),
                            LineStatus::InService => None,
                        },
                        energized: *fc && *tc && energized,
                        flow: flows.get(&l.id).copied(),
                        midpoint: (from + to) / 2.0 + Vec3::Y * 0.5,
                    },
                ))
            })
            .collect();
        Self {
            region,
            phase: s.phase(),
            month: s.clock().month(),
            tick: s.clock().tick(),
            served,
            plants,
            cities,
            lines,
            nodes: nodes.into_iter().map(|(id, (at, _))| (id, at)).collect(),
        }
    }
    pub fn size(&self) -> Vec2 {
        Vec2::new(
            f32::from(self.region.terrain().width()),
            f32::from(self.region.terrain().height()),
        )
    }
}
