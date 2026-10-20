//! The map legend, drawn by the real egui pass. It is collapsed by default on a map too small to
//! letter its plants, so at the smallest window it hides no city. It is opened and closed from a
//! toggle in the layer-button row, and the player's choice lasts the whole game. When it is open
//! it stays inside the drawn map, even with every panel dragged to its maximum, and a click on it
//! selects nothing.

use application::Command;
use bevy::prelude::*;
use bevy_egui::egui;
use domain::session::DayPhase;
use game::hud::{LegendChoice, MapLayout, Selection, UiAction, hit_test};
use game::sim::Session;
use tempfile::TempDir;

use crate::hud_smoke::{EASTERN, WESTERN, dispatch_in};
use crate::support::egui_harness::{Widget, button, widget};
use crate::support::egui_pass::EguiPass;
use crate::support::{hud_app, request, update_n};

const SMALL: (f32, f32) = (1024.0, 640.0);
const MEDIUM: (f32, f32) = (1280.0, 720.0);
const LARGE: (f32, f32) = (2560.0, 1440.0);
const LEGEND: &str = "Map legend";
const TOGGLE: &str = "Legend";
/// Pixel rounding: egui snaps widgets to whole points.
const ROUNDING: f32 = 1.0;

fn session(app: &App) -> &domain::session::GameSession {
    app.world().resource::<Session>().0.session()
}

fn selection(app: &App) -> Selection {
    *app.world().resource::<Selection>()
}

fn choice(app: &App) -> LegendChoice {
    *app.world().resource::<LegendChoice>()
}

/// A new game in `region` on seed 7, in `phase`: Planning, or the day just begun.
fn app_in(logs: &TempDir, region: &str, phase: DayPhase) -> App {
    let mut app = hud_app(logs.path());
    request(&mut app, region);
    if phase == DayPhase::Operating {
        dispatch_in(&mut app, vec![UiAction::Command(Command::BeginOperating)]);
        update_n(&mut app, 2);
    }
    assert_eq!(session(&app).phase(), phase, "{region}");
    app
}

/// The legend's rect, if the pass drew it.
fn legend(widgets: &[Widget]) -> Option<egui::Rect> {
    widgets.iter().find(|w| w.text == LEGEND).map(|w| w.rect)
}

fn layout(app: &App, widgets: &[Widget]) -> MapLayout {
    let terrain = session(app).region().terrain();
    MapLayout::fit(
        widget(widgets, "Map").rect,
        terrain.width(),
        terrain.height(),
    )
}

/// Every area egui drew over the panels in the last pass, with its rect: the legend when it
/// shows, toasts, windows and modals. The panels themselves are on the background layer.
fn overlays(pass: &EguiPass) -> Vec<(egui::LayerId, egui::Rect)> {
    pass.ctx().memory(|memory| {
        memory
            .areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|layer| layer.order != egui::Order::Background)
            .filter_map(|layer| Some((layer, memory.area_rect(layer.id)?)))
            .collect()
    })
}

/// On a settled screen of `size`, every city is picked at its centre, nothing is drawn over that
/// point, and a real click there selects it. The legend shows only if `legend_shown`.
fn every_city_is_clickable(region: &str, phase: DayPhase, size: (f32, f32), legend_shown: bool) {
    let logs = TempDir::new().unwrap();
    let mut app = app_in(&logs, region, phase);
    let pass = EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, size.0, size.1);
    let what = format!("{region} {phase:?} at {size:?}");
    assert_eq!(legend(&widgets).is_some(), legend_shown, "{what}");
    let map = widget(&widgets, "Map").rect;
    let layout = layout(&app, &widgets);
    let cities = session(&app).city_views();
    assert!(cities.len() >= 10, "{what}: {} cities", cities.len());

    for city in cities {
        let centre = layout.cell_center(city.cell);
        let what = format!("{what}, {:?} at {:?}", city.id, city.cell);
        assert_eq!(
            hit_test(&layout, centre, &app.world().resource::<Session>().0),
            Selection::City(city.id),
            "{what}"
        );
        for (layer, rect) in overlays(&pass) {
            assert!(
                !rect.contains(centre),
                "{what}: {layer:?} {rect:?} covers it"
            );
        }

        pass.click_at(&mut app, size.0, size.1, centre);
        let widgets = pass.settled(&mut app, size.0, size.1);

        assert_eq!(selection(&app), Selection::City(city.id), "{what}");
        assert_eq!(widget(&widgets, "Map").rect, map, "{what}: the map stays");
    }
}

#[test]
fn at_the_smallest_window_the_legend_is_collapsed_and_every_city_can_be_clicked() {
    for region in [EASTERN, WESTERN] {
        for phase in [DayPhase::Planning, DayPhase::Operating] {
            every_city_is_clickable(region, phase, SMALL, false);
        }
    }
}

/// 1280×720 is still too small to letter the plants (Eastern cells are about 6.7 pt, under the
/// 13.3 pt the 9 pt letters need), so the legend is collapsed there too.
#[test]
fn at_1280_by_720_the_legend_is_collapsed_and_every_city_can_be_clicked() {
    for region in [EASTERN, WESTERN] {
        for phase in [DayPhase::Planning, DayPhase::Operating] {
            every_city_is_clickable(region, phase, MEDIUM, false);
        }
    }
}

#[test]
fn on_a_map_big_enough_for_plant_letters_the_legend_opens_and_hides_no_city() {
    for region in [EASTERN, WESTERN] {
        for phase in [DayPhase::Planning, DayPhase::Operating] {
            every_city_is_clickable(region, phase, LARGE, true);
        }
    }
}

/// Drags the inner edge of the outer panel `id` as far as it goes.
fn drag_to_maximum(app: &mut App, pass: &EguiPass, id: &str, size: (f32, f32)) {
    let rect = egui::PanelState::load(pass.ctx(), egui::Id::new(id))
        .unwrap_or_else(|| panic!("{id} is drawn"))
        .outer_rect;
    let (from, to) = match id {
        "planning" | "operating" => {
            let from = egui::pos2(rect.center().x, rect.top());
            (from, egui::pos2(from.x, 1.0))
        }
        "inspector" => {
            let from = egui::pos2(rect.right(), rect.center().y);
            (from, egui::pos2(size.0 - 1.0, from.y))
        }
        _ => {
            let from = egui::pos2(rect.left(), rect.center().y);
            (from, egui::pos2(1.0, from.y))
        }
    };
    pass.drag(app, size.0, size.1, from, to);
    pass.settled(app, size.0, size.1);
}

/// Clicks the one button labelled `label` on a settled screen, then settles.
fn click_button(app: &mut App, pass: &EguiPass, size: (f32, f32), label: &str) -> Vec<Widget> {
    let widgets = pass.settled(app, size.0, size.1);
    let at = button(&widgets, label).rect.center();
    pass.click_at(app, size.0, size.1, at);
    pass.settled(app, size.0, size.1)
}

/// With every panel at its maximum at 1024×640 the drawn map is about 304×155. The legend the
/// player opens there fits inside it, by dropping to fewer columns; a click on it selects
/// nothing, and the toggle closes it again.
#[test]
fn the_legend_the_player_opens_stays_inside_the_map_with_every_panel_at_its_maximum() {
    let panels = [
        (
            DayPhase::Planning,
            ["inspector", "build", "planning"].as_slice(),
        ),
        (
            DayPhase::Operating,
            ["operating_side", "operating"].as_slice(),
        ),
    ];
    for region in [EASTERN, WESTERN] {
        for (phase, ids) in panels {
            let logs = TempDir::new().unwrap();
            let mut app = app_in(&logs, region, phase);
            let pass = EguiPass::attach(&mut app);
            let what = format!("{region} {phase:?}");
            let opened = pass.settled(&mut app, SMALL.0, SMALL.1);
            let open_map = widget(&opened, "Map").rect;
            for id in ids {
                drag_to_maximum(&mut app, &pass, id, SMALL);
            }
            let widgets = pass.settled(&mut app, SMALL.0, SMALL.1);
            let map = widget(&widgets, "Map").rect;
            assert!(
                map.width() < open_map.width() && map.height() < open_map.height(),
                "{what}: the panels grew, the map is {map:?}"
            );
            assert_eq!(legend(&widgets), None, "{what}: collapsed by default");
            assert_eq!(choice(&app), LegendChoice(None), "{what}");

            // Opened by the player: inside the drawn map, which doesn't move.
            let widgets = click_button(&mut app, &pass, SMALL, TOGGLE);
            assert_eq!(choice(&app), LegendChoice(Some(true)), "{what}");
            let shown = legend(&widgets).unwrap_or_else(|| panic!("{what}: the legend shows"));
            assert_eq!(widget(&widgets, "Map").rect, map, "{what}: the map stays");
            assert!(
                map.expand(ROUNDING).contains_rect(shown),
                "{what}: the legend {shown:?} is inside the map {map:?}"
            );

            // A click on it selects nothing.
            assert_eq!(selection(&app), Selection::None, "{what}");
            pass.click_at(&mut app, SMALL.0, SMALL.1, shown.center());
            pass.settled(&mut app, SMALL.0, SMALL.1);
            assert_eq!(selection(&app), Selection::None, "{what}: on the legend");

            // And the toggle closes it.
            let widgets = click_button(&mut app, &pass, SMALL, TOGGLE);
            assert_eq!(choice(&app), LegendChoice(Some(false)), "{what}");
            assert_eq!(legend(&widgets), None, "{what}: closed again");
        }
    }
}

/// On the largest maps the legend opens by default inside the map, and the player can close it.
#[test]
fn the_legend_open_by_default_fits_the_map_and_the_player_can_close_it() {
    for region in [EASTERN, WESTERN] {
        for phase in [DayPhase::Planning, DayPhase::Operating] {
            let logs = TempDir::new().unwrap();
            let mut app = app_in(&logs, region, phase);
            let pass = EguiPass::attach(&mut app);
            let what = format!("{region} {phase:?}");
            let widgets = pass.settled(&mut app, LARGE.0, LARGE.1);
            let map = widget(&widgets, "Map").rect;
            let shown = legend(&widgets).unwrap_or_else(|| panic!("{what}: open by default"));
            assert!(map.contains_rect(shown), "{what}: {shown:?} in {map:?}");

            let widgets = click_button(&mut app, &pass, LARGE, TOGGLE);
            assert_eq!(choice(&app), LegendChoice(Some(false)), "{what}");
            assert_eq!(legend(&widgets), None, "{what}");
        }
    }
}

/// The player's choice is the game's, not the month's: it lasts through Operating, the Review
/// and the next month's planning. (A new game starts undecided: `tests/game/hud.rs`.)
#[test]
fn the_players_legend_choice_lasts_through_the_phases_of_a_game() {
    let logs = TempDir::new().unwrap();
    let mut app = app_in(&logs, EASTERN, DayPhase::Planning);
    let pass = EguiPass::attach(&mut app);
    let widgets = click_button(&mut app, &pass, SMALL, TOGGLE);
    assert!(legend(&widgets).is_some());

    for (command, phase) in [
        (Command::BeginOperating, DayPhase::Operating),
        (Command::AdvanceTicks { n: 1_440 }, DayPhase::Review),
        (Command::FinishReview, DayPhase::Planning),
    ] {
        dispatch_in(&mut app, vec![UiAction::Command(command)]);
        update_n(&mut app, 3);
        assert_eq!(session(&app).phase(), phase);
        let widgets = pass.settled(&mut app, SMALL.0, SMALL.1);
        assert_eq!(choice(&app), LegendChoice(Some(true)), "{phase:?}");
        assert!(legend(&widgets).is_some(), "{phase:?}: still open");
    }
}
