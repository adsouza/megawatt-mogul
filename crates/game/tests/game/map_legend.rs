//! The map legend, drawn by the real egui pass. By default it opens in the first corner of the map
//! that hides no city or plant, and is collapsed if none is clear, so every city stays clickable.
//! It is opened and closed from a toggle in the layer-button row, and the player's choice lasts the
//! whole game. When it is open it stays inside the drawn map, even with every panel dragged to its
//! maximum, and a click on it selects nothing.

use application::Command;
use bevy::prelude::*;
use bevy_egui::egui;
use domain::session::DayPhase;
use game::hud::{
    LegendChoice, MapLayout, Selection, UiAction, hit_test, legend_placement, legend_rect,
};
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

/// The centre of every mark a click can pick: each city and each plant, built or not.
fn pickable_marks(app: &App, layout: &MapLayout) -> Vec<(String, egui::Pos2)> {
    let session = session(app);
    let cities = session
        .city_views()
        .into_iter()
        .map(|city| (format!("{:?}", city.id), city.cell));
    let plants = session
        .plant_views()
        .into_iter()
        .map(|plant| (format!("{:?}", plant.id), plant.cell));
    cities
        .chain(plants)
        .map(|(name, cell)| (name, layout.cell_center(cell)))
        .collect()
}

/// On a settled screen of `size` in the legend's default state: if the legend is open, no city
/// or plant centre lies inside it; every city is picked at its centre, nothing is drawn over that
/// point, and a real click there selects it. Whether the legend opened.
fn every_city_is_clickable(region: &str, phase: DayPhase, size: (f32, f32)) -> bool {
    let logs = TempDir::new().unwrap();
    let mut app = app_in(&logs, region, phase);
    let pass = EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, size.0, size.1);
    let what = format!("{region} {phase:?} at {size:?}");
    assert_eq!(choice(&app), LegendChoice(None), "{what}");
    let map = widget(&widgets, "Map").rect;
    let layout = layout(&app, &widgets);
    let shown = legend(&widgets);
    let marks: Vec<egui::Pos2> = pickable_marks(&app, &layout)
        .into_iter()
        .map(|(_, centre)| centre)
        .collect();
    if let Some(shown) = shown {
        assert!(map.contains_rect(shown), "{what}: {shown:?} in {map:?}");
        // It sits in the corner the placement rule picks for its drawn size: the size the map
        // measured to place it is the size egui drew.
        let corner = legend_placement(map, shown.size(), &marks)
            .unwrap_or_else(|| panic!("{what}: no clear corner for {shown:?}"));
        let expected = legend_rect(map, shown.size(), corner);
        assert!(
            (shown.min - expected.min).length() <= ROUNDING,
            "{what}: {shown:?} is in the {corner:?} corner {expected:?}"
        );
        for (mark, centre) in pickable_marks(&app, &layout) {
            assert!(
                !shown.contains(centre),
                "{what}: the legend {shown:?} hides {mark} at {centre:?}"
            );
        }
    }
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
    shown.is_some()
}

/// Every region and phase at `size`: how many opened the legend by default.
fn opened_by_default(size: (f32, f32)) -> usize {
    [EASTERN, WESTERN]
        .into_iter()
        .flat_map(|region| {
            [DayPhase::Planning, DayPhase::Operating]
                .into_iter()
                .map(move |phase| every_city_is_clickable(region, phase, size))
        })
        .filter(|opened| *opened)
        .count()
}

#[test]
fn at_the_smallest_window_the_default_legend_hides_no_mark() {
    opened_by_default(SMALL);
}

/// The key shows at common window sizes: in a corner that hides nothing.
#[test]
fn at_1280_by_720_the_legend_opens_by_default_and_hides_no_mark() {
    // Only Western Planning keeps it closed: the West's narrow terrain puts a city in every corner.
    assert_eq!(opened_by_default(MEDIUM), 3, "1280×720 cases that open it");
}

#[test]
fn on_a_large_map_the_default_legend_hides_no_mark() {
    assert_eq!(opened_by_default(LARGE), 4, "2560×1440 cases that open it");
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

/// Clicks *Legend* until the player has opened the legend: once, or twice when it was open by
/// default (the first click closes it). The widgets drawn after.
fn open_by_player(app: &mut App, pass: &EguiPass, size: (f32, f32)) -> Vec<Widget> {
    let mut widgets = click_button(app, pass, size, TOGGLE);
    if choice(app) == LegendChoice(Some(false)) {
        widgets = click_button(app, pass, size, TOGGLE);
    }
    assert_eq!(choice(app), LegendChoice(Some(true)));
    widgets
}

/// Clicks the one button labelled `label` on a settled screen, then settles.
fn click_button(app: &mut App, pass: &EguiPass, size: (f32, f32), label: &str) -> Vec<Widget> {
    let widgets = pass.settled(app, size.0, size.1);
    let at = button(&widgets, label).rect.center();
    pass.click_at(app, size.0, size.1, at);
    pass.settled(app, size.0, size.1)
}

/// With every panel at its maximum at 1024×640 the drawn map is about 304×155. The legend, by
/// default or opened by the player, fits inside it by dropping to fewer columns; a click on it
/// selects nothing, and the toggle closes it again.
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
            assert_eq!(choice(&app), LegendChoice(None), "{what}");
            if let Some(shown) = legend(&widgets) {
                assert!(
                    map.contains_rect(shown),
                    "{what}: by default {shown:?} in {map:?}"
                );
            }

            // Opened by the player: inside the drawn map, which doesn't move.
            let widgets = open_by_player(&mut app, &pass, SMALL);
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

/// Opened by the player on a large map, the legend sits inside the map, and the toggle closes it.
#[test]
fn on_a_large_map_the_player_opens_and_closes_the_legend() {
    for region in [EASTERN, WESTERN] {
        for phase in [DayPhase::Planning, DayPhase::Operating] {
            let logs = TempDir::new().unwrap();
            let mut app = app_in(&logs, region, phase);
            let pass = EguiPass::attach(&mut app);
            let what = format!("{region} {phase:?}");
            let widgets = open_by_player(&mut app, &pass, LARGE);
            let map = widget(&widgets, "Map").rect;
            let shown = legend(&widgets).unwrap_or_else(|| panic!("{what}: open"));
            assert!(map.contains_rect(shown), "{what}: {shown:?} in {map:?}");

            let widgets = click_button(&mut app, &pass, LARGE, TOGGLE);
            assert_eq!(choice(&app), LegendChoice(Some(false)), "{what}");
            assert_eq!(legend(&widgets), None, "{what}");
        }
    }
}

/// The player's choice lasts through Operating and Review, where the map is hidden, then
/// reopens the legend in the next month's planning. (A new game starts undecided: `tests/game/hud.rs`.)
#[test]
fn the_players_legend_choice_lasts_through_the_phases_of_a_game() {
    let logs = TempDir::new().unwrap();
    let mut app = app_in(&logs, EASTERN, DayPhase::Planning);
    let pass = EguiPass::attach(&mut app);
    let widgets = open_by_player(&mut app, &pass, SMALL);
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
        if phase == DayPhase::Review {
            assert_eq!(legend(&widgets), None, "Review hides the map legend");
            for label in ["Map", TOGGLE] {
                assert!(
                    !widgets.iter().any(|w| w.text == label),
                    "Review hides {label}"
                );
            }
        } else {
            assert!(legend(&widgets).is_some(), "{phase:?}: still open");
        }
    }
}

/// The layer row shows which layer is on as a selected button, not a disabled one: it looks unlike
/// the other layers' buttons, and choosing another layer moves that look. Clicking the layer that
/// is on changes nothing.
#[test]
fn the_active_map_layer_is_shown_as_selected() {
    use crate::contrast::button_look;
    use game::hud::MapLayer;
    const LAYERS: [&str; 6] = [
        "Terrain",
        "Sunshine",
        "Cooling water",
        "Hydro potential",
        "Wind potential",
        "Geothermal potential",
    ];
    let logs = TempDir::new().unwrap();
    let mut app = app_in(&logs, EASTERN, DayPhase::Planning);
    let pass = EguiPass::attach(&mut app);
    let looks = |app: &mut App| {
        let widgets = pass.settled(app, MEDIUM.0, MEDIUM.1);
        let output = pass.paint_with(app, MEDIUM.0, MEDIUM.1, vec![]);
        let painted = output.shapes.clone();
        output.drop_without_applying_deltas();
        LAYERS.map(|label| {
            let layer = button(&widgets, label);
            assert!(!layer.disabled, "{label} can be clicked");
            (layer.selected, button_look(&painted, layer.rect))
        })
    };
    let shows_on = |looks: &[(bool, crate::contrast::ButtonLook); 6], on: usize| {
        for (index, (selected, look)) in looks.iter().enumerate() {
            assert_eq!(*selected, index == on, "{}", LAYERS[index]);
            if index != on {
                assert_ne!(
                    *look, looks[on].1,
                    "{} looks like the layer on",
                    LAYERS[index]
                );
            }
        }
    };

    assert_eq!(*app.world().resource::<MapLayer>(), MapLayer::Terrain);
    shows_on(&looks(&mut app), 0);

    for (label, layer, index) in [
        ("Sunshine", MapLayer::Irradiance, 1),
        ("Wind potential", MapLayer::WindPotential, 4),
        ("Geothermal potential", MapLayer::GeothermalPotential, 5),
    ] {
        for _ in 0..2 {
            click_button(&mut app, &pass, MEDIUM, label);
            assert_eq!(*app.world().resource::<MapLayer>(), layer);
            shows_on(&looks(&mut app), index);
        }
    }
}
