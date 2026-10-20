//! The planning warnings: where the committed fleet will not cope with the forecast day.
//!
//! Three kinds, listed in this order: a capacity shortfall (§15.5, the plan would leave load to a
//! rotating outage), committed minimum generation above the load (§14.3, the plan would have to
//! dump or curtail), and hydro plants with no water to spare today (§2.2). The two checks sample
//! the worse end of every hour, as the domain does. A warning that names an hour has a *show hour*
//! button, which tints that column of the commitment grid.

use application::Game;
use bevy_egui::egui;
use domain::session::{CapacityShortfallWarning, MinimumGenerationWarning};
use domain::time::TICKS_PER_HOUR;
use domain::units::MegawattHours;

use super::action::{ButtonModel, UiAction, button};
use super::plant_list::plants_by_kind;
use crate::l10n::{Localizer, Names, format};

/// What a warning is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningKind {
    /// The committed fleet falls short of the forecast demand in some hour.
    Shortfall,
    /// Committed minimum generation exceeds the net load in some hour.
    MinimumGeneration,
    /// A hydro plant has no water for the day.
    DryHydro,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WarningRow {
    pub kind: WarningKind,
    /// `18:00: short by 12.0 GW. Expect a rotating outage.`
    pub text: String,
    /// Shows the hour in the commitment grid. Only a warning about an hour has one.
    pub show_hour: Option<ButtonModel>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WarningsModel {
    pub heading: String,
    pub rows: Vec<WarningRow>,
    /// Shown when there is nothing to warn about.
    pub none: Option<String>,
}

fn time_of(hour: u8) -> String {
    format::clock(u32::from(hour) * TICKS_PER_HOUR)
}

fn show_hour(l10n: &Localizer, hour: u8) -> ButtonModel {
    ButtonModel::new(
        l10n.text("warning-show-hour"),
        UiAction::ShowHour(Some(hour)),
    )
}

/// The warnings for the given findings: shortfall first, then minimum generation, then each dry
/// hydro plant (by name). Pure, so the order and wording are testable without a game that
/// happens to trigger all three.
pub fn warnings_for(
    shortfall: Option<CapacityShortfallWarning>,
    min_generation: Option<MinimumGenerationWarning>,
    dry_hydro: &[String],
    l10n: &Localizer,
) -> WarningsModel {
    let mut rows = Vec::new();
    if let Some(warning) = shortfall {
        rows.push(WarningRow {
            kind: WarningKind::Shortfall,
            text: l10n.args(
                "warning-shortfall",
                &[
                    ("time", time_of(warning.hour).into()),
                    ("amount", format::megawatts(warning.shortfall).into()),
                ],
            ),
            show_hour: Some(show_hour(l10n, warning.hour)),
        });
    }
    if let Some(warning) = min_generation {
        rows.push(WarningRow {
            kind: WarningKind::MinimumGeneration,
            text: l10n.args(
                "warning-min-gen",
                &[
                    ("time", time_of(warning.hour).into()),
                    ("amount", format::megawatts(warning.excess).into()),
                ],
            ),
            show_hour: Some(show_hour(l10n, warning.hour)),
        });
    }
    for plant in dry_hydro {
        rows.push(WarningRow {
            kind: WarningKind::DryHydro,
            text: l10n.args("warning-hydro-dry", &[("plant", plant.as_str().into())]),
            show_hour: None,
        });
    }
    WarningsModel {
        heading: l10n.text("warnings-title"),
        none: rows.is_empty().then(|| l10n.text("warnings-none")),
        rows,
    }
}

/// The warnings for the game as planned now.
pub fn warnings_model(game: &Game, l10n: &Localizer) -> WarningsModel {
    let session = game.session();
    let names = Names::new(l10n, session);
    let dry_hydro: Vec<String> = plants_by_kind(game)
        .iter()
        .flat_map(|group| &group.plants)
        .filter(|plant| {
            plant
                .hydro_day_budget
                .is_some_and(|budget| budget <= MegawattHours::ZERO)
        })
        .map(|plant| names.plant(plant.id))
        .collect();
    warnings_for(
        session.capacity_shortfall_check(),
        session.minimum_generation_check(),
        &dry_hydro,
        l10n,
    )
}

pub fn draw_warnings(ui: &mut egui::Ui, model: &WarningsModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.horizontal_wrapped(|ui| {
        ui.strong(model.heading.as_str());
        if let Some(none) = &model.none {
            ui.weak(none.as_str());
        }
        for row in &model.rows {
            ui.separator();
            ui.colored_label(ui.visuals().warn_fg_color, row.text.as_str());
            if let Some(show) = &row.show_hour {
                button(ui, show, &mut actions);
            }
        }
    });
    actions
}

#[cfg(test)]
mod tests {
    use application::InMemoryCommandLog;
    use domain::region::PlantKind;
    use domain::units::Megawatts;

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_game, shipped_l10n};

    fn shortfall(hour: u8) -> CapacityShortfallWarning {
        CapacityShortfallWarning {
            hour,
            demand: Megawatts::new(22_000.0),
            deliverable: Megawatts::new(10_000.0),
            shortfall: Megawatts::new(12_000.0),
        }
    }

    fn min_gen(hour: u8) -> MinimumGenerationWarning {
        MinimumGenerationWarning {
            hour,
            committed_minimum: Megawatts::new(3_300.0),
            net_load: Megawatts::new(3_000.0),
            excess: Megawatts::new(300.0),
        }
    }

    #[test]
    fn warnings_list_shortfall_then_min_gen_then_hydro() {
        let l10n = shipped_l10n();

        let model = warnings_for(
            Some(shortfall(18)),
            Some(min_gen(3)),
            &["Hydro plant 5".to_owned(), "Hydro plant 9".to_owned()],
            &l10n,
        );

        let kinds: Vec<WarningKind> = model.rows.iter().map(|row| row.kind).collect();
        assert_eq!(
            kinds,
            [
                WarningKind::Shortfall,
                WarningKind::MinimumGeneration,
                WarningKind::DryHydro,
                WarningKind::DryHydro,
            ]
        );
        let texts: Vec<&str> = model.rows.iter().map(|row| row.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "18:00: short by 12.0 GW. Expect a rotating outage.",
                "03:00: committed minimum exceeds load by 300 MW.",
                "Hydro plant 5 has no water to spare today.",
                "Hydro plant 9 has no water to spare today.",
            ]
        );
        assert_eq!(model.none, None);
        assert_eq!(model.heading, "Warnings");
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn a_warning_about_an_hour_can_show_that_hour_and_a_hydro_one_cannot() {
        let l10n = shipped_l10n();

        let model = warnings_for(
            Some(shortfall(18)),
            Some(min_gen(3)),
            &["Hydro plant 5".to_owned()],
            &l10n,
        );

        let shows: Vec<Option<UiAction>> = model
            .rows
            .iter()
            .map(|row| row.show_hour.as_ref().map(|button| button.action.clone()))
            .collect();
        assert_eq!(
            shows,
            [
                Some(UiAction::ShowHour(Some(18))),
                Some(UiAction::ShowHour(Some(3))),
                None
            ]
        );
        let button = model.rows[0].show_hour.as_ref().unwrap();
        assert_eq!(button.label, "Show hour");
        assert!(button.enabled);
    }

    #[test]
    fn no_findings_say_so() {
        let l10n = shipped_l10n();

        let model = warnings_for(None, None, &[], &l10n);

        assert!(model.rows.is_empty());
        assert_eq!(
            model.none.as_deref(),
            Some("No warnings: the commitments cover the forecast.")
        );
    }

    #[test]
    fn an_uncommitted_fleet_warns_of_a_shortfall_at_the_domains_hour() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let worst = session
            .capacity_shortfall_check()
            .expect("nothing is committed, so demand goes uncovered");

        let model = warnings_model(&game, &l10n);

        assert_eq!(model.rows[0].kind, WarningKind::Shortfall);
        assert_eq!(
            model.rows[0].text,
            l10n.args(
                "warning-shortfall",
                &[
                    ("time", format::clock(u32::from(worst.hour) * 60).into()),
                    ("amount", format::megawatts(worst.shortfall).into()),
                ]
            )
        );
        assert_eq!(
            model.rows[0].show_hour.as_ref().unwrap().action,
            UiAction::ShowHour(Some(worst.hour))
        );
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_shipped_western_fleet_has_water_so_no_hydro_warning() {
        let l10n = shipped_l10n();
        let game = shipped_game(
            "western_interconnection",
            Box::new(InMemoryCommandLog::default()),
        );
        assert!(
            game.session()
                .plant_views()
                .iter()
                .any(|p| p.kind == PlantKind::Hydro),
            "the western fleet has hydro"
        );

        let model = warnings_model(&game, &l10n);

        assert!(
            model
                .rows
                .iter()
                .all(|row| row.kind != WarningKind::DryHydro),
            "{:?}",
            model.rows
        );
    }
}
