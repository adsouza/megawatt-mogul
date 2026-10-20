//! The city card (spec §16.8), drawn by the real egui pass at the smallest window: a click on a
//! city's marker shows its name and demand in Planning's inspector, and in Operating's
//! selected-object block, which keeps its height so the plant list doesn't move.

use bevy::prelude::*;
use bevy_egui::egui;
use domain::ids::CityId;
use domain::session::CityView;
use game::hud::{MapLayout, Selection, UiAction, hit_test};
use game::l10n::{Names, format};
use game::sim::Session;
use tempfile::TempDir;

use crate::hud_smoke::{EASTERN, WESTERN, dispatch_in};
use crate::operating_side::{HINT, label, region, side_panel};
use crate::support::egui_harness::{Widget, widget};
use crate::support::egui_pass::EguiPass;
use crate::support::{empty_start_in, hud_app, request, shipped_l10n, update_n};

const SMALL: (f32, f32) = (1024.0, 640.0);
/// Pixel rounding: egui snaps widgets to whole points, and a panel's edge can fall between them.
const ROUNDING: f32 = 1.0;

fn selection(app: &App) -> Selection {
    *app.world().resource::<Selection>()
}

fn city_name(app: &App, city: CityId) -> String {
    Names::new(
        &shipped_l10n(),
        app.world().resource::<Session>().0.session(),
    )
    .city(city)
}

/// Where the city's marker is drawn on the settled screen, if a click there picks the city: not
/// under a plant's square, nor under the legend when it floats over the map.
fn marker(app: &App, widgets: &[Widget], city: &CityView) -> Option<egui::Pos2> {
    let game = &app.world().resource::<Session>().0;
    let terrain = game.session().region().terrain();
    let layout = MapLayout::fit(
        widget(widgets, "Map").rect,
        terrain.width(),
        terrain.height(),
    );
    let legend = widgets.iter().find(|w| w.text == "Map legend");
    let at = layout.cell_center(city.cell);
    (hit_test(&layout, at, game) == Selection::City(city.id)
        && !legend.is_some_and(|legend| legend.rect.contains(at)))
    .then_some(at)
}

/// The first city with `connected` whose marker a click can reach, and where it is.
fn clickable_city(app: &App, widgets: &[Widget], connected: bool) -> (CityView, egui::Pos2) {
    app.world()
        .resource::<Session>()
        .0
        .session()
        .city_views()
        .into_iter()
        .filter(|city| city.connected == connected)
        .find_map(|city| marker(app, widgets, &city).map(|at| (city, at)))
        .unwrap_or_else(|| panic!("a clickable city with connected = {connected}"))
}

/// Clicks the city's marker on a settled screen, then settles: the widgets drawn after.
fn click_city(app: &mut App, pass: &EguiPass, at: egui::Pos2) -> Vec<Widget> {
    pass.click_at(app, SMALL.0, SMALL.1, at);
    pass.settled(app, SMALL.0, SMALL.1)
}

fn base_load(app: &App, city: CityId) -> String {
    let session = app.world().resource::<Session>().0.session();
    format::megawatts(session.city_detail(city).unwrap().base_load)
}

/// A label reading `text` inside `rect`.
fn label_in(widgets: &[Widget], text: &str, rect: egui::Rect) -> egui::Rect {
    widgets
        .iter()
        .find(|w| {
            w.text == text
                && w.role == egui::accesskit::Role::Label
                && rect.expand(ROUNDING).contains_rect(w.rect)
        })
        .unwrap_or_else(|| panic!("no label {text:?} inside {rect:?}"))
        .rect
}

#[test]
fn in_planning_a_clicked_city_shows_its_name_and_base_load_in_the_inspector() {
    for region_id in [EASTERN, WESTERN] {
        let logs = TempDir::new().unwrap();
        let mut app = hud_app(logs.path());
        request(&mut app, region_id);
        let pass = EguiPass::attach(&mut app);
        for connected in [true, false] {
            let widgets = pass.settled(&mut app, SMALL.0, SMALL.1);
            let (city, at) = clickable_city(&app, &widgets, connected);
            let what = format!("{region_id} {:?}", city.id);

            let widgets = click_city(&mut app, &pass, at);

            assert_eq!(selection(&app), Selection::City(city.id), "{what}");
            let inspector = egui::PanelState::load(pass.ctx(), egui::Id::new("inspector"))
                .expect("the inspector is drawn")
                .outer_rect;
            label_in(&widgets, &city_name(&app, city.id), inspector);
            label_in(&widgets, "Base load", inspector);
            label_in(&widgets, &base_load(&app, city.id), inspector);
            let status = if connected {
                "Connected"
            } else {
                "Not connected"
            };
            label_in(&widgets, status, inspector);
        }
    }
}

/// A day begun with nothing committed: it runs in a rotating outage, so a connected city's card
/// shows every figure it can, the share cut included.
fn operating_outage_app(logs: &TempDir, region_id: &str) -> App {
    let mut app = hud_app(logs.path());
    request(&mut app, region_id);
    empty_start_in(&mut app);
    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(application::Command::BeginOperating),
            UiAction::Command(application::Command::AdvanceTicks { n: 5 }),
        ],
    );
    update_n(&mut app, 3);
    let session = app.world().resource::<Session>().0.session();
    assert_eq!(session.clock().tick(), 5, "{region_id}");
    assert!(
        session.grid_snapshot().outage > domain::units::Ratio::ZERO,
        "{region_id}: the day is in an outage"
    );
    app
}

#[test]
fn in_operating_a_clicked_city_shows_its_card_in_the_details_block() {
    for region_id in [EASTERN, WESTERN] {
        let logs = TempDir::new().unwrap();
        let mut app = operating_outage_app(&logs, region_id);
        let pass = EguiPass::attach(&mut app);
        let widgets = pass.settled(&mut app, SMALL.0, SMALL.1);
        let details = region(&widgets, HINT);
        let list = region(&widgets, "Plants");

        // A connected city: its name and every figure, inside the block, which keeps its place.
        let (city, at) = clickable_city(&app, &widgets, true);
        let widgets = click_city(&mut app, &pass, at);
        let what = format!("{region_id} {:?}", city.id);
        assert_eq!(selection(&app), Selection::City(city.id), "{what}");
        let name = city_name(&app, city.id);
        assert_eq!(
            region(&widgets, &name),
            details,
            "{what}: the block keeps its rect"
        );
        assert_eq!(
            region(&widgets, "Plants"),
            list,
            "{what}: the list doesn't move"
        );
        for text in [
            name.as_str(),
            "Connected",
            "Base load",
            "Peak today",
            "Demand now",
            "Served now",
            "Cut",
        ] {
            let row = label(&widgets, text);
            assert!(
                details.contains_rect(row),
                "{what}: {text} {row:?} is outside the block {details:?}"
            );
        }

        // An unconnected city: what it would add, and the hint, inside the block too.
        let (city, at) = clickable_city(&app, &widgets, false);
        let widgets = click_city(&mut app, &pass, at);
        let what = format!("{region_id} {:?}", city.id);
        assert_eq!(selection(&app), Selection::City(city.id), "{what}");
        assert_eq!(
            region(&widgets, &city_name(&app, city.id)),
            details,
            "{what}"
        );
        assert_eq!(region(&widgets, "Plants"), list, "{what}");
        for text in [
            "Not connected",
            "Would add at peak",
            "Build a line to it from the build menu to connect it.",
        ] {
            let row = label(&widgets, text);
            assert!(
                details.contains_rect(row),
                "{what}: {text} {row:?} is outside the block {details:?}"
            );
        }
        assert!(side_panel(&pass).contains_rect(details), "{what}");

        // A plant clicked in the list takes the block back: the first row in view.
        let session = app.world().resource::<Session>().0.session();
        let l10n = shipped_l10n();
        let names = Names::new(&l10n, session);
        let (plant, row) = session
            .plant_views()
            .into_iter()
            .find_map(|plant| {
                let name = names.plant(plant.id);
                widgets
                    .iter()
                    .find(|w| {
                        w.text == name
                            && w.role != egui::accesskit::Role::Label
                            && w.role != egui::accesskit::Role::Unknown
                            && list.contains_rect(w.rect)
                    })
                    .map(|w| (plant.id, w.rect))
            })
            .unwrap_or_else(|| panic!("{region_id}: a plant row in view"));
        let plant_name = names.plant(plant);
        pass.click_at(&mut app, SMALL.0, SMALL.1, row.center());
        let widgets = pass.settled(&mut app, SMALL.0, SMALL.1);
        assert_eq!(selection(&app), Selection::Plant(plant), "{region_id}");
        assert_eq!(region(&widgets, &plant_name), details, "{region_id}");
        assert_eq!(region(&widgets, "Plants"), list, "{region_id}");
    }
}

/// While a line to an unconnected city is being built, its card says when the city connects
/// instead of asking for a line, in Planning's inspector and in Operating's details block, which
/// keeps its height for it.
#[test]
fn a_city_with_a_line_under_construction_says_when_it_connects() {
    for region_id in [EASTERN, WESTERN] {
        let logs = TempDir::new().unwrap();
        let mut app = hud_app(logs.path());
        request(&mut app, region_id);
        let session = app.world().resource::<Session>().0.session();
        let nodes: Vec<_> = std::iter::once(session.substation())
            .chain(session.city_views().iter().map(|c| c.node))
            .collect();
        let (_, city, from, to) = session
            .city_views()
            .into_iter()
            .filter(|city| !city.connected)
            .flat_map(|city| nodes.iter().map(move |to| (city.clone(), *to)))
            .filter_map(|(city, to)| {
                let quote = session.line_quote(city.node, to).ok()?;
                Some((quote.months, city.id, city.node, to))
            })
            .min_by_key(|(months, city, _, _)| (*months, *city))
            .expect("an unconnected city can be reached");
        dispatch_in(
            &mut app,
            vec![UiAction::Command(application::Command::BuildLine {
                from,
                to,
            })],
        );
        update_n(&mut app, 1);
        let l10n = shipped_l10n();
        let game = &app.world().resource::<Session>().0;
        let hint = game::hud::city_card_model(game, &l10n, city)
            .and_then(|card| card.hint)
            .expect("an unconnected city's hint");
        assert!(
            hint.starts_with("A line to it is under construction: connected from "),
            "{region_id}: {hint}"
        );
        let view = game
            .session()
            .city_views()
            .into_iter()
            .find(|c| c.id == city)
            .unwrap();
        let pass = EguiPass::attach(&mut app);

        // Planning: in the inspector.
        let widgets = pass.settled(&mut app, SMALL.0, SMALL.1);
        let at = marker(&app, &widgets, &view).expect("the city can be clicked");
        let widgets = click_city(&mut app, &pass, at);
        assert_eq!(selection(&app), Selection::City(city), "{region_id}");
        let inspector = egui::PanelState::load(pass.ctx(), egui::Id::new("inspector"))
            .expect("the inspector is drawn")
            .outer_rect;
        label_in(&widgets, &hint, inspector);

        // Operating: in the details block, at its fixed height.
        dispatch_in(
            &mut app,
            vec![
                UiAction::Command(application::Command::BeginOperating),
                UiAction::Select(Selection::None),
            ],
        );
        update_n(&mut app, 2);
        let widgets = pass.settled(&mut app, SMALL.0, SMALL.1);
        let details = region(&widgets, HINT);
        let at = marker(&app, &widgets, &view).expect("the city can be clicked");
        let widgets = click_city(&mut app, &pass, at);
        assert_eq!(selection(&app), Selection::City(city), "{region_id}");
        assert_eq!(
            region(&widgets, &city_name(&app, city)),
            details,
            "{region_id}"
        );
        let row = label(&widgets, &hint);
        assert!(
            details.contains_rect(row),
            "{region_id}: the hint {row:?} is outside the block {details:?}"
        );
    }
}
