//! The frequency gauge (spec §8.4, §15.3): how far the grid's frequency is from nominal on a dial
//! coloured by protection's bands, the current imbalance, the grid mode, and the load cut on
//! purpose (a rotating outage) or by protection (shed).
//!
//! The dial's zones are the region's own thresholds, read through the domain's [`band`] rule, so
//! the colour a deviation shows is the band protection puts it in.

use application::Game;
use bevy_egui::egui;
use domain::grid::{FrequencyBand, GridMode, band};
use domain::region::FrequencyParams;
use domain::session::GridSnapshot;
use domain::units::{Hertz, Megawatts};

use super::action::UiAction;
use super::map::Rgb;
use super::top_bar::Stat;
use crate::l10n::{Localizer, format};

/// Inside the normal band.
pub const NORMAL_COLOR: Rgb = [90, 190, 110];
/// Below the normal band, above the load-shedding threshold.
pub const LOW_COLOR: Rgb = [230, 190, 60];
/// Below the load-shedding threshold.
pub const SHEDDING_COLOR: Rgb = [220, 70, 60];
/// Above the curtailment threshold.
pub const HIGH_COLOR: Rgb = [90, 150, 230];

/// The colour of a band, on the dial and on the deviation.
pub fn band_color(band: FrequencyBand) -> Rgb {
    match band {
        FrequencyBand::Normal => NORMAL_COLOR,
        FrequencyBand::Low => LOW_COLOR,
        FrequencyBand::Shedding => SHEDDING_COLOR,
        FrequencyBand::High => HIGH_COLOR,
    }
}

/// A stretch of the dial, as fractions of its width (0 is its left end).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DialZone {
    pub from: f32,
    pub to: f32,
    pub color: Rgb,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DialModel {
    /// The bands, left (low frequency) to right, covering the whole dial.
    pub zones: Vec<DialZone>,
    /// Where the deviation is, from 0 (the left end) to 1. Nominal is in the middle.
    pub needle: f32,
    pub band: FrequencyBand,
    pub color: Rgb,
}

/// A rotating outage in effect.
#[derive(Debug, Clone, PartialEq)]
pub struct OutageModel {
    /// `Rotating outage: 12.0% of demand`.
    pub text: String,
    /// `ordered: 15.0%`, while the outage is still moving toward the share ordered.
    pub ordered: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrequencyModel {
    pub title: String,
    /// `+0.03 Hz`.
    pub deviation: String,
    pub dial: DialModel,
    pub imbalance: Stat,
    /// `Normal` or `Blackout`.
    pub mode: String,
    pub blackout: bool,
    /// Shown while part of demand is cut on purpose.
    pub outage: Option<OutageModel>,
    /// `Protection has shed 5.0% of the load`, while protection holds load off.
    pub shed: Option<String>,
}

pub fn frequency_model(game: &Game, l10n: &Localizer) -> FrequencyModel {
    let session = game.session();
    frequency_model_for(&session.grid_snapshot(), session.region().frequency(), l10n)
}

/// The dial runs symmetrically around nominal, far enough to show the blackout and the
/// over-frequency trip thresholds.
fn dial_span(params: &FrequencyParams) -> f64 {
    params
        .blackout_below
        .value()
        .abs()
        .max(params.trip_above.value().abs())
        .max(params.curtail_above.value().abs())
}

fn dial(deviation: Hertz, params: &FrequencyParams) -> DialModel {
    let span = dial_span(params);
    let at = |hz: f64| (((hz + span) / (2.0 * span)).clamp(0.0, 1.0)) as f32;
    let edges = [
        (-span, params.shed_below.value(), FrequencyBand::Shedding),
        (
            params.shed_below.value(),
            -params.normal_band.value(),
            FrequencyBand::Low,
        ),
        (
            -params.normal_band.value(),
            params.curtail_above.value(),
            FrequencyBand::Normal,
        ),
        (params.curtail_above.value(), span, FrequencyBand::High),
    ];
    let current = band(deviation, params);
    DialModel {
        zones: edges
            .into_iter()
            .map(|(from, to, zone)| DialZone {
                from: at(from),
                to: at(to),
                color: band_color(zone),
            })
            .collect(),
        needle: at(deviation.value()),
        band: current,
        color: band_color(current),
    }
}

/// The gauge for a grid snapshot under the region's frequency rules.
pub fn frequency_model_for(
    grid: &GridSnapshot,
    params: &FrequencyParams,
    l10n: &Localizer,
) -> FrequencyModel {
    let deviation = grid.frequency - params.nominal;
    let imbalance = grid
        .last_tick
        .as_ref()
        .map_or(Megawatts::ZERO, |tick| tick.imbalance);
    let outage = (grid.outage.value() > 0.0).then(|| OutageModel {
        text: l10n.args(
            "outage-rotating",
            &[("share", format::percent(grid.outage).into())],
        ),
        ordered: (grid.outage_target != grid.outage).then(|| {
            l10n.args(
                "outage-target",
                &[("share", format::percent(grid.outage_target).into())],
            )
        }),
    });
    FrequencyModel {
        title: l10n.text("frequency-title"),
        deviation: format::hertz_deviation(deviation),
        dial: dial(deviation, params),
        imbalance: Stat {
            label: l10n.text("frequency-imbalance"),
            value: format::megawatts(imbalance),
        },
        mode: l10n.text(match grid.mode {
            GridMode::Normal => "grid-mode-normal",
            GridMode::Blackout => "grid-mode-blackout",
        }),
        blackout: grid.mode == GridMode::Blackout,
        outage,
        shed: (grid.shed.value() > 0.0).then(|| {
            l10n.args(
                "shed-share",
                &[("share", format::percent(grid.shed).into())],
            )
        }),
    }
}

fn color32([r, g, b]: Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}

/// The dial's height.
const DIAL_HEIGHT: f32 = 14.0;

fn draw_dial(ui: &mut egui::Ui, model: &FrequencyModel) {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, DIAL_HEIGHT), egui::Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, model.title.as_str())
    });
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    let x = |fraction: f32| rect.left() + fraction * rect.width();
    for zone in &model.dial.zones {
        let band = egui::Rect::from_x_y_ranges(x(zone.from)..=x(zone.to), rect.y_range());
        painter.rect_filled(band, 0.0, color32(zone.color).gamma_multiply(0.55));
    }
    let nominal = x(0.5);
    painter.line_segment(
        [
            egui::pos2(nominal, rect.top()),
            egui::pos2(nominal, rect.bottom()),
        ],
        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
    );
    let needle = x(model.dial.needle);
    painter.line_segment(
        [
            egui::pos2(needle, rect.top() - 2.0),
            egui::pos2(needle, rect.bottom() + 2.0),
        ],
        egui::Stroke::new(3.0, ui.visuals().strong_text_color()),
    );
}

/// The gauge. It asks for nothing.
pub fn draw_frequency(ui: &mut egui::Ui, model: &FrequencyModel) -> Vec<UiAction> {
    ui.horizontal(|ui| {
        ui.strong(model.title.as_str());
        ui.colored_label(color32(model.dial.color), model.deviation.as_str());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if model.blackout {
                ui.colored_label(color32(SHEDDING_COLOR), model.mode.as_str());
            } else {
                ui.label(model.mode.as_str());
            }
        });
    });
    draw_dial(ui, model);
    ui.horizontal(|ui| {
        ui.weak(model.imbalance.label.as_str());
        ui.label(model.imbalance.value.as_str());
    });
    if let Some(outage) = &model.outage {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(ui.visuals().warn_fg_color, outage.text.as_str());
            if let Some(ordered) = &outage.ordered {
                ui.weak(ordered.as_str());
            }
        });
    }
    if let Some(shed) = &model.shed {
        ui.colored_label(color32(SHEDDING_COLOR), shed.as_str());
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use application::{Command, InMemoryCommandLog};
    use domain::units::Ratio;

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_l10n};

    #[test]
    fn frequency_model_bands_and_outage_indicator() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        game.execute(Command::BeginOperating).unwrap();
        let session = game.session();
        let params = session.region().frequency().clone();
        let nominal = params.nominal;
        // Nothing is committed, so the day opens in a full rotating outage: start from a grid
        // with none.
        let base = GridSnapshot {
            outage: Ratio::ZERO,
            outage_target: Ratio::ZERO,
            shed: Ratio::ZERO,
            mode: GridMode::Normal,
            ..session.grid_snapshot()
        };
        let at = |deviation: f64| GridSnapshot {
            frequency: nominal + Hertz::new(deviation),
            ..base.clone()
        };

        // Each band colours the dial and the deviation, at the region's own thresholds.
        let normal = params.normal_band.value();
        let shed_below = params.shed_below.value();
        let curtail = params.curtail_above.value();
        for (deviation, expected) in [
            (0.0, FrequencyBand::Normal),
            (-normal + 0.001, FrequencyBand::Normal),
            (-normal - 0.001, FrequencyBand::Low),
            (shed_below + 0.001, FrequencyBand::Low),
            (shed_below - 0.001, FrequencyBand::Shedding),
            (curtail - 0.001, FrequencyBand::Normal),
            (curtail + 0.001, FrequencyBand::High),
        ] {
            let model = frequency_model_for(&at(deviation), &params, &l10n);
            assert_eq!(model.dial.band, expected, "{deviation} Hz");
            assert_eq!(model.dial.color, band_color(expected), "{deviation} Hz");
            assert_eq!(
                model.deviation,
                format::hertz_deviation(Hertz::new(deviation))
            );
        }
        assert_eq!(band_color(FrequencyBand::Shedding), SHEDDING_COLOR);
        assert_ne!(NORMAL_COLOR, LOW_COLOR);
        assert_ne!(LOW_COLOR, HIGH_COLOR);

        // The needle is in the middle at nominal and moves the way the frequency does; the zones
        // cover the dial, left to right, in the order the bands go.
        let nominal_model = frequency_model_for(&at(0.0), &params, &l10n);
        assert_eq!(nominal_model.dial.needle, 0.5);
        assert!(frequency_model_for(&at(-0.3), &params, &l10n).dial.needle < 0.5);
        assert!(frequency_model_for(&at(0.3), &params, &l10n).dial.needle > 0.5);
        assert_eq!(
            frequency_model_for(&at(-50.0), &params, &l10n).dial.needle,
            0.0
        );
        let zones = &nominal_model.dial.zones;
        assert_eq!(zones.first().unwrap().from, 0.0);
        assert_eq!(zones.last().unwrap().to, 1.0);
        assert!(zones.windows(2).all(|pair| pair[0].to == pair[1].from));
        let colors: Vec<Rgb> = zones.iter().map(|zone| zone.color).collect();
        assert_eq!(
            colors,
            [SHEDDING_COLOR, LOW_COLOR, NORMAL_COLOR, HIGH_COLOR]
        );

        // A normal grid with no outage and nothing shed.
        assert_eq!(nominal_model.title, "Frequency");
        assert_eq!(nominal_model.mode, "Normal");
        assert!(!nominal_model.blackout);
        assert_eq!(nominal_model.outage, None);
        assert_eq!(nominal_model.shed, None);
        assert_eq!(nominal_model.imbalance.label, "Imbalance");

        // A rotating outage shows its share of demand, and the share ordered while they differ.
        let share = |v: f64| Ratio::new(v).unwrap();
        let cutting = GridSnapshot {
            outage: share(0.12),
            outage_target: share(0.15),
            ..base.clone()
        };
        let model = frequency_model_for(&cutting, &params, &l10n);
        assert_eq!(
            model.outage,
            Some(OutageModel {
                text: "Rotating outage: 12.0% of demand".to_owned(),
                ordered: Some("ordered: 15.0%".to_owned()),
            })
        );
        let settled = GridSnapshot {
            outage: share(0.12),
            outage_target: share(0.12),
            ..base.clone()
        };
        let model = frequency_model_for(&settled, &params, &l10n);
        assert_eq!(model.outage.unwrap().ordered, None);
        // Ordered but not cut yet: the indicator waits for the outage itself.
        let ordered_only = GridSnapshot {
            outage_target: share(0.10),
            ..base.clone()
        };
        assert_eq!(
            frequency_model_for(&ordered_only, &params, &l10n).outage,
            None
        );

        // Protection's shed is a share of the load the outage leaves, and says so.
        let shedding = GridSnapshot {
            shed: share(0.05),
            mode: GridMode::Blackout,
            ..base.clone()
        };
        let model = frequency_model_for(&shedding, &params, &l10n);
        assert_eq!(
            model.shed.as_deref(),
            Some("Protection has shed 5.0% of the load")
        );
        assert_eq!(model.mode, "Blackout");
        assert!(model.blackout);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_imbalance_is_the_last_ticks() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        game.execute(Command::BeginOperating).unwrap();
        assert_eq!(frequency_model(&game, &l10n).imbalance.value, "0.0 MW");

        game.execute(Command::AdvanceTicks { n: 3 }).unwrap();

        let grid = game.session().grid_snapshot();
        let last = grid.last_tick.as_ref().expect("a tick ran");
        let model = frequency_model(&game, &l10n);
        assert_eq!(model.imbalance.value, format::megawatts(last.imbalance));
        assert_eq!(
            model.deviation,
            format::hertz_deviation(grid.frequency - game.session().region().frequency().nominal)
        );
    }
}
