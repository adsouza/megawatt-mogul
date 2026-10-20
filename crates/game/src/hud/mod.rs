//! The egui HUD (spec §8.4). Its systems run in bevy_egui's `EguiPrimaryContextPass` schedule.
//!
//! Every panel follows one pattern:
//! - a pure presenter `fn …_model(..) -> …Model` reads the game and the [`Localizer`] and returns
//!   plain data: display strings, [`ButtonModel`]s, no egui types;
//! - a draw function `fn draw_…(ui: &mut egui::Ui, model: &…Model) -> Vec<UiAction>` lays the model
//!   out and returns what was clicked;
//! - a Bevy system glues them together in [`EguiPrimaryContextPass`] and hands the actions to
//!   [`dispatch`], the only place a click becomes a message.
//!
//! [`Localizer`]: crate::l10n::Localizer

mod action;
mod game_over;
mod layout;
mod menu;
mod prompt;
mod selection;
mod toasts;
mod top_bar;

use bevy::prelude::*;
use bevy_egui::EguiPrimaryContextPass;

pub use self::action::{
    ActionSink, ButtonModel, MenuScreen, OpenPrompt, Prompt, UiAction, button, dispatch,
};
pub use self::game_over::{GameOverModel, draw_game_over, game_over_model, game_over_model_for};
pub use self::layout::HudArea;
pub use self::menu::{
    BootErrorModel, MainMenuModel, MenuForm, RegionChoice, RegionListings, RegionProblem,
    RegionSelectModel, boot_error_model, draw_boot_error, draw_main_menu, draw_region_select,
    main_menu_model, new_game_action, region_select_model,
};
pub use self::prompt::{PromptModel, draw_prompt, prompt_after, prompt_model};
pub use self::selection::Selection;
pub use self::toasts::{
    MAX_TOASTS, TOAST_SECONDS, ToastQueue, ToastsModel, draw_toasts, toasts_model,
};
pub use self::top_bar::{Stat, TopBarModel, draw_top_bar, top_bar_model};
use crate::l10n::Localizer;
use crate::sim::{Catalog, Session, SimSet, default_seed};
use crate::state::{AppState, BootError};

/// The order panels are laid out in each egui pass. Edge panels claim screen space in order, so
/// the top bar comes before any side panel, and overlays (toasts, modals) come last.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HudSet {
    /// Resets the shared [`HudArea`].
    Begin,
    /// Full-width bars: the top bar.
    Bars,
    /// Side panels, then central panels and whole screens.
    Panels,
    /// Floating windows, toasts and modals.
    Overlays,
}

/// The egui panels: the shell screens, the top bar, toasts, prompts and the game-over modal.
pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        // bevy_egui runs the pass with `try_run_schedule`. If no system was ever added to
        // `EguiPrimaryContextPass` the schedule doesn't exist, the pass errors, and the dropped
        // egui output panics in epaint ("Dropped TexturesDelta with 1 unapplied deltas"). The
        // systems below create it; this keeps that true even if they become conditional.
        app.init_schedule(EguiPrimaryContextPass);

        let has_l10n = resource_exists::<Localizer>;
        let in_game = in_state(AppState::InGame).and_then(resource_exists::<Session>);
        app.init_resource::<Selection>()
            .init_resource::<OpenPrompt>()
            .init_resource::<ToastQueue>()
            .init_resource::<HudArea>()
            .insert_resource(MenuForm::new(default_seed()))
            .configure_sets(
                EguiPrimaryContextPass,
                (
                    HudSet::Begin,
                    HudSet::Bars,
                    HudSet::Panels,
                    HudSet::Overlays,
                )
                    .chain(),
            )
            .add_systems(
                OnEnter(AppState::MainMenu),
                menu::list_regions.run_if(resource_exists::<Catalog>),
            )
            .add_systems(
                OnEnter(AppState::InGame),
                (selection::reset_selection, action::close_prompt),
            )
            .add_systems(
                OnExit(AppState::InGame),
                (
                    menu::reseed_menu,
                    selection::reset_selection,
                    action::close_prompt,
                ),
            )
            .add_systems(
                Update,
                (
                    toasts::collect_toasts.after(SimSet::Apply).run_if(has_l10n),
                    prompt::close_prompt_when_over
                        .after(SimSet::Apply)
                        .run_if(in_game.clone()),
                ),
            )
            .add_systems(
                EguiPrimaryContextPass,
                (
                    layout::reset_hud_area.in_set(HudSet::Begin),
                    menu::boot_error_screen
                        .in_set(HudSet::Panels)
                        .run_if(in_state(AppState::Boot).and_then(resource_exists::<BootError>)),
                    menu::main_menu_screen.in_set(HudSet::Panels).run_if(
                        in_state(AppState::MainMenu)
                            .and_then(has_l10n)
                            .and_then(resource_exists::<RegionListings>),
                    ),
                    menu::region_select_screen.in_set(HudSet::Panels).run_if(
                        in_state(AppState::RegionSelect)
                            .and_then(has_l10n)
                            .and_then(resource_exists::<RegionListings>),
                    ),
                    // All in-game phases (spec §8.4), so gated by `AppState` only.
                    top_bar::top_bar_panel
                        .in_set(HudSet::Bars)
                        .run_if(in_game.clone().and_then(has_l10n)),
                    game_over::game_over_modal
                        .in_set(HudSet::Overlays)
                        .run_if(in_game.and_then(has_l10n)),
                    prompt::prompt_overlay
                        .in_set(HudSet::Overlays)
                        .run_if(has_l10n.and_then(|open: Res<OpenPrompt>| open.0.is_some())),
                    toasts::toasts_overlay
                        .in_set(HudSet::Overlays)
                        .run_if(has_l10n),
                ),
            );
    }
}

/// Fixtures for the presenter tests: the shipped translations and a real game.
#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};

    use application::{CommandLogError, CommandLogSink, Game, LogHeader, LogRecord};
    use domain::ids::RegionId;
    use domain::session::GameSettings;
    use infrastructure::region::RonRegionCatalog;

    use crate::l10n::{LOCALE, Localizer};

    /// The workspace's shipped `assets/`. Tests only read from it.
    pub fn shipped_assets() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
    }

    pub fn shipped_l10n() -> Localizer {
        Localizer::load(&shipped_assets().join("locales").join(LOCALE))
            .unwrap_or_else(|error| panic!("the shipped translations must load: {error}"))
    }

    /// A new eastern game, seed 7, in Planning, recording to `sink`.
    pub fn eastern_game(sink: Box<dyn CommandLogSink>) -> Game {
        let catalog = RonRegionCatalog::new(shipped_assets().join("regions"));
        Game::start_new(
            &catalog,
            &RegionId::new("eastern_interconnection"),
            7,
            GameSettings::default(),
            sink,
        )
        .unwrap_or_else(|error| panic!("the shipped eastern region must start: {error}"))
    }

    /// A command log that takes the header and then fails every command, like a full disk.
    pub struct FullDisk;

    impl CommandLogSink for FullDisk {
        fn initialize(&mut self, _: &LogHeader, _: &[u8]) -> Result<(), CommandLogError> {
            Ok(())
        }

        fn append(&mut self, _: &LogRecord) -> Result<(), CommandLogError> {
            Err(CommandLogError {
                message: "disk full".to_owned(),
            })
        }
    }
}
