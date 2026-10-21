//! The screens outside a game: the boot error, the main menu and the region picker.

use application::{RegionCatalog, RegionListing, RegionLoadError, auto_select_region};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use super::action::{ActionSink, ButtonModel, MenuScreen, UiAction, add_enabled, button, dispatch};
use super::layout::HudArea;
use super::region_atlas::{self, RegionAtlas};
use crate::l10n::{Localize, Localizer};
use crate::sim::{Catalog, default_seed};
use crate::state::BootError;

// ---- boot error ---------------------------------------------------------------------------

/// The boot error screen's text when the translations themselves failed to load. The only
/// hard-coded English in the game: with no [`Localizer`] there is nothing else to show.
const FALLBACK_BOOT_ERROR_TITLE: &str = "The game could not start";
const FALLBACK_QUIT: &str = "Quit";

#[derive(Debug, Clone, PartialEq)]
pub struct BootErrorModel {
    pub title: String,
    /// The error itself. It comes from the loaders as plain text, not as a translation key.
    pub message: String,
    pub quit: ButtonModel,
}

/// The boot error screen. `l10n` is `None` when the failure was loading the translations.
pub fn boot_error_model(error: &BootError, l10n: Option<&Localizer>) -> BootErrorModel {
    let (title, quit) = match l10n {
        Some(l10n) => (l10n.text("boot-error-title"), l10n.text("ui-quit")),
        None => (
            FALLBACK_BOOT_ERROR_TITLE.to_owned(),
            FALLBACK_QUIT.to_owned(),
        ),
    };
    BootErrorModel {
        title,
        message: error.0.clone(),
        quit: ButtonModel::new(quit, UiAction::Quit),
    }
}

pub fn draw_boot_error(ui: &mut egui::Ui, model: &BootErrorModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.25);
        ui.heading(model.title.as_str());
        ui.add_space(12.0);
        ui.label(model.message.as_str());
        ui.add_space(12.0);
        button(ui, &model.quit, &mut actions);
    });
    actions
}

pub(super) fn boot_error_screen(
    mut contexts: EguiContexts,
    area: Res<HudArea>,
    error: Res<BootError>,
    l10n: Option<Res<Localizer>>,
    mut sink: ActionSink,
) -> Result {
    let model = boot_error_model(&error, l10n.as_deref());
    let mut ui = area.root_ui(contexts.ctx_mut()?, "boot_error");
    let actions = egui::CentralPanel::default()
        .show(&mut ui, |ui| draw_boot_error(ui, &model))
        .inner;
    dispatch(actions, &mut sink);
    Ok(())
}

// ---- main menu ----------------------------------------------------------------------------

/// The regions on disk, listed on entering the main menu. Both menu screens read this one
/// listing, so the menu's auto-skip and the picker always agree.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct RegionListings(pub Result<Vec<RegionListing>, RegionLoadError>);

/// What the player has typed into the menu. It survives the trip to the region picker and back,
/// and is re-seeded after each game.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct MenuForm {
    pub seed: String,
}

impl MenuForm {
    pub fn new(seed: u64) -> Self {
        Self {
            seed: seed.to_string(),
        }
    }

    /// The typed seed, if it is a whole number that fits in a `u64`.
    pub fn parsed_seed(&self) -> Option<u64> {
        self.seed.trim().parse().ok()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MainMenuModel {
    pub title: String,
    pub seed_label: String,
    /// Shown under the seed field when it holds something that isn't a seed.
    pub seed_error: Option<String>,
    pub new_game: ButtonModel,
    pub load_game: ButtonModel,
    pub quit: ButtonModel,
}

/// What *New game* does: start the only region straight away when there is exactly one and it is
/// available (spec §2.6), and otherwise open the picker, which also explains a broken or empty
/// regions folder.
pub fn new_game_action(
    listings: &Result<Vec<RegionListing>, RegionLoadError>,
    seed: u64,
) -> UiAction {
    match listings.as_deref().ok().and_then(auto_select_region) {
        Some(region) => UiAction::NewGame {
            region: region.clone(),
            seed,
        },
        None => UiAction::Navigate(MenuScreen::RegionSelect),
    }
}

pub fn main_menu_model(
    listings: &RegionListings,
    form: &MenuForm,
    l10n: &Localizer,
) -> MainMenuModel {
    let seed = form.parsed_seed();
    // A disabled button still needs an action; it is never sent.
    let new_game = new_game_action(&listings.0, seed.unwrap_or_default());
    MainMenuModel {
        title: l10n.text("app-title"),
        seed_label: l10n.text("menu-seed"),
        seed_error: seed.is_none().then(|| l10n.text("menu-seed-invalid")),
        new_game: ButtonModel::new(l10n.text("menu-new-game"), new_game).enabled_if(seed.is_some()),
        load_game: ButtonModel::new(
            l10n.text("save-load"),
            UiAction::Persistence(crate::sim::PersistAction::OpenBrowser),
        ),
        quit: ButtonModel::new(l10n.text("ui-quit"), UiAction::Quit),
    }
}

/// The main menu. The seed field edits `form` directly: it is widget state, not a game action.
pub fn draw_main_menu(
    ui: &mut egui::Ui,
    model: &MainMenuModel,
    form: &mut MenuForm,
) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.25);
        ui.heading(model.title.as_str());
        ui.add_space(24.0);
        // Label above field: a horizontal row would not centre in this column.
        ui.label(model.seed_label.as_str());
        ui.add(egui::TextEdit::singleline(&mut form.seed).desired_width(200.0));
        if let Some(error) = &model.seed_error {
            ui.colored_label(ui.visuals().warn_fg_color, error.as_str());
        }
        ui.add_space(12.0);
        button(ui, &model.new_game, &mut actions);
        button(ui, &model.load_game, &mut actions);
        button(ui, &model.quit, &mut actions);
    });
    actions
}

pub(super) fn main_menu_screen(
    mut contexts: EguiContexts,
    area: Res<HudArea>,
    l10n: Res<Localizer>,
    listings: Res<RegionListings>,
    mut form: ResMut<MenuForm>,
    mut sink: ActionSink,
) -> Result {
    let model = main_menu_model(&listings, &form, &l10n);
    let mut ui = area.root_ui(contexts.ctx_mut()?, "main_menu");
    let actions = egui::CentralPanel::default()
        .show(&mut ui, |ui| draw_main_menu(ui, &model, &mut form))
        .inner;
    dispatch(actions, &mut sink);
    Ok(())
}

pub(super) fn list_regions(mut commands: Commands, catalog: Res<Catalog>) {
    let listings = RegionListings(catalog.0.list());
    commands.insert_resource(RegionAtlas::load(&catalog.0, &listings));
    commands.insert_resource(listings);
}

pub(super) fn reseed_menu(mut form: ResMut<MenuForm>) {
    *form = MenuForm::new(default_seed());
}

// ---- region select ------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct RegionChoice {
    pub button: ButtonModel,
    pub description: Option<String>,
    pub detail: Option<String>,
    pub(super) artwork: Option<std::sync::Arc<region_atlas::Artwork>>,
    /// Why the region can't be played, when it can't: a status word and the localised error.
    pub problem: Option<RegionProblem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegionProblem {
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegionSelectModel {
    pub title: String,
    pub subtitle: String,
    pub map_title: String,
    pub map_note: String,
    pub regions: Vec<RegionChoice>,
    /// Shown instead of the list when there is nothing to pick from.
    pub empty: Option<String>,
    /// Why the regions folder itself could not be read, under `empty`.
    pub folder_error: Option<String>,
    pub back: ButtonModel,
}

/// The region picker. Every listing is a button; a broken one is disabled, named by its file and
/// shows its error, and the good ones stay playable (Review Focus 3). `seed` is `None` when the
/// menu's seed field holds no valid seed, which disables every region.
pub fn region_select_model(
    listings: &RegionListings,
    seed: Option<u64>,
    l10n: &Localizer,
) -> RegionSelectModel {
    let (regions, folder_error) = match &listings.0 {
        Ok(listings) => (
            listings
                .iter()
                .map(|listing| region_choice(listing, seed, l10n))
                .collect(),
            None,
        ),
        Err(error) => (Vec::new(), Some(error.localize(l10n))),
    };
    let empty = regions.is_empty().then(|| l10n.text("region-select-empty"));
    RegionSelectModel {
        title: l10n.text("region-select-title"),
        subtitle: l10n.text("region-select-subtitle"),
        map_title: l10n.text("region-select-map-title"),
        map_note: l10n.text("region-select-map-note"),
        regions,
        empty,
        folder_error,
        back: ButtonModel::new(
            l10n.text("ui-back"),
            UiAction::Navigate(MenuScreen::MainMenu),
        ),
    }
}

fn region_choice(listing: &RegionListing, seed: Option<u64>, l10n: &Localizer) -> RegionChoice {
    match &listing.entry {
        Ok(summary) => RegionChoice {
            description: match summary.id.as_str() {
                "eastern_interconnection" => Some(l10n.text("region-select-east-description")),
                "western_interconnection" => Some(l10n.text("region-select-west-description")),
                _ => None,
            },
            detail: None,
            artwork: None,
            button: ButtonModel::new(
                l10n.text(&summary.name_key),
                UiAction::NewGame {
                    region: summary.id.clone(),
                    seed: seed.unwrap_or_default(),
                },
            )
            .enabled_if(seed.is_some()),
            problem: None,
        },
        Err(error) => RegionChoice {
            description: None,
            detail: None,
            artwork: None,
            // A broken file has no name to translate: its id (the file name) is all there is.
            button: ButtonModel::new(
                listing.id.as_str().to_owned(),
                UiAction::NewGame {
                    region: listing.id.clone(),
                    seed: seed.unwrap_or_default(),
                },
            )
            .enabled_if(false),
            problem: Some(RegionProblem {
                status: l10n.text("region-select-unavailable"),
                detail: error.localize(l10n),
            }),
        },
    }
}

pub fn draw_region_select(ui: &mut egui::Ui, model: &RegionSelectModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.add_space(18.0);
    ui.label(egui::RichText::new(&model.title).size(30.0).strong());
    ui.label(&model.subtitle);
    ui.add_space(18.0);
    let content_height = (ui.available_height() - 50.0).max(80.0);
    egui::ScrollArea::vertical()
        .max_height(content_height)
        .show(ui, |ui| {
            let has_map = model.regions.iter().any(|choice| choice.artwork.is_some());
            let wide = has_map && ui.available_width() >= 800.0;
            let cards = |ui: &mut egui::Ui, actions: &mut Vec<UiAction>| {
                let mut hovered = None;
                let mut rects = Vec::new();
                for (index, choice) in model.regions.iter().enumerate() {
                    let frame = egui::Frame::new()
                        .fill(egui::Color32::from_gray(32))
                        .corner_radius(10.0)
                        .inner_margin(14.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(6.0, 28.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter()
                                    .rect_filled(rect, 3.0, region_atlas::colour(index));
                                let response = add_enabled(
                                    ui,
                                    choice.button.enabled,
                                    egui::Button::new(
                                        egui::RichText::new(&choice.button.label)
                                            .size(18.0)
                                            .strong(),
                                    )
                                    .min_size(egui::vec2(ui.available_width(), 48.0))
                                    .wrap(),
                                );
                                if response.hovered() || response.has_focus() {
                                    hovered = Some(index);
                                }
                                if response.clicked() {
                                    actions.push(choice.button.action.clone());
                                }
                            });
                            if let Some(description) = &choice.description {
                                ui.add_space(10.0);
                                ui.label(description);
                            }
                            if let Some(detail) = &choice.detail {
                                ui.add_space(10.0);
                                ui.weak(detail);
                            }
                            if let Some(problem) = &choice.problem {
                                ui.colored_label(
                                    ui.visuals().warn_fg_color,
                                    problem.status.as_str(),
                                );
                                ui.weak(problem.detail.as_str());
                            }
                        });
                    if frame.response.hovered() && choice.button.enabled {
                        hovered = Some(index);
                    }
                    rects.push(frame.response.rect);
                    ui.add_space(14.0);
                }
                (hovered, rects)
            };
            if wide {
                let width = ui.available_width();
                let map_width = (width * 0.66).min(content_height * 1.6);
                ui.horizontal_top(|ui| {
                    let map_rect = egui::Rect::from_min_size(
                        ui.cursor().min,
                        egui::vec2(map_width, map_width / 1.6),
                    );
                    // Draw the cards first so either a card or its map area can highlight both.
                    let right = egui::Rect::from_min_max(
                        map_rect.right_top() + egui::vec2(20.0, 0.0),
                        egui::pos2(ui.max_rect().right(), ui.max_rect().bottom()),
                    );
                    let (hovered, rects) = ui
                        .scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(right)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                            |ui| cards(ui, &mut actions),
                        )
                        .inner;
                    let active = ui
                        .scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(map_rect)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                            |ui| region_atlas::draw_map(ui, model, hovered, &mut actions),
                        )
                        .inner;
                    if let Some(index) = active {
                        ui.painter().rect_stroke(
                            rects[index],
                            10.0,
                            egui::Stroke::new(2.0, region_atlas::colour(index)),
                            egui::StrokeKind::Inside,
                        );
                    }
                });
            } else {
                if has_map {
                    region_atlas::draw_map(ui, model, None, &mut actions);
                    ui.add_space(14.0);
                }
                cards(ui, &mut actions);
            }
            if let Some(empty) = &model.empty {
                ui.label(empty.as_str());
            }
            if let Some(error) = &model.folder_error {
                ui.weak(error.as_str());
            }
        });
    ui.add_space(16.0);
    button(ui, &model.back, &mut actions);
    actions
}

pub(super) fn region_select_screen(
    mut contexts: EguiContexts,
    area: Res<HudArea>,
    l10n: Res<Localizer>,
    listings: Res<RegionListings>,
    atlas: Option<Res<RegionAtlas>>,
    form: Res<MenuForm>,
    mut sink: ActionSink,
) -> Result {
    let mut model = region_select_model(&listings, form.parsed_seed(), &l10n);
    if let Some(atlas) = atlas {
        model = model.with_atlas(&atlas, &l10n);
    }
    let mut ui = area.root_ui(contexts.ctx_mut()?, "region_select");
    let actions = egui::CentralPanel::default()
        .show(&mut ui, |ui| draw_region_select(ui, &model))
        .inner;
    dispatch(actions, &mut sink);
    Ok(())
}

#[cfg(test)]
mod tests {
    use application::RegionSummary;
    use domain::ids::RegionId;

    use super::*;
    use crate::hud::test_support::shipped_l10n;

    fn available(id: &str, name_key: &str) -> RegionListing {
        RegionListing {
            id: RegionId::new(id),
            entry: Ok(RegionSummary {
                id: RegionId::new(id),
                name_key: name_key.to_owned(),
            }),
        }
    }

    fn broken(id: &str) -> RegionListing {
        RegionListing {
            id: RegionId::new(id),
            entry: Err(RegionLoadError::Parse {
                region: RegionId::new(id),
                message: "expected `(`".to_owned(),
            }),
        }
    }

    const EAST: (&str, &str) = ("eastern_interconnection", "region-eastern-interconnection");
    const WEST: (&str, &str) = ("western_interconnection", "region-western-interconnection");

    #[test]
    fn menu_auto_skips_with_one_available_region() {
        let l10n = shipped_l10n();
        let one = RegionListings(Ok(vec![available(EAST.0, EAST.1)]));

        let model = main_menu_model(&one, &MenuForm::new(42), &l10n);

        assert_eq!(
            model.new_game.action,
            UiAction::NewGame {
                region: RegionId::new(EAST.0),
                seed: 42
            }
        );
        assert!(model.new_game.enabled);
    }

    #[test]
    fn menu_opens_the_picker_unless_exactly_one_region_is_available() {
        let picker = UiAction::Navigate(MenuScreen::RegionSelect);
        let cases = [
            Ok(vec![available(EAST.0, EAST.1), available(WEST.0, WEST.1)]),
            Ok(vec![broken(EAST.0)]),
            Ok(vec![available(EAST.0, EAST.1), broken(WEST.0)]),
            Ok(vec![]),
            Err(RegionLoadError::Io {
                region: RegionId::new("assets/regions"),
                message: "no such directory".to_owned(),
            }),
        ];

        for listings in cases {
            assert_eq!(new_game_action(&listings, 1), picker, "{listings:?}");
        }
    }

    #[test]
    fn an_invalid_seed_disables_new_game_and_says_why() {
        let l10n = shipped_l10n();
        let one = RegionListings(Ok(vec![available(EAST.0, EAST.1)]));
        let form = MenuForm {
            seed: "not a number".to_owned(),
        };

        let model = main_menu_model(&one, &form, &l10n);

        assert!(!model.new_game.enabled);
        assert_eq!(model.seed_error, Some(l10n.text("menu-seed-invalid")));
        assert_eq!(MenuForm::new(7).parsed_seed(), Some(7));
        let padded = MenuForm {
            seed: " 12 ".to_owned(),
        };
        assert_eq!(padded.parsed_seed(), Some(12));
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn region_select_shows_broken_entries_disabled() {
        let l10n = shipped_l10n();
        let listings = RegionListings(Ok(vec![available(EAST.0, EAST.1), broken(WEST.0)]));

        let model = region_select_model(&listings, Some(9), &l10n);

        let [good, bad] = model.regions.as_slice() else {
            panic!("expected two choices: {:?}", model.regions);
        };
        assert!(good.button.enabled);
        assert_eq!(good.button.label, l10n.text(EAST.1));
        assert_eq!(
            good.button.action,
            UiAction::NewGame {
                region: RegionId::new(EAST.0),
                seed: 9
            }
        );
        assert_eq!(good.problem, None);

        assert!(!bad.button.enabled);
        assert_eq!(bad.button.label, WEST.0);
        let problem = bad.problem.as_ref().expect("a broken region says why");
        assert_eq!(problem.status, l10n.text("region-select-unavailable"));
        assert!(
            problem.detail.contains("expected `(`"),
            "{}",
            problem.detail
        );
        assert_eq!(model.empty, None);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn an_empty_or_unreadable_regions_folder_shows_a_message_and_back() {
        let l10n = shipped_l10n();
        let back = UiAction::Navigate(MenuScreen::MainMenu);

        let empty = region_select_model(&RegionListings(Ok(vec![])), Some(1), &l10n);
        assert!(empty.regions.is_empty());
        assert_eq!(empty.empty, Some(l10n.text("region-select-empty")));
        assert_eq!(empty.folder_error, None);
        assert_eq!(empty.back.action, back);

        let unreadable = RegionListings(Err(RegionLoadError::Io {
            region: RegionId::new("assets/regions"),
            message: "no such directory".to_owned(),
        }));
        let missing = region_select_model(&unreadable, Some(1), &l10n);
        assert!(missing.regions.is_empty());
        assert_eq!(missing.empty, Some(l10n.text("region-select-empty")));
        let detail = missing.folder_error.expect("the folder error is shown");
        assert!(detail.contains("no such directory"), "{detail}");
        assert_eq!(missing.back.action, back);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn an_invalid_seed_disables_every_region() {
        let l10n = shipped_l10n();
        let listings = RegionListings(Ok(vec![available(EAST.0, EAST.1)]));

        let model = region_select_model(&listings, None, &l10n);

        assert!(model.regions.iter().all(|choice| !choice.button.enabled));
        assert!(model.back.enabled);
    }

    #[test]
    fn the_boot_error_screen_falls_back_to_english_without_translations() {
        let error = BootError("bad.ftl has syntax errors".to_owned());

        let bare = boot_error_model(&error, None);
        assert_eq!(bare.title, FALLBACK_BOOT_ERROR_TITLE);
        assert_eq!(bare.quit.label, FALLBACK_QUIT);
        assert_eq!(bare.message, error.0);
        assert_eq!(bare.quit.action, UiAction::Quit);

        let l10n = shipped_l10n();
        let localised = boot_error_model(&error, Some(&l10n));
        assert_eq!(localised.title, l10n.text("boot-error-title"));
        assert_eq!(localised.quit.label, l10n.text("ui-quit"));
        assert_eq!(localised.message, error.0);
    }
}
