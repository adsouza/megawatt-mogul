//! Electrical-area seams and exact DC flow, projected over the terrain.
use super::{CameraPose, GridTransform};
use crate::l10n::Localizer;
use bevy::prelude::*;
use bevy_egui::egui;
use domain::{grid::LineStatus, ids::CellCoord, session::GameSession};

pub(super) fn draw(
    painter: &egui::Painter,
    rect: egui::Rect,
    pose: CameraPose,
    session: &GameSession,
    l10n: &Localizer,
    time: f64,
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
        let Some((a, b)) = session
            .power()
            .network()
            .node(link.from)
            .zip(session.power().network().node(link.to))
        else {
            continue;
        };
        let Some((a, b)) = grid
            .cell_center(terrain, a.cell)
            .zip(grid.cell_center(terrain, b.cell))
        else {
            continue;
        };
        let Some((a, b)) = project(a + Vec3::Y * 0.6).zip(project(b + Vec3::Y * 0.6)) else {
            continue;
        };
        let color = if link.status == LineStatus::InService {
            egui::Color32::from_rgb(110, 220, 240)
        } else {
            egui::Color32::GRAY
        };
        painter.line_segment([a, b], egui::Stroke::new(2.5, color));
        for end in [a, b] {
            painter.rect_stroke(
                egui::Rect::from_center_size(end, egui::vec2(8.0, 8.0)),
                0.0,
                egui::Stroke::new(1.5, color),
                egui::StrokeKind::Middle,
            );
        }
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
    painter.text(
        rect.left_bottom() + egui::vec2(10.0, -6.0),
        egui::Align2::LEFT_BOTTOM,
        l10n.text("hvdc-map-legend"),
        egui::FontId::proportional(10.0),
        egui::Color32::WHITE,
    );
}
