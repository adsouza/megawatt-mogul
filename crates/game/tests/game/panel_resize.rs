//! The outer panels resized by dragging their inner edge, through the real egui pass (spec §16.7).
//!
//! Each opens at its fixed size, which is also its minimum, and can grow up to its maximum: a
//! share of the screen that leaves the map usable. Only a drag changes a panel's size: the map
//! still never moves on its own.

use application::Command;
use bevy::prelude::*;
use bevy_egui::egui;
use domain::ids::CellCoord;
use domain::session::DayPhase;
use game::hud::{
    BUILD_PANEL_WIDTH, Edge, HudArea, INSPECTOR_PANEL_WIDTH, MAP_MIN_SIZE, OPERATING_PANEL_HEIGHT,
    OPERATING_SIDE_WIDTH, OperatingTab, PLANNING_PANEL_HEIGHT, PlanningTab, Selection, UiAction,
    operating_side_min_height, panel_max,
};
use game::sim::Session;
use tempfile::TempDir;

use crate::hud_smoke::{EASTERN, WESTERN, dispatch_in, operating_hour_app};
use crate::support::egui_harness::widget;
use crate::support::egui_pass::EguiPass;
use crate::support::{hud_app, request, update_n};

const SMALL: (f32, f32) = (1024.0, 640.0);
const LARGE: (f32, f32) = (2560.0, 1440.0);

/// An outer panel the player can resize: its egui id, its edge and its opening size.
#[derive(Debug, Clone, Copy)]
struct Resizable {
    id: &'static str,
    edge: Edge,
    size: f32,
}

const INSPECTOR: Resizable = Resizable {
    id: "inspector",
    edge: Edge::Left,
    size: INSPECTOR_PANEL_WIDTH,
};
const BUILD: Resizable = Resizable {
    id: "build",
    edge: Edge::Right,
    size: BUILD_PANEL_WIDTH,
};
const PLANNING: Resizable = Resizable {
    id: "planning",
    edge: Edge::Bottom,
    size: PLANNING_PANEL_HEIGHT,
};
const OPERATING_SIDE: Resizable = Resizable {
    id: "operating_side",
    edge: Edge::Right,
    size: OPERATING_SIDE_WIDTH,
};
const OPERATING: Resizable = Resizable {
    id: "operating",
    edge: Edge::Bottom,
    size: OPERATING_PANEL_HEIGHT,
};

/// Pixel rounding: egui snaps a panel's rect to whole points.
const ROUNDING: f32 = 1.0;

/// A screen of `size` drawn by the real pass, in a phase's app.
struct Screen {
    app: App,
    pass: EguiPass,
    width: f32,
    height: f32,
    _logs: TempDir,
}

impl Screen {
    fn planning(region: &str, (width, height): (f32, f32)) -> Self {
        let logs = TempDir::new().unwrap();
        let mut app = hud_app(logs.path());
        request(&mut app, region);
        Self::attach(app, logs, width, height)
    }

    fn operating(region: &str, (width, height): (f32, f32)) -> Self {
        let logs = TempDir::new().unwrap();
        let app = operating_hour_app(&logs, region);
        Self::attach(app, logs, width, height)
    }

    fn attach(mut app: App, logs: TempDir, width: f32, height: f32) -> Self {
        let pass = EguiPass::attach(&mut app);
        pass.settled(&mut app, width, height);
        Self {
            app,
            pass,
            width,
            height,
            _logs: logs,
        }
    }

    /// Two plain passes, so the layout after an input pass has settled: the map's rect.
    fn map(&mut self) -> egui::Rect {
        self.pass.settled(&mut self.app, self.width, self.height);
        let widgets = self.pass.run(&mut self.app, self.width, self.height);
        widget(&widgets, "Map").rect
    }

    /// The space the panels left the map in the last pass: what the map's central panel fills.
    fn map_area(&self) -> egui::Rect {
        self.app
            .world()
            .resource::<HudArea>()
            .free_rect(self.pass.ctx())
    }

    /// The panel's outer rect, as egui last laid it out.
    fn rect(&self, panel: Resizable) -> egui::Rect {
        egui::PanelState::load(self.pass.ctx(), egui::Id::new(panel.id))
            .unwrap_or_else(|| panic!("{} has been drawn", panel.id))
            .outer_rect
    }

    fn size(&self, panel: Resizable) -> f32 {
        along(panel.edge, self.rect(panel).size())
    }

    /// Drags `panel`'s inner edge by `by` points outward (negative: inward).
    fn drag(&mut self, panel: Resizable, by: f32) {
        let rect = self.rect(panel);
        let from = match panel.edge {
            Edge::Left => egui::pos2(rect.right(), rect.center().y),
            Edge::Right => egui::pos2(rect.left(), rect.center().y),
            Edge::Bottom => egui::pos2(rect.center().x, rect.top()),
        };
        let to = from + outward(panel.edge) * by;
        let to = egui::pos2(
            to.x.clamp(1.0, self.width - 1.0),
            to.y.clamp(1.0, self.height - 1.0),
        );
        self.pass
            .drag(&mut self.app, self.width, self.height, from, to);
    }

    /// The most `panel` can grow to here, from the space the panels before it leave.
    fn expected_max(&self, panel: Resizable, side: Resizable) -> f32 {
        let screen = egui::vec2(self.width, self.height);
        // The side panels start below the bars, where the bottom panel's free space starts.
        let below_bars = egui::vec2(self.width, self.height - self.rect(side).top());
        match panel.id {
            "inspector" => panel_max(panel.edge, panel.size, screen, below_bars, self.size(BUILD)),
            "build" => panel_max(
                panel.edge,
                panel.size,
                screen,
                below_bars - egui::vec2(self.size(INSPECTOR), 0.0),
                0.0,
            ),
            // The side panel above it keeps its minimum height, beyond the map's.
            "operating" => panel_max(
                panel.edge,
                panel.size,
                screen,
                below_bars,
                (operating_side_min_height(self.pass.ctx()) - MAP_MIN_SIZE.y).max(0.0),
            ),
            _ => panel_max(panel.edge, panel.size, screen, below_bars, 0.0),
        }
    }
}

fn along(edge: Edge, size: egui::Vec2) -> f32 {
    match edge {
        Edge::Left | Edge::Right => size.x,
        Edge::Bottom => size.y,
    }
}

fn outward(edge: Edge) -> egui::Vec2 {
    match edge {
        Edge::Left => egui::vec2(1.0, 0.0),
        Edge::Right => egui::vec2(-1.0, 0.0),
        Edge::Bottom => egui::vec2(0.0, -1.0),
    }
}

fn assert_near(actual: f32, expected: f32, what: &str) {
    assert!(
        (actual - expected).abs() <= ROUNDING,
        "{what}: {actual}, expected {expected}"
    );
}

/// The map's area keeps at least [`MAP_MIN_SIZE`], and the map is drawn in it.
fn assert_map_usable(screen: &mut Screen, what: &str) {
    let area = screen.map_area();
    assert!(
        area.width() >= MAP_MIN_SIZE.x - ROUNDING && area.height() >= MAP_MIN_SIZE.y - ROUNDING,
        "{what}: the map's area is {area:?}"
    );
    let widgets = screen
        .pass
        .run(&mut screen.app, screen.width, screen.height);
    let map = widget(&widgets, "Map").rect;
    assert!(
        area.contains_rect(map) && map.width() > 0.0 && map.height() > 0.0,
        "{what}: the map is {map:?} in {area:?}"
    );
}

/// Grows `panel`, shrinks it past its minimum, then drags it as far as it goes, on `screen`.
fn resize_each_way(screen: &mut Screen, panel: Resizable, side: Resizable, what: &str) {
    let map = screen.map();
    assert_near(
        screen.size(panel),
        panel.size,
        &format!("{what}: opens at its size"),
    );
    let max = screen.expected_max(panel, side);

    // Outward: the panel grows by the drag and the map shrinks by as much. 150 where the
    // maximum leaves room for it; at 1024×640 no panel has 150 to spare, so half its room, but at
    // least 10 (egui takes a move of under 6 for a click) and never more than all of it. The
    // Operating bottom panel has only 10 there: the side panel above it keeps its minimum.
    let room = (max - panel.size).floor();
    let by = 150.0_f32.min((room / 2.0).floor().max(room.min(10.0)));
    assert!(by > 0.0, "{what}: there is room to grow");
    if screen.width >= LARGE.0 {
        assert_eq!(by, 150.0, "{what}");
    }
    screen.drag(panel, by);
    let grown = screen.map();
    assert_near(
        screen.size(panel),
        panel.size + by,
        &format!("{what}: grows"),
    );
    assert_near(
        along(panel.edge, map.size()) - along(panel.edge, grown.size()),
        by,
        &format!("{what}: the map gives way"),
    );

    // Inward past the minimum: it stops at its opening size, and the map is as it was.
    screen.drag(panel, -(by + 200.0));
    assert_near(
        screen.size(panel),
        panel.size,
        &format!("{what}: stops at its minimum"),
    );
    assert_eq!(screen.map(), map, "{what}: the map is back where it was");

    // Far outward: it stops at its maximum, and the map stays usable.
    screen.drag(panel, 5_000.0);
    screen.map();
    assert_near(
        screen.size(panel),
        max,
        &format!("{what}: stops at its maximum"),
    );
    assert_map_usable(screen, what);
}

// Every case keeps its own app, temp directory and complete drag sequence.
macro_rules! resize_cases {
    ($phase:ident, $anchor:ident; $($case:ident: ($region:ident, $size:ident, $panel:ident)),+ $(,)?) => {
        $(
            #[test]
            fn $case() {
                let mut screen = Screen::$phase($region, $size);
                let what = format!("{} at {:?}: {}", $region, $size, $panel.id);
                resize_each_way(&mut screen, $panel, $anchor, &what);
            }
        )+
    };
}

mod each_planning_panel_resizes_between_its_minimum_and_its_maximum {
    use super::*;

    resize_cases!(planning, INSPECTOR;
        eastern_small_inspector: (EASTERN, SMALL, INSPECTOR),
        eastern_small_build: (EASTERN, SMALL, BUILD),
        eastern_small_planning: (EASTERN, SMALL, PLANNING),
        eastern_large_inspector: (EASTERN, LARGE, INSPECTOR),
        eastern_large_build: (EASTERN, LARGE, BUILD),
        eastern_large_planning: (EASTERN, LARGE, PLANNING),
        western_small_inspector: (WESTERN, SMALL, INSPECTOR),
        western_small_build: (WESTERN, SMALL, BUILD),
        western_small_planning: (WESTERN, SMALL, PLANNING),
        western_large_inspector: (WESTERN, LARGE, INSPECTOR),
        western_large_build: (WESTERN, LARGE, BUILD),
        western_large_planning: (WESTERN, LARGE, PLANNING),
    );
}

mod each_operating_panel_resizes_between_its_minimum_and_its_maximum {
    use super::*;

    resize_cases!(operating, OPERATING_SIDE;
        eastern_small_side: (EASTERN, SMALL, OPERATING_SIDE),
        eastern_small_bottom: (EASTERN, SMALL, OPERATING),
        eastern_large_side: (EASTERN, LARGE, OPERATING_SIDE),
        eastern_large_bottom: (EASTERN, LARGE, OPERATING),
        western_small_side: (WESTERN, SMALL, OPERATING_SIDE),
        western_small_bottom: (WESTERN, SMALL, OPERATING),
        western_large_side: (WESTERN, LARGE, OPERATING_SIDE),
        western_large_bottom: (WESTERN, LARGE, OPERATING),
    );
}

/// Both Planning side panels and the bottom panel dragged as far as they go: the map keeps its
/// room, and growing one never squeezes another.
#[test]
fn every_planning_panel_at_its_maximum_still_leaves_the_map_usable() {
    for size in [SMALL, LARGE] {
        for order in [[BUILD, INSPECTOR, PLANNING], [INSPECTOR, BUILD, PLANNING]] {
            let mut screen = Screen::planning(EASTERN, size);
            screen.map();
            for panel in order {
                screen.drag(panel, 5_000.0);
                screen.map();
            }
            let what = format!("{size:?}, {order:?}");
            screen.map();
            assert_map_usable(&mut screen, &what);
            let [first, second, _] = order;
            assert!(
                screen.size(first) > first.size + 1.0,
                "{what}: {} kept its growth",
                first.id
            );
            assert!(screen.size(second) >= second.size, "{what}");
        }
    }
}

fn phase(app: &App) -> DayPhase {
    app.world().resource::<Session>().0.session().phase()
}

/// Grows `panels` by 40 each.
fn grow(screen: &mut Screen, panels: &[Resizable]) {
    for panel in panels {
        screen.drag(*panel, 40.0);
        screen.map();
    }
}

fn sizes(screen: &Screen, panels: &[Resizable]) -> Vec<f32> {
    panels.iter().map(|panel| screen.size(*panel)).collect()
}

/// After a drag, a selection change, a tab change and a whole day leave the panels' sizes and
/// the map's rect alone: only a drag moves the map.
#[test]
fn the_map_never_moves_on_its_own_after_a_resize() {
    let planning = [INSPECTOR, BUILD, PLANNING];
    let operating = [OPERATING_SIDE, OPERATING];
    for region in [EASTERN, WESTERN] {
        for size in [SMALL, LARGE] {
            let what = format!("{region} at {size:?}");
            let mut screen = Screen::planning(region, size);
            screen.map();
            grow(&mut screen, &planning);
            let planning_sizes = sizes(&screen, &planning);
            let planning_map = screen.map();
            for (panel, grown) in planning.iter().zip(&planning_sizes) {
                assert_near(*grown, panel.size + 40.0, &format!("{what}: {}", panel.id));
            }

            // A selection change and a tab change.
            let land = first_land(&screen.app);
            for actions in [
                vec![UiAction::Select(Selection::Cell(land))],
                vec![UiAction::SetPlanningTab(PlanningTab::Forecast)],
                vec![UiAction::SetPlanningTab(PlanningTab::Tariff)],
                vec![UiAction::SetPlanningTab(PlanningTab::Construction)],
                vec![UiAction::Select(Selection::None)],
            ] {
                dispatch_in(&mut screen.app, actions.clone());
                update_n(&mut screen.app, 1);
                assert_eq!(screen.map(), planning_map, "{what}: {actions:?}");
                assert_eq!(
                    sizes(&screen, &planning),
                    planning_sizes,
                    "{what}: {actions:?}"
                );
            }

            // Into the day: the Operating panels open at their sizes, and keep a resize through a
            // selection and a tab change.
            dispatch_in(
                &mut screen.app,
                vec![UiAction::Command(Command::BeginOperating)],
            );
            update_n(&mut screen.app, 2);
            assert_eq!(phase(&screen.app), DayPhase::Operating, "{what}");
            screen.map();
            grow(&mut screen, &operating);
            let operating_sizes = sizes(&screen, &operating);
            let operating_map = screen.map();
            let plant = screen
                .app
                .world()
                .resource::<Session>()
                .0
                .session()
                .plant_views()[0]
                .id;
            for actions in [
                vec![UiAction::Select(Selection::Plant(plant))],
                vec![UiAction::SetOperatingTab(OperatingTab::Hydro)],
                vec![UiAction::SetOperatingTab(OperatingTab::Supply)],
                vec![UiAction::Select(Selection::None)],
            ] {
                dispatch_in(&mut screen.app, actions.clone());
                update_n(&mut screen.app, 1);
                assert_eq!(screen.map(), operating_map, "{what}: {actions:?}");
                assert_eq!(
                    sizes(&screen, &operating),
                    operating_sizes,
                    "{what}: {actions:?}"
                );
            }

            // The rest of the day, the review, and back to Planning: as the player left it.
            dispatch_in(
                &mut screen.app,
                vec![UiAction::Command(Command::AdvanceTicks { n: 1_440 })],
            );
            update_n(&mut screen.app, 3);
            assert_eq!(phase(&screen.app), DayPhase::Review, "{what}");
            let widgets = screen
                .pass
                .settled(&mut screen.app, screen.width, screen.height);
            assert!(!widgets.iter().any(|w| w.text == "Map"));
            dispatch_in(
                &mut screen.app,
                vec![UiAction::Command(Command::FinishReview)],
            );
            update_n(&mut screen.app, 2);
            assert_eq!(phase(&screen.app), DayPhase::Planning, "{what}");
            assert_eq!(screen.map(), planning_map, "{what}: after the round trip");
            assert_eq!(sizes(&screen, &planning), planning_sizes, "{what}");

            // And the next day's Operating panels keep theirs.
            dispatch_in(
                &mut screen.app,
                vec![UiAction::Command(Command::BeginOperating)],
            );
            update_n(&mut screen.app, 2);
            assert_eq!(screen.map(), operating_map, "{what}: the next day");
            assert_eq!(sizes(&screen, &operating), operating_sizes, "{what}");
        }
    }
}

/// The first land cell of the running game's region.
fn first_land(app: &App) -> CellCoord {
    let terrain = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .region()
        .terrain();
    (0..terrain.height())
        .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
        .find(|cell| terrain.get(*cell).is_some_and(|tile| tile.land))
        .expect("a land cell")
}

#[test]
fn incidents_have_a_wider_column_and_a_draggable_divider() {
    fn drag_log(screen: &mut Screen, log: Resizable, by: f32) {
        let rect = screen.rect(log);
        let edge = egui::pos2(rect.left(), rect.center().y);
        let to = egui::pos2((edge.x - by).clamp(1.0, screen.width - 1.0), edge.y);
        screen.pass.drag(
            &mut screen.app,
            screen.width,
            screen.height,
            edge + egui::vec2(2.0, 0.0),
            to,
        );
    }
    let log = Resizable {
        id: "operating_log",
        edge: Edge::Right,
        size: 480.0,
    };
    for size in [SMALL, LARGE] {
        let mut screen = Screen::operating(EASTERN, size);
        let original_map = screen.map();
        assert_near(screen.size(log), 480.0, "incidents open wider");
        drag_log(&mut screen, log, 100.0);
        screen.map();
        assert_near(screen.size(log), 580.0, "incidents can grow");
        assert_eq!(
            screen.map(),
            original_map,
            "the inner divider keeps the map still"
        );
        drag_log(&mut screen, log, 10_000.0);
        screen.map();
        assert!(
            screen.rect(log).left() >= 420.0,
            "the chart keeps usable width"
        );
        drag_log(&mut screen, log, -10_000.0);
        screen.map();
        assert!(
            screen.size(log) >= 300.0,
            "the incident column keeps its minimum"
        );
    }
}
