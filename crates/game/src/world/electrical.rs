//! Electrical-area seams and exact DC flow, projected over the terrain.
use super::{CameraPose, GridTransform};
use crate::l10n::Localizer;
use bevy::prelude::*;
use bevy_egui::egui;
use domain::{grid::LineStatus, ids::CellCoord, session::GameSession};

#[derive(Debug, Clone, Copy)]
pub(super) struct ConverterMarker {
    pub station: domain::grid::ConverterStation,
    pub at: egui::Pos2,
    pub bus_at: egui::Pos2,
    pub highlighted: bool,
}

/// One marker per station, offset from its AC bus so a city is never drawn as a DC endpoint.
/// Parallel projects at the same bus receive distinct, deterministic marker positions.
pub(super) fn converter_markers(
    session: &GameSession,
    pose: CameraPose,
    rect: egui::Rect,
) -> Vec<ConverterMarker> {
    let terrain = session.region().terrain();
    let grid = GridTransform::new(terrain.width(), terrain.height());
    let mut offsets = std::collections::BTreeMap::<domain::ids::NodeId, usize>::new();
    session
        .power()
        .network()
        .converter_stations()
        .filter_map(|station| {
            let node = session.power().network().node(station.grid_node)?;
            let world = grid.cell_center(terrain, node.cell)? + Vec3::Y * 0.6;
            let ndc = pose.project(world)?;
            let bus_at = egui::pos2(
                rect.left() + (ndc.x + 1.0) * 0.5 * rect.width(),
                rect.top() + (1.0 - ndc.y) * 0.5 * rect.height(),
            );
            let offset = offsets.entry(station.grid_node).or_default();
            let at = bus_at
                + egui::vec2(
                    12.0 + (*offset % 4) as f32 * 16.0,
                    -12.0 - (*offset / 4) as f32 * 16.0,
                );
            *offset += 1;
            Some(ConverterMarker {
                station,
                at,
                bus_at,
                highlighted: false,
            })
        })
        .collect()
}

pub(super) fn hit_converter(
    markers: &[ConverterMarker],
    pointer: egui::Pos2,
) -> Option<domain::grid::ConverterStation> {
    markers
        .iter()
        .find(|marker| marker.at.distance(pointer) <= 7.0)
        .map(|marker| marker.station)
}

/// Nearest visible DC segment; station hit tests take priority at its endpoints.
pub(super) fn hit_dc_route(
    markers: &[ConverterMarker],
    pointer: egui::Pos2,
) -> Option<domain::ids::LinkId> {
    markers
        .iter()
        .filter(|marker| marker.station.id.end == domain::grid::ConverterEnd::From)
        .filter_map(|from| {
            let to = markers.iter().find(|m| {
                m.station.id.link == from.station.id.link
                    && m.station.id.end == domain::grid::ConverterEnd::To
            })?;
            let segment = to.at - from.at;
            let length = segment.length_sq();
            if length <= f32::EPSILON {
                return None;
            }
            let fraction = ((pointer - from.at).dot(segment) / length).clamp(0.0, 1.0);
            let distance = pointer.distance(from.at + fraction * segment);
            (distance <= 5.0).then_some((distance, from.station.id.link))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, id)| id)
}

pub(super) fn draw(
    painter: &egui::Painter,
    rect: egui::Rect,
    pose: CameraPose,
    session: &GameSession,
    l10n: &Localizer,
    time: f64,
    markers: &[ConverterMarker],
) {
    let region = session.region();
    if region.areas().is_none() {
        return;
    }
    let terrain = region.terrain();
    let grid = GridTransform::new(terrain.width(), terrain.height());
    let project = |point: Vec3| -> Option<egui::Pos2> {
        let ndc = pose.project(point)?;
        Some(egui::pos2(
            rect.left() + (ndc.x + 1.0) * 0.5 * rect.width(),
            rect.top() + (1.0 - ndc.y) * 0.5 * rect.height(),
        ))
    };
    for y in 0..terrain.height() {
        for x in 0..terrain.width() {
            let cell = CellCoord { x, y };
            let area = region.area_at(cell);
            if area.0 == 0 {
                continue;
            }
            let Some(center) = grid.cell_center(terrain, cell) else {
                continue;
            };
            for (next, offset, tangent) in [
                (CellCoord { x: x + 1, y }, Vec3::X * 0.5, Vec3::Z * 0.5),
                (CellCoord { x, y: y + 1 }, Vec3::Z * 0.5, Vec3::X * 0.5),
            ] {
                if terrain.get(next).is_none()
                    || region.area_at(next).0 == 0
                    || region.area_at(next) == area
                {
                    continue;
                }
                let h = super::surface_height(terrain, next)
                    .unwrap_or(center.y)
                    .max(center.y)
                    + 0.1;
                let mut a = center + offset - tangent;
                let mut b = center + offset + tangent;
                a.y = h;
                b.y = h;
                if let Some((a, b)) = project(a).zip(project(b)) {
                    painter.line_segment(
                        [a, b],
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(240, 210, 160)),
                    );
                }
            }
        }
    }
    for link in session.power().network().links() {
        let Some((a, b)) = markers
            .iter()
            .find(|m| m.station.id == link.from.id)
            .zip(markers.iter().find(|m| m.station.id == link.to.id))
        else {
            continue;
        };
        let (a, b) = (a.at, b.at);
        let color = if link.status == LineStatus::InService {
            egui::Color32::from_rgb(110, 220, 240)
        } else {
            egui::Color32::GRAY
        };
        painter.line_segment(
            [a, b],
            egui::Stroke::new(
                if markers
                    .iter()
                    .any(|m| m.station.id.link == link.id && m.highlighted)
                {
                    4.0
                } else {
                    2.5
                },
                color,
            ),
        );
        if link.flow.value() != 0.0 {
            let forward = link.flow.value() > 0.0;
            let (from, to) = if forward { (a, b) } else { (b, a) };
            for n in 0..4 {
                let f = ((time * 0.25 + f64::from(n) * 0.25) % 1.0) as f32;
                let at = from.lerp(to, f);
                painter.circle_filled(at, 2.5, color);
            }
            painter.arrow(
                from.lerp(to, 0.48),
                (to - from).normalized() * 12.0,
                egui::Stroke::new(2.0, color),
            );
        }
    }
    // Stations remain visible even when their other terminal is outside the viewport.
    for marker in markers {
        let Some(link) = session.power().network().link(marker.station.id.link) else {
            continue;
        };
        let color = if link.status == LineStatus::InService {
            egui::Color32::from_rgb(110, 220, 240)
        } else {
            egui::Color32::GRAY
        };
        painter.line_segment([marker.bus_at, marker.at], egui::Stroke::new(1.0, color));
        let shape = egui::Rect::from_center_size(marker.at, egui::vec2(10.0, 10.0));
        painter.rect_filled(shape, 1.0, egui::Color32::from_rgb(20, 34, 45));
        painter.rect_stroke(
            shape,
            1.0,
            egui::Stroke::new(
                if marker.highlighted { 3.0 } else { 1.5 },
                if marker.highlighted {
                    egui::Color32::WHITE
                } else {
                    color
                },
            ),
            egui::StrokeKind::Middle,
        );
    }
    painter.text(
        rect.left_bottom() + egui::vec2(10.0, -6.0),
        egui::Align2::LEFT_BOTTOM,
        l10n.text("hvdc-map-legend"),
        egui::FontId::proportional(10.0),
        egui::Color32::WHITE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::test_support::sgcc_planning_with_small_remote_source;
    #[test]
    fn route_picking_chooses_the_nearest_project_and_ignores_degenerate_segments() {
        use domain::{
            grid::{ConverterEnd, ConverterStation, ConverterStationId},
            ids::{LinkId, NodeId},
        };
        let marker = |link, end, at| ConverterMarker {
            station: ConverterStation {
                id: ConverterStationId {
                    link: LinkId(link),
                    end,
                },
                grid_node: NodeId(link),
            },
            at,
            bus_at: at,
            highlighted: false,
        };
        let markers = vec![
            marker(1, ConverterEnd::From, egui::pos2(100.0, 100.0)),
            marker(1, ConverterEnd::To, egui::pos2(300.0, 100.0)),
            marker(2, ConverterEnd::From, egui::pos2(100.0, 120.0)),
            marker(2, ConverterEnd::To, egui::pos2(300.0, 120.0)),
            marker(3, ConverterEnd::From, egui::pos2(0.0, 0.0)),
            marker(3, ConverterEnd::To, egui::pos2(0.0, 0.0)),
        ];
        assert_eq!(
            hit_dc_route(&markers, egui::pos2(200.0, 103.0)),
            Some(LinkId(1))
        );
        assert_eq!(
            hit_dc_route(&markers, egui::pos2(200.0, 120.0)),
            Some(LinkId(2))
        );
        assert_eq!(hit_dc_route(&markers, egui::pos2(200.0, 110.0)), None);
        assert_eq!(hit_dc_route(&markers, egui::Pos2::ZERO), None);
    }

    #[test]
    fn converter_markers_are_distinct_from_their_local_buses_and_parallel_projects() {
        let mut game = sgcc_planning_with_small_remote_source();
        let link = game
            .session()
            .power()
            .network()
            .links()
            .next()
            .unwrap()
            .clone();
        game.execute(application::Command::BuildLink {
            from: link.from_grid(),
            to: link.to_grid(),
        })
        .unwrap();
        let terrain = game.session().region().terrain();
        let pose = super::super::OrbitCamera::default().pose(
            Vec2::new(f32::from(terrain.width()), f32::from(terrain.height())),
            1.6,
        );
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0));
        let markers = converter_markers(game.session(), pose, rect);
        assert_eq!(markers.len(), 4);
        for marker in &markers {
            assert!(marker.at.distance(marker.bus_at) > 7.0);
            assert_eq!(hit_converter(&markers, marker.at), Some(marker.station));
            assert_eq!(hit_converter(&markers, marker.bus_at), None);
        }
        // Keep offscreen endpoints for clipping the DC route through the viewport edge.
        let bus = GridTransform::new(terrain.width(), terrain.height())
            .cell_center(
                terrain,
                game.session()
                    .power()
                    .network()
                    .node(link.from_grid())
                    .unwrap()
                    .cell,
            )
            .unwrap();
        let close_pose = super::super::OrbitCamera {
            target: bus.xz(),
            zoom: 0.08,
            elevation: 1.56,
            ..Default::default()
        }
        .pose(
            Vec2::new(f32::from(terrain.width()), f32::from(terrain.height())),
            1.6,
        );
        let markers = converter_markers(game.session(), close_pose, rect);
        assert_eq!(markers.len(), 4);
        assert!(markers.iter().any(|marker| !rect.contains(marker.at)));
    }
}
