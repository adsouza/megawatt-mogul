//! The predictive ramp warnings (spec §8.4): the [`RampWarningProjection`]'s current warning,
//! localised and timed, in the Operating panel's side column.
//!
//! [`RampWarningProjection`]: application::projections::RampWarningProjection

use application::Game;
use application::projections::RampWarning;
use bevy_egui::egui;

use super::action::UiAction;
use super::theme::{TextTone, heading_color};
use crate::l10n::{Localize, Localizer, format};

#[derive(Debug, Clone, PartialEq)]
pub struct RampWarningsModel {
    pub title: String,
    /// `17:00: Net load is about to rise by ...`: none or the one current warning.
    pub rows: Vec<String>,
    /// Shown when there is none.
    pub empty: Option<String>,
}

pub fn ramp_warnings_model(game: &Game, l10n: &Localizer) -> RampWarningsModel {
    let now = game.session().clock().tick();
    ramp_warnings_for(game.projections().ramp_warnings.current(), now, l10n)
}

/// The warnings to show at tick `now`. The projection keeps its last warning until it is next
/// evaluated, so one from yesterday's evening (a tick at or after `now`) is not today's.
pub fn ramp_warnings_for(
    current: Option<&RampWarning>,
    now: u32,
    l10n: &Localizer,
) -> RampWarningsModel {
    // The projection holds only the current warning, so there is at most one row.
    let rows: Vec<String> = current
        .into_iter()
        .filter(|warning| warning.at_tick < now)
        .map(|warning| {
            l10n.args(
                "ramp-warning-row",
                &[
                    ("time", format::clock(warning.at_tick).into()),
                    ("warning", warning.localize(l10n).into()),
                ],
            )
        })
        .collect();
    RampWarningsModel {
        title: l10n.text("ramp-warnings-title"),
        empty: rows.is_empty().then(|| l10n.text("ramp-warnings-none")),
        rows,
    }
}

/// The heading and the warnings, which wrap. It asks for nothing.
pub fn draw_ramp_warnings(ui: &mut egui::Ui, model: &RampWarningsModel) -> Vec<UiAction> {
    ui.label(
        egui::RichText::new(&model.title)
            .strong()
            .color(heading_color()),
    );
    if let Some(empty) = &model.empty {
        ui.colored_label(TextTone::Positive.color(), empty);
    }
    for row in &model.rows {
        ui.colored_label(TextTone::Caution.color(), row);
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use application::{Command, InMemoryCommandLog};
    use domain::units::Megawatts;

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_l10n};

    fn warning(at_tick: u32) -> RampWarning {
        RampWarning {
            at_tick,
            horizon_minutes: 60,
            net_load_change: Megawatts::new(6_200.0),
            capability: Megawatts::new(4_900.0),
        }
    }

    #[test]
    fn a_warning_is_timed_and_localised() {
        let l10n = shipped_l10n();
        let current = warning(1_020);

        let model = ramp_warnings_for(Some(&current), 1_021, &l10n);

        assert_eq!(model.title, "Ramp warnings");
        assert_eq!(model.rows, [format!("17:00: {}", current.localize(&l10n))]);
        assert!(model.rows[0].contains("6,200 MW"), "{}", model.rows[0]);
        assert_eq!(model.empty, None);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn no_warning_or_yesterdays_says_so() {
        let l10n = shipped_l10n();
        let none = ramp_warnings_for(None, 600, &l10n);
        assert_eq!(none.rows, Vec::<String>::new());
        assert_eq!(none.empty.as_deref(), Some("No ramp warnings."));

        let yesterday = warning(1_435);
        assert_eq!(ramp_warnings_for(Some(&yesterday), 0, &l10n).rows.len(), 0);
    }

    #[test]
    fn the_model_reads_the_projection() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: 30 }).unwrap();

        let model = ramp_warnings_model(&game, &l10n);

        let expected = ramp_warnings_for(
            game.projections().ramp_warnings.current(),
            game.session().clock().tick(),
            &l10n,
        );
        assert_eq!(model, expected);
    }
}
