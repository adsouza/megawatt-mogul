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
mod build;
mod chart_style;
mod city_card;
mod commitment;
mod construction;
mod forecast_chart;
mod frequency;
mod game_over;
mod hydro_panel;
mod incidents;
mod inspector;
mod layout;
mod map;
mod menu;
mod milestones;
mod operating;
mod planning;
mod planning_warnings;
mod plant_list;
mod plant_panel;
mod prompt;
mod ramp_warnings;
mod region_atlas;
mod review;
pub use region_atlas::RegionAtlas;
mod saves;
mod selection;
pub use saves::{SaveForm, SaveRowModel, SavesModel, draw_saves, save_error, saves_model};
mod speed;
mod supply_chart;
mod tariff;
pub mod theme;
mod toasts;
mod top_bar;
mod ui_state;

use bevy::prelude::*;
use bevy_egui::EguiPrimaryContextPass;

pub use self::action::{
    ActionSink, ButtonModel, MenuScreen, OpenPrompt, Prompt, ToggleModel, UiAction, button,
    dispatch, toggle_button,
};
pub use self::build::{
    BUILD_PANEL_WIDTH, BuildModel, LinePreview, LineSection, LineTarget, PlantRow, PlantSection,
    build_model, draw_build,
};
pub use self::city_card::{CityCardModel, MOST_CITY_STATS, city_card_model, draw_city_card};
pub use self::commitment::{
    CELL_WIDTH, CommitmentModel, CommitmentRow, HourCell, HourHeader, KindGroup, NAME_WIDTH,
    ROW_HEIGHT, TargetModel, commitment_model, draw_commitment, has_unsaved_draft,
};
pub use self::construction::{
    ConstructionCardModel, ConstructionModel, ConstructionRow, Project, construction_card_model,
    construction_model, construction_tab_label, draw_construction, draw_construction_card,
    in_service_month,
};
pub use self::forecast_chart::{
    ForecastModel, StackModel, draw_forecast_chart, forecast_model, stacked_committed_capacity,
};
pub use self::frequency::{
    DialModel, DialZone, FrequencyModel, HIGH_COLOR, LOW_COLOR, NORMAL_COLOR, OutageModel,
    SHEDDING_COLOR, band_color, draw_frequency, frequency_model, frequency_model_for,
};
pub use self::game_over::{
    GameOverModel, GameOverView, GameOverViewShown, draw_game_over, game_over_model,
    game_over_model_for,
};
pub use self::hydro_panel::{
    HYDRO_TEXT_WIDTH, HydroModel, HydroRow, draw_hydro, hydro_model, hydro_model_for, hydro_shade,
};
pub use self::incidents::{
    IncidentsModel, MAX_INCIDENTS, draw_incidents, incidents_for, incidents_model,
};
pub use self::inspector::{
    CellInspector, INSPECTOR_PANEL_WIDTH, InspectorModel, SuitabilityRow, draw_inspector,
    inspector_model, suitability_row,
};
pub use self::layout::{
    BOTTOM_PANEL_MAX_SHARE, Edge, HudArea, MAP_MIN_SIZE, OuterPanel, SIDE_PANEL_MAX_SHARE,
    panel_max,
};
pub use self::map::{
    CityMarker, GLYPH_MIN_FONT, LEGEND_COLUMNS, LEGEND_CORNERS, LEGEND_INSET, LabelAnchor,
    LegendChoice, LegendEntry, LegendSwatch, LineMarker, MapCache, MapLayer, MapLayout, MapModel,
    MapObjects, MonthsLeftLabel, PlacedLabel, PlantMarker, Rgb, SUITABLE, TerrainKey, UNSUITABLE,
    WATER, cell_color, city_radius, draw_map, glyph_font_size, hit_test, kind_color,
    legend_columns, legend_corner, legend_placement, legend_rect, legend_toggle, map_model,
    months_left_labels, pick, pickable_marks, plant_half_size, terrain_image,
};
pub use self::menu::{
    BootErrorModel, MainMenuModel, MenuForm, RegionChoice, RegionListings, RegionProblem,
    RegionSelectModel, boot_error_model, draw_boot_error, draw_main_menu, draw_region_select,
    main_menu_model, new_game_action, region_select_model,
};
pub use self::milestones::{
    MILESTONES_WIDTH, MilestoneRow, MilestonesModel, MilestonesOpen, draw_milestones,
    milestone_row, milestones_model,
};
pub use self::operating::{
    FREQUENCY_GAUGE_HEIGHT, OPERATING_LOG_WIDTH, OPERATING_PANEL_HEIGHT, OPERATING_SIDE_WIDTH,
    OperatingContent, OperatingPanelModel, OperatingSideModel, OperatingTab, SelectedObject,
    draw_operating_panel, draw_operating_side, operating_panel_model, operating_side_min_height,
    operating_side_model,
};
pub use self::planning::{
    PLANNING_PANEL_HEIGHT, PlanningContent, PlanningModel, draw_planning, planning_model,
};
pub use self::planning_warnings::{
    WarningKind, WarningRow, WarningsModel, draw_warnings, warnings_for, warnings_model,
};
pub use self::plant_list::{KindPlants, is_commitable, kinds_in_merit_order, plants_by_kind};
pub use self::plant_panel::{
    MOST_PLANT_STATS, PLANT_LIST_MIN_ROWS, PlantListModel, PlantListRow, PlantPanelForm,
    PlantPanelModel, SelectedPlant, TargetSlider, draw_plant_list, draw_plant_panel,
    plant_details_height, plant_list_min_height, plant_list_model, plant_panel_model,
};
pub use self::prompt::{PromptModel, draw_prompt, prompt_after, prompt_in_phase, prompt_model};
pub use self::ramp_warnings::{
    RampWarningsModel, draw_ramp_warnings, ramp_warnings_for, ramp_warnings_model,
};
pub use self::review::{
    MeritRow, REVIEW_MARGIN, REVIEW_WIDTH, ReviewModel, draw_review, review_applies, review_model,
    review_model_for, review_rect,
};
pub use self::selection::Selection;
pub use self::speed::{LastSpeed, SpeedModel, draw_speed, space_action, speed_model};
pub use self::supply_chart::{
    MARGINAL_COST_COLOR, SERVED_COLOR, SupplyModel, SupplyStack, draw_supply_chart, supply_model,
    todays_points,
};
pub use self::tariff::{TariffModel, draw_tariff, months_until_review, tariff_model};
pub use self::toasts::{
    MAX_TOASTS, TOAST_SECONDS, ToastQueue, ToastsModel, draw_toasts, toasts_model,
};
pub use self::top_bar::{Stat, TopBarModel, draw_top_bar, top_bar_model};
pub use self::ui_state::{CommitDraft, CommitmentForm, PendingCommit, PlanningTab, UiState};
use domain::session::DayPhase;

use crate::l10n::Localizer;
use crate::sim::{Catalog, Session, SimSet, default_seed};
use crate::state::{AppState, BootError, PhaseState};

/// The order panels are laid out in each egui pass. Edge panels claim screen space in order, so
/// the top bar comes before any side panel, and overlays (toasts, modals) come last.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HudSet {
    /// Installs the verified text palette before any UI is created.
    Theme,
    /// Resets the shared [`HudArea`].
    Begin,
    /// Full-width bars: the top bar, then (Operating) the speed controls.
    Bars,
    /// Side panels, then central panels and whole screens.
    Panels,
    /// Floating windows, toasts and modals.
    Overlays,
}

/// The egui panels: the shell screens, the top bar, toasts, prompts, the Milestones panel and the
/// game-over modal; the planning screen's map, cell inspector, build menu and bottom panel
/// (commitment grid, forecast chart, tariff); the operating screen's speed controls, side panel
/// (frequency gauge, plant list, selected plant) and bottom panel (supply chart, hydro schedule,
/// ramp warnings, incidents); and the Review modal over the dimmed map.
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
        // The map draws in every in-game phase: in Review it is the dimmed diorama behind the modal.
        let on_the_map = in_game.clone().and_then(has_l10n);
        app.init_resource::<Selection>()
            .init_resource::<MapLayer>()
            .init_resource::<LegendChoice>()
            .init_resource::<MapCache>()
            .init_resource::<OpenPrompt>()
            .init_resource::<MilestonesOpen>()
            .init_resource::<GameOverView>()
            .init_resource::<GameOverViewShown>()
            .init_resource::<UiState>()
            .init_resource::<CommitmentForm>()
            .init_resource::<LastSpeed>()
            .init_resource::<OperatingTab>()
            .init_resource::<PlantPanelForm>()
            .init_resource::<ToastQueue>()
            .init_resource::<HudArea>()
            .init_resource::<SaveForm>()
            .init_resource::<saves::PersistenceShown>()
            .add_systems(
                EguiPrimaryContextPass,
                saves::snapshot.in_set(HudSet::Begin),
            )
            .add_systems(
                EguiPrimaryContextPass,
                saves::saves_overlay
                    .in_set(HudSet::Overlays)
                    .run_if(resource_exists::<Localizer>),
            )
            .insert_resource(MenuForm::new(default_seed()))
            .configure_sets(
                EguiPrimaryContextPass,
                (
                    HudSet::Theme,
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
                (
                    selection::reset_selection,
                    saves::reset_form,
                    map::reset_layer,
                    map::reset_legend,
                    map::reset_cache,
                    action::close_prompt,
                    milestones::close_milestones,
                    game_over::reset_game_over_view,
                    ui_state::reset_ui_state,
                    speed::reset_last_speed,
                    operating::reset_operating_tab,
                    plant_panel::reset_plant_form,
                    toasts::clear_toasts,
                ),
            )
            .add_systems(
                OnExit(AppState::InGame),
                (
                    menu::reseed_menu,
                    selection::reset_selection,
                    saves::reset_form,
                    map::reset_layer,
                    map::reset_legend,
                    map::reset_cache,
                    action::close_prompt,
                    milestones::close_milestones,
                    game_over::reset_game_over_view,
                    ui_state::reset_ui_state,
                    speed::reset_last_speed,
                    operating::reset_operating_tab,
                    plant_panel::reset_plant_form,
                    toasts::clear_toasts,
                ),
            )
            // A highlighted hour or an unapplied edit belongs to the month being planned.
            .add_systems(OnExit(PhaseState::Planning), ui_state::reset_ui_state)
            .add_systems(
                Update,
                (
                    toasts::collect_toasts.after(SimSet::Apply).run_if(has_l10n),
                    prompt::close_stale_prompt
                        .after(SimSet::Apply)
                        .run_if(in_game.clone()),
                    ui_state::settle_commit
                        .after(SimSet::Apply)
                        .run_if(in_game.clone()),
                    // The driver reads the speed at the next fixed step.
                    speed::pause_on_space.run_if(
                        in_game
                            .clone()
                            .and_then(in_state(PhaseState::Operating))
                            .and_then(resource_exists::<ButtonInput<KeyCode>>),
                    ),
                ),
            )
            .add_systems(
                EguiPrimaryContextPass,
                (
                    theme::apply_theme.in_set(HudSet::Theme),
                    layout::reset_hud_area.in_set(HudSet::Begin),
                    game_over::snapshot_game_over_view.in_set(HudSet::Begin),
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
                    speed::speed_bar
                        .in_set(HudSet::Bars)
                        .after(top_bar::top_bar_panel)
                        .run_if(
                            in_game
                                .clone()
                                .and_then(has_l10n)
                                .and_then(in_phase(DayPhase::Operating)),
                        ),
                    review::review_modal
                        .in_set(HudSet::Overlays)
                        .run_if(saves::other_overlays)
                        .run_if(
                            in_game
                                .clone()
                                .and_then(has_l10n)
                                .and_then(in_phase(DayPhase::Review)),
                        ),
                    milestones::milestones_overlay
                        .in_set(HudSet::Overlays)
                        .run_if(saves::other_overlays)
                        .run_if(
                            in_game
                                .clone()
                                .and_then(has_l10n)
                                .and_then(|open: Res<MilestonesOpen>| open.0),
                        ),
                    game_over::game_over_modal
                        .in_set(HudSet::Overlays)
                        .run_if(saves::other_overlays)
                        .run_if(in_game.and_then(has_l10n)),
                    prompt::prompt_overlay
                        .in_set(HudSet::Overlays)
                        .run_if(saves::other_overlays)
                        .run_if(has_l10n.and_then(|open: Res<OpenPrompt>| open.0.is_some())),
                    toasts::toasts_overlay
                        .in_set(HudSet::Overlays)
                        .run_if(has_l10n),
                ),
            )
            // The planning and operating screens: each phase's bottom panel claims the full width
            // first, then its side panels claim their share, then the map fills what is left.
            // Planning has the commitments, forecast and tariff along the bottom and the
            // inspector and build menu at the sides; Operating has the supply chart, hydro
            // schedule, ramp warnings and incidents along the bottom and the frequency gauge and
            // plants on the right. Every panel holds its size (its opening size, or where the
            // player dragged it), so the map doesn't move when the selection changes or a tab
            // opens.
            .add_systems(
                EguiPrimaryContextPass,
                (
                    planning::planning_panel
                        .run_if(in_phase(DayPhase::Planning).and_then(on_the_map.clone())),
                    inspector::inspector_panel
                        .run_if(in_phase(DayPhase::Planning).and_then(on_the_map.clone())),
                    build::build_panel
                        .run_if(in_phase(DayPhase::Planning).and_then(on_the_map.clone())),
                    operating::operating_panel
                        .run_if(in_phase(DayPhase::Operating).and_then(on_the_map.clone())),
                    operating::operating_side
                        .run_if(in_phase(DayPhase::Operating).and_then(on_the_map.clone())),
                    map::map_panel.run_if(on_the_map),
                )
                    .chain()
                    .in_set(HudSet::Panels),
            );
    }
}

/// Whether the running game's day is in `phase`: the domain's phase, not [`PhaseState`].
///
/// The egui panels gate on this. The domain's phase changes in `Update`, as the command runs, and
/// the presenters read it; [`PhaseState`] mirrors it only at the next frame's state transition.
/// Gating the panels on the mirror would leave one egui pass in which the systems of the old
/// phase run but their presenters already see the new one and draw nothing. `PhaseState` stays
/// for what belongs to entering and leaving a phase (`OnEnter`/`OnExit` hooks).
fn in_phase(phase: DayPhase) -> impl Fn(Option<Res<Session>>) -> bool + Clone {
    move |session: Option<Res<Session>>| {
        session.is_some_and(|session| session.0.session().phase() == phase)
    }
}

/// Fixtures for the presenter tests: the shipped translations and a real game.
#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};

    use application::{Command, CommandLogError, CommandLogSink, Game, LogHeader, LogRecord};
    use domain::events::DomainEvent;
    use domain::ids::{CellCoord, LineId, NodeId, PlantId, RegionId};
    use domain::region::PlantKind;
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

    /// A new game in the shipped region `region`, seed 7, in Planning, recording to `sink`.
    pub fn shipped_game(region: &str, sink: Box<dyn CommandLogSink>) -> Game {
        game_in_dir(&shipped_assets().join("regions"), region, sink)
    }

    fn shipped_game_in(dir: &Path, region: &str) -> Game {
        game_in_dir(
            dir,
            region,
            Box::new(application::InMemoryCommandLog::default()),
        )
    }

    fn game_in_dir(dir: &Path, region: &str, sink: Box<dyn CommandLogSink>) -> Game {
        let catalog = RonRegionCatalog::new(dir.to_path_buf());
        Game::start_new(
            &catalog,
            &RegionId::new(region),
            7,
            GameSettings::default(),
            sink,
        )
        .unwrap_or_else(|error| panic!("the shipped region {region} must start: {error}"))
    }

    /// A new game in the shipped western region after `edit` has rewritten its RON, seed 7, in
    /// Planning. For a case the shipped data never reaches, such as a tiny load.
    pub fn edited_western_game(edit: impl FnOnce(String) -> String) -> Game {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        let source = shipped_assets()
            .join("regions")
            .join("western_interconnection.ron");
        let text = std::fs::read_to_string(&source).expect("the shipped region reads");
        std::fs::write(dir.path().join("western_interconnection.ron"), edit(text))
            .expect("the edited region writes");
        // The game holds the loaded region in memory, so the directory can go once it started.
        shipped_game_in(dir.path(), "western_interconnection")
    }

    /// The text of a region with every city's base load set to `mw`.
    pub fn flat_load(text: String, mw: f64) -> String {
        text.lines()
            .map(|line| match line.find("base_load_mw: ") {
                Some(at) => {
                    let from = at + "base_load_mw: ".len();
                    let to = from + line[from..].find([',', ')']).expect("the field ends");
                    format!("{}{mw:?}{}", &line[..from], &line[to..])
                }
                None => line.to_owned(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// A new eastern game, seed 7, in Planning, recording to `sink`.
    pub fn eastern_game(sink: Box<dyn CommandLogSink>) -> Game {
        shipped_game("eastern_interconnection", sink)
    }

    /// Undoes the inherited schedule (spec §16.3) as a player would in Planning: `StopPlant`
    /// (the all-off pattern) and AGC off on every dispatchable, commissioned plant. Recreates the
    /// empty start of the tests written for a fleet left offline.
    pub fn empty_start(game: &mut Game) {
        let plants: Vec<PlantId> = game
            .session()
            .plant_views()
            .iter()
            .filter(|plant| crate::hud::is_commitable(plant))
            .map(|plant| plant.id)
            .collect();
        for plant in plants {
            game.execute(Command::StopPlant { plant })
                .expect("stop plant in Planning");
            game.execute(Command::SetAutoFollow { plant, on: false })
                .expect("turn AGC off");
        }
    }

    /// Orders a plant of `kind` (Planning only) on the `nth` cell, in row order, where the region
    /// allows one and no plant stands yet. Returns the new plant.
    pub fn order_plant(game: &mut Game, kind: PlantKind, nth: usize) -> PlantId {
        let session = game.session();
        let region = session.region();
        let taken: Vec<CellCoord> = session.plant_views().iter().map(|p| p.cell).collect();
        let (w, h) = (region.terrain().width(), region.terrain().height());
        let cell = (0..h)
            .flat_map(|y| (0..w).map(move |x| CellCoord { x, y }))
            .filter(|at| region.site_check(kind, *at).is_ok() && !taken.contains(at))
            .nth(nth)
            .unwrap_or_else(|| panic!("a site for {kind:?}"));
        let events = game
            .execute(Command::BuildPlant { kind, cell })
            .expect("a plant can be ordered in Planning");
        events
            .iter()
            .find_map(|event| match event {
                DomainEvent::PlantConstructionStarted { plant, .. } => Some(*plant),
                _ => None,
            })
            .expect("the plant's construction starts")
    }

    /// Orders the quickest line from a city the start leaves unconnected (Planning only).
    /// Returns the line and its build time in months.
    pub fn order_short_line(game: &mut Game) -> (LineId, u32) {
        order_line(game, 1)
    }

    /// Orders the quickest line that takes at least `months` to build, from a city the start
    /// leaves unconnected (Planning only). Returns the line and its build time in months.
    pub fn order_line(game: &mut Game, months: u32) -> (LineId, u32) {
        let session = game.session();
        let nodes: Vec<NodeId> = session
            .city_views()
            .iter()
            .map(|c| c.node)
            .chain(session.plant_views().iter().map(|p| p.node))
            .collect();
        let (took, from, to) = session
            .city_views()
            .iter()
            .filter(|city| !city.connected)
            .flat_map(|city| nodes.iter().map(move |to| (city.node, *to)))
            .filter_map(|(from, to)| {
                session
                    .line_quote(from, to)
                    .ok()
                    .map(|quote| (quote.months, from, to))
            })
            .filter(|(took, _, _)| *took >= months)
            .min()
            .expect("an unconnected city can be reached");
        let events = game
            .execute(Command::BuildLine { from, to })
            .expect("a line can be ordered in Planning");
        let line = events
            .iter()
            .find_map(|event| match event {
                DomainEvent::LineConstructionStarted { line } => Some(*line),
                _ => None,
            })
            .expect("the line's construction starts");
        (line, took)
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
