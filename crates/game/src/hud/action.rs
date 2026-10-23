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
use domain::time::SimMonth;
use domain::units::Megawatts;

use super::game_over::GameOverView;
use super::map::{LegendChoice, MapLayer};
use super::milestones::MilestonesOpen;
use super::operating::OperatingTab;
use super::selection::Selection;
use super::speed::LastSpeed;
use super::theme;
use super::ui_state::{PlanningTab, UiState};
use crate::sim::{
    NewGameRequest, PersistAction, Persistence, PlayerCommand, ReturnToMenu, SessionGeneration,
    SimSpeed,
};
use crate::state::AppState;

/// Something the player asked for through the HUD.
#[derive(Debug, Clone, PartialEq)]
pub enum UiAction {
    World(crate::world::WorldAction),
    Persistence(PersistAction),
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
    /// Show the map legend, or hide it: the player's choice, for the rest of the game.
    ShowLegend(bool),
    /// Open another tab of the Planning panel.
    SetPlanningTab(PlanningTab),
    /// Open another tab of the Operating panel.
    SetOperatingTab(OperatingTab),
    SelectSupplyMonth(SimMonth),
    SetSupplyChartLayout(super::SupplyChartLayout),
    ShowReviewSupply(bool),
    /// Tint an hour's column in the commitment grid (a warning's *show hour*), or clear it.
    ShowHour(Option<u8>),
    /// Open or close the list of the planning warnings past the first.
    ToggleWarnings,
    /// Open or close the Milestones panel.
    ToggleMilestones,
    /// From the game-over modal, show the review of the month that ended the game.
    ShowFinalMonth,
    /// From that review, go back to the game-over modal.
    ShowGameOverSummary,
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Prompt {
    /// Leaving the running game for the menu, which drops it.
    LeaveGame,
    /// Starting a day the forecast says the committed fleet can't cover (spec §16.4). It holds
    /// the figures the player saw when they clicked *Start the day*: the worst hour and the
    /// shortfall then, from `capacity_shortfall_check()`.
    ShortfallStart { hour: u8, shortfall: Megawatts },
}

/// The prompt on screen, if any. Cleared whenever a game starts or ends.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
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
    let response = add_enabled(
        ui,
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
    let response = add_enabled(ui, model.enabled, egui::Button::new(model.label.as_str()));
    if response.clicked() {
        actions.push(model.action.clone());
    }
    response
}

/// Adds `widget`, usable only when `enabled` and when `ui` is; a disabled one is drawn with
/// [`theme::disabled_visuals`], so it looks unavailable. Use this, not `Ui::add_enabled`: the
/// theme turns egui's own disabled fade off, so that would draw it like an enabled one (clippy's
/// `disallowed-methods`, in `clippy.toml`, says so everywhere else).
#[allow(clippy::disallowed_methods)]
pub fn add_enabled(ui: &mut egui::Ui, enabled: bool, widget: impl egui::Widget) -> egui::Response {
    if enabled && ui.is_enabled() {
        return ui.add(widget);
    }
    // The visuals are swapped in place rather than in a child `Ui`, so the widget keeps its id.
    let visuals = ui.visuals().clone();
    *ui.visuals_mut() = theme::disabled_visuals(&visuals);
    let response = ui.add_enabled(false, widget);
    *ui.visuals_mut() = visuals;
    response
}

/// Everything [`dispatch`] can write to. A panel system takes it as a parameter and passes it on.
#[derive(SystemParam)]
pub struct ActionSink<'w> {
    world: ResMut<'w, crate::world::WorldControls>,
    commands: MessageWriter<'w, PlayerCommand>,
    persistence: ResMut<'w, Persistence>,
    generation: Res<'w, SessionGeneration>,
    log_dir: Option<Res<'w, crate::paths::LogDir>>,
    new_games: MessageWriter<'w, NewGameRequest>,
    to_menu: MessageWriter<'w, ReturnToMenu>,
    exit: MessageWriter<'w, AppExit>,
    speed: ResMut<'w, SimSpeed>,
    last_speed: ResMut<'w, LastSpeed>,
    operating_tab: ResMut<'w, OperatingTab>,
    selection: ResMut<'w, Selection>,
    layer: ResMut<'w, MapLayer>,
    legend: ResMut<'w, LegendChoice>,
    prompt: ResMut<'w, OpenPrompt>,
    milestones: ResMut<'w, MilestonesOpen>,
    game_over_view: ResMut<'w, GameOverView>,
    ui: ResMut<'w, UiState>,
    next_state: ResMut<'w, NextState<AppState>>,
}

impl ActionSink<'_> {
    pub fn world(&self) -> &crate::world::WorldControls {
        &self.world
    }

    pub fn persistence(&self) -> &Persistence {
        &self.persistence
    }

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

    /// Whether the player chose to show the map legend.
    pub fn legend_choice(&self) -> LegendChoice {
        *self.legend
    }

    /// The prompt now open, if any.
    pub fn open_prompt(&self) -> OpenPrompt {
        *self.prompt
    }

    /// Whether the Milestones panel is open.
    pub fn milestones_open(&self) -> MilestonesOpen {
        *self.milestones
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
            UiAction::World(action) => sink.world.apply(action),
            UiAction::Persistence(action) => {
                let generation = sink.generation.0;
                let dir = sink.log_dir.as_ref().map(|d| d.0.clone());
                sink.persistence
                    .request(action, generation, &mut sink.speed, dir);
            }
            UiAction::Command(command) => {
                if !sink.persistence.frozen() {
                    sink.commands
                        .write(PlayerCommand(command, sink.generation.0));
                }
            }
            UiAction::NewGame { region, seed } => {
                if sink.persistence.frozen() {
                    continue;
                }
                sink.new_games.write(NewGameRequest { region, seed });
            }
            UiAction::ReturnToMenu => {
                sink.to_menu.write(ReturnToMenu);
            }
            UiAction::SetSpeed(speed) => {
                if sink.persistence.frozen() {
                    continue;
                }
                *sink.speed = speed;
                if speed != SimSpeed::Paused {
                    *sink.last_speed = LastSpeed(speed);
                }
            }
            UiAction::SetOperatingTab(tab) => *sink.operating_tab = tab,
            UiAction::SelectSupplyMonth(_)
            | UiAction::ShowReviewSupply(_)
            | UiAction::SetSupplyChartLayout(_) => {}
            UiAction::Select(selection) => *sink.selection = selection,
            UiAction::SetLayer(layer) => {
                *sink.layer = layer;
                sink.world.resource_mode = true;
                sink.world.overlay = true;
            }
            UiAction::ShowLegend(open) => *sink.legend = LegendChoice(Some(open)),
            // Written by `UiState::apply`, above.
            UiAction::SetPlanningTab(_)
            | UiAction::ShowHour(_)
            | UiAction::EditCommitment { .. }
            | UiAction::ToggleWarnings
            | UiAction::DiscardCommitmentDraft => {}
            UiAction::Navigate(screen) => {
                sink.persistence.discard_preload();
                sink.next_state.set(screen.into());
            }
            UiAction::Prompt(Some(Prompt::LeaveGame)) if sink.persistence.enabled() => {
                sink.persistence.request(
                    PersistAction::OpenLeave,
                    sink.generation.0,
                    &mut sink.speed,
                    None,
                );
            }
            UiAction::Prompt(prompt) => sink.prompt.0 = prompt,
            UiAction::ToggleMilestones => sink.milestones.0 = !sink.milestones.0,
            UiAction::ShowFinalMonth => *sink.game_over_view = GameOverView::FinalMonth,
            UiAction::ShowGameOverSummary => *sink.game_over_view = GameOverView::Summary,
            UiAction::Quit => {
                sink.persistence.discard_preload();
                sink.exit.write(AppExit::Success);
            }
        }
    }
}
