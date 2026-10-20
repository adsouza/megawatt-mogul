//! Construction progress (spec §16.9), drawn by the real egui pass at the smallest window: a plant
//! and a line ordered in Planning show on the Construction tab, whose label counts them, and a
//! click on the plant's faded marker shows its construction card, in Planning's inspector and in
//! Operating's selected-object block, which keeps its height so the plant list doesn't move.

use application::Command;
use bevy::prelude::*;
use bevy_egui::egui;
use domain::ids::{CellCoord, LineId, NodeId, PlantId};
use domain::region::PlantKind;
use game::hud::{
    ConstructionCardModel, ConstructionModel, MapLayout, PlanningTab, Selection, UiAction, UiState,
    construction_card_model, construction_model, hit_test,
};
use game::sim::Session;
use tempfile::TempDir;

use crate::hud_smoke::{EASTERN, WESTERN, dispatch_in};
use crate::operating_side::{HINT, region, side_panel};
use crate::support::egui_harness::{Widget, button, widget};
use crate::support::egui_pass::EguiPass;
use crate::support::{hud_app, request, shipped_l10n, update_n};

const SMALL: (f32, f32) = (1024.0, 640.0);
/// Pixel rounding: egui snaps widgets to whole points, and a panel's edge can fall between them.
const ROUNDING: f32 = 1.0;

fn session(app: &App) -> &domain::session::GameSession {
    app.world().resource::<Session>().0.session()
}

fn selection(app: &App) -> Selection {
    *app.world().resource::<Selection>()
}

/// The map's layout and the legend's rect on the settled screen: `Rect::NOTHING` when the legend
/// is collapsed.
fn map(app: &App, widgets: &[Widget]) -> (MapLayout, egui::Rect) {
    let terrain = session(app).region().terrain();
    let layout = MapLayout::fit(
        widget(widgets, "Map").rect,
        terrain.width(),
        terrain.height(),
    );
    let legend = widgets
        .iter()
        .find(|w| w.text == "Map legend")
        .map_or(egui::Rect::NOTHING, |w| w.rect);
    (layout, legend)
}

/// A cell a peaker can be built on whose centre a click reaches: nothing is drawn there yet, and
/// the legend doesn't cover it. Searched from the bottom left, away from the legend in the top
/// right, so it stays clickable in Operating's layout too.
fn clickable_site(app: &App, widgets: &[Widget]) -> CellCoord {
    let game = &app.world().resource::<Session>().0;
    let region = game.session().region();
    let (layout, legend) = map(app, widgets);
    let (w, h) = (region.terrain().width(), region.terrain().height());
    (0..h)
        .rev()
        .flat_map(|y| (0..w).map(move |x| CellCoord { x, y }))
        .find(|at| {
            let centre = layout.cell_center(*at);
            region.site_check(PlantKind::Peaker, *at).is_ok()
                && hit_test(&layout, centre, game) == Selection::Cell(*at)
                && !legend.expand(8.0).contains(centre)
        })
        .expect("a free site for a peaker")
}

/// The quickest line from a city the start leaves unconnected.
fn short_line(app: &App) -> (NodeId, NodeId) {
    let session = session(app);
    let nodes: Vec<NodeId> = std::iter::once(session.substation())
        .chain(session.city_views().iter().map(|c| c.node))
        .collect();
    let (_, from, to) = session
        .city_views()
        .iter()
        .filter(|city| !city.connected)
        .flat_map(|city| nodes.iter().map(move |to| (city.node, *to)))
        .filter_map(|(from, to)| {
            session
                .line_quote(from, to)
                .ok()
                .map(|quote| (quote.months, from, to))
        })
        .min()
        .expect("an unconnected city can be reached");
    (from, to)
}

/// Orders a peaker at a clickable site and the quickest line, through the real `dispatch` and the
/// frame that applies them.
fn order_both(app: &mut App, pass: &EguiPass) -> (PlantId, LineId) {
    let widgets = pass.settled(app, SMALL.0, SMALL.1);
    let cell = clickable_site(app, &widgets);
    let (from, to) = short_line(app);
    dispatch_in(
        app,
        vec![
            UiAction::Command(Command::BuildPlant {
                kind: PlantKind::Peaker,
                cell,
            }),
            UiAction::Command(Command::BuildLine { from, to }),
        ],
    );
    update_n(app, 1);
    let session = session(app);
    let plant = session
        .plant_views()
        .into_iter()
        .find(|p| p.cell == cell && p.kind == PlantKind::Peaker)
        .expect("the peaker was ordered")
        .id;
    let line = session
        .line_views()
        .into_iter()
        .find(|l| (l.from, l.to) == (from, to))
        .expect("the line was ordered")
        .id;
    (plant, line)
}

fn model(app: &App) -> ConstructionModel {
    construction_model(&app.world().resource::<Session>().0, &shipped_l10n())
}

fn card(app: &App, plant: PlantId) -> ConstructionCardModel {
    construction_card_model(&app.world().resource::<Session>().0, &shipped_l10n(), plant)
        .expect("the plant is under construction")
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

fn panel(pass: &EguiPass, id: &str) -> egui::Rect {
    egui::PanelState::load(pass.ctx(), egui::Id::new(id))
        .unwrap_or_else(|| panic!("the {id} panel is drawn"))
        .outer_rect
}

/// Clicks the plant's marker on the settled screen `widgets`, then settles.
fn click_plant(app: &mut App, pass: &EguiPass, widgets: &[Widget], plant: PlantId) -> Vec<Widget> {
    let (layout, legend) = map(app, widgets);
    let cell = session(app)
        .plant_views()
        .into_iter()
        .find(|p| p.id == plant)
        .unwrap()
        .cell;
    let at = layout.cell_center(cell);
    let game = &app.world().resource::<Session>().0;
    assert_eq!(hit_test(&layout, at, game), Selection::Plant(plant));
    assert!(!legend.contains(at), "the marker is clear of the legend");
    pass.click_at(app, SMALL.0, SMALL.1, at);
    pass.settled(app, SMALL.0, SMALL.1)
}

#[test]
fn in_planning_the_construction_tab_lists_both_projects_and_a_click_shows_the_card() {
    for region_id in [EASTERN, WESTERN] {
        let logs = TempDir::new().unwrap();
        let mut app = hud_app(logs.path());
        request(&mut app, region_id);
        let pass = EguiPass::attach(&mut app);

        // With nothing ordered the tab has no count.
        let widgets = pass.settled(&mut app, SMALL.0, SMALL.1);
        button(&widgets, "Construction");

        let (plant, line) = order_both(&mut app, &pass);
        let widgets = pass.settled(&mut app, SMALL.0, SMALL.1);
        let tab = button(&widgets, "Construction (2)").rect;
        pass.click_at(&mut app, SMALL.0, SMALL.1, tab.center());
        let widgets = pass.settled(&mut app, SMALL.0, SMALL.1);
        assert_eq!(
            app.world().resource::<UiState>().tab,
            PlanningTab::Construction,
            "{region_id}"
        );

        // Both rows, in full, inside the bottom panel, which keeps *Start the day* in view.
        let planning = panel(&pass, "planning");
        let model = model(&app);
        assert_eq!(model.rows.len(), 2, "{region_id}");
        for row in &model.rows {
            for text in [&row.name, &row.months, &row.in_service_text] {
                label_in(&widgets, text, planning);
            }
        }
        assert!(
            planning
                .expand(ROUNDING)
                .contains_rect(button(&widgets, "Start the day").rect),
            "{region_id}"
        );
        let projects: Vec<_> = model.rows.iter().map(|row| row.project).collect();
        assert_eq!(
            projects,
            [
                game::hud::Project::Line(line),
                game::hud::Project::Plant(plant)
            ],
            "{region_id}: the line is in service first"
        );

        // A click on the faded marker shows the card in the inspector.
        let widgets = click_plant(&mut app, &pass, &widgets, plant);
        assert_eq!(selection(&app), Selection::Plant(plant), "{region_id}");
        let inspector = panel(&pass, "inspector");
        let card = card(&app, plant);
        for text in [
            card.name.as_str(),
            card.status.as_str(),
            "Capacity",
            card.stats[0].value.as_str(),
            card.months.as_str(),
            card.in_service.as_str(),
        ] {
            label_in(&widgets, text, inspector);
        }
    }
}

#[test]
fn in_operating_a_clicked_plant_under_construction_shows_its_card_in_the_details_block() {
    for region_id in [EASTERN, WESTERN] {
        let logs = TempDir::new().unwrap();
        let mut app = hud_app(logs.path());
        request(&mut app, region_id);
        let pass = EguiPass::attach(&mut app);
        let (plant, _) = order_both(&mut app, &pass);
        dispatch_in(
            &mut app,
            vec![
                UiAction::Select(Selection::None),
                UiAction::Command(Command::BeginOperating),
            ],
        );
        update_n(&mut app, 2);
        assert_eq!(
            session(&app).phase(),
            domain::session::DayPhase::Operating,
            "{region_id}"
        );
        let widgets = pass.settled(&mut app, SMALL.0, SMALL.1);
        let details = region(&widgets, HINT);
        let list = region(&widgets, "Plants");

        let widgets = click_plant(&mut app, &pass, &widgets, plant);

        assert_eq!(selection(&app), Selection::Plant(plant), "{region_id}");
        let card = card(&app, plant);
        assert_eq!(region(&widgets, &card.name), details, "{region_id}");
        assert_eq!(
            region(&widgets, "Plants"),
            list,
            "{region_id}: the list doesn't move"
        );
        for text in [
            card.name.as_str(),
            card.status.as_str(),
            "Capacity",
            card.stats[0].value.as_str(),
            card.months.as_str(),
            card.in_service.as_str(),
        ] {
            label_in(&widgets, text, details);
        }
        assert!(side_panel(&pass).contains_rect(details), "{region_id}");
    }
}
