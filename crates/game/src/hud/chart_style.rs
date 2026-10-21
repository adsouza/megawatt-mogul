//! Plot labels share the HUD's contrast contract. Ticks appear at full contrast or stay hidden;
//! bar hover labels get an opaque backing instead of writing directly over coloured data.
use std::ops::RangeInclusive;

use bevy_egui::egui::{self, Color32, Pos2, Shape, Ui};
use egui_plot::{
    AxisHints, BarChart, ClosestElem, Cursor, LabelFormatterFn, PlotBounds, PlotConfig,
    PlotGeometry, PlotItem, PlotItemBase, PlotTransform,
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
