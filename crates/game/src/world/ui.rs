//! The transparent map surface, current-pass input ownership and opaque projected annotations.
use super::{CameraPose, TuningConfig, WorldAction, WorldScene, physical_viewport, presented_hit};
use crate::{
    hud::{ActionSink, ButtonModel, MapModel, UiAction, button, dispatch, theme},
    l10n::{Localize, Localizer},
    sim::{SaveScreen, Session},
};
use bevy::{ecs::system::SystemParam, prelude::*};
use bevy_egui::{EguiContexts, egui};
use domain::session::{DayPhase, SessionStatus};

#[derive(Resource, Default)]
pub struct MapSurface {
    pub rect: Option<egui::Rect>,
    pub scale: f32,
    pub pose: Option<CameraPose>,
    pub legend: Option<egui::Rect>,
    response: Option<egui::Response>,
    blocked: bool,
}
impl MapSurface {
    pub fn clear(&mut self) {
        *self = default();
    }
}
#[derive(Debug, Clone)]
pub struct ToolbarModel {
    pub resources_visible: bool,
    pub reset: ButtonModel,
    pub overlay: ButtonModel,
    pub effects: ButtonModel,
    pub help: String,
    pub help_label: String,
    pub layer_label: String,
    pub control_room: Option<ButtonModel>,
    pub supply: String,
}
pub fn toolbar_model(
    l10n: &Localizer,
    overlay: bool,
    effects: bool,
    holo: bool,
    operating: bool,
) -> ToolbarModel {
    ToolbarModel {
        resources_visible: overlay && !holo,
        reset: ButtonModel::new(
            l10n.text("world-reset"),
            UiAction::World(WorldAction::Reset),
        ),
        overlay: ButtonModel::new(
            l10n.text(if overlay {
                "world-overlay-hide"
            } else {
                "world-overlay-show"
            }),
            UiAction::World(WorldAction::ToggleOverlay),
        ),
        effects: ButtonModel::new(
            l10n.text(if effects {
                "world-effects-off"
            } else {
                "world-effects-on"
            }),
            UiAction::World(WorldAction::ToggleEffects),
        ),
        help: l10n.text("world-help"),
        help_label: l10n.text("world-help-label"),
        layer_label: l10n.text("world-layer-label"),
        control_room: operating.then(|| {
            ButtonModel::new(
                l10n.text("world-control-room"),
                UiAction::World(WorldAction::ControlRoom),
            )
            .enabled_if(!holo)
        }),
        supply: if holo {
            l10n.text("world-copper-plate")
        } else {
            String::new()
        },
    }
}
pub fn draw_surface(
    ui: &mut egui::Ui,
    model: &MapModel,
    toolbar: &ToolbarModel,
    surface: &mut MapSurface,
) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let choices = |ui: &mut egui::Ui, actions: &mut Vec<UiAction>, close: bool| {
        for layer in &model.layers {
            let mut layer = layer.clone();
            layer.button.enabled &= !surface.blocked;
            layer.on &= toolbar.supply.is_empty();
            if crate::hud::toggle_button(ui, &layer, actions).clicked() && close {
                ui.close();
            }
        }
    };
    if ui.available_width() < 650.0 {
        let selected = if toolbar.supply.is_empty() {
            model
                .layers
                .iter()
                .find(|layer| layer.on)
                .map_or(model.label.as_str(), |layer| layer.button.label.as_str())
        } else {
            toolbar
                .control_room
                .as_ref()
                .map_or(model.label.as_str(), |b| b.label.as_str())
        };
        ui.horizontal(|ui| {
            ui.label(&toolbar.layer_label);
            egui::ComboBox::from_id_salt("world_layer_selector")
                .width(ui.available_width())
                .truncate()
                .selected_text(selected)
                .show_ui(ui, |ui| choices(ui, &mut actions, true));
        });
    } else {
        ui.horizontal_wrapped(|ui| choices(ui, &mut actions, false));
    }
    ui.horizontal_wrapped(|ui| {
        for model in [&toolbar.reset, &toolbar.overlay, &toolbar.effects]
            .into_iter()
            .chain(toolbar.control_room.iter())
        {
            let mut model = model.clone();
            model.enabled &= !surface.blocked;
            button(ui, &model, &mut actions);
        }
        let open = model.legend_choice.0.unwrap_or(false);
        crate::hud::toggle_button(
            ui,
            &crate::hud::legend_toggle(&model.legend_toggle, open),
            &mut actions,
        );
        ui.label(&toolbar.help_label).on_hover_text(&toolbar.help);
    });
    if !toolbar.supply.is_empty() {
        egui::Frame::NONE
            .fill(theme::LABEL_BACKING)
            .inner_margin(3.0)
            .show(ui, |ui| {
                ui.small(&toolbar.supply);
            });
    }
    if toolbar.resources_visible
        && let Some(scale) = &model.resource_scale
    {
        crate::hud::map::draw_resource_scale(ui, scale);
    }
    let response = ui.allocate_response(
        ui.available_size().max(egui::vec2(1.0, 1.0)),
        egui::Sense::click_and_drag(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &model.label));
    surface.rect = Some(response.rect);
    surface.scale = ui.ctx().pixels_per_point();
    surface.response = Some(response.clone());
    surface.legend = None;
    if model.legend_choice.0.unwrap_or(false) {
        // Collapsed by default in 3D: perspective and orbit can put marks in any corner.
        surface.legend = Some(super::super::hud::map::draw_world_legend(
            ui.ctx(),
            response.rect,
            model,
        ));
    }
    if surface.blocked {
        actions.clear();
    }
    actions
}
fn blocked(session: &Session, sink: &ActionSink) -> bool {
    session.0.session().phase() == DayPhase::Review
        || session.0.session().status() != SessionStatus::Active
        || sink.open_prompt().0.is_some()
        || sink.persistence().screen != SaveScreen::Closed
        || sink.persistence().frozen()
}
pub fn mark_blocked(session: &Session, sink: &ActionSink, surface: &mut MapSurface) {
    surface.blocked = blocked(session, sink);
}
/// Last HUD system: all floating windows have registered their *current* layer and geometry.
#[derive(SystemParam)]
pub(super) struct MapReads<'w> {
    scene: Res<'w, WorldScene>,
    l10n: Res<'w, Localizer>,
    tuning: Res<'w, TuningConfig>,
}
pub fn finish_surface(
    mut contexts: EguiContexts,
    session: Option<Res<Session>>,
    reads: MapReads,
    mut surface: ResMut<MapSurface>,
    mut sink: ActionSink,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) -> Result {
    let MapReads {
        scene,
        l10n,
        tuning,
    } = reads;
    let ctx = contexts.ctx_mut()?;
    let Some(session) = session else {
        surface.clear();
        return Ok(());
    };
    let (Some(model), Some(response)) = (&scene.model, surface.response.clone()) else {
        return Ok(());
    };
    let rect = response.rect;
    // Align projection and rays to the exact rounded physical viewport, including fractional DPI.
    let rect = if let Some(window) = windows.iter().next() {
        let Some(view) = physical_viewport(
            rect,
            surface.scale,
            UVec2::new(window.physical_width(), window.physical_height()),
        ) else {
            surface.pose = None;
            return Ok(());
        };
        let min = view.physical_position.as_vec2() / surface.scale;
        let size = view.physical_size.as_vec2() / surface.scale;
        egui::Rect::from_min_size(egui::pos2(min.x, min.y), egui::vec2(size.x, size.y))
    } else {
        rect
    };
    surface.rect = Some(rect);
    let pointer = ctx.pointer_latest_pos();
    let owns = pointer.is_some_and(|p| {
        rect.contains(p)
            && !surface.legend.is_some_and(|l| l.contains(p))
            && ctx.layer_id_at(p) == Some(response.layer_id)
            && ctx.memory(|m| m.top_modal_layer().is_none())
            && !egui::Popup::is_any_open(ctx)
    });
    let allowed = !surface.blocked && !blocked(&session, &sink);
    let mut actions = Vec::new();
    if allowed && owns {
        if response.dragged_by(egui::PointerButton::Primary) {
            let d = response.drag_delta();
            actions.push(UiAction::World(WorldAction::Orbit(Vec2::new(d.x, d.y))));
        }
        if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
        {
            let d = response.drag_delta();
            actions.push(UiAction::World(WorldAction::Pan(
                Vec2::new(d.x, d.y),
                model.size(),
                rect.height(),
            )));
        }
        // Smoothed scroll outlives its original widget: moving from a panel to the map must
        // not turn the panel's leftover inertia into camera zoom. Use only this frame's wheel
        // events, reserving modified wheel gestures for egui's UI zoom/navigation.
        let line = ctx.options(|options| options.input_options.line_scroll_speed);
        let scroll = ctx.input(|input| {
            input
                .raw
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::MouseWheel {
                        unit,
                        delta,
                        modifiers,
                        ..
                    } if *modifiers == egui::Modifiers::NONE => Some(
                        delta.y
                            * match unit {
                                egui::MouseWheelUnit::Point => 1.0,
                                egui::MouseWheelUnit::Line => line,
                                egui::MouseWheelUnit::Page => rect.height(),
                            },
                    ),
                    _ => None,
                })
                .sum::<f32>()
        });
        if scroll != 0.0 {
            actions.push(UiAction::World(WorldAction::Zoom(scroll)));
        }
        if !ctx.egui_wants_keyboard_input() {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::R)) {
                actions.push(UiAction::World(WorldAction::Reset));
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::O)) {
                actions.push(UiAction::World(WorldAction::ToggleOverlay));
            }
        }
    }
    dispatch(actions, &mut sink);
    let pose = sink
        .world()
        .orbit
        .pose(model.size(), rect.width() / rect.height());
    surface.pose = Some(pose);
    if allowed
        && owns
        && response.clicked_by(egui::PointerButton::Primary)
        && let Some(p) = pointer
        && let Some(ray) = pose.ray(screen_ndc(rect, p))
    {
        let selected = presented_hit(ray, model, sink.world(), &tuning)
            .map_or(crate::hud::Selection::None, |hit| hit.selection);
        dispatch([UiAction::Select(selected)], &mut sink);
    }
    let painter = ctx.layer_painter(response.layer_id).with_clip_rect(rect);
    let mut occupied = surface.legend.into_iter().collect::<Vec<_>>();
    // Construction labels and selected/hovered objects take priority over city names.
    let hover = if owns {
        pointer
            .and_then(|p| pose.ray(screen_ndc(rect, p)))
            .map(|ray| {
                presented_hit(ray, model, sink.world(), &tuning)
                    .map_or(crate::hud::Selection::None, |hit| hit.selection)
            })
    } else {
        None
    };
    let selection = sink.selection();
    let mut labels = Vec::new();
    for p in model.plants.values() {
        let at = p.at + Vec3::Y * 2.8 * tuning.scale();
        let title = if let Some(months) = p.building {
            l10n.args("map-months-left", &[("months", months.into())])
        } else if selection == crate::hud::Selection::Plant(p.id)
            || hover == Some(crate::hud::Selection::Plant(p.id))
        {
            p.kind.localize(&l10n)
        } else {
            continue;
        };
        labels.push((at, title));
    }
    for l in model.lines.values() {
        if let Some(months) = l.building {
            labels.push((
                l.midpoint,
                l10n.args("map-months-left", &[("months", months.into())]),
            ));
        }
    }
    for c in model.cities.values() {
        labels.push((
            c.at + Vec3::Y * 1.4 * c.scale * tuning.scale(),
            l10n.text(&c.name_key),
        ));
    }
    for (at, text) in labels {
        let Some(ndc) = pose.project(at) else {
            continue;
        };
        if ndc.x.abs() > 1.0 || ndc.y.abs() > 1.0 {
            continue;
        }
        let Some(ray) = pose.ray(ndc.truncate()) else {
            continue;
        };
        if presented_hit(ray, model, sink.world(), &tuning)
            .is_some_and(|hit| hit.distance + 0.2 < at.distance(pose.eye))
        {
            continue;
        }
        let anchor = egui::pos2(
            rect.left() + (ndc.x + 1.0) * 0.5 * rect.width(),
            rect.top() + (1.0 - ndc.y) * 0.5 * rect.height(),
        );
        let galley = painter.layout_no_wrap(
            text.clone(),
            egui::FontId::proportional(11.0),
            theme::LABEL_TEXT,
        );
        let Some(label) = label_rect(
            rect.shrink(3.0),
            anchor,
            galley.size() + egui::vec2(4.0, 4.0),
        ) else {
            continue;
        };
        if occupied.iter().any(|r| r.intersects(label)) {
            continue;
        }
        painter.rect_filled(label, 2.0, theme::LABEL_BACKING);
        painter.galley(label.min + egui::vec2(2.0, 2.0), galley, theme::LABEL_TEXT);
        occupied.push(label);
    }
    Ok(())
}
/// Flip then clamp the opaque backing, with no partially cut-off text at viewport edges.
pub fn label_rect(map: egui::Rect, anchor: egui::Pos2, size: egui::Vec2) -> Option<egui::Rect> {
    if !map.is_positive() || size.x > map.width() || size.y > map.height() || !size.is_finite() {
        return None;
    }
    let x = if anchor.x + size.x + 4.0 > map.right() {
        anchor.x - size.x - 4.0
    } else {
        anchor.x + 4.0
    };
    let y = if anchor.y - size.y - 4.0 < map.top() {
        anchor.y + 4.0
    } else {
        anchor.y - size.y - 4.0
    };
    let min = egui::pos2(
        x.clamp(map.left(), map.right() - size.x),
        y.clamp(map.top(), map.bottom() - size.y),
    );
    Some(egui::Rect::from_min_size(min, size))
}

pub fn screen_ndc(rect: egui::Rect, p: egui::Pos2) -> Vec2 {
    Vec2::new(
        2.0 * (p.x - rect.left()) / rect.width() - 1.0,
        1.0 - 2.0 * (p.y - rect.top()) / rect.height(),
    )
}
