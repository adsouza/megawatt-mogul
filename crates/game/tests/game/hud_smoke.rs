//! Smoke tests for the HUD's draw functions and its action dispatch.
//!
//! Each screen is drawn in a bare `egui::Context` with no renderer, against real data (the shipped
//! regions and translations, a real `Game`). A scripted click (pointer press and release on the
//! button's rect, found through egui's AccessKit tree) must produce the expected `UiAction`.

use std::fs;
use std::path::Path;

use application::{Command, Game, InMemoryCommandLog, RegionCatalog};
use bevy::ecs::message::{Message, Messages};
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy_egui::egui::{self, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, accesskit};
use domain::ids::{CellCoord, RegionId};
use domain::session::{GameOverReason, GameSettings};
use game::hud::{
    ActionSink, MenuForm, MenuScreen, RegionListings, Selection, ToastQueue, UiAction,
    boot_error_model, dispatch, draw_boot_error, draw_game_over, draw_main_menu,
    draw_region_select, draw_toasts, draw_top_bar, game_over_model_for, main_menu_model,
    region_select_model, toasts_model, top_bar_model,
};
use game::l10n::Localizer;
use game::sim::{NewGameRequest, PlayerCommand, ReturnToMenu, SimSpeed};
use game::state::{AppState, BootError};
use infrastructure::region::RonRegionCatalog;
use tempfile::TempDir;

use crate::support::{headless_app, shipped_assets, update_n};

const EASTERN: &str = "eastern_interconnection";

// ---- a headless egui harness ----------------------------------------------------------------

/// A widget as AccessKit describes it.
#[derive(Debug)]
struct Widget {
    role: accesskit::Role,
    text: String,
    rect: Rect,
    disabled: bool,
}

struct Harness {
    ctx: egui::Context,
}

impl Harness {
    fn new() -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        Self { ctx }
    }

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 800.0))),
            events,
            ..RawInput::default()
        }
    }

    /// Runs one egui pass with `events`: the actions drawn, and every widget on screen.
    fn frame(
        &self,
        events: Vec<Event>,
        draw: &mut dyn FnMut(&mut egui::Ui) -> Vec<UiAction>,
    ) -> (Vec<UiAction>, Vec<Widget>) {
        let mut actions = Vec::new();
        let output = self
            .ctx
            .run_ui(Self::input(events), |ui| actions.extend(draw(ui)));
        let widgets = output
            .platform_output
            .accesskit_update
            .as_ref()
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| {
                        let bounds = node.bounds()?;
                        let text = node.label().or(node.value())?.to_owned();
                        Some(Widget {
                            role: node.role(),
                            text,
                            rect: Rect::from_min_max(
                                Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
                                Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
                            ),
                            disabled: node.is_disabled(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        output.drop_without_applying_deltas();
        (actions, widgets)
    }

    /// Draws once, with no input: nothing may be asked for unprompted.
    fn draw(&self, mut draw: impl FnMut(&mut egui::Ui) -> Vec<UiAction>) -> Vec<Widget> {
        let (actions, widgets) = self.frame(Vec::new(), &mut draw);
        assert_eq!(actions, vec![], "an unclicked screen asked for something");
        widgets
    }

    /// Draws once to find the button labelled `label`, then presses and releases the primary
    /// pointer button on it: the actions of those two frames.
    fn click(
        &self,
        label: &str,
        mut draw: impl FnMut(&mut egui::Ui) -> Vec<UiAction>,
    ) -> Vec<UiAction> {
        let widgets = self.draw(&mut draw);
        let at = button(&widgets, label).rect.center();
        let press = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let mut actions = self
            .frame(vec![Event::PointerMoved(at), press(true)], &mut draw)
            .0;
        actions.extend(self.frame(vec![press(false)], &mut draw).0);
        actions
    }
}

fn button<'a>(widgets: &'a [Widget], label: &str) -> &'a Widget {
    widgets
        .iter()
        .find(|widget| widget.role == accesskit::Role::Button && widget.text == label)
        .unwrap_or_else(|| panic!("no button {label:?} among {widgets:#?}"))
}

fn shows(widgets: &[Widget], text: &str) -> bool {
    widgets.iter().any(|widget| widget.text.contains(text))
}

// ---- fixtures -------------------------------------------------------------------------------

fn shipped_l10n() -> Localizer {
    Localizer::load(&shipped_assets().join("locales").join("en-US")).unwrap()
}

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

fn eastern_game() -> Game {
    let catalog = RonRegionCatalog::new(shipped_assets().join("regions"));
    Game::start_new(
        &catalog,
        &RegionId::new(EASTERN),
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap()
}

fn central(ui: &mut egui::Ui, draw: impl FnOnce(&mut egui::Ui) -> Vec<UiAction>) -> Vec<UiAction> {
    egui::CentralPanel::default().show(ui, draw).inner
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
    assert!(button(&widgets, "broken").disabled);
    assert!(!button(&widgets, "Eastern Interconnection").disabled);
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
fn the_top_bar_draws_a_real_game_and_its_menu_button_leaves() {
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
    for text in [&model.date, &model.phase, &model.cash.value] {
        assert!(shows(&widgets, text), "{text} is not shown");
    }
    assert_eq!(harness.click("Menu", draw), vec![UiAction::ReturnToMenu]);
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
    app.init_resource::<Selection>();
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
}

#[test]
fn navigating_moves_between_the_menu_screens() {
    let logs = TempDir::new().unwrap();
    let mut app = headless_app(&shipped_assets(), logs.path());
    app.init_resource::<Selection>();
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
