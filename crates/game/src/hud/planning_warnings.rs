//! The planning warnings: where the committed fleet will not cope with the forecast day.
//!
//! Three kinds, listed in this order: a capacity shortfall (§15.5, the plan would leave load to a
//! rotating outage), committed minimum generation above the load (§14.3, the plan would have to
//! dump or curtail), and hydro plants with no water to spare today (§2.2). The two checks sample
//! the worse end of every hour, as the domain does. A warning that names an hour has a *show hour*
//! button, which tints that column of the commitment grid.

use application::Game;
use bevy_egui::egui;
use domain::region::PlantKind;
use domain::session::{CapacityShortfallWarning, MinimumGenerationWarning, PlantView};
use domain::time::TICKS_PER_HOUR;
use domain::units::MegawattHours;

use super::action::{ButtonModel, ToggleModel, UiAction, button, toggle_button};
use super::plant_list::is_commitable;
use super::theme::{TextTone, heading_color};
use super::ui_state::UiState;
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
    Hvdc,
    InactiveIsland,
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
    /// Every warning, in order. The strip shows the first; the rest are listed on request, so a
    /// long list never takes rows from the grid.
    pub rows: Vec<WarningRow>,
    /// Shown when there is nothing to warn about.
    pub none: Option<String>,
    /// `+3 more`, or `Hide` once they are listed. Only when there is more than one warning.
    pub more: Option<ToggleModel>,
    /// Whether the warnings past the first are listed.
    pub expanded: bool,
}

/// `18:00`: the start of `hour`, as a warning names it.
pub(super) fn time_of(hour: u8) -> String {
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
    expanded: bool,
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
    model_for_rows(rows, expanded, l10n)
}

fn model_for_rows(rows: Vec<WarningRow>, expanded: bool, l10n: &Localizer) -> WarningsModel {
    let more = (rows.len() > 1).then(|| ToggleModel {
        button: ButtonModel::new(
            if expanded {
                l10n.text("warnings-less")
            } else {
                l10n.args(
                    "warnings-more",
                    &[("count", (rows.len() as u32 - 1).into())],
                )
            },
            UiAction::ToggleWarnings,
        ),
        on: expanded,
    });
    WarningsModel {
        heading: l10n.text("warnings-title"),
        none: rows.is_empty().then(|| l10n.text("warnings-none")),
        expanded: expanded && more.is_some(),
        more,
        rows,
    }
}

/// The names of the hydro plants among `views` that start the day with no water, in id order.
/// Water the plant has is not a warning, however little.
pub fn dry_hydro_plants(views: &[PlantView], names: &Names) -> Vec<String> {
    views
        .iter()
        .filter(|plant| plant.kind == PlantKind::Hydro && is_commitable(plant))
        .filter(|plant| {
            plant
                .hydro_day_budget
                .is_some_and(|budget| budget <= MegawattHours::ZERO)
        })
        .map(|plant| names.plant(plant.id))
        .collect()
}

/// The warnings for the game as planned now.
pub fn warnings_model(game: &Game, l10n: &Localizer, ui: &UiState) -> WarningsModel {
    warnings_for_session(game.session(), l10n, ui)
}

pub(super) fn warnings_for_session(
    session: &domain::session::GameSession,
    l10n: &Localizer,
    ui: &UiState,
) -> WarningsModel {
    if !session.power().island_nodes().is_empty() {
        let mut rows = Vec::new();
        let names = Names::new(l10n, session);
        for id in session
            .power()
            .island_nodes()
            .keys()
            .filter(|id| ui.island.is_none_or(|selected| selected == **id))
        {
            if !session.power().island_active(*id) {
                rows.push(WarningRow {
                    kind: WarningKind::InactiveIsland,
                    text: l10n.args(
                        "island-inactive-warning",
                        &[("island", names.island(session, *id).into())],
                    ),
                    show_hour: Some(ButtonModel::new(
                        l10n.text("forecast-inspect-island"),
                        UiAction::InspectForecast {
                            island: *id,
                            hour: 0,
                        },
                    )),
                });
                continue;
            }
            let Some(local) = session.island_session(*id) else {
                continue;
            };
            let dry = dry_hydro_plants(&local.plant_views(), &names);
            let model = warnings_for(
                local.capacity_shortfall_check(),
                local.minimum_generation_check(),
                &dry,
                false,
                l10n,
            );
            for mut row in model.rows {
                row.text = l10n.args(
                    "warning-island",
                    &[
                        ("island", names.island(session, *id).into()),
                        ("warning", row.text.into()),
                    ],
                );
                if let Some(show) = &mut row.show_hour
                    && let UiAction::ShowHour(Some(hour)) = show.action
                {
                    show.action = UiAction::InspectForecast { island: *id, hour };
                    show.label = l10n.text("forecast-inspect-island");
                }
                rows.push(row);
            }
        }
        rows.sort_by_key(|r| match r.kind {
            WarningKind::Shortfall => 0,
            WarningKind::MinimumGeneration => 1,
            WarningKind::DryHydro => 2,
            WarningKind::Hvdc => 3,
            WarningKind::InactiveIsland => 3,
        });
        for link in session.power().network().links() {
            let readiness = session
                .power()
                .link_planning_forecast(link.id)
                .expect("existing link");
            if ui.island.is_some_and(|id| {
                readiness.from_island != Some(id) && readiness.to_island != Some(id)
            }) {
                continue;
            }
            for text in super::links::link_planning_issues(session, link.id, l10n) {
                rows.push(WarningRow {
                    kind: WarningKind::Hvdc,
                    text,
                    show_hour: Some(ButtonModel::new(
                        l10n.text("hvdc-title"),
                        UiAction::ShowLinks(true),
                    )),
                });
            }
        }
        return model_for_rows(rows, ui.warnings_open, l10n);
    }
    let names = Names::new(l10n, session);
    let views = session.plant_views();
    let dry_hydro = dry_hydro_plants(&views, &names);
    warnings_for(
        session.capacity_shortfall_check(),
        session.minimum_generation_check(),
        &dry_hydro,
        ui.warnings_open,
        l10n,
    )
}

/// The strip: the heading and the first warning on one line, with a button for the rest, which
/// are listed in a popup above it rather than in the panel.
pub fn draw_warnings(ui: &mut egui::Ui, model: &WarningsModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let mut more_at = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(&model.heading)
                .strong()
                .color(heading_color()),
        );
        if let Some(none) = &model.none {
            ui.colored_label(TextTone::Positive.color(), none);
        }
        if let Some(first) = model.rows.first() {
            draw_row(ui, first, &mut actions);
        }
        if let Some(more) = &model.more {
            more_at = Some(toggle_button(ui, more, &mut actions).rect);
        }
    });
    if let (true, Some(at)) = (model.expanded, more_at) {
        egui::Area::new(egui::Id::new("warnings_more"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(at.left_top())
            .show(ui.ctx(), |ui| {
                ui.set_max_width((ui.ctx().content_rect().width() - 32.0).max(200.0));
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("planning_warning_details")
                        .max_height(240.0)
                        .show(ui, |ui| {
                            for row in &model.rows[1..] {
                                ui.horizontal_wrapped(|ui| draw_row(ui, row, &mut actions));
                            }
                        });
                });
            });
    }
    actions
}

fn draw_row(ui: &mut egui::Ui, row: &WarningRow, actions: &mut Vec<UiAction>) {
    let tone = match row.kind {
        WarningKind::Shortfall => TextTone::Negative,
        WarningKind::MinimumGeneration
        | WarningKind::DryHydro
        | WarningKind::Hvdc
        | WarningKind::InactiveIsland => TextTone::Caution,
    };
    ui.colored_label(tone.color(), &row.text);
    if let Some(show) = &row.show_hour {
        button(ui, show, actions);
    }
}

#[cfg(test)]
mod tests {
    use application::{Command, InMemoryCommandLog};
    use domain::region::PlantKind;
    use domain::units::Megawatts;

    use super::*;
    use crate::hud::test_support::{
        eastern_game, edited_western_game, empty_start, flat_load, shipped_game, shipped_l10n,
    };

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
    fn multi_island_warnings_identify_the_starved_city_and_the_surplus_island() {
        let l10n = shipped_l10n();
        let mut game = crate::hud::test_support::sgcc_planning_with_small_remote_source();
        for plant in game
            .session()
            .plant_views()
            .into_iter()
            .filter(|p| p.kind.is_dispatchable())
        {
            game.execute(Command::CommitPlant {
                plant: plant.id,
                hours: [true; 24],
            })
            .unwrap();
            game.execute(Command::SetAutoFollow {
                plant: plant.id,
                on: false,
            })
            .unwrap();
            let mw = game.session().region().spec(plant.kind).capacity;
            game.execute(Command::SetDispatchTarget {
                plant: plant.id,
                mw,
            })
            .unwrap();
        }
        let model = warnings_model(&game, &l10n, &UiState::default());
        assert!(
            model
                .rows
                .iter()
                .any(|row| row.kind == WarningKind::Shortfall && row.text.contains("Shanghai")),
            "{:?}",
            model.rows
        );
        assert!(
            model
                .rows
                .iter()
                .any(|row| row.kind == WarningKind::MinimumGeneration
                    && row.text.contains("Beijing")),
            "{:?}",
            model.rows
        );
        assert!(
            model
                .rows
                .iter()
                .any(|row| row.text.contains("hourly transfers are zero")),
            "{:?}",
            model.rows
        );
        assert!(l10n.problems().is_empty());
    }

    #[test]
    fn warnings_list_shortfall_then_min_gen_then_hydro() {
        let l10n = shipped_l10n();

        let model = warnings_for(
            Some(shortfall(18)),
            Some(min_gen(3)),
            &["Hydro plant 5".to_owned(), "Hydro plant 9".to_owned()],
            false,
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
            false,
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

        let model = warnings_for(None, None, &[], false, &l10n);

        assert!(model.rows.is_empty());
        assert_eq!(
            model.none.as_deref(),
            Some("No warnings: the commitments cover the forecast.")
        );
    }

    #[test]
    fn an_uncommitted_fleet_warns_of_a_shortfall_at_the_domains_hour() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        let session = game.session();
        let worst = session
            .capacity_shortfall_check()
            .expect("nothing is committed, so demand goes uncovered");

        let model = warnings_model(&game, &l10n, &UiState::default());

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

        let model = warnings_model(&game, &l10n, &UiState::default());

        assert!(
            model
                .rows
                .iter()
                .all(|row| row.kind != WarningKind::DryHydro),
            "{:?}",
            model.rows
        );
    }

    #[test]
    fn only_the_first_warning_is_in_the_strip_and_the_rest_are_behind_a_button() {
        let l10n = shipped_l10n();
        let dry = ["Hydro plant 5".to_owned(), "Hydro plant 9".to_owned()];

        let closed = warnings_for(Some(shortfall(18)), Some(min_gen(3)), &dry, false, &l10n);
        let more = closed.more.as_ref().expect("four warnings");
        assert_eq!(closed.rows.len(), 4, "all are kept for the popup");
        assert_eq!(more.button.label, "+3 more");
        assert_eq!(more.button.action, UiAction::ToggleWarnings);
        assert!(!more.on && !closed.expanded);

        let open = warnings_for(Some(shortfall(18)), Some(min_gen(3)), &dry, true, &l10n);
        assert_eq!(open.more.as_ref().unwrap().button.label, "Hide");
        assert!(open.expanded);

        // One warning, or none, needs no button, whatever was asked for.
        let one = warnings_for(Some(shortfall(18)), None, &[], true, &l10n);
        assert!(one.more.is_none() && !one.expanded);
        assert!(warnings_for(None, None, &[], true, &l10n).more.is_none());
        assert_eq!(l10n.problems(), vec![]);
    }

    /// The shipped regions' rivers always run (a region's monthly flow must be positive), so no
    /// start fleet is dry. The rule is tested on real views with their budgets set.
    #[test]
    fn only_a_hydro_plant_with_no_water_for_the_day_is_dry() {
        let l10n = shipped_l10n();
        // This presenter case needs three distinct water budgets, independently of
        // how many hydro units the current starting fleet contains.
        let game = crate::hud::test_support::edited_western_game(|text| {
            text.replace(
            "fleet: [",
            "fleet: [\n            (Hydro, -119.63, 47.99),\n            (Hydro, -120.69, 45.72),",
        )
        });
        let names = Names::new(&l10n, game.session());
        let mut views = game.session().plant_views();
        let hydro: Vec<usize> = (0..views.len())
            .filter(|i| views[*i].kind == PlantKind::Hydro)
            .collect();
        assert_eq!(hydro.len(), 3);
        assert_eq!(dry_hydro_plants(&views, &names), Vec::<String>::new());

        // The first has none, the second a trickle, the third plenty.
        views[hydro[0]].hydro_day_budget = Some(MegawattHours::ZERO);
        views[hydro[1]].hydro_day_budget = Some(MegawattHours::new(0.5));
        assert_eq!(
            dry_hydro_plants(&views, &names),
            [names.plant(views[hydro[0]].id)]
        );

        // Every hydro plant dry, and the dry ones come in id order.
        for i in &hydro {
            views[*i].hydro_day_budget = Some(MegawattHours::ZERO);
        }
        let all: Vec<String> = hydro.iter().map(|i| names.plant(views[*i].id)).collect();
        assert_eq!(dry_hydro_plants(&views, &names), all);

        // A plant that is not hydro is never dry.
        let peaker = views
            .iter_mut()
            .find(|p| p.kind == PlantKind::Peaker)
            .unwrap();
        peaker.hydro_day_budget = Some(MegawattHours::ZERO);
        assert_eq!(dry_hydro_plants(&views, &names).len(), 3);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn a_committed_minimum_above_a_tiny_load_is_warned_about_at_the_domains_hour() {
        let l10n = shipped_l10n();
        let mut game = edited_western_game(|text| flat_load(text, 5.0));
        let plants: Vec<_> = game
            .session()
            .plant_views()
            .into_iter()
            .filter(|plant| plant.kind == PlantKind::Ccgt)
            .map(|plant| plant.id)
            .collect();
        for plant in plants {
            game.execute(Command::StartPlant { plant }).unwrap();
        }
        let worst = game
            .session()
            .minimum_generation_check()
            .expect("combined-cycle minimums exceed a 5 MW load");

        let model = warnings_model(&game, &l10n, &UiState::default());

        let row = model
            .rows
            .iter()
            .find(|row| row.kind == WarningKind::MinimumGeneration)
            .expect("a minimum generation warning");
        assert_eq!(
            row.text,
            l10n.args(
                "warning-min-gen",
                &[
                    ("time", format::clock(u32::from(worst.hour) * 60).into()),
                    ("amount", format::megawatts(worst.excess).into()),
                ]
            )
        );
        assert_eq!(
            row.show_hour.as_ref().unwrap().action,
            UiAction::ShowHour(Some(worst.hour))
        );
        // No shortfall on a load that small, so this warning leads.
        assert_eq!(model.rows[0].kind, WarningKind::MinimumGeneration);
        assert_eq!(l10n.problems(), vec![]);
    }
}
