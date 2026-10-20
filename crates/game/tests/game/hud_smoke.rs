//! Smoke tests for the HUD's draw functions and its action dispatch.
//!
//! Each screen is drawn with the shared headless [`Harness`] against real data (the shipped
//! regions and translations, a real `Game`), and a scripted click must produce the expected
//! `UiAction`.

use std::cell::RefCell;
use std::fs;
use std::path::Path;

use application::{Command, Game, InMemoryCommandLog, RegionCatalog};
use bevy::ecs::message::{Message, Messages};
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy_egui::egui;
use domain::ids::{CellCoord, RegionId};
use domain::session::{GameOverReason, GameSettings};
use game::hud::{
    ActionSink, ButtonModel, HudArea, InspectorModel, LinePreview, LineSection, MapCache, MapLayer,
    MapLayout, MenuForm, MenuScreen, OpenPrompt, PlantSection, Prompt, RegionListings, Selection,
    ToastQueue, UiAction, boot_error_model, build_model, button, dispatch, draw_boot_error,
    draw_build, draw_game_over, draw_inspector, draw_main_menu, draw_map, draw_prompt,
    draw_region_select, draw_toasts, draw_top_bar, game_over_model_for, hit_test, inspector_model,
    main_menu_model, map_model, plant_half_size, prompt_model, region_select_model, toasts_model,
    top_bar_model,
};
use game::sim::{NewGameRequest, PlayerCommand, ReturnToMenu, SimSpeed};
use game::state::{AppState, BootError};
use infrastructure::region::RonRegionCatalog;
use tempfile::TempDir;

use crate::support::egui_harness::{
    Harness, button as find_button, buttons, central, shows, widget,
};
use crate::support::{headless_app, shipped_assets, shipped_l10n, update_n};

const EASTERN: &str = "eastern_interconnection";
const WESTERN: &str = "western_interconnection";

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

        let widgets = harness.draw(draw);
        let map = widget(&widgets, "Map");
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
        assert_eq!(
            actions,
            vec![UiAction::Select(hit_test(&layout, at, &game))]
        );
        assert!(
            matches!(actions[..], [UiAction::Select(Selection::Plant(_))]),
            "{region}: {actions:?}"
        );

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
            if covered {
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

/// The planning screen as its systems lay it out: the top bar, then the inspector and the build
/// menu claiming the sides, then the map filling what is left.
fn planning_screen(
    ui: &mut egui::Ui,
    game: &Game,
    selection: Selection,
    cache: &mut MapCache,
) -> Vec<UiAction> {
    let l10n = shipped_l10n();
    let bar = top_bar_model(game, &l10n);
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
    let mut root = area.root_ui(&ctx, "inspector");
    actions.extend(
        egui::Panel::left("inspector")
            .resizable(false)
            .exact_size(260.0)
            .show(&mut root, |ui| draw_inspector(ui, &inspector))
            .inner,
    );
    area.claim(&root);
    let mut root = area.root_ui(&ctx, "build");
    actions.extend(
        egui::Panel::right("build")
            .resizable(false)
            .exact_size(300.0)
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
    let draw = |ui: &mut egui::Ui| planning_screen(ui, &game, selection, &mut cache.borrow_mut());

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

    let rects: Vec<_> = selections
        .into_iter()
        .map(|selection| {
            let widgets =
                harness.draw(|ui| planning_screen(ui, &game, selection, &mut cache.borrow_mut()));
            widget(&widgets, "Map").rect
        })
        .collect();

    assert!(
        rects.windows(2).all(|pair| pair[0] == pair[1]),
        "the map moved: {rects:?}"
    );
}

// ---- dispatch -------------------------------------------------------------------------------

fn sent<M: Message + Clone>(app: &App) -> Vec<M> {
    app.world()
        .resource::<Messages<M>>()
        .iter_current_update_messages()
        .cloned()
        .collect()
}

fn dispatch_in(app: &mut App, actions: Vec<UiAction>) {
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
        .init_resource::<OpenPrompt>();
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
            UiAction::Prompt(Some(Prompt::LeaveGame)),
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
    assert_eq!(
        *app.world().resource::<OpenPrompt>(),
        OpenPrompt(Some(Prompt::LeaveGame))
    );
}

#[test]
fn navigating_moves_between_the_menu_screens() {
    let logs = TempDir::new().unwrap();
    let mut app = headless_app(&shipped_assets(), logs.path());
    app.init_resource::<Selection>()
        .init_resource::<MapLayer>()
        .init_resource::<OpenPrompt>();
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
