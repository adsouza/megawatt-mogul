//! The Operating side panel's layout, drawn by the real egui pass: the frequency gauge, then the
//! selected plant's details in a block of fixed height, then the plant list filling the rest.
//!
//! The details block keeps its height whatever is selected (a placeholder of the same height
//! when nothing is), so the list never jumps; the list takes all the height left, so a taller
//! window or a shorter bottom panel gives it the room.

use bevy::prelude::*;
use bevy_egui::egui;
use domain::region::PlantKind;
use domain::session::PlantView;
use game::hud::{
    Edge, MAP_MIN_SIZE, OPERATING_PANEL_HEIGHT, OperatingTab, Selection, UiAction,
    operating_side_min_height, panel_max,
};
use game::l10n::Names;
use game::sim::Session;
use tempfile::TempDir;

use crate::hud_smoke::{EASTERN, WESTERN, dispatch_in, operating_hour_app};
use crate::support::egui_harness::Widget;
use crate::support::egui_pass::EguiPass;
use crate::support::shipped_l10n;

const SMALL: (f32, f32) = (1024.0, 640.0);
const LARGE: (f32, f32) = (2560.0, 1440.0);
/// Pixel rounding: egui snaps widgets to whole points, and a panel's edge can fall between them.
const ROUNDING: f32 = 1.0;

/// The placeholder's text when no plant is selected.
pub(crate) const HINT: &str = "Select a plant in the list or on the map.";
/// The selected plant's figures that must show without scrolling, down to the last.
const FIGURES: [&str; 7] = [
    "Grid connection",
    "Output / capacity",
    "Target",
    "Ramp rate",
    "Marginal cost",
    "Energy today",
    "Closed months",
];

/// The named region (not a label or a button) labelled `text`: a block of the side panel.
pub(crate) fn region(widgets: &[Widget], text: &str) -> egui::Rect {
    let mut found = widgets
        .iter()
        .filter(|w| w.text == text && w.role == egui::accesskit::Role::Unknown);
    match (found.next(), found.next()) {
        (Some(only), None) => only.rect,
        other => panic!("one region {text:?}, found {other:?}"),
    }
}

pub(crate) fn label(widgets: &[Widget], text: &str) -> egui::Rect {
    widgets
        .iter()
        .find(|w| w.text == text && w.role == egui::accesskit::Role::Label)
        .unwrap_or_else(|| panic!("no label {text:?}"))
        .rect
}

/// The side panel's outer rect, as egui last laid it out.
pub(crate) fn side_panel(pass: &EguiPass) -> egui::Rect {
    egui::PanelState::load(pass.ctx(), egui::Id::new("operating_side"))
        .expect("the side panel is drawn")
        .outer_rect
}

/// The plant of `kind` that is online, an hour in.
fn online(app: &App, kind: PlantKind) -> PlantView {
    app.world()
        .resource::<Session>()
        .0
        .session()
        .plant_views()
        .into_iter()
        .find(|plant| {
            plant.kind == kind
                && matches!(plant.status, domain::generation::PlantStatus::Online { .. })
        })
        .unwrap_or_else(|| panic!("an online {kind:?} plant"))
}

fn name_of(app: &App, plant: &PlantView) -> String {
    Names::new(
        &shipped_l10n(),
        app.world().resource::<Session>().0.session(),
    )
    .plant(plant.id)
}

/// Every plant's name, as the list's rows show them.
fn row_names(app: &App) -> Vec<String> {
    let session = app.world().resource::<Session>().0.session();
    let l10n = shipped_l10n();
    let names = Names::new(&l10n, session);
    session
        .plant_views()
        .into_iter()
        .map(|plant| names.plant(plant.id))
        .collect()
}

/// The blocks of the side panel with `selection`, on a settled screen of `size`.
struct Blocks {
    details: egui::Rect,
    list: egui::Rect,
    panel: egui::Rect,
    widgets: Vec<Widget>,
}

fn blocks(app: &mut App, pass: &EguiPass, selection: Selection, size: (f32, f32)) -> Blocks {
    dispatch_in(app, vec![UiAction::Select(selection)]);
    let widgets = pass.settled(app, size.0, size.1);
    let details_label = match selection {
        Selection::Plant(plant) => {
            let session = app.world().resource::<Session>().0.session();
            let l10n = shipped_l10n();
            Names::new(&l10n, session).plant(plant)
        }
        _ => HINT.to_owned(),
    };
    Blocks {
        details: region(&widgets, &details_label),
        list: region(&widgets, "Plants"),
        panel: side_panel(pass),
        widgets,
    }
}

#[test]
fn at_increased_zoom_the_full_plant_card_and_list_remain_reachable() {
    let logs = TempDir::new().unwrap();
    let mut app = operating_hour_app(&logs, WESTERN);
    let hydro = online(&app, PlantKind::Hydro);
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Plant(hydro.id))]);
    let pass = EguiPass::attach(&mut app);
    pass.ctx().set_zoom_factor(1.25);
    let (width, height) = (SMALL.0 / 1.25, SMALL.1 / 1.25);
    let widgets = pass.settled(&mut app, width, height);
    let side = side_panel(&pass);
    assert!(side.contains_rect(label(&widgets, "Grid connection")));
    // Scroll the outer column from its gauge, away from the nested card/list scroll areas.
    let at = egui::pos2(side.center().x, side.top() + 10.0);
    pass.run_with(
        &mut app,
        width,
        height,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: egui::vec2(0.0, -400.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let widgets = pass.settled(&mut app, width, height);
    let list = region(&widgets, "Plants");
    assert!(
        side.expand(ROUNDING).contains_rect(list),
        "plant list {list:?} outside {side:?}"
    );
    assert!(widgets.iter().any(|w| w.text == "Specifications"));
}

#[test]
fn the_details_sit_above_a_plant_list_that_fills_the_side_panel() {
    for region_id in [EASTERN, WESTERN] {
        for tab in [OperatingTab::Supply, OperatingTab::Hydro] {
            let logs = TempDir::new().unwrap();
            let mut app = operating_hour_app(&logs, region_id);
            dispatch_in(&mut app, vec![UiAction::SetOperatingTab(tab)]);
            let pass = EguiPass::attach(&mut app);
            let what = format!("{region_id} {tab:?}");
            let kinds: &[PlantKind] = if region_id == WESTERN {
                // The tallest details: a hydro plant has its water as a figure too.
                &[PlantKind::Ccgt, PlantKind::Hydro]
            } else {
                &[PlantKind::Ccgt]
            };
            for kind in kinds {
                let plant = online(&app, *kind);
                let b = blocks(&mut app, &pass, Selection::Plant(plant.id), SMALL);
                let what = format!("{what} {kind:?}");

                // The details first, then the list just under them.
                let name = label(&b.widgets, &name_of(&app, &plant));
                assert!(b.details.contains_rect(name), "{what}: {name:?}");
                let gap = b.list.top() - b.details.bottom();
                assert!(gap > 0.0 && gap <= 12.0, "{what}: the list is {gap} below");

                // Every figure through *Closed months* is in view, in the details block.
                for figure in FIGURES {
                    let row = label(&b.widgets, figure);
                    assert!(
                        b.details.contains_rect(row),
                        "{what}: {figure} {row:?} is outside the details {:?}",
                        b.details
                    );
                }
                // The block is the height its content needs: no room to spare under the tallest.
                if *kind == PlantKind::Hydro {
                    let last = label(&b.widgets, "Water left today");
                    assert!(b.details.contains_rect(last), "{what}: {last:?}");
                    let spare = b.details.bottom() - last.bottom();
                    assert!(spare <= 3.0, "{what}: {spare} spare under the last figure");
                }

                // The list reaches the panel's bottom, and shows at least three rows.
                let spare = b.panel.bottom() - b.list.bottom();
                assert!(
                    (0.0..=4.0).contains(&spare),
                    "{what}: the list ends {spare} above the panel's bottom"
                );
                let visible = row_names(&app)
                    .iter()
                    .filter(|name| {
                        b.widgets.iter().any(|w| {
                            &w.text == *name
                                && w.role != egui::accesskit::Role::Label
                                && w.role != egui::accesskit::Role::Unknown
                                && b.list.expand(ROUNDING).contains_rect(w.rect)
                        })
                    })
                    .count();
                assert!(visible >= 3, "{what}: {visible} rows are in view");
            }
        }
    }
}

#[test]
fn a_taller_window_gives_its_height_to_the_list() {
    for region_id in [EASTERN, WESTERN] {
        let logs = TempDir::new().unwrap();
        let mut app = operating_hour_app(&logs, region_id);
        let pass = EguiPass::attach(&mut app);
        let plant = online(&app, PlantKind::Ccgt);
        let small = blocks(&mut app, &pass, Selection::Plant(plant.id), SMALL);
        let large = blocks(&mut app, &pass, Selection::Plant(plant.id), LARGE);

        assert_eq!(
            large.details.height(),
            small.details.height(),
            "{region_id}: the details keep their height"
        );
        let extra = large.panel.height() - small.panel.height();
        assert!(extra > 500.0, "{region_id}: the panel is {extra} taller");
        assert!(
            (large.list.height() - small.list.height() - extra).abs() <= 1.0,
            "{region_id}: the list grows by the panel's extra {extra}: {:?} then {:?}",
            small.list,
            large.list
        );
    }
}

#[test]
fn the_list_does_not_move_when_the_selection_changes() {
    for region_id in [EASTERN, WESTERN] {
        let logs = TempDir::new().unwrap();
        let mut app = operating_hour_app(&logs, region_id);
        let pass = EguiPass::attach(&mut app);
        let first = online(&app, PlantKind::Ccgt);
        let other = app
            .world()
            .resource::<Session>()
            .0
            .session()
            .plant_views()
            .into_iter()
            .find(|plant| plant.kind != PlantKind::Ccgt)
            .expect("another plant");
        let start = blocks(&mut app, &pass, Selection::Plant(first.id), SMALL);
        for selection in [Selection::None, Selection::Plant(other.id)] {
            let now = blocks(&mut app, &pass, selection, SMALL);
            assert_eq!(now.list, start.list, "{region_id}: {selection:?}");
            assert_eq!(now.details, start.details, "{region_id}: {selection:?}");
        }
    }
}

/// Drags the inner edge of the outer panel `id` as far out as it goes.
fn drag_to_maximum(app: &mut App, pass: &EguiPass, id: &str, size: (f32, f32)) {
    let rect = egui::PanelState::load(pass.ctx(), egui::Id::new(id))
        .unwrap_or_else(|| panic!("{id} is drawn"))
        .outer_rect;
    let (from, to) = if id == "operating" {
        let from = egui::pos2(rect.center().x, rect.top());
        (from, egui::pos2(from.x, 1.0))
    } else {
        let from = egui::pos2(rect.left(), rect.center().y);
        (from, egui::pos2(1.0, from.y))
    };
    pass.drag(app, size.0, size.1, from, to);
    pass.settled(app, size.0, size.1);
}

/// However far the bottom panel is dragged, the side panel keeps room for the gauge, every figure
/// of the plant with the most of them, and three rows of the list (spec §16.7): the bottom panel's
/// maximum leaves it that much.
#[test]
fn a_bottom_panel_at_its_maximum_leaves_the_details_and_the_list_in_view() {
    for region_id in [EASTERN, WESTERN] {
        for size in [SMALL, (1280.0, 800.0)] {
            let logs = TempDir::new().unwrap();
            let mut app = operating_hour_app(&logs, region_id);
            let pass = EguiPass::attach(&mut app);
            // The plant with the most figures: a hydro plant, which the Eastern start fleet
            // doesn't have, so a CCGT there.
            let views = app.world().resource::<Session>().0.session().plant_views();
            let plant = views
                .iter()
                .find(|plant| plant.kind == PlantKind::Hydro)
                .or_else(|| views.iter().find(|plant| plant.kind == PlantKind::Ccgt))
                .expect("a hydro or CCGT plant")
                .clone();
            let figures: Vec<&str> = if plant.kind == PlantKind::Hydro {
                FIGURES.into_iter().chain(["Water left today"]).collect()
            } else {
                FIGURES.to_vec()
            };
            blocks(&mut app, &pass, Selection::Plant(plant.id), size);
            drag_to_maximum(&mut app, &pass, "operating_side", size);
            drag_to_maximum(&mut app, &pass, "operating", size);
            let b = blocks(&mut app, &pass, Selection::Plant(plant.id), size);
            let what = format!("{region_id} at {size:?}");
            let bottom = egui::PanelState::load(pass.ctx(), egui::Id::new("operating"))
                .unwrap()
                .outer_rect;
            // Its maximum, as `panel_resize`'s `expected_max` works it out: from the space below
            // the bars, reserving what the side panel's minimum height needs beyond the map's.
            let below_bars = egui::vec2(size.0, size.1 - b.panel.top());
            let max = panel_max(
                Edge::Bottom,
                OPERATING_PANEL_HEIGHT,
                egui::vec2(size.0, size.1),
                below_bars,
                (operating_side_min_height(pass.ctx()) - MAP_MIN_SIZE.y).max(0.0),
            );
            assert!(
                (bottom.height() - max).abs() <= ROUNDING,
                "{what}: the bottom panel reached its maximum {max}: {bottom:?}"
            );

            for figure in figures {
                let row = label(&b.widgets, figure);
                assert!(
                    b.panel.contains_rect(row),
                    "{what}: {figure} {row:?} is outside the side panel {:?}",
                    b.panel
                );
            }
            let visible = row_names(&app)
                .iter()
                .filter(|name| {
                    b.widgets.iter().any(|w| {
                        &w.text == *name
                            && w.role != egui::accesskit::Role::Label
                            && w.role != egui::accesskit::Role::Unknown
                            && b.list.expand(ROUNDING).contains_rect(w.rect)
                            && b.panel.expand(ROUNDING).contains_rect(w.rect)
                    })
                })
                .count();
            assert!(
                visible >= 3,
                "{what}: {visible} rows in view, the list {:?} in the panel {:?}",
                b.list,
                b.panel
            );
        }
    }
}
