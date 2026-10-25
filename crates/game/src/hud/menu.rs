//! The screens outside a game: the boot error, the main menu and the region picker.

use application::{RegionCatalog, RegionListing, RegionLoadError};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use super::action::{ActionSink, ButtonModel, MenuScreen, UiAction, add_enabled, button, dispatch};
use super::layout::HudArea;
use super::region_atlas::{self, RegionAtlas};
use crate::l10n::{Localize, Localizer};
use crate::sim::{Catalog, PersistAction, Persistence, default_seed};
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

/// The regions on disk, listed on entering the main menu for the new-game setup screen.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct RegionListings(pub Result<Vec<RegionListing>, RegionLoadError>);

/// New-game setup state. It survives Back to the opening menu and returning to setup,
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
    pub eyebrow: String,
    pub subtitle: String,
    pub continue_game: Option<ButtonModel>,
    pub continue_details: Option<String>,
    pub notice: Option<String>,
    pub new_game: ButtonModel,
    pub load_game: ButtonModel,
    pub quit: ButtonModel,
}

/// The opening menu offers navigation; new-game settings belong to the setup screen.
pub fn main_menu_model(persistence: &Persistence, l10n: &Localizer) -> MainMenuModel {
    let entry = persistence.continue_entry();
    let available = !persistence.frozen();
    MainMenuModel {
        continue_game: entry.map(|entry| {
            ButtonModel::new(
                l10n.text("menu-continue"),
                UiAction::Persistence(PersistAction::Continue(entry.metadata.slot.clone())),
            )
            .enabled_if(available)
        }),
        continue_details: entry.map(|entry| {
            l10n.args(
                "menu-continue-details",
                &[
                    (
                        "name",
                        super::saves::display_name(&entry.metadata, l10n).into(),
                    ),
                    ("region", l10n.text(&entry.region_name_key).into()),
                    (
                        "month",
                        crate::l10n::format::month_year(entry.summary.month).into(),
                    ),
                    ("phase", entry.summary.phase.localize(l10n).into()),
                    (
                        "time",
                        crate::l10n::format::clock(entry.summary.tick).into(),
                    ),
                ],
            )
        }),
        notice: persistence
            .error
            .as_ref()
            .map(|error| super::saves::save_error(error, l10n)),
        title: l10n.text("app-title"),
        eyebrow: l10n.text("menu-eyebrow"),
        subtitle: l10n.text("menu-subtitle"),
        new_game: ButtonModel::new(
            l10n.text("menu-new-game"),
            UiAction::Navigate(MenuScreen::RegionSelect),
        )
        .enabled_if(available),
        load_game: ButtonModel::new(
            l10n.text("save-load"),
            UiAction::Persistence(PersistAction::OpenBrowser),
        )
        .enabled_if(available),
        quit: ButtonModel::new(l10n.text("ui-quit"), UiAction::Quit),
    }
}

/// The opening menu has no new-game form state.
pub fn draw_main_menu(ui: &mut egui::Ui, model: &MainMenuModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let rect = ui.max_rect();
    super::title_scene::backdrop(ui.painter(), rect);
    let animation_time = super::title_scene::menu_elapsed(ui.ctx());
    let wide = rect.width() >= 740.0;
    let compact = rect.height() < 600.0;
    let padding = (rect.width() * 0.055).clamp(20.0, 64.0);
    let width = if wide {
        (rect.width() * 0.38).clamp(280.0, 410.0)
    } else {
        (rect.width() - padding * 2.0).min(400.0)
    };
    let left = if wide {
        rect.left() + padding
    } else {
        rect.center().x - width * 0.5
    };
    if wide {
        let artwork = egui::Rect::from_min_max(
            egui::pos2(left + width + 20.0, rect.top() + 12.0),
            rect.right_bottom() - egui::vec2(12.0, 20.0),
        );
        super::title_scene::draw(ui.painter(), artwork, animation_time);
    }
    let content = egui::Rect::from_min_max(
        egui::pos2(
            left,
            rect.top() + if compact { 20.0 } else { rect.height() * 0.1 },
        ),
        egui::pos2(left + width, rect.bottom() - 12.0),
    );
    // Artwork never sits beneath text or intercepts a menu click.
    ui.painter()
        .rect_filled(content.expand(8.0), 0.0, super::title_scene::BACKGROUND);
    ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("opening_menu_content")
            .show(ui, |ui| {
                ui.set_width(width);
                ui.label(
                    egui::RichText::new(&model.eyebrow)
                        .size(11.0)
                        .color(super::title_scene::ACCENT)
                        .strong(),
                );
                ui.add_space(if compact { 6.0 } else { 12.0 });
                let title_size = if !wide {
                    30.0
                } else if compact {
                    40.0
                } else {
                    54.0
                };
                ui.label(
                    egui::RichText::new(&model.title)
                        .size(title_size)
                        .color(super::theme::TEXT)
                        .strong(),
                );
                ui.add_space(if compact { 8.0 } else { 16.0 });
                ui.label(
                    egui::RichText::new(&model.subtitle)
                        .size(if compact { 17.0 } else { 20.0 })
                        .color(super::theme::TEXT),
                );
                ui.add_space(if compact { 18.0 } else { 28.0 });
                if let Some(notice) = &model.notice {
                    ui.label(egui::RichText::new(notice).color(super::theme::TEXT));
                    ui.add_space(8.0);
                }
                let buttons = model
                    .continue_game
                    .iter()
                    .map(|button| (button, true))
                    .chain([
                        (&model.new_game, model.continue_game.is_none()),
                        (&model.load_game, false),
                    ]);
                for (button, primary) in buttons {
                    let mut widget =
                        egui::Button::new(egui::RichText::new(&button.label).size(16.0).strong())
                            .min_size(egui::vec2(width, if primary { 46.0 } else { 40.0 }));
                    if primary {
                        widget = widget
                            .fill(egui::Color32::from_rgb(18, 78, 81))
                            .stroke(egui::Stroke::new(1.0, super::title_scene::ACCENT));
                    }
                    let response = add_enabled(ui, button.enabled, widget);
                    if primary
                        && response.enabled()
                        && ui.memory(|memory| memory.focused().is_none())
                    {
                        response.request_focus();
                    }
                    if response.clicked() {
                        actions.push(button.action.clone());
                    }
                    if primary && let Some(details) = &model.continue_details {
                        ui.label(
                            egui::RichText::new(details)
                                .size(13.0)
                                .color(super::theme::TEXT),
                        );
                        ui.add_space(5.0);
                    }
                    ui.add_space(5.0);
                }
                let response = add_enabled(
                    ui,
                    model.quit.enabled,
                    egui::Button::new(&model.quit.label)
                        .frame(false)
                        .min_size(egui::vec2(width, 28.0)),
                );
                if response.clicked() {
                    actions.push(model.quit.action.clone());
                }
            });
    });
    actions
}

pub(super) fn main_menu_screen(
    mut contexts: EguiContexts,
    area: Res<HudArea>,
    l10n: Res<Localizer>,
    mut sink: ActionSink,
) -> Result {
    let model = main_menu_model(sink.persistence(), &l10n);
    let mut ui = area.root_ui(contexts.ctx_mut()?, "main_menu");
    let mut actions = egui::CentralPanel::default()
        .show(&mut ui, |ui| draw_main_menu(ui, &model))
        .inner;
    // The first paint has been built before speculative replay is allowed to start.
    if actions.is_empty()
        && sink.persistence().screen == crate::sim::SaveScreen::Closed
        && model.continue_game.is_some()
    {
        actions.push(UiAction::Persistence(PersistAction::PreloadResume));
    }
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
    pub metadata: Option<String>,
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
    /// Accessible name of the map; not painted over its geography.
    pub map_label: String,
    pub map_note: String,
    pub land_note: String,
    pub seed_label: String,
    pub seed_invalid: String,
    pub regions: Vec<RegionChoice>,
    /// Shown instead of the list when there is nothing to pick from.
    pub empty: Option<String>,
    /// Why the regions folder itself could not be read, under `empty`.
    pub folder_error: Option<String>,
    pub back: ButtonModel,
}

/// The region picker. Every listing is a button; a broken one is disabled, named by its file and
/// shows its error, and the good ones stay playable (Review Focus 3). `seed` is `None` when the
/// setup's seed field holds no valid seed, which disables every region.
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
        map_label: l10n.text("region-select-map-label"),
        map_note: l10n.text("region-select-map-note"),
        land_note: l10n.text("region-select-land-note"),
        seed_label: l10n.text("menu-seed"),
        seed_invalid: l10n.text("menu-seed-invalid"),
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
            metadata: None,
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
            metadata: None,
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

pub fn draw_region_select(
    ui: &mut egui::Ui,
    model: &RegionSelectModel,
    form: &mut MenuForm,
) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.add_space(18.0);
    ui.label(egui::RichText::new(&model.title).size(30.0).strong());
    ui.label(&model.subtitle);
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(&model.seed_label);
        ui.add(
            egui::TextEdit::singleline(&mut form.seed)
                .desired_width(220.0)
                .font(egui::TextStyle::Monospace),
        );
    });
    let seed = form.parsed_seed();
    if seed.is_none() {
        ui.colored_label(ui.visuals().warn_fg_color, &model.seed_invalid);
    }
    // Bind clicks to this pass's edited value, never to the seed from the previous frame.
    let mut model = model.clone();
    for choice in &mut model.regions {
        choice.button.enabled = choice.problem.is_none() && seed.is_some();
        if let UiAction::NewGame { seed: current, .. } = &mut choice.button.action {
            *current = seed.unwrap_or_default();
        }
    }
    ui.add_space(12.0);
    let content_height = (ui.available_height() - 50.0).max(80.0);
    egui::ScrollArea::vertical()
        .animated(false)
        .max_height(content_height)
        .show(ui, |ui| {
            let has_map = model.regions.iter().any(|choice| choice.artwork.is_some());
            let wide = has_map && ui.available_width() >= 800.0;
            let cards = |ui: &mut egui::Ui, actions: &mut Vec<UiAction>, allow_columns: bool| {
                let gap = 14.0;
                let columns = if allow_columns && ui.available_width() >= 2.0 * 340.0 + gap {
                    2
                } else {
                    1
                };
                let card_width =
                    (ui.available_width() - gap * (columns - 1) as f32) / columns as f32;
                let mut hovered = None;
                let mut rects = Vec::new();
                egui::Grid::new("region_cards")
                    .num_columns(columns)
                    .spacing(egui::vec2(gap, gap))
                    .min_col_width(card_width)
                    .max_col_width(card_width)
                    .show(ui, |ui| {
                        for (index, choice) in model.regions.iter().enumerate() {
                            let mut title_rect = egui::Rect::NOTHING;
                            let frame = ui
                                .vertical(|ui| {
                                    ui.set_width(card_width);
                                    egui::Frame::new()
                                        .fill(egui::Color32::from_gray(32))
                                        .corner_radius(10.0)
                                        .inner_margin(14.0)
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                let (rect, _) = ui.allocate_exact_size(
                                                    egui::vec2(6.0, 28.0),
                                                    egui::Sense::hover(),
                                                );
                                                ui.painter().rect_filled(
                                                    rect,
                                                    3.0,
                                                    region_atlas::colour(choice, index),
                                                );
                                                let response = add_enabled(
                                                    ui,
                                                    choice.button.enabled,
                                                    egui::Button::new(
                                                        egui::RichText::new(&choice.button.label)
                                                            .size(18.0)
                                                            .strong(),
                                                    )
                                                    .min_size(egui::vec2(
                                                        ui.available_width(),
                                                        48.0,
                                                    ))
                                                    .wrap(),
                                                );
                                                title_rect = response.rect;
                                                if response.has_focus() {
                                                    response
                                                        .scroll_to_me(Some(egui::Align::Center));
                                                }
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
                                            if let Some(metadata) = &choice.metadata {
                                                ui.add_space(6.0);
                                                ui.label(metadata);
                                            }
                                            if let Some(problem) = &choice.problem {
                                                ui.colored_label(
                                                    ui.visuals().warn_fg_color,
                                                    problem.status.as_str(),
                                                );
                                                ui.weak(problem.detail.as_str());
                                            }
                                        })
                                })
                                .inner;
                            if frame.response.hovered() && choice.button.enabled {
                                hovered = Some(index);
                            }
                            if choice.button.enabled && choice.artwork.is_some() {
                                // Exclude the start button from the body's click targets. These
                                // pointer-only targets add no extra keyboard focus stops.
                                let r = frame.response.rect;
                                let bodies = [
                                    egui::Rect::from_min_max(
                                        r.min,
                                        egui::pos2(r.right(), title_rect.top()),
                                    ),
                                    egui::Rect::from_min_max(
                                        egui::pos2(r.left(), title_rect.bottom()),
                                        r.max,
                                    ),
                                    egui::Rect::from_min_max(
                                        egui::pos2(r.left(), title_rect.top()),
                                        title_rect.left_bottom(),
                                    ),
                                    egui::Rect::from_min_max(
                                        title_rect.right_top(),
                                        egui::pos2(r.right(), title_rect.bottom()),
                                    ),
                                ];
                                for (part, body) in bodies.into_iter().enumerate() {
                                    let response = ui
                                        .interact(
                                            body,
                                            ui.id().with(("region_body", index, part)),
                                            egui::Sense::CLICK,
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                                    if response.hovered() {
                                        hovered = Some(index);
                                    }
                                    if response.clicked() {
                                        region_atlas::focus_region(ui.ctx(), choice);
                                    }
                                }
                            }
                            rects.push(frame.response.rect);
                            if (index + 1) % columns == 0 {
                                ui.end_row();
                            }
                        }
                    });
                (hovered, rects)
            };
            if wide {
                let width = ui.available_width();
                let map_width = (width * 0.60).min((content_height - 28.0).max(80.0));
                ui.horizontal_top(|ui| {
                    let map_rect = egui::Rect::from_min_size(
                        ui.cursor().min,
                        egui::vec2(map_width, map_width / region_atlas::aspect()),
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
                            |ui| cards(ui, &mut actions, true),
                        )
                        .inner;
                    let active = ui
                        .scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(map_rect)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                            |ui| region_atlas::draw_globe(ui, &model, hovered, &mut actions),
                        )
                        .inner;
                    if let Some(index) = active {
                        ui.painter().rect_stroke(
                            rects[index],
                            10.0,
                            egui::Stroke::new(
                                2.0,
                                region_atlas::colour(&model.regions[index], index),
                            ),
                            egui::StrokeKind::Inside,
                        );
                    }
                });
            } else {
                if has_map {
                    region_atlas::draw_globe(ui, &model, None, &mut actions);
                    ui.add_space(14.0);
                }
                cards(ui, &mut actions, false);
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
    mut form: ResMut<MenuForm>,
    mut sink: ActionSink,
) -> Result {
    let mut model = region_select_model(&listings, form.parsed_seed(), &l10n);
    if let Some(atlas) = atlas {
        model = model.with_atlas(&atlas, &l10n);
    }
    let mut ui = area.root_ui(contexts.ctx_mut()?, "region_select");
    let actions = egui::CentralPanel::default()
        .show(&mut ui, |ui| draw_region_select(ui, &model, &mut form))
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
    fn menu_always_opens_setup_before_starting_a_game() {
        let model = main_menu_model(&Persistence::default(), &shipped_l10n());
        assert_eq!(
            model.new_game.action,
            UiAction::Navigate(MenuScreen::RegionSelect)
        );
        assert!(model.new_game.enabled);
    }

    #[test]
    fn an_invalid_seed_disables_region_selection_and_says_why() {
        let l10n = shipped_l10n();
        let one = RegionListings(Ok(vec![available(EAST.0, EAST.1)]));
        let form = MenuForm {
            seed: "not a number".to_owned(),
        };

        let model = region_select_model(&one, form.parsed_seed(), &l10n);

        assert!(model.regions.iter().all(|r| !r.button.enabled));
        assert_eq!(model.seed_invalid, l10n.text("menu-seed-invalid"));
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
