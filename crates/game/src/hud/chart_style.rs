//! Plot labels share the HUD's contrast contract. Ticks appear at full contrast or stay hidden;
//! bar hover labels get an opaque backing instead of writing directly over coloured data.
use std::ops::RangeInclusive;

use bevy_egui::egui::{self, Color32, Pos2, Shape, Ui};
use egui_plot::{
    AxisHints, BarChart, ClosestElem, Cursor, LabelFormatterFn, PlotBounds, PlotConfig,
    PlotGeometry, PlotItem, PlotItemBase, PlotMemory, PlotTransform,
};

use super::theme::{LABEL_BACKING, LABEL_TEXT, TEXT};

pub(super) fn x_axis(label: &str) -> AxisHints<'static> {
    AxisHints::new_x()
        .label(label.to_owned())
        .tick_label_color(TEXT)
        .label_spacing(egui::emath::Rangef::new(60.0, 60.0))
}

pub(super) fn y_axis(label: &str) -> AxisHints<'static> {
    AxisHints::new_y()
        .label(label.to_owned())
        .tick_label_color(TEXT)
        .label_spacing(egui::emath::Rangef::new(20.0, 20.0))
}

#[derive(Clone, Copy)]
pub(super) enum KeyMark {
    Band,
    Line,
    Dashed,
}

/// Reserve layout space above the plot while retaining its series visibility and hover state.
pub(super) fn draw_key<'a>(
    ui: &mut Ui,
    plot_id: egui::Id,
    entries: impl IntoIterator<Item = (&'a str, Color32, KeyMark)>,
) -> Option<egui::Id> {
    draw_key_with_control(ui, plot_id, entries, |_| {})
}

/// A wrapping key with an optional chart control in the same reserved layout space.
pub(super) fn draw_key_with_control<'a>(
    ui: &mut Ui,
    plot_id: egui::Id,
    entries: impl IntoIterator<Item = (&'a str, Color32, KeyMark)>,
    control: impl FnOnce(&mut Ui),
) -> Option<egui::Id> {
    let mut memory = PlotMemory::load(ui.ctx(), plot_id);
    let mut hovered = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 2.0);
        ui.horizontal_wrapped(|ui| {
            control(ui);
            for (name, colour, mark) in entries {
                let id = egui::Id::new(name);
                let mut visible = !memory
                    .as_ref()
                    .is_some_and(|m| m.hidden_items.contains(&id));
                let response = key_entry(ui, name, colour, mark, &mut visible);
                if response.hovered() {
                    hovered = Some(id);
                }
                if let Some(memory) = &mut memory {
                    if visible {
                        memory.hidden_items.remove(&id);
                    } else {
                        memory.hidden_items.insert(id);
                    }
                }
            }
        });
    });
    if let Some(mut memory) = memory {
        // egui_plot's legend-hover handling hides axes; highlight our hovered item directly.
        memory.hovered_legend_item = None;
        memory.store(ui.ctx(), plot_id);
    }
    hovered
}

fn key_entry(
    ui: &mut egui::Ui,
    name: &str,
    colour: egui::Color32,
    mark: KeyMark,
    visible: &mut bool,
) -> egui::Response {
    let text = ui.painter().layout_no_wrap(
        name.to_owned(),
        egui::FontId::proportional(12.0),
        ui.visuals().text_color(),
    );
    let size = egui::vec2(text.size().x + 22.0, text.size().y.max(16.0));
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *visible = !*visible;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *visible, name)
    });
    let painter = ui.painter();
    if response.hovered() || response.has_focus() {
        painter.rect_stroke(
            rect,
            2.0,
            ui.visuals().widgets.hovered.fg_stroke,
            egui::StrokeKind::Inside,
        );
    }
    let swatch = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 8.0, rect.center().y),
        egui::vec2(16.0, 8.0),
    );
    if !*visible {
        painter.rect_stroke(
            swatch,
            0.0,
            egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
            egui::StrokeKind::Inside,
        );
    } else {
        match mark {
            KeyMark::Band => {
                painter.rect_filled(swatch, 1.0, colour);
            }
            KeyMark::Line => {
                painter.line_segment(
                    [swatch.left_center(), swatch.right_center()],
                    egui::Stroke::new(2.0, colour),
                );
            }
            KeyMark::Dashed => painter.extend(egui::Shape::dashed_line(
                &[swatch.left_center(), swatch.right_center()],
                egui::Stroke::new(1.0, colour),
                3.0,
                2.0,
            )),
        }
    }
    painter.galley(
        egui::pos2(rect.left() + 22.0, rect.center().y - text.size().y * 0.5),
        text,
        ui.visuals().text_color(),
    );
    response
}

pub(super) struct ReadableBars(pub BarChart);

impl PlotItem for ReadableBars {
    fn shapes(&self, ui: &Ui, transform: &PlotTransform, shapes: &mut Vec<Shape>) {
        self.0.shapes(ui, transform, shapes);
    }
    fn initialize(&mut self, range: RangeInclusive<f64>) {
        self.0.initialize(range);
    }
    fn color(&self) -> Color32 {
        PlotItem::color(&self.0)
    }
    fn geometry(&self) -> PlotGeometry<'_> {
        self.0.geometry()
    }
    fn bounds(&self) -> PlotBounds {
        self.0.bounds()
    }
    fn base(&self) -> &PlotItemBase {
        self.0.base()
    }
    fn base_mut(&mut self) -> &mut PlotItemBase {
        self.0.base_mut()
    }
    fn find_closest(&self, point: Pos2, transform: &PlotTransform) -> Option<ClosestElem> {
        self.0.find_closest(point, transform)
    }
    fn on_hover(
        &self,
        response: &egui::Response,
        elem: ClosestElem,
        shapes: &mut Vec<Shape>,
        cursors: &mut Vec<Cursor>,
        plot: &PlotConfig<'_>,
        formatter: Option<&LabelFormatterFn<'_>>,
    ) {
        let mut hovered = Vec::new();
        self.0
            .on_hover(response, elem, &mut hovered, cursors, plot, formatter);
        for shape in hovered {
            if let Shape::Text(mut text) = shape {
                text.override_text_color = Some(LABEL_TEXT);
                shapes.push(Shape::rect_filled(
                    text.visual_bounding_rect().expand(2.0),
                    2.0,
                    LABEL_BACKING,
                ));
                shapes.push(Shape::Text(text));
            } else {
                shapes.push(shape);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_plot::{Bar, Plot, PlotPoint};

    #[test]
    fn bar_hover_text_gets_its_own_opaque_surface() {
        let ctx = egui::Context::default();
        ctx.set_visuals(super::super::theme::visuals());
        let mut pointer = None;
        let mut count = 0;
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(700.0, 400.0),
                )),
                events: pointer.map(egui::Event::PointerMoved).into_iter().collect(),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                let response = Plot::new("contrast-bars")
                    .height(300.0)
                    .include_x(0.0)
                    .include_x(2.0)
                    .include_y(0.0)
                    .include_y(100.0)
                    .show(ui, |plot| {
                        plot.add(ReadableBars(BarChart::new(
                            "Test bar",
                            vec![
                                Bar::new(1.0, 100.0)
                                    .name("Test bar")
                                    .width(0.8)
                                    .fill(Color32::YELLOW),
                            ],
                        )))
                    });
                pointer = Some(
                    response
                        .transform
                        .position_from_point(&PlotPoint::new(1.0, 90.0)),
                );
            });
            let shapes = output.shapes.clone();
            output.drop_without_applying_deltas();
            fn check(shapes: &[Shape]) -> usize {
                let mut found = 0;
                for (i, shape) in shapes.iter().enumerate() {
                    match shape {
                        Shape::Vec(children) => found += check(children),
                        Shape::Text(text) if text.galley.text().starts_with("Test bar") => {
                            let Shape::Rect(backing) = &shapes[i - 1] else {
                                panic!("hover label needs a backing")
                            };
                            assert_eq!(backing.fill, LABEL_BACKING);
                            assert!(backing.rect.contains_rect(text.visual_bounding_rect()));
                            assert_eq!(text.override_text_color, Some(LABEL_TEXT));
                            found += 1;
                        }
                        _ => {}
                    }
                }
                found
            }
            fn flatten(shape: Shape, flat: &mut Vec<Shape>) {
                match shape {
                    Shape::Vec(children) => {
                        for child in children {
                            flatten(child, flat);
                        }
                    }
                    other => flat.push(other),
                }
            }
            let mut flat = Vec::new();
            for clipped in shapes {
                flatten(clipped.shape, &mut flat);
            }
            count += check(&flat);
        }
        assert!(
            count > 0,
            "the pointer must actually show the bar's hover label"
        );
    }
}
