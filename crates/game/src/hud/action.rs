//! What the HUD asks for, and the one place those requests become messages.
//!
//! Every panel's draw function returns `Vec<UiAction>` and writes nothing itself. Its system hands
//! the actions to [`dispatch`], which is the only code that turns a click into a message, a state
//! change or a resource write.

use application::Command;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::egui;
use domain::ids::{PlantId, RegionId};

use super::map::MapLayer;
use super::operating::OperatingTab;
use super::selection::Selection;
use super::speed::LastSpeed;
use super::ui_state::{PlanningTab, UiState};
use crate::sim::{NewGameRequest, PlayerCommand, ReturnToMenu, SimSpeed};
use crate::state::AppState;

/// Something the player asked for through the HUD.
#[derive(Debug, Clone, PartialEq)]
pub enum UiAction {
    /// A command for the running game.
    Command(Command),
    /// Start a new game.
    NewGame {
        region: RegionId,
        seed: u64,
    },
    /// Leave the running game for the main menu.
    ReturnToMenu,
    SetSpeed(SimSpeed),
    Select(Selection),
    /// Colour the map by another layer.
    SetLayer(MapLayer),
    /// Open another tab of the Planning panel.
    SetPlanningTab(PlanningTab),
    /// Open another tab of the Operating panel.
    SetOperatingTab(OperatingTab),
    /// Tint an hour's column in the commitment grid (a warning's *show hour*), or clear it.
    ShowHour(Option<u8>),
    /// Open or close the list of the planning warnings past the first.
    ToggleWarnings,
    /// Change the commitment pattern being edited for a plant. Nothing reaches the game until the
    /// pattern is applied, as a `CommitPlant` command.
    EditCommitment {
        plant: PlantId,
        hours: [bool; 24],
    },
    /// Drop the pattern being edited and show the plant's committed one again.
    DiscardCommitmentDraft,
    /// Move between the menu screens. Entering the game is only ever [`UiAction::NewGame`].
    Navigate(MenuScreen),
    /// Open a confirmation prompt, or close it with `None`. The prompt's own buttons say what
    /// confirming does.
    Prompt(Option<Prompt>),
    /// Close the game.
    Quit,
}

/// The screens outside a game that the HUD can move between by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuScreen {
    MainMenu,
    RegionSelect,
}

impl From<MenuScreen> for AppState {
    fn from(screen: MenuScreen) -> Self {
        match screen {
            MenuScreen::MainMenu => Self::MainMenu,
            MenuScreen::RegionSelect => Self::RegionSelect,
        }
    }
}

/// A question the HUD asks before doing something it can't undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prompt {
    /// Leaving the running game for the menu, which drops it.
    LeaveGame,
}

/// The prompt on screen, if any. Cleared whenever a game starts or ends.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OpenPrompt(pub Option<Prompt>);

pub(super) fn close_prompt(mut prompt: ResMut<OpenPrompt>) {
    prompt.0 = None;
}

/// A button in a panel model: its text, what clicking it asks for, and whether it can be clicked.
#[derive(Debug, Clone, PartialEq)]
pub struct ButtonModel {
    pub label: String,
    pub action: UiAction,
    pub enabled: bool,
}

impl ButtonModel {
    /// An enabled button.
    pub fn new(label: String, action: UiAction) -> Self {
        Self {
            label,
            action,
            enabled: true,
        }
    }

    /// The same button, clickable only when `enabled`.
    #[must_use]
    pub fn enabled_if(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// A button that is either on or off: a tab, the AGC switch.
#[derive(Debug, Clone, PartialEq)]
pub struct ToggleModel {
    pub button: ButtonModel,
    pub on: bool,
}

/// Draws `model` as a button that shows whether it is on and, when clicked, pushes its action.
pub fn toggle_button(
    ui: &mut egui::Ui,
    model: &ToggleModel,
    actions: &mut Vec<UiAction>,
) -> egui::Response {
    let response = ui.add_enabled(
        model.button.enabled,
        egui::Button::new(model.button.label.as_str()).selected(model.on),
    );
    if response.clicked() {
        actions.push(model.button.action.clone());
    }
    response
}

/// Draws `model` as an egui button and, when it is clicked, pushes its action.
pub fn button(
    ui: &mut egui::Ui,
    model: &ButtonModel,
    actions: &mut Vec<UiAction>,
) -> egui::Response {
    let response = ui.add_enabled(model.enabled, egui::Button::new(model.label.as_str()));
    if response.clicked() {
        actions.push(model.action.clone());
    }
    response
}

/// Everything [`dispatch`] can write to. A panel system takes it as a parameter and passes it on.
#[derive(SystemParam)]
pub struct ActionSink<'w> {
    commands: MessageWriter<'w, PlayerCommand>,
    new_games: MessageWriter<'w, NewGameRequest>,
    to_menu: MessageWriter<'w, ReturnToMenu>,
    exit: MessageWriter<'w, AppExit>,
    speed: ResMut<'w, SimSpeed>,
    last_speed: ResMut<'w, LastSpeed>,
    operating_tab: ResMut<'w, OperatingTab>,
    selection: ResMut<'w, Selection>,
    layer: ResMut<'w, MapLayer>,
    prompt: ResMut<'w, OpenPrompt>,
    ui: ResMut<'w, UiState>,
    next_state: ResMut<'w, NextState<AppState>>,
}

impl ActionSink<'_> {
    /// The current selection. A panel system reads the state the sink writes through the sink:
    /// a second `Res` of the same resource beside it would conflict (Bevy error B0002).
    pub fn selection(&self) -> Selection {
        *self.selection
    }

    /// How fast the day runs now.
    pub fn speed(&self) -> SimSpeed {
        *self.speed
    }

    /// The last running speed, which Space resumes.
    pub fn last_speed(&self) -> LastSpeed {
        *self.last_speed
    }

    /// The Operating panel's open tab.
    pub fn operating_tab(&self) -> OperatingTab {
        *self.operating_tab
    }

    /// The map layer now shown.
    pub fn layer(&self) -> MapLayer {
        *self.layer
    }

    /// The prompt now open, if any.
    pub fn open_prompt(&self) -> OpenPrompt {
        *self.prompt
    }

    /// The Planning panel's state: the tab, the highlighted hour and the commitment draft.
    pub fn ui_state(&self) -> &UiState {
        &self.ui
    }
}

/// Turns the HUD's actions into messages and state changes, in order: the single mapping from a
/// click to the rest of the game.
pub fn dispatch(actions: impl IntoIterator<Item = UiAction>, sink: &mut ActionSink) {
    for action in actions {
        sink.ui.apply(&action);
        match action {
            UiAction::Command(command) => {
                sink.commands.write(PlayerCommand(command));
            }
            UiAction::NewGame { region, seed } => {
                sink.new_games.write(NewGameRequest { region, seed });
            }
            UiAction::ReturnToMenu => {
                sink.to_menu.write(ReturnToMenu);
            }
            UiAction::SetSpeed(speed) => {
                *sink.speed = speed;
                if speed != SimSpeed::Paused {
                    *sink.last_speed = LastSpeed(speed);
                }
            }
            UiAction::SetOperatingTab(tab) => *sink.operating_tab = tab,
            UiAction::Select(selection) => *sink.selection = selection,
            UiAction::SetLayer(layer) => *sink.layer = layer,
            // Written by `UiState::apply`, above.
            UiAction::SetPlanningTab(_)
            | UiAction::ShowHour(_)
            | UiAction::EditCommitment { .. }
            | UiAction::ToggleWarnings
            | UiAction::DiscardCommitmentDraft => {}
            UiAction::Navigate(screen) => sink.next_state.set(screen.into()),
            UiAction::Prompt(prompt) => sink.prompt.0 = prompt,
            UiAction::Quit => {
                sink.exit.write(AppExit::Success);
            }
        }
    }
}
