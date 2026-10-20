//! Smoke tests for the HUD's draw functions and its action dispatch.
//!
//! Each screen is drawn with the shared headless [`Harness`] against real data (the shipped
//! regions and translations, a real `Game`), and a scripted click must produce the expected
//! `UiAction`.

use std::cell::RefCell;
use std::fs;
use std::path::Path;

use application::{
    Command, CommandLogError, CommandLogSink, Game, InMemoryCommandLog, LogHeader, LogRecord,
    RegionCatalog,
};
use bevy::ecs::message::{Message, Messages};
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy_egui::egui;
use domain::ids::{CellCoord, RegionId};
use domain::session::{GameOverReason, GameSettings};
use game::hud::{
    ActionSink, BUILD_PANEL_WIDTH, ButtonModel, CommitmentForm, GameOverView, HudArea,
    INSPECTOR_PANEL_WIDTH, InspectorModel, LastSpeed, LinePreview, LineSection, MapCache, MapLayer,
    MapLayout, MenuForm, MenuScreen, MilestonesOpen, OPERATING_LOG_WIDTH, OPERATING_PANEL_HEIGHT,
    OPERATING_SIDE_WIDTH, OpenPrompt, OperatingTab, PLANNING_PANEL_HEIGHT, PlanningTab,
    PlantPanelForm, PlantPanelModel, PlantSection, Prompt, RegionListings, Selection, ToastQueue,
    UiAction, UiState, boot_error_model, build_model, button, dispatch, draw_boot_error,
    draw_build, draw_game_over, draw_inspector, draw_main_menu, draw_map, draw_planning,
    draw_plant_panel, draw_prompt, draw_region_select, draw_speed, draw_toasts, draw_top_bar,
    draw_warnings, game_over_model_for, hit_test, inspector_model, is_commitable, main_menu_model,
    map_model, planning_model, plant_half_size, plant_panel_model, prompt_model,
    region_select_model, speed_model, toasts_model, top_bar_model, warnings_for,
};
use game::l10n::Localize;
use game::sim::{NewGameRequest, PlayerCommand, ReturnToMenu, Session, SimSpeed};
use game::state::{AppState, BootError};
use infrastructure::region::RonRegionCatalog;
use tempfile::TempDir;

use crate::support::egui_harness::{
    Harness, Widget, button as find_button, buttons, central, shows, widget,
};
use crate::support::egui_pass::EguiPass;
use crate::support::{
    empty_start, empty_start_in, headless_app, hud_app, request, shipped_assets, shipped_l10n,
    update_n,
};

pub(crate) const EASTERN: &str = "eastern_interconnection";
pub(crate) const WESTERN: &str = "western_interconnection";

// ---- fixtures -------------------------------------------------------------------------------

fn listings_in(dir: &Path) -> RegionListings {
    RegionListings(RonRegionCatalog::new(dir.to_path_buf()).list())
}

fn shipped_listings() -> RegionListings {
    listings_in(&shipped_assets().join("regions"))
}

/// A regions folder holding only the shipped eastern region, plus `extra` files.
fn regions_with_eastern(extra: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    let eastern = format!("{EASTERN}.ron");
    fs::copy(
        shipped_assets().join("regions").join(&eastern),
        dir.path().join(&eastern),
    )
    .unwrap();
    for (name, contents) in extra {
        fs::write(dir.path().join(name), contents).unwrap();
    }
    dir
}

fn game_in(region: &str) -> Game {
    let catalog = RonRegionCatalog::new(shipped_assets().join("regions"));
    Game::start_new(
        &catalog,
        &RegionId::new(region),
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap()
}

fn eastern_game() -> Game {
    game_in(EASTERN)
}

/// [`eastern_game`] with the inherited schedule undone: nothing committed, nothing on AGC.
fn empty_eastern_game() -> Game {
    let mut game = eastern_game();
    empty_start(&mut game);
    game
}

// ---- screens --------------------------------------------------------------------------------

#[test]
fn the_boot_error_screen_quits_even_without_translations() {
    let error = BootError("bad.ftl has syntax errors".to_owned());
    let model = boot_error_model(&error, None);
    let harness = Harness::new();

    assert!(shows(
        &harness.draw(|ui| central(ui, |ui| draw_boot_error(ui, &model))),
        "bad.ftl"
    ));
    let actions = harness.click("Quit", |ui| central(ui, |ui| draw_boot_error(ui, &model)));

    assert_eq!(actions, vec![UiAction::Quit]);
}

#[test]
fn new_game_with_two_regions_opens_the_picker() {
    let l10n = shipped_l10n();
    let listings = shipped_listings();
    let mut form = MenuForm::new(42);
    let model = main_menu_model(&listings, &form, &l10n);

    let actions = Harness::new().click("New game", |ui| {
        central(ui, |ui| draw_main_menu(ui, &model, &mut form))
    });

    assert_eq!(actions, vec![UiAction::Navigate(MenuScreen::RegionSelect)]);
}

#[test]
fn new_game_with_one_region_starts_it_directly() {
    let l10n = shipped_l10n();
    let regions = regions_with_eastern(&[]);
    let listings = listings_in(regions.path());
    let mut form = MenuForm::new(42);
    let model = main_menu_model(&listings, &form, &l10n);

    let actions = Harness::new().click("New game", |ui| {
        central(ui, |ui| draw_main_menu(ui, &model, &mut form))
    });

    assert_eq!(
        actions,
        vec![UiAction::NewGame {
            region: RegionId::new(EASTERN),
            seed: 42
        }]
    );
}

#[test]
fn the_picker_starts_the_region_clicked_with_the_menu_seed() {
    let l10n = shipped_l10n();
    let model = region_select_model(&shipped_listings(), Some(42), &l10n);

    let actions = Harness::new().click("Eastern Interconnection", |ui| {
        central(ui, |ui| draw_region_select(ui, &model))
    });

    assert_eq!(
        actions,
        vec![UiAction::NewGame {
            region: RegionId::new(EASTERN),
            seed: 42
        }]
    );
}

#[test]
fn a_broken_region_is_listed_disabled_next_to_a_good_one() {
    let l10n = shipped_l10n();
    let regions = regions_with_eastern(&[("broken.ron", "this is not RON (")]);
    let model = region_select_model(&listings_in(regions.path()), Some(3), &l10n);
    let harness = Harness::new();
    let draw = |ui: &mut egui::Ui| central(ui, |ui| draw_region_select(ui, &model));

    let widgets = harness.draw(draw);
    assert!(find_button(&widgets, "broken").disabled);
    assert!(!find_button(&widgets, "Eastern Interconnection").disabled);
    assert!(shows(&widgets, "Unavailable"));

    assert_eq!(harness.click("broken", draw), vec![]);
    assert_eq!(
        harness.click("Eastern Interconnection", draw),
        vec![UiAction::NewGame {
            region: RegionId::new(EASTERN),
            seed: 3
        }]
    );
}

#[test]
fn an_empty_or_missing_regions_folder_offers_back() {
    let l10n = shipped_l10n();
    let empty = TempDir::new().unwrap();
    for listings in [
        listings_in(empty.path()),
        listings_in(&empty.path().join("missing")),
    ] {
        let model = region_select_model(&listings, Some(1), &l10n);
        let harness = Harness::new();
        let draw = |ui: &mut egui::Ui| central(ui, |ui| draw_region_select(ui, &model));

        assert!(shows(&harness.draw(draw), "No regions were found"));
        assert_eq!(
            harness.click("Back", draw),
            vec![UiAction::Navigate(MenuScreen::MainMenu)]
        );
    }
}

#[test]
fn the_top_bar_draws_a_real_game_and_its_menu_button_asks_first() {
    let l10n = shipped_l10n();
    let game = eastern_game();
    let model = top_bar_model(&game, &l10n);
    let harness = Harness::new();
    let draw = |ui: &mut egui::Ui| {
        egui::Panel::top("top_bar")
            .show(ui, |ui| draw_top_bar(ui, &model))
            .inner
    };

    let widgets = harness.draw(draw);
    for text in [&model.date, &model.phase, &model.cash.value, "0/3"] {
        assert!(shows(&widgets, text), "{text} is not shown");
    }
    assert_eq!(
        harness.click("Menu", draw),
        vec![UiAction::Prompt(Some(Prompt::LeaveGame))]
    );
}

#[test]
fn the_leave_prompt_leaves_only_on_leave() {
    let l10n = shipped_l10n();
    let game = eastern_game();
    let prompt = prompt_model(OpenPrompt(Some(Prompt::LeaveGame)), Some(&game), &l10n)
        .expect("the prompt is open");
    let harness = Harness::new();
    let draw = |ui: &mut egui::Ui| {
        egui::Modal::new(egui::Id::new("prompt"))
            .show(ui.ctx(), |ui| draw_prompt(ui, &prompt))
            .inner
    };

    assert!(shows(&harness.draw(draw), "can't be resumed yet"));
    assert_eq!(harness.click("Stay", draw), vec![UiAction::Prompt(None)]);
    assert_eq!(harness.click("Leave", draw), vec![UiAction::ReturnToMenu]);
}

#[test]
fn the_harness_tells_repeated_labels_apart() {
    let first = ButtonModel::new("Go".to_owned(), UiAction::Quit);
    let second = ButtonModel::new("Go".to_owned(), UiAction::ReturnToMenu);
    let harness = Harness::new();
    let draw = |ui: &mut egui::Ui| {
        central(ui, |ui| {
            let mut actions = Vec::new();
            button(ui, &first, &mut actions);
            button(ui, &second, &mut actions);
            actions
        })
    };

    assert_eq!(harness.click_nth("Go", 0, draw), vec![UiAction::Quit]);
    assert_eq!(
        harness.click_nth("Go", 1, draw),
        vec![UiAction::ReturnToMenu]
    );
}

#[test]
fn the_game_over_modal_returns_to_the_menu() {
    let l10n = shipped_l10n();
    let game = eastern_game();
    let model = game_over_model_for(
        GameOverReason::Bankrupt,
        &game.session().company_view(),
        true,
        &l10n,
    );
    let harness = Harness::new();
    let draw = |ui: &mut egui::Ui| {
        egui::Modal::new(egui::Id::new("game_over"))
            .show(ui.ctx(), |ui| draw_game_over(ui, &model))
            .inner
    };

    assert!(shows(&harness.draw(draw), "bankrupt"));
    assert_eq!(
        harness.click("Main menu", draw),
        vec![UiAction::ReturnToMenu]
    );
    assert_eq!(
        harness.click("See the month", draw),
        vec![UiAction::ShowFinalMonth]
    );

    // Without a report there is nothing to see.
    let model = game_over_model_for(
        GameOverReason::Bankrupt,
        &game.session().company_view(),
        false,
        &l10n,
    );
    let draw = |ui: &mut egui::Ui| {
        egui::Modal::new(egui::Id::new("game_over"))
            .show(ui.ctx(), |ui| draw_game_over(ui, &model))
            .inner
    };
    assert!(!shows(&harness.draw(draw), "See the month"));
}

#[test]
fn toasts_draw_newest_first_and_ask_for_nothing() {
    let l10n = shipped_l10n();
    let mut queue = ToastQueue::default();
    queue.push("Older".to_owned(), 0.0);
    queue.push("Newer".to_owned(), 1.0);
    let model = toasts_model(&queue, &l10n, 1.5);
    let harness = Harness::new();

    let widgets = harness.draw(|ui| {
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -12.0))
            .show(ui.ctx(), |ui| draw_toasts(ui, &model))
            .inner
    });

    let newer = widgets.iter().find(|w| w.text == "Newer").unwrap();
    let older = widgets.iter().find(|w| w.text == "Older").unwrap();
    assert!(
        newer.rect.top() < older.rect.top(),
        "the newest toast is on top"
    );
}

/// An egui `Area` sizes itself over a few frames (it can't place its right edge before it knows
/// its width), so draw until the legend has stopped moving before measuring anything.
fn settle_legend(harness: &Harness, mut draw: impl FnMut(&mut egui::Ui) -> Vec<UiAction>) {
    let mut last = None;
    for _ in 0..10 {
        let rect = widget(&harness.draw(&mut draw), "Map legend").rect;
        if last == Some(rect) {
            return;
        }
        last = Some(rect);
    }
    panic!("the legend never settled: {last:?}");
}

#[test]
fn the_map_draws_both_shipped_regions_and_selects_what_is_clicked() {
    let l10n = shipped_l10n();
    for region in [EASTERN, WESTERN] {
        let game = game_in(region);
        let session = game.session();
        let terrain = session.region().terrain();
        let model = map_model(&game, &l10n, MapLayer::Terrain, Selection::None);
        let harness = Harness::new();
        let cache = RefCell::new(MapCache::default());
        let draw =
            |ui: &mut egui::Ui| central(ui, |ui| draw_map(ui, &model, &mut cache.borrow_mut()));

        settle_legend(&harness, draw);
        let widgets = harness.draw(draw);
        let map = widget(&widgets, "Map");
        let legend = widget(&widgets, "Map legend").rect;
        let layout = MapLayout::fit(map.rect, terrain.width(), terrain.height());
        assert!(
            map.rect.expand(0.01).contains_rect(layout.bounds()),
            "{region}: the grid fits the panel"
        );
        for label in [
            "Sunshine",
            "Cooling water",
            "Hydro potential",
            "Suitability: Coal plant",
        ] {
            assert!(!find_button(&widgets, label).disabled, "{region}: {label}");
        }
        assert!(
            find_button(&widgets, "Terrain").disabled,
            "the current layer"
        );

        // A plant, a city and a bare cell, clicked where the map says they are.
        let plant = &session.plant_views()[0];
        let at = layout.cell_center(plant.cell);
        let actions = harness.click_at(at, draw);
        if legend.contains(at) {
            assert_eq!(actions, vec![], "{region}: the plant is under the legend");
        } else {
            assert_eq!(
                actions,
                vec![UiAction::Select(hit_test(&layout, at, &game))]
            );
            assert!(
                matches!(actions[..], [UiAction::Select(Selection::Plant(_))]),
                "{region}: {actions:?}"
            );
        }

        // Every city is selectable at its own centre, unless a plant is drawn over that point.
        let half = plant_half_size(&layout);
        let plants = session.plant_views();
        let mut selected = 0;
        for city in session.city_views() {
            let at = layout.cell_center(city.cell);
            let covered = plants.iter().any(|plant| {
                let centre = layout.cell_center(plant.cell);
                (at.x - centre.x).abs() <= half && (at.y - centre.y).abs() <= half
            });
            let actions = harness.click_at(at, draw);
            if legend.contains(at) {
                assert_eq!(
                    actions,
                    vec![],
                    "{region} {:?}: under the legend",
                    city.cell
                );
            } else if covered {
                assert!(
                    matches!(actions[..], [UiAction::Select(Selection::Plant(_))]),
                    "{region} {:?}: {actions:?}",
                    city.cell
                );
            } else {
                assert_eq!(
                    actions,
                    vec![UiAction::Select(Selection::City(city.id))],
                    "{region} {:?}",
                    city.cell
                );
                selected += 1;
            }
        }
        assert!(selected > 0, "{region}: some city is selectable");

        let bare = layout.cell_center(domain::ids::CellCoord { x: 0, y: 0 });
        assert_eq!(
            harness.click_at(bare, draw),
            vec![UiAction::Select(Selection::Cell(domain::ids::CellCoord {
                x: 0,
                y: 0
            }))]
        );
        // The letterbox around the grid selects nothing.
        let outside = map.rect.right_bottom() - egui::vec2(0.5, 0.5);
        if !layout.bounds().contains(outside) {
            assert_eq!(
                harness.click_at(outside, draw),
                vec![UiAction::Select(Selection::None)]
            );
        }

        // A layer button asks for its layer; the current layer's button does nothing.
        assert_eq!(
            harness.click("Sunshine", draw),
            vec![UiAction::SetLayer(MapLayer::Irradiance)]
        );
        assert_eq!(harness.click("Terrain", draw), vec![]);
    }
    assert_eq!(l10n.problems(), vec![]);
}

#[test]
fn the_legend_names_every_mark_and_takes_no_map_clicks() {
    let l10n = shipped_l10n();
    for region in [EASTERN, WESTERN] {
        let game = game_in(region);
        let model = map_model(&game, &l10n, MapLayer::Terrain, Selection::None);
        let harness = Harness::new();
        let cache = RefCell::new(MapCache::default());
        let draw =
            |ui: &mut egui::Ui| central(ui, |ui| draw_map(ui, &model, &mut cache.borrow_mut()));
        settle_legend(&harness, draw);
        let widgets = harness.draw(draw);
        let map = widget(&widgets, "Map").rect;
        let legend = widget(&widgets, "Map legend").rect;

        // It sits inside the map, and every kind of plant and mark is named in it.
        assert!(map.contains_rect(legend), "{region}: {legend:?} in {map:?}");
        for entry in &model.legend {
            // egui reports a label as a Label node and a TextRun node; either has the rect.
            let label = widgets
                .iter()
                .find(|w| w.text == entry.label)
                .unwrap_or_else(|| panic!("{region}: {} is not shown", entry.label));
            assert!(
                legend.contains_rect(label.rect),
                "{region}: {} {:?} in {legend:?}",
                entry.label,
                label.rect
            );
        }
        for kind in domain::region::PlantKind::ALL {
            assert!(shows(&widgets, &kind.localize(&l10n)), "{region}: {kind:?}");
        }

        // A click anywhere on the legend picks nothing...
        for at in [
            legend.center(),
            legend.left_top() + egui::vec2(2.0, 2.0),
            legend.right_bottom() - egui::vec2(2.0, 2.0),
        ] {
            assert_eq!(
                harness.click_at(at, draw),
                vec![],
                "{region}: {at:?} is on the legend"
            );
        }
        // ...and just beside it, on the map, a click does.
        for at in [
            legend.left_center() - egui::vec2(3.0, 0.0),
            legend.left_bottom() + egui::vec2(0.0, 3.0),
        ] {
            let actions = harness.click_at(at, draw);
            assert!(
                matches!(actions[..], [UiAction::Select(_)]),
                "{region}: {at:?} is on the map: {actions:?}"
            );
        }
    }
}

#[test]
fn every_map_layer_draws() {
    let l10n = shipped_l10n();
    let game = eastern_game();
    let harness = Harness::new();
    let cache = RefCell::new(MapCache::default());
    for layer in MapLayer::all() {
        let model = map_model(
            &game,
            &l10n,
            layer,
            Selection::Cell(domain::ids::CellCoord { x: 3, y: 3 }),
        );
        let widgets =
            harness.draw(|ui| central(ui, |ui| draw_map(ui, &model, &mut cache.borrow_mut())));
        assert!(shows(&widgets, "Map"), "{layer:?}");
    }
}

#[test]
fn the_inspector_lists_what_can_be_built_on_the_selected_cell() {
    let l10n = shipped_l10n();
    let game = eastern_game();
    let region = game.session().region();
    let terrain = region.terrain();
    let water = (0..terrain.height())
        .flat_map(|y| (0..terrain.width()).map(move |x| domain::ids::CellCoord { x, y }))
        .find(|at| !terrain.get(*at).unwrap().land)
        .unwrap();
    let model = inspector_model(&game, &l10n, Selection::Cell(water)).unwrap();
    let InspectorModel::Cell(cell) = &model else {
        panic!("{model:?}");
    };
    let title = cell.title.clone();
    let harness = Harness::new();
    let draw = |ui: &mut egui::Ui| {
        egui::Panel::left("inspector")
            .show(ui, |ui| draw_inspector(ui, &model))
            .inner
    };

    let widgets = harness.draw(draw);

    assert!(shows(&widgets, &title));
    assert!(shows(&widgets, "Water"));
    assert!(shows(&widgets, "Can be built here"));
    assert!(
        shows(&widgets, "That spot is water"),
        "the reason is spelled out"
    );
    assert_eq!(
        harness.click("Close", draw),
        vec![UiAction::Select(Selection::None)]
    );
}

#[test]
fn the_build_menu_orders_the_selected_kind_on_the_selected_cell() {
    let l10n = shipped_l10n();
    let game = eastern_game();
    let region = game.session().region();
    let terrain = region.terrain();
    let cells = || {
        (0..terrain.height())
            .flat_map(|y| (0..terrain.width()).map(move |x| domain::ids::CellCoord { x, y }))
    };
    // Land that takes a peaker but not hydro: some buttons enabled, one not.
    let at = cells()
        .find(|at| {
            region
                .site_check(domain::region::PlantKind::Hydro, *at)
                .is_err()
                && region
                    .site_check(domain::region::PlantKind::Peaker, *at)
                    .is_ok()
        })
        .unwrap();
    let model = build_model(&game, &l10n, Selection::Cell(at)).unwrap();
    let harness = Harness::new();
    let draw = |ui: &mut egui::Ui| {
        egui::Panel::right("build")
            .show(ui, |ui| draw_build(ui, &model))
            .inner
    };

    let widgets = harness.draw(draw);
    let build_buttons = buttons(&widgets, "Build plant");
    assert_eq!(build_buttons.len(), 6, "one per kind");
    let kinds = domain::region::PlantKind::ALL;
    let peaker = kinds
        .iter()
        .position(|k| *k == domain::region::PlantKind::Peaker)
        .unwrap();
    let hydro = kinds
        .iter()
        .position(|k| *k == domain::region::PlantKind::Hydro)
        .unwrap();
    assert!(!build_buttons[peaker].disabled);
    assert!(build_buttons[hydro].disabled, "no river here");
    assert!(shows(&widgets, &format!("Plant at ({}, {})", at.x, at.y)));

    assert_eq!(
        harness.click_nth("Build plant", peaker, draw),
        vec![UiAction::Command(Command::BuildPlant {
            kind: domain::region::PlantKind::Peaker,
            cell: at
        })]
    );
    assert_eq!(harness.click_nth("Build plant", hydro, draw), vec![]);
}

#[test]
fn the_build_menu_picks_a_line_start_then_prices_and_orders_the_line() {
    let l10n = shipped_l10n();
    let game = eastern_game();
    let session = game.session();
    let harness = Harness::new();

    // First choose where the line starts.
    let model = build_model(&game, &l10n, Selection::None).unwrap();
    let draw = |ui: &mut egui::Ui| {
        egui::Panel::right("build")
            .show(ui, |ui| draw_build(ui, &model))
            .inner
    };
    assert!(shows(&harness.draw(draw), "Choose where the line starts."));
    assert_eq!(
        harness.click("Line from Substation", draw),
        vec![UiAction::Select(Selection::LineFrom(session.substation()))]
    );

    // From the substation, the nodes it already reaches are refused and can't be clicked...
    let model = build_model(&game, &l10n, Selection::LineFrom(session.substation())).unwrap();
    let draw = |ui: &mut egui::Ui| {
        egui::Panel::right("build")
            .show(ui, |ui| draw_build(ui, &model))
            .inner
    };
    let LineSection::End { targets, .. } = &model.line else {
        panic!("{model:?}");
    };
    let widgets = harness.draw(draw);
    assert!(shows(&widgets, "Starting at Substation."));
    let build_buttons = buttons(&widgets, "Build line");
    assert_eq!(build_buttons.len(), targets.len());
    let mut refused = 0;
    for (target, build) in targets.iter().zip(&build_buttons) {
        let LinePreview::Rejected(reason) = &target.preview else {
            assert!(!build.disabled, "{}", target.name);
            continue;
        };
        refused += 1;
        assert!(build.disabled, "{}", target.name);
        assert!(shows(&widgets, reason), "{reason} is shown");
    }
    assert!(
        refused > 0,
        "the start network is already joined to something"
    );

    // ...while from a city nothing reaches yet, every node comes with its price.
    let city = session
        .city_views()
        .into_iter()
        .find(|city| !city.connected)
        .expect("the start leaves cities unconnected");
    let model = build_model(&game, &l10n, Selection::LineFrom(city.node)).unwrap();
    let draw = |ui: &mut egui::Ui| {
        egui::Panel::right("build")
            .show(ui, |ui| draw_build(ui, &model))
            .inner
    };
    let LineSection::End { targets, .. } = &model.line else {
        panic!("{model:?}");
    };
    let widgets = harness.draw(draw);
    let nearest = &targets[0];
    let LinePreview::Quote(stats) = &nearest.preview else {
        panic!("a city with no lines can reach its neighbour: {nearest:?}");
    };
    for stat in stats {
        assert!(shows(&widgets, &stat.value), "{} is shown", stat.value);
    }
    assert_eq!(
        harness.click_nth("Build line", 0, draw),
        vec![UiAction::Command(Command::BuildLine {
            from: city.node,
            to: nearest.node
        })]
    );
    assert_eq!(
        harness.click("Cancel line", draw),
        vec![UiAction::Select(Selection::None)]
    );
}

#[test]
fn a_plant_section_without_a_cell_asks_for_one() {
    let l10n = shipped_l10n();
    let game = eastern_game();
    let model = build_model(&game, &l10n, Selection::None).unwrap();
    assert!(matches!(model.plant, PlantSection::PickCell { .. }));
    let harness = Harness::new();

    let widgets = harness.draw(|ui| {
        egui::Panel::right("build")
            .show(ui, |ui| draw_build(ui, &model))
            .inner
    });

    assert!(shows(&widgets, "Select a cell on the map"));
    assert!(buttons(&widgets, "Build plant").is_empty());
}

/// What the Planning panel remembers between frames, as the game's `UiState` and
/// `CommitmentForm` resources do. A test applies the actions a click returns, as `dispatch` does.
#[derive(Default)]
struct PlanningState {
    ui: UiState,
    form: CommitmentForm,
}

impl PlanningState {
    fn apply(&mut self, actions: &[UiAction]) {
        for action in actions {
            self.ui.apply(action);
        }
    }
}

/// The planning screen as its systems lay it out: the top bar, then the Planning panel along the
/// bottom, the inspector and the build menu claiming the sides, and the map filling what is left.
fn planning_screen(
    ui: &mut egui::Ui,
    game: &Game,
    selection: Selection,
    cache: &mut MapCache,
    planning: &mut PlanningState,
) -> Vec<UiAction> {
    let l10n = shipped_l10n();
    let bar = top_bar_model(game, &l10n);
    let panel = planning_model(game, &l10n, &planning.ui, &planning.form).unwrap();
    let inspector = inspector_model(game, &l10n, selection).unwrap();
    let build = build_model(game, &l10n, selection).unwrap();
    let map = map_model(game, &l10n, MapLayer::Terrain, selection);
    let ctx = ui.ctx().clone();
    let mut area = HudArea::default();
    let mut actions = Vec::new();
    let mut root = area.root_ui(&ctx, "top_bar");
    actions.extend(
        egui::Panel::top("top_bar")
            .show(&mut root, |ui| draw_top_bar(ui, &bar))
            .inner,
    );
    area.claim(&root);
    let mut root = area.root_ui(&ctx, "planning");
    actions.extend(
        egui::Panel::bottom("planning")
            .resizable(false)
            .exact_size(PLANNING_PANEL_HEIGHT)
            .show(&mut root, |ui| {
                draw_planning(ui, &panel, &mut planning.form)
            })
            .inner,
    );
    area.claim(&root);
    let mut root = area.root_ui(&ctx, "inspector");
    actions.extend(
        egui::Panel::left("inspector")
            .resizable(false)
            .exact_size(INSPECTOR_PANEL_WIDTH)
            .show(&mut root, |ui| draw_inspector(ui, &inspector))
            .inner,
    );
    area.claim(&root);
    let mut root = area.root_ui(&ctx, "build");
    actions.extend(
        egui::Panel::right("build")
            .resizable(false)
            .exact_size(BUILD_PANEL_WIDTH)
            .show(&mut root, |ui| draw_build(ui, &build))
            .inner,
    );
    area.claim(&root);
    let mut root = area.root_ui(&ctx, "map");
    actions.extend(
        egui::CentralPanel::default()
            .show(&mut root, |ui| draw_map(ui, &map, cache))
            .inner,
    );
    actions
}

#[test]
fn the_planning_screen_gives_the_map_the_space_the_panels_leave() {
    let game = eastern_game();
    let land = domain::ids::CellCoord { x: 60, y: 25 };
    assert!(game.session().region().terrain().get(land).unwrap().land);
    let selection = Selection::Cell(land);
    let harness = Harness::new();
    let cache = RefCell::new(MapCache::default());
    let state = RefCell::new(PlanningState::default());
    let draw = |ui: &mut egui::Ui| {
        planning_screen(
            ui,
            &game,
            selection,
            &mut cache.borrow_mut(),
            &mut state.borrow_mut(),
        )
    };

    let widgets = harness.draw(draw);

    let map_rect = widget(&widgets, "Map").rect;
    let menu = find_button(&widgets, "Menu").rect;
    let close = find_button(&widgets, "Close").rect;
    let build_button = buttons(&widgets, "Build plant")[0].rect;
    assert!(
        map_rect.top() >= menu.bottom(),
        "the map is below the top bar"
    );
    assert!(
        map_rect.left() >= close.right(),
        "the map is right of the inspector"
    );
    assert!(
        map_rect.right() <= build_button.left(),
        "the map is left of the build menu"
    );
    assert!(
        map_rect.width() > 300.0 && map_rect.height() > 300.0,
        "{map_rect:?}"
    );

    // A click on the map still reaches it, and a panel button still works, side by side.
    let terrain = game.session().region().terrain();
    let layout = MapLayout::fit(map_rect, terrain.width(), terrain.height());
    let other = domain::ids::CellCoord { x: 5, y: 5 };
    assert_eq!(
        harness.click_at(layout.cell_center(other), draw),
        vec![UiAction::Select(hit_test(
            &layout,
            layout.cell_center(other),
            &game
        ))]
    );
    assert_eq!(
        harness.click("Close", draw),
        vec![UiAction::Select(Selection::None)]
    );
}

/// Clicking around the map changes what the side panels hold, but never the map's own place: it
/// would jump under the pointer otherwise.
#[test]
fn the_map_does_not_move_when_the_selection_changes_kind() {
    let game = eastern_game();
    let session = game.session();
    let land = domain::ids::CellCoord { x: 60, y: 25 };
    let selections = [
        Selection::None,
        Selection::Cell(land),
        Selection::Plant(session.plant_views()[0].id),
        Selection::City(session.city_views()[0].id),
        Selection::LineFrom(session.substation()),
        Selection::None,
        Selection::Cell(land),
    ];
    // One harness throughout, so egui's remembered panel sizes carry from frame to frame.
    let harness = Harness::new();
    let cache = RefCell::new(MapCache::default());
    let state = RefCell::new(PlanningState::default());

    let rects: Vec<_> = selections
        .into_iter()
        .map(|selection| {
            let widgets = harness.draw(|ui| {
                planning_screen(
                    ui,
                    &game,
                    selection,
                    &mut cache.borrow_mut(),
                    &mut state.borrow_mut(),
                )
            });
            widget(&widgets, "Map").rect
        })
        .collect();

    assert!(
        rects.windows(2).all(|pair| pair[0] == pair[1]),
        "the map moved: {rects:?}"
    );
}

// ---- the planning panel ---------------------------------------------------------------------

/// The first land cell, which both shipped regions have, to select on the map.
fn some_land(game: &Game) -> CellCoord {
    let terrain = game.session().region().terrain();
    (0..terrain.height())
        .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
        .find(|at| terrain.get(*at).unwrap().land)
        .expect("the region has land")
}

/// The names of the grid's rows, top to bottom: the hour-0 cell of each row is `<name> at 00:00:
/// off` or `<name> at 00:00: committed`, and AccessKit lists widgets in no promised order.
fn row_names(widgets: &[crate::support::egui_harness::Widget]) -> Vec<String> {
    let mut cells: Vec<_> = widgets
        .iter()
        .filter_map(|w| {
            let name = w
                .text
                .strip_suffix(" at 00:00: off")
                .or_else(|| w.text.strip_suffix(" at 00:00: committed"))?;
            Some((name, w.rect))
        })
        .collect();
    cells.sort_by(|a, b| a.1.top().partial_cmp(&b.1.top()).expect("finite"));
    cells.into_iter().map(|(name, _)| name.to_owned()).collect()
}

/// The plant a row's name (as `Names` spells it) stands for.
fn plant_named(game: &Game, name: &str) -> domain::ids::PlantId {
    let l10n = shipped_l10n();
    let names = game::l10n::Names::new(&l10n, game.session());
    game.session()
        .plant_views()
        .iter()
        .find(|plant| names.plant(plant.id) == name)
        .unwrap_or_else(|| panic!("no plant is named {name:?}"))
        .id
}

/// However many warnings there are, the strip is one line: the first warning and a button for
/// the rest, which open in a popup instead of taking rows from the grid.
#[test]
fn the_warnings_strip_is_one_line_with_the_rest_behind_a_button() {
    use domain::session::{CapacityShortfallWarning, MinimumGenerationWarning};
    use domain::units::Megawatts;
    let l10n = shipped_l10n();
    let dry = ["Hydro plant 5".to_owned(), "Hydro plant 9".to_owned()];
    let model = |expanded| {
        warnings_for(
            Some(CapacityShortfallWarning {
                hour: 18,
                demand: Megawatts::new(22_000.0),
                deliverable: Megawatts::new(10_000.0),
                shortfall: Megawatts::new(12_000.0),
            }),
            Some(MinimumGenerationWarning {
                hour: 3,
                committed_minimum: Megawatts::new(3_300.0),
                net_load: Megawatts::new(3_000.0),
                excess: Megawatts::new(300.0),
            }),
            &dry,
            expanded,
            &l10n,
        )
    };
    let harness = Harness::with_screen(1024.0, 640.0);
    let open = std::cell::Cell::new(false);
    let draw = |ui: &mut egui::Ui| {
        let model = model(open.get());
        egui::Panel::bottom("planning")
            .exact_size(PLANNING_PANEL_HEIGHT)
            .show(ui, |ui| draw_warnings(ui, &model))
            .inner
    };

    // Closed: the heading, the first warning and "+3 more" share one line, and nothing else shows.
    let widgets = harness.draw(draw);
    let strip = widgets
        .iter()
        .find(|w| w.text == "Warnings")
        .expect("the heading")
        .rect;
    assert!(strip.height() < 30.0, "{strip:?}");
    assert!(shows(&widgets, "18:00: short by 12.0 GW"));
    assert!(!shows(&widgets, "committed minimum") && !shows(&widgets, "no water"));
    let more = find_button(&widgets, "+3 more").rect;
    assert!(
        (more.top() - strip.top()).abs() < 4.0,
        "on the strip's line"
    );
    assert_eq!(buttons(&widgets, "Show hour").len(), 1);

    // The button asks to open the rest.
    assert_eq!(
        harness.click("+3 more", draw),
        vec![UiAction::ToggleWarnings]
    );
    open.set(true);

    // Open: the rest are in a popup above the strip, so the strip itself did not move or grow.
    let widgets = harness.draw(draw);
    let again = widgets.iter().find(|w| w.text == "Warnings").unwrap().rect;
    assert_eq!(again, strip);
    for text in [
        "03:00: committed minimum exceeds load by 300 MW.",
        "Hydro plant 5 has no water to spare today.",
        "Hydro plant 9 has no water to spare today.",
    ] {
        assert!(shows(&widgets, text), "{text}");
    }
    assert_eq!(
        buttons(&widgets, "Show hour").len(),
        2,
        "the first's and the second's"
    );
    assert_eq!(harness.click("Hide", draw), vec![UiAction::ToggleWarnings]);
    // The popup's Show hour works like the strip's.
    assert_eq!(
        harness.click_nth("Show hour", 0, draw),
        vec![UiAction::ShowHour(Some(3))],
        "the popup sits above the strip, so its button is the first in drawing order"
    );
}

/// A planning screen of one game, drawn by one harness so egui's remembered sizes carry over.
struct PlanningScreen {
    game: Game,
    harness: Harness,
    cache: RefCell<MapCache>,
    state: RefCell<PlanningState>,
    selection: Selection,
}

impl PlanningScreen {
    fn new(game: Game, harness: Harness) -> Self {
        let selection = Selection::Cell(some_land(&game));
        Self {
            game,
            harness,
            cache: RefCell::default(),
            state: RefCell::default(),
            selection,
        }
    }

    fn draw_fn(&self) -> impl FnMut(&mut egui::Ui) -> Vec<UiAction> + '_ {
        |ui| {
            planning_screen(
                ui,
                &self.game,
                self.selection,
                &mut self.cache.borrow_mut(),
                &mut self.state.borrow_mut(),
            )
        }
    }

    /// Draws until the layout settles (egui sizes some widgets from the previous frame).
    fn widgets(&self) -> Vec<crate::support::egui_harness::Widget> {
        self.harness.draw(self.draw_fn());
        self.harness.draw(self.draw_fn())
    }

    /// Clicks the `n`th button labelled `label`, then applies the actions as `dispatch` would.
    fn click_nth(&self, label: &str, n: usize) -> Vec<UiAction> {
        let actions = self.harness.click_nth(label, n, self.draw_fn());
        self.state.borrow_mut().apply(&actions);
        actions
    }

    fn click(&self, label: &str) -> Vec<UiAction> {
        self.click_nth(label, 0)
    }
}

/// The smallest window the game allows is 1024×640. At that size the map still has room, the
/// grid has all 24 hours and the first row's controls on screen, and no panel's widgets sit on
/// another's or on the map.
#[test]
fn at_the_smallest_window_the_map_the_panels_and_the_grid_all_fit_without_overlap() {
    for region in [EASTERN, WESTERN] {
        let screen = PlanningScreen::new(game_in(region), Harness::with_screen(1024.0, 640.0));
        let screen_rect = screen.harness.screen_rect();

        let widgets = screen.widgets();

        let map = widget(&widgets, "Map").rect;
        let menu = find_button(&widgets, "Menu").rect;
        // The inspector's title heads its column; its Close button is further down, in the
        // scrolling part, and may be out of view.
        let inspector = widgets
            .iter()
            .find(|w| w.text.starts_with("Cell ("))
            .expect("the inspector's title")
            .rect;
        let build = buttons(&widgets, "Build plant")[0].rect;
        let tabs = ["Commitments", "Forecast", "Tariff"].map(|tab| find_button(&widgets, tab).rect);
        let begin = find_button(&widgets, "Start the day").rect;
        assert!(
            map.width() >= 400.0 && map.height() >= 270.0,
            "{region}: the map is {map:?}"
        );
        assert!(
            map.top() >= menu.bottom(),
            "{region}: the map is under the top bar"
        );
        assert!(
            map.left() >= inspector.right(),
            "{region}: the map is right of the inspector"
        );
        assert!(
            map.right() <= build.left(),
            "{region}: the map is left of the build menu"
        );
        assert!(
            tabs.iter()
                .chain([&begin])
                .all(|rect| map.bottom() <= rect.top()),
            "{region}: the map is above the Planning panel: {map:?} {tabs:?}"
        );

        // The grid: all 24 hour headers, and the first plant's 24 cells and controls.
        let hours: Vec<_> = (0..24)
            .map(|hour| format!("Show {hour:02}:00 in the grid"))
            .map(|label| find_button(&widgets, &label).rect)
            .collect();
        let first_cell = hours[0];
        let all_day = buttons(&widgets, "All day")[0].rect;
        let off = buttons(&widgets, "Off")[0].rect;
        let agc = buttons(&widgets, "AGC")[0].rect;
        let set = buttons(&widgets, "Set")[0].rect;
        // A new game opens on the inherited schedule: every row committed all day.
        let cell_prefix = row_names(&widgets).remove(0);
        let cells: Vec<_> = (0..24)
            .map(|hour| {
                let label = format!("{cell_prefix} at {hour:02}:00: committed");
                find_button(&widgets, &label).rect
            })
            .collect();
        let mut on_screen: Vec<egui::Rect> =
            vec![menu, inspector, build, begin, all_day, off, agc, set];
        on_screen.extend(tabs);
        on_screen.extend(&hours);
        on_screen.extend(&cells);
        for rect in &on_screen {
            assert!(
                screen_rect.contains_rect(*rect),
                "{region}: {rect:?} is off screen"
            );
        }
        assert!(
            first_cell.width() >= 12.0,
            "{region}: an hour is {first_cell:?}"
        );

        // Nothing sits on anything else: the panel's widgets and the side panels' keep clear of
        // the map and of each other.
        let mut all: Vec<(&str, egui::Rect)> = vec![
            ("map", map),
            ("menu", menu),
            ("inspector", inspector),
            ("build", build),
            ("begin", begin),
            ("all day", all_day),
            ("off", off),
            ("agc", agc),
            ("set", set),
        ];
        all.extend(tabs.iter().map(|rect| ("tab", *rect)));
        all.extend(hours.iter().map(|rect| ("header", *rect)));
        all.extend(cells.iter().map(|rect| ("cell", *rect)));
        for (i, (a_name, a)) in all.iter().enumerate() {
            for (b_name, b) in &all[i + 1..] {
                assert!(
                    !a.intersects(*b),
                    "{region}: {a_name} {a:?} overlaps {b_name} {b:?}"
                );
            }
        }

        // A row being edited grows Apply and Revert, the controls the player needs most (the day
        // waits for them), so they must fit too: inside the screen and the panel, with the row's
        // last widget (the range label) clear of the panel's right edge and its scroll bar.
        let first = row_names(&widgets).remove(0);
        screen.click(&format!("{first} at 00:00: committed"));
        let dirty = screen.widgets();
        let apply = find_button(&dirty, "Apply").rect;
        let revert = find_button(&dirty, "Revert").rect;
        let panel = egui::Rect::from_min_max(
            egui::pos2(0.0, 640.0 - PLANNING_PANEL_HEIGHT),
            egui::pos2(1024.0, 640.0),
        );
        for (name, rect) in [("Apply", apply), ("Revert", revert)] {
            assert!(
                screen_rect.contains_rect(rect) && panel.contains_rect(rect),
                "{region}: {name} {rect:?} is not inside the panel"
            );
        }
        let row_right = dirty
            .iter()
            .filter(|w| (w.rect.top() - apply.top()).abs() < 4.0 && w.rect.height() < 30.0)
            .map(|w| w.rect.right())
            .fold(0.0_f32, f32::max);
        assert!(
            row_right <= begin.right() - 16.0,
            "{region}: the dirty row reaches {row_right}, the panel's edge is {}",
            begin.right()
        );
        assert!(!apply.intersects(revert), "{region}");
        assert!(
            !apply.intersects(set) && !revert.intersects(set),
            "{region}"
        );
    }
}

/// The map's place is set by the side panels and the bottom panel, all fixed: opening a tab,
/// editing a row or tinting an hour must not move it.
#[test]
fn the_map_does_not_move_when_the_planning_panel_changes() {
    let screen = PlanningScreen::new(empty_eastern_game(), Harness::with_screen(1024.0, 640.0));
    let before = widget(&screen.widgets(), "Map").rect;
    let plant = plant_named(&screen.game, &row_names(&screen.widgets()).remove(0));

    let states = [
        vec![UiAction::SetPlanningTab(PlanningTab::Forecast)],
        vec![UiAction::SetPlanningTab(PlanningTab::Tariff)],
        vec![UiAction::SetPlanningTab(PlanningTab::Commitments)],
        vec![UiAction::ShowHour(Some(18))],
        vec![UiAction::EditCommitment {
            plant,
            hours: [true; 24],
        }],
    ];
    for actions in states {
        screen.state.borrow_mut().apply(&actions);
        let map = widget(&screen.widgets(), "Map").rect;
        assert_eq!(map, before, "after {actions:?}");
    }
}

#[test]
fn a_cell_click_edits_the_row_and_apply_sends_the_commit() {
    let screen = PlanningScreen::new(empty_eastern_game(), Harness::new());
    let widgets = screen.widgets();
    let first = row_names(&widgets).remove(0);
    let plant = plant_named(&screen.game, &first);
    assert_eq!(buttons(&widgets, "Apply").len(), 0, "nothing to apply yet");

    // Clicking hour 6 edits the row: nothing is sent to the game.
    let label = format!("{first} at 06:00: off");
    let actions = screen.click(&label);
    let mut hours = [false; 24];
    hours[6] = true;
    assert_eq!(actions, vec![UiAction::EditCommitment { plant, hours }]);

    // The row now shows the draft, with Apply and Revert beside it.
    let widgets = screen.widgets();
    assert!(shows(&widgets, &format!("{first} at 06:00: committed")));
    assert!(!find_button(&widgets, "Apply").disabled);
    assert!(!find_button(&widgets, "Revert").disabled);
    // And the day waits for it.
    assert!(find_button(&widgets, "Start the day").disabled);
    assert!(shows(&widgets, "Apply or revert your changes"));

    // Apply sends the draft as CommitPlant.
    assert_eq!(
        screen.click("Apply"),
        vec![UiAction::Command(Command::CommitPlant { plant, hours })]
    );
    assert!(screen.state.borrow().ui.pending.is_some());
}

/// `CommitmentForm` holds a field from the grid's first draw of its row, typed in or not: an
/// untouched field holds the plant's current target.
#[test]
fn the_grid_fills_each_rows_target_field_on_its_first_draw() {
    let screen = PlanningScreen::new(eastern_game(), Harness::new());
    assert_eq!(screen.state.borrow().form.targets, Default::default());

    screen.widgets();

    let state = screen.state.borrow();
    let targets = &state.form.targets;
    let commitable: Vec<_> = screen
        .game
        .session()
        .plant_views()
        .into_iter()
        .filter(is_commitable)
        .collect();
    assert_eq!(targets.len(), commitable.len(), "{targets:?}");
    for plant in commitable {
        assert_eq!(
            targets.get(&plant.id),
            Some(&game::l10n::format::plain_megawatts(plant.initial_target)),
            "{:?}",
            plant.id
        );
    }
}

#[test]
fn revert_drops_the_draft_and_the_row_shows_the_committed_pattern_again() {
    let screen = PlanningScreen::new(empty_eastern_game(), Harness::new());
    let first = row_names(&screen.widgets()).remove(0);
    screen.click(&format!("{first} at 10:00: off"));
    assert!(shows(
        &screen.widgets(),
        &format!("{first} at 10:00: committed")
    ));

    assert_eq!(
        screen.click("Revert"),
        vec![UiAction::DiscardCommitmentDraft]
    );

    let widgets = screen.widgets();
    assert!(shows(&widgets, &format!("{first} at 10:00: off")));
    assert!(buttons(&widgets, "Apply").is_empty());
    assert!(!find_button(&widgets, "Start the day").disabled);
}

#[test]
fn editing_another_row_sends_the_row_being_left() {
    let screen = PlanningScreen::new(empty_eastern_game(), Harness::new());
    let names = row_names(&screen.widgets());
    let (first, second) = (names[0].clone(), names[1].clone());
    let (a, b) = (
        plant_named(&screen.game, &first),
        plant_named(&screen.game, &second),
    );
    screen.click(&format!("{first} at 06:00: off"));

    let actions = screen.click(&format!("{second} at 07:00: off"));

    let mut sent = [false; 24];
    sent[6] = true;
    let mut started = [false; 24];
    started[7] = true;
    assert_eq!(
        actions,
        vec![
            UiAction::Command(Command::CommitPlant {
                plant: a,
                hours: sent
            }),
            UiAction::EditCommitment {
                plant: b,
                hours: started
            },
        ]
    );
}

#[test]
fn all_day_and_off_send_start_and_stop() {
    let mut screen = PlanningScreen::new(empty_eastern_game(), Harness::new());
    let plant = plant_named(&screen.game, &row_names(&screen.widgets()).remove(0));
    // Off is not offered while the plant is off already.
    assert!(find_button_nth_disabled(&screen.widgets(), "Off", 0));

    assert_eq!(
        screen.click("All day"),
        vec![UiAction::Command(Command::StartPlant { plant })]
    );

    screen.game.execute(Command::StartPlant { plant }).unwrap();
    let widgets = screen.widgets();
    assert!(
        find_button_nth_disabled(&widgets, "All day", 0),
        "it is all day already"
    );
    assert_eq!(
        screen.click_nth("Off", 0),
        vec![UiAction::Command(Command::StopPlant { plant })]
    );
}

fn find_button_nth_disabled(
    widgets: &[crate::support::egui_harness::Widget],
    label: &str,
    n: usize,
) -> bool {
    buttons(widgets, label)[n].disabled
}

#[test]
fn the_agc_switch_and_the_initial_target_field_send_their_commands() {
    let screen = PlanningScreen::new(empty_eastern_game(), Harness::new());
    let session = screen.game.session();
    let first_plant = plant_named(&screen.game, &row_names(&screen.widgets()).remove(0));
    let first = session
        .plant_views()
        .into_iter()
        .find(|plant| plant.id == first_plant)
        .unwrap();
    let spec = session.region().spec(first.kind).clone();
    let (min, capacity) = (spec.min_output().value(), spec.capacity.value());

    assert_eq!(
        screen.click_nth("AGC", 0),
        vec![UiAction::Command(Command::SetAutoFollow {
            plant: first.id,
            on: true
        })]
    );

    // An untouched field holds the current target, so there is nothing to set; the range is
    // shown beside it.
    let widgets = screen.widgets();
    assert!(buttons(&widgets, "Set")[0].disabled);
    assert!(shows(
        &widgets,
        &format!(
            "{}–{} MW",
            game::l10n::format::plain_megawatts(spec.min_output()),
            game::l10n::format::plain_megawatts(spec.capacity)
        )
    ));

    // A number out of range can't be sent, however the button is clicked.
    screen
        .state
        .borrow_mut()
        .form
        .targets
        .insert(first.id, format!("{}", capacity + 100.0));
    let widgets = screen.widgets();
    assert!(buttons(&widgets, "Set")[0].disabled);
    assert_eq!(screen.click_nth("Set", 0), vec![]);

    // One in range is sent, as written.
    let target = min + 50.0;
    screen
        .state
        .borrow_mut()
        .form
        .targets
        .insert(first.id, format!("{target}"));
    let widgets = screen.widgets();
    assert!(!buttons(&widgets, "Set")[0].disabled);
    assert_eq!(
        screen.click_nth("Set", 0),
        vec![UiAction::Command(Command::SetDispatchTarget {
            plant: first.id,
            mw: domain::units::Megawatts::new(target)
        })]
    );
}

#[test]
fn a_warning_shows_its_hour_and_start_the_day_begins_it() {
    let screen = PlanningScreen::new(empty_eastern_game(), Harness::new());
    let worst = screen
        .game
        .session()
        .capacity_shortfall_check()
        .expect("nothing is committed");
    let widgets = screen.widgets();
    assert!(shows(&widgets, "Expect a rotating outage"));
    assert!(shows(&widgets, &format!("{:02}:00: short by", worst.hour)));

    assert_eq!(
        screen.click("Show hour"),
        vec![UiAction::ShowHour(Some(worst.hour))]
    );
    assert_eq!(screen.state.borrow().ui.highlight_hour, Some(worst.hour));

    // Clicking the hour's header clears the tint.
    let header = format!("Show {:02}:00 in the grid", worst.hour);
    assert_eq!(screen.click(&header), vec![UiAction::ShowHour(None)]);
    assert_eq!(screen.state.borrow().ui.highlight_hour, None);

    assert_eq!(
        screen.click("Start the day"),
        vec![UiAction::Command(Command::BeginOperating)]
    );
}

#[test]
fn the_forecast_and_tariff_tabs_draw_for_both_shipped_regions() {
    for region in [EASTERN, WESTERN] {
        let screen = PlanningScreen::new(game_in(region), Harness::with_screen(1024.0, 640.0));

        assert_eq!(
            screen.click("Forecast"),
            vec![UiAction::SetPlanningTab(PlanningTab::Forecast)]
        );
        let widgets = screen.widgets();
        assert!(find_button(&widgets, "Start the day").rect.right() <= 1024.0);
        // The chart replaces the grid, and the map is where it was.
        assert!(buttons(&widgets, "All day").is_empty(), "{region}");
        // Highlight it too, which draws a marker line in the chart.
        screen
            .state
            .borrow_mut()
            .apply(&[UiAction::ShowHour(Some(12))]);
        screen.widgets();

        assert_eq!(
            screen.click("Tariff"),
            vec![UiAction::SetPlanningTab(PlanningTab::Tariff)]
        );
        let widgets = screen.widgets();
        for text in [
            "Current tariff",
            "Allowed return",
            "Projected rate review",
            "Reserve margin",
            "Capital held out of the rate base",
            "The December review is 11 months away.",
        ] {
            assert!(shows(&widgets, text), "{region}: {text} is not shown");
        }

        assert_eq!(
            screen.click("Commitments"),
            vec![UiAction::SetPlanningTab(PlanningTab::Commitments)]
        );
        assert!(shows(&screen.widgets(), "Warnings"));
    }
}

#[test]
fn hydro_rows_show_the_days_water_in_the_western_grid() {
    let screen = PlanningScreen::new(game_in(WESTERN), Harness::new());

    let widgets = screen.widgets();

    // Each line is a label and its text run in AccessKit's tree: count the labels.
    let rows = widgets
        .iter()
        .filter(|w| {
            w.role == egui::accesskit::Role::Label && w.text.starts_with("Water for the day:")
        })
        .count();
    assert_eq!(rows, 3, "one line for each of the three hydro plants");
    assert!(
        !shows(&widgets, "no water to spare"),
        "the shipped plants have water"
    );
}

// ---- the operating screen -------------------------------------------------------------------

/// A game an hour into its first day, with every plant that can be committed committed all day,
/// under manual control except hydro.
fn operating_game(region: &str) -> Game {
    let mut game = game_in(region);
    empty_start(&mut game);
    for plant in game.session().plant_views() {
        if is_commitable(&plant) {
            game.execute(Command::StartPlant { plant: plant.id })
                .unwrap();
        }
        // Hydro is scheduled (spec §12.3) only under AGC.
        if is_commitable(&plant) && plant.kind == domain::region::PlantKind::Hydro {
            game.execute(Command::SetAutoFollow {
                plant: plant.id,
                on: true,
            })
            .unwrap();
        }
    }
    game.execute(Command::BeginOperating).unwrap();
    game.execute(Command::AdvanceTicks { n: 60 }).unwrap();
    game
}

/// The first online plant of `kind`, as the player would find it in the list.
fn online_plant(game: &Game, kind: domain::region::PlantKind) -> domain::session::PlantView {
    game.session()
        .plant_views()
        .into_iter()
        .find(|plant| {
            plant.kind == kind
                && matches!(plant.status, domain::generation::PlantStatus::Online { .. })
        })
        .unwrap_or_else(|| panic!("an online {kind:?} plant"))
}

/// The one slider labelled `label`.
fn slider<'a>(
    widgets: &'a [crate::support::egui_harness::Widget],
    label: &str,
) -> &'a crate::support::egui_harness::Widget {
    let mut found = widgets
        .iter()
        .filter(|w| w.role == egui::accesskit::Role::Slider && w.text == label);
    match (found.next(), found.next()) {
        (Some(only), None) => only,
        _ => panic!("not exactly one slider {label:?} among {widgets:#?}"),
    }
}

#[test]
fn speed_buttons_emit_set_speed() {
    let l10n = shipped_l10n();
    let game = operating_game(EASTERN);
    let model = speed_model(&game, &l10n, SimSpeed::X1);
    let harness = Harness::new();
    let draw = |ui: &mut egui::Ui| {
        egui::Panel::top("speed")
            .show(ui, |ui| draw_speed(ui, &model))
            .inner
    };

    assert!(shows(&harness.draw(draw), "01:00"), "the time of day");
    for (label, speed) in [
        ("Pause", SimSpeed::Paused),
        ("1×", SimSpeed::X1),
        ("2×", SimSpeed::X2),
        ("4×", SimSpeed::X4),
        ("8×", SimSpeed::X8),
    ] {
        assert_eq!(
            harness.click(label, draw),
            vec![UiAction::SetSpeed(speed)],
            "{label}"
        );
    }
}

#[test]
fn target_slider_sends_once_on_release_within_bounds() {
    let l10n = shipped_l10n();
    let game = operating_game(EASTERN);
    let gas = online_plant(&game, domain::region::PlantKind::Ccgt);
    assert!(!gas.auto_follow);
    let spec = game.session().region().spec(gas.kind).clone();
    let model = plant_panel_model(&game, &l10n, Selection::Plant(gas.id));
    let PlantPanelModel::Plant(plant) = &model else {
        panic!("{model:?}");
    };
    assert!(plant.slider.enabled);
    let form = RefCell::new(PlantPanelForm::default());
    let harness = Harness::with_screen(1024.0, 640.0);
    let draw = |ui: &mut egui::Ui| {
        egui::Panel::right("operating_side")
            .exact_size(OPERATING_SIDE_WIDTH)
            .show(ui, |ui| {
                draw_plant_panel(ui, &model, &mut form.borrow_mut())
            })
            .inner
    };
    let rail = slider(&harness.draw(draw), "Set target").rect;
    let target = |mw: f64| {
        UiAction::Command(Command::SetDispatchTarget {
            plant: gas.id,
            mw: domain::units::Megawatts::new(mw),
        })
    };

    // Dragged past the right end: nothing goes while the pointer moves, and the release sends
    // the capacity once.
    let right = rail.right_center() + egui::vec2(80.0, 0.0);
    let frames = harness.drag(
        rail.center(),
        &[rail.center() + egui::vec2(10.0, 0.0), right, right],
        draw,
    );
    assert_eq!(frames.len(), 5);
    let (release, during) = frames.split_last().unwrap();
    assert!(
        during.iter().all(Vec::is_empty),
        "sent mid-drag: {during:?}"
    );
    assert_eq!(release, &vec![target(spec.capacity.value())]);

    // And past the left end, the minimum stable output.
    let left = rail.left_center() - egui::vec2(80.0, 0.0);
    let frames = harness.drag(rail.center(), &[left, left], draw);
    let sent: Vec<UiAction> = frames.into_iter().flatten().collect();
    assert_eq!(sent, vec![target(spec.min_output().value())]);

    // From the keyboard: Tab past AGC and Stop (Start is disabled) to the slider, where an arrow
    // key is one committed step, sent at once.
    for _ in 0..3 {
        assert_eq!(harness.press_key(egui::Key::Tab, draw), vec![]);
    }
    let stepped = harness.press_key(egui::Key::ArrowRight, draw);
    let [UiAction::Command(Command::SetDispatchTarget { plant, mw })] = stepped.as_slice() else {
        panic!("one target per key press: {stepped:?}");
    };
    assert_eq!(*plant, gas.id);
    assert!(
        *mw > spec.min_output() && *mw <= spec.capacity,
        "a step up from the minimum: {mw:?}"
    );
}

/// The size [`Harness::new`] draws at, for the real pass.
pub(crate) const WIDE: (f32, f32) = (1280.0, 800.0);

/// Clicks the one button labelled `label` on the settled screen `widgets`, through the real pass.
fn click_in_pass(pass: &EguiPass, app: &mut App, widgets: &[Widget], label: &str) {
    let (width, height) = WIDE;
    pass.click_at(
        app,
        width,
        height,
        find_button(widgets, label).rect.center(),
    );
}

#[test]
fn the_operating_screen_draws_every_panel_an_hour_in() {
    let (width, height) = WIDE;
    for region in [EASTERN, WESTERN] {
        let logs = TempDir::new().unwrap();
        let mut app = operating_hour_app(&logs, region);
        let pass = EguiPass::attach(&mut app);
        let widgets = pass.settled(&mut app, width, height);
        for text in [
            "01:00",
            "Frequency",
            "Imbalance",
            "Plants",
            "Select a plant in the list or on the map.",
            "Ramp warnings",
            "Incidents",
            "Map",
        ] {
            assert!(shows(&widgets, text), "{region}: {text} is not shown");
        }
        assert!(!find_button(&widgets, "Supply").disabled);

        // A row of the list selects its plant, and the panel shows it.
        let l10n = shipped_l10n();
        let online = {
            let game = &app.world().resource::<Session>().0;
            let plant = game
                .session()
                .plant_views()
                .into_iter()
                .find(|plant| {
                    matches!(plant.status, domain::generation::PlantStatus::Online { .. })
                })
                .expect("something is online");
            (
                plant.id,
                game::l10n::Names::new(&l10n, game.session()).plant(plant.id),
            )
        };
        let (plant, name) = online;
        click_in_pass(&pass, &mut app, &widgets, &name);
        assert_eq!(
            *app.world().resource::<Selection>(),
            Selection::Plant(plant)
        );
        let widgets = pass.settled(&mut app, width, height);
        assert!(shows(&widgets, "Set target"), "{region}");
        assert!(!find_button(&widgets, "AGC").disabled);
        assert!(!find_button(&widgets, "Stop").disabled);
        assert!(find_button(&widgets, "Start").disabled);
        assert!(shows(&widgets, "Marginal cost"), "{region}");

        // The hydro tab.
        click_in_pass(&pass, &mut app, &widgets, "Hydro");
        assert_eq!(*app.world().resource::<OperatingTab>(), OperatingTab::Hydro);
        let widgets = pass.settled(&mut app, width, height);
        if region == WESTERN {
            assert!(shows(&widgets, "Hours already played are frozen"));
            assert!(
                shows(&widgets, "Hydro covers everything above")
                    || shows(&widgets, "Hydro has no water to spare today"),
                "{region}"
            );
        } else {
            assert!(shows(&widgets, "No hydro plant is scheduled today."));
        }
        click_in_pass(&pass, &mut app, &widgets, "Supply");
        assert_eq!(
            *app.world().resource::<OperatingTab>(),
            OperatingTab::Supply
        );
        let widgets = pass.settled(&mut app, width, height);
        click_in_pass(&pass, &mut app, &widgets, "8×");
        assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::X8);
    }
}

/// An app an hour into its first day in `region`, run through the real plugins: every plant that
/// can be committed is committed all day, and only hydro is on AGC (it is scheduled only then).
pub(crate) fn operating_hour_app(logs: &TempDir, region: &str) -> App {
    let mut app = hud_app(logs.path());
    request(&mut app, region);
    empty_start_in(&mut app);
    let mut actions = Vec::new();
    for plant in app.world().resource::<Session>().0.session().plant_views() {
        if is_commitable(&plant) {
            actions.push(UiAction::Command(Command::StartPlant { plant: plant.id }));
            if plant.kind == domain::region::PlantKind::Hydro {
                actions.push(UiAction::Command(Command::SetAutoFollow {
                    plant: plant.id,
                    on: true,
                }));
            }
        }
    }
    actions.push(UiAction::Command(Command::BeginOperating));
    actions.push(UiAction::Command(Command::AdvanceTicks { n: 60 }));
    dispatch_in(&mut app, actions);
    update_n(&mut app, 3);
    assert_eq!(
        app.world().resource::<Session>().0.session().clock().tick(),
        60
    );
    app
}

/// Selects `selection` and opens `tab` through `dispatch`, as clicks would.
fn select_and_open(app: &mut App, selection: Selection, tab: OperatingTab) {
    dispatch_in(
        app,
        vec![UiAction::Select(selection), UiAction::SetOperatingTab(tab)],
    );
}

/// The smallest window the game allows is 1024×640. Drawn by the real systems, every Operating
/// control must be on screen and inside its own panel there, clear of the map and of each other,
/// with no horizontal scroll, and the selected plant's figures must show without scrolling.
#[test]
fn at_the_smallest_window_the_operating_controls_fit_without_overlap() {
    for region in [EASTERN, WESTERN] {
        for tab in [OperatingTab::Supply, OperatingTab::Hydro] {
            let logs = TempDir::new().unwrap();
            let mut app = operating_hour_app(&logs, region);
            let pass = EguiPass::attach(&mut app);
            let plant = online_plant(
                &app.world().resource::<Session>().0,
                domain::region::PlantKind::Ccgt,
            );
            select_and_open(&mut app, Selection::Plant(plant.id), tab);
            let screen_rect =
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0));

            let widgets = pass.settled(&mut app, 1024.0, 640.0);

            let map = widget(&widgets, "Map").rect;
            let menu = find_button(&widgets, "Menu").rect;
            let speeds = ["Pause", "1×", "2×", "4×", "8×"].map(|l| find_button(&widgets, l).rect);
            let agc = find_button(&widgets, "AGC").rect;
            let start = find_button(&widgets, "Start").rect;
            let stop = find_button(&widgets, "Stop").rect;
            let target = slider(&widgets, "Set target").rect;
            let tabs = ["Supply", "Hydro"].map(|l| find_button(&widgets, l).rect);
            let label = format!("{region} {tab:?}");

            let bottom_top = 640.0 - OPERATING_PANEL_HEIGHT;
            let side = egui::Rect::from_min_max(
                egui::pos2(1024.0 - OPERATING_SIDE_WIDTH, menu.bottom()),
                egui::pos2(1024.0, bottom_top),
            );
            let bottom = egui::Rect::from_min_max(
                egui::pos2(0.0, bottom_top),
                egui::pos2(1024.0 - OPERATING_LOG_WIDTH, 640.0),
            );
            for (name, rect) in speeds.iter().map(|r| ("speed", r)) {
                assert!(screen_rect.contains_rect(*rect), "{label}: {name} {rect:?}");
                assert!(
                    rect.top() >= menu.bottom() && rect.bottom() <= map.top(),
                    "{label}: the {name} button {rect:?} is in its bar, under the top bar"
                );
            }
            for (name, rect) in [
                ("agc", agc),
                ("start", start),
                ("stop", stop),
                ("slider", target),
            ] {
                assert!(
                    side.contains_rect(rect),
                    "{label}: {name} {rect:?} is not inside the side panel {side:?}"
                );
            }
            // Every figure of the selected plant, down to the last, is in view without scrolling.
            for figure in [
                "Output",
                "Target",
                "Ramp rate",
                "Marginal cost",
                "Energy today",
                "Closed months",
            ] {
                let row = widgets
                    .iter()
                    .find(|w| w.text == figure && w.role == egui::accesskit::Role::Label)
                    .unwrap_or_else(|| panic!("{label}: no {figure} row"))
                    .rect;
                assert!(
                    side.contains_rect(row),
                    "{label}: the {figure} row {row:?} is below the side panel {side:?}"
                );
            }
            for rect in tabs {
                assert!(
                    bottom.contains_rect(rect),
                    "{label}: a tab {rect:?} is not inside the bottom panel"
                );
            }
            assert!(
                map.width() >= 500.0 && map.height() >= 250.0,
                "{label}: the map is {map:?}"
            );
            assert!(
                map.right() <= side.left() + 0.5,
                "{label}: the map is left of the side panel"
            );
            assert!(
                map.bottom() <= bottom_top + 0.5,
                "{label}: the map is above the panel"
            );

            let mut all: Vec<(&str, egui::Rect)> = vec![
                ("map", map),
                ("menu", menu),
                ("agc", agc),
                ("start", start),
                ("stop", stop),
                ("slider", target),
            ];
            all.extend(speeds.iter().map(|rect| ("speed", *rect)));
            all.extend(tabs.iter().map(|rect| ("tab", *rect)));
            for (i, (a_name, a)) in all.iter().enumerate() {
                for (b_name, b) in &all[i + 1..] {
                    assert!(
                        !a.intersects(*b),
                        "{label}: {a_name} {a:?} overlaps {b_name} {b:?}"
                    );
                }
            }
            // No horizontal scroll: nothing reaches past the window's right edge.
            for w in &widgets {
                assert!(
                    w.rect.right() <= 1024.5,
                    "{label}: {:?} {:?} runs off the right edge",
                    w.text,
                    w.rect
                );
            }
        }
    }
}

/// The map's place is set by the bars and the fixed panels: selecting a plant, opening a tab or
/// changing speed must not move it. Drawn by the real systems.
#[test]
fn the_map_does_not_move_when_the_operating_panels_change() {
    let logs = TempDir::new().unwrap();
    let mut app = operating_hour_app(&logs, WESTERN);
    let pass = EguiPass::attach(&mut app);
    let before = widget(&pass.settled(&mut app, 1024.0, 640.0), "Map").rect;
    let plant = online_plant(
        &app.world().resource::<Session>().0,
        domain::region::PlantKind::Ccgt,
    );
    let states = [
        vec![UiAction::Select(Selection::Plant(plant.id))],
        vec![UiAction::SetOperatingTab(OperatingTab::Hydro)],
        vec![UiAction::Select(Selection::None)],
        vec![UiAction::SetOperatingTab(OperatingTab::Supply)],
        vec![UiAction::SetSpeed(SimSpeed::Paused)],
    ];
    for actions in states {
        dispatch_in(&mut app, actions.clone());
        let map = widget(&pass.settled(&mut app, 1024.0, 640.0), "Map").rect;
        assert_eq!(map, before, "after {actions:?}");
    }
}

/// The error a full disk gives `FileCommandLog`, word for word: the longest reason a player sees.
const NO_SPACE: &str = "cannot write record: No space left on device (os error 28)";

/// A command log that takes the header and then fails every command as a full disk does.
struct NoSpaceLeft;

impl CommandLogSink for NoSpaceLeft {
    fn initialize(&mut self, _: &LogHeader, _: &[u8]) -> Result<(), CommandLogError> {
        Ok(())
    }

    fn append(&mut self, _: &LogRecord) -> Result<(), CommandLogError> {
        Err(CommandLogError {
            message: NO_SPACE.to_owned(),
        })
    }
}

/// At the smallest window the recording-stopped banner with a real error must not push Menu or
/// Milestones off the screen: the bar stays one row, the banner gives way, and Menu still works.
/// Drawn by the real systems.
#[test]
fn at_the_smallest_window_the_recording_banner_leaves_menu_and_milestones_on_screen() {
    let (width, height) = (1024.0, 640.0);
    for region in [EASTERN, WESTERN] {
        let logs = TempDir::new().unwrap();
        let mut app = hud_app(logs.path());
        request(&mut app, region);
        let catalog = RonRegionCatalog::new(shipped_assets().join("regions"));
        let mut game = Game::start_new(
            &catalog,
            &RegionId::new(region),
            7,
            GameSettings::default(),
            Box::new(NoSpaceLeft),
        )
        .unwrap();
        let plant = game
            .session()
            .plant_views()
            .into_iter()
            .find(is_commitable)
            .expect("a plant to commit");
        // The game goes on; only its recording stops.
        game.execute(Command::StartPlant { plant: plant.id })
            .unwrap();
        assert!(game.log_failure().is_some(), "{region}");
        app.insert_resource(Session(game));
        let pass = EguiPass::attach(&mut app);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));

        let widgets = pass.settled(&mut app, width, height);

        let banner = widgets
            .iter()
            .find(|w| w.text.starts_with("Recording stopped"))
            .unwrap_or_else(|| panic!("{region}: the banner is shown"))
            .rect;
        let menu = find_button(&widgets, "Menu").rect;
        let milestones = find_button(&widgets, "Milestones").rect;
        for (name, rect) in [("Menu", menu), ("Milestones", milestones)] {
            assert!(
                screen.contains_rect(rect),
                "{region}: {name} {rect:?} is off the screen"
            );
            assert!(
                !rect.intersects(banner),
                "{region}: {name} {rect:?} overlaps the banner {banner:?}"
            );
        }
        assert!(
            (menu.center().y - banner.center().y).abs() < 1.0,
            "{region}: the bar is one row: {menu:?} {banner:?}"
        );

        // The banner is cut short to fit; the whole reason is in its tooltip.
        pass.ctx()
            .all_styles_mut(|style| style.interaction.tooltip_delay = 0.0);
        let hover = vec![egui::Event::PointerMoved(banner.center())];
        pass.run_with(&mut app, width, height, hover.clone());
        let hovered = pass.run_with(&mut app, width, height, hover);
        assert!(
            hovered
                .iter()
                .any(|w| w.text.contains(NO_SPACE) && w.rect.top() >= banner.bottom()),
            "{region}: a tooltip under the bar gives the whole reason"
        );

        pass.click_at(&mut app, width, height, menu.center());
        assert_eq!(
            *app.world().resource::<OpenPrompt>(),
            OpenPrompt(Some(Prompt::LeaveGame)),
            "{region}: Menu opens the leave prompt"
        );
    }
}

// ---- dispatch -------------------------------------------------------------------------------

pub(crate) fn sent<M: Message + Clone>(app: &App) -> Vec<M> {
    app.world()
        .resource::<Messages<M>>()
        .iter_current_update_messages()
        .cloned()
        .collect()
}

pub(crate) fn dispatch_in(app: &mut App, actions: Vec<UiAction>) {
    app.world_mut()
        .run_system_once(move |mut sink: ActionSink| dispatch(actions.clone(), &mut sink))
        .unwrap();
}

#[test]
fn dispatch_is_the_one_mapping_from_actions_to_messages() {
    let logs = TempDir::new().unwrap();
    let mut app = headless_app(&shipped_assets(), logs.path());
    app.init_resource::<Selection>()
        .init_resource::<MapLayer>()
        .init_resource::<OpenPrompt>()
        .init_resource::<MilestonesOpen>()
        .init_resource::<GameOverView>()
        .init_resource::<UiState>()
        .init_resource::<LastSpeed>()
        .init_resource::<OperatingTab>();
    update_n(&mut app, 3);
    let region = RegionId::new(EASTERN);
    let cell = CellCoord { x: 1, y: 2 };

    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::BeginOperating),
            UiAction::NewGame {
                region: region.clone(),
                seed: 5,
            },
            UiAction::ReturnToMenu,
            UiAction::SetSpeed(SimSpeed::X4),
            UiAction::Select(Selection::Cell(cell)),
            UiAction::SetLayer(MapLayer::Irradiance),
            UiAction::SetOperatingTab(OperatingTab::Hydro),
            UiAction::Prompt(Some(Prompt::LeaveGame)),
            UiAction::ToggleMilestones,
            UiAction::Quit,
        ],
    );

    assert_eq!(
        sent::<PlayerCommand>(&app),
        vec![PlayerCommand(Command::BeginOperating)]
    );
    assert_eq!(
        sent::<NewGameRequest>(&app),
        vec![NewGameRequest { region, seed: 5 }]
    );
    assert_eq!(sent::<ReturnToMenu>(&app), vec![ReturnToMenu]);
    assert_eq!(sent::<AppExit>(&app), vec![AppExit::Success]);
    assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::X4);
    assert_eq!(*app.world().resource::<Selection>(), Selection::Cell(cell));
    assert_eq!(*app.world().resource::<MapLayer>(), MapLayer::Irradiance);
    assert_eq!(*app.world().resource::<OperatingTab>(), OperatingTab::Hydro);
    assert_eq!(
        *app.world().resource::<LastSpeed>(),
        LastSpeed(SimSpeed::X4),
        "a running speed is remembered for Space"
    );
    assert_eq!(
        *app.world().resource::<OpenPrompt>(),
        OpenPrompt(Some(Prompt::LeaveGame))
    );
    assert_eq!(
        *app.world().resource::<MilestonesOpen>(),
        MilestonesOpen(true)
    );
    dispatch_in(&mut app, vec![UiAction::ToggleMilestones]);
    assert_eq!(
        *app.world().resource::<MilestonesOpen>(),
        MilestonesOpen(false),
        "the second click closes it"
    );
    assert_eq!(
        *app.world().resource::<GameOverView>(),
        GameOverView::Summary
    );
    dispatch_in(&mut app, vec![UiAction::ShowFinalMonth]);
    assert_eq!(
        *app.world().resource::<GameOverView>(),
        GameOverView::FinalMonth
    );
    dispatch_in(&mut app, vec![UiAction::ShowGameOverSummary]);
    assert_eq!(
        *app.world().resource::<GameOverView>(),
        GameOverView::Summary
    );
}

#[test]
fn navigating_moves_between_the_menu_screens() {
    let logs = TempDir::new().unwrap();
    let mut app = headless_app(&shipped_assets(), logs.path());
    app.init_resource::<Selection>()
        .init_resource::<MapLayer>()
        .init_resource::<OpenPrompt>()
        .init_resource::<MilestonesOpen>()
        .init_resource::<GameOverView>()
        .init_resource::<UiState>()
        .init_resource::<LastSpeed>()
        .init_resource::<OperatingTab>();
    update_n(&mut app, 3);
    let state = |app: &App| *app.world().resource::<State<AppState>>().get();
    assert_eq!(state(&app), AppState::MainMenu);

    dispatch_in(&mut app, vec![UiAction::Navigate(MenuScreen::RegionSelect)]);
    app.update();
    assert_eq!(state(&app), AppState::RegionSelect);

    dispatch_in(&mut app, vec![UiAction::Navigate(MenuScreen::MainMenu)]);
    app.update();
    assert_eq!(state(&app), AppState::MainMenu);
}
