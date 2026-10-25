//! The simulation bridge (spec §8.1): the running [`Game`] as a resource, the command funnel, the
//! fixed-step driver and the one-way phase mirror.
//!
//! - Everything the player (or the driver) does reaches the game as a [`PlayerCommand`] message.
//!   [`apply_player_commands`] runs them through the `Game` pipeline, in order, and turns the
//!   results into [`DomainEventMessage`]s and [`CommandFailed`]s.
//! - [`mirror_phase`] copies the domain's phase into [`PhaseState`]. Nothing else sets it.
//! - [`drive_ticks`] sends `AdvanceTicks { n: speed }` from `FixedUpdate`, at 4 Hz.

mod driver;
mod new_game;
mod persistence;
pub use persistence::{PersistAction, Persistence, SaveScreen, SessionGeneration};

use application::{Command, CommandError, Game};
use bevy::prelude::*;
use domain::events::DomainEvent;
use domain::ids::RegionId;
use domain::session::DayPhase;
use infrastructure::region::RonRegionCatalog;

use self::driver::TicksInFlight;
pub use self::driver::{FIXED_STEP_HZ, drive_ticks};
use self::new_game::start_new_game;
pub use self::new_game::{default_seed, utc_date_time, utc_stamp};
use crate::crash::CrashContext;
use crate::paths::AssetRoot;
use crate::state::{AppState, BootChecklistAppExt, BootError, PhaseState, finish_boot};

/// The running game. A new game inserts it as it moves to [`AppState::InGame`] (the state
/// changes at the start of the next frame), and leaving `InGame` removes it.
#[derive(Resource)]
pub struct Session(pub Game);

/// The region files under `<AssetRoot>/regions`, inserted at boot. The only catalog the game
/// builds: new games and the region picker both read this one.
#[derive(Resource, Debug, Clone)]
pub struct Catalog(pub RonRegionCatalog);

/// A command for the running game, from the HUD or the driver.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct PlayerCommand(pub Command, pub u64);

/// A command (or a new game) that was refused. The session is unchanged.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct CommandFailed(pub CommandError);

/// A domain event from a command that succeeded. `TickCompleted` is not re-emitted: the HUD reads
/// ticks from the projections.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct DomainEventMessage(pub DomainEvent);

/// Start a new game in `region` with `seed`, from the menu.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct NewGameRequest {
    pub region: RegionId,
    pub seed: u64,
}

/// Leave the game for the main menu, dropping the session.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReturnToMenu;

/// How fast the day runs: the ticks (game minutes) per fixed step.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SimSpeed {
    #[default]
    Paused,
    X1,
    X2,
    X4,
    X8,
    X16,
    X32,
}

impl SimSpeed {
    /// The `n` of each `AdvanceTicks` the driver sends.
    pub fn ticks(self) -> u32 {
        match self {
            Self::Paused => 0,
            Self::X1 => 1,
            Self::X2 => 2,
            Self::X4 => 4,
            Self::X8 => 8,
            Self::X16 => 16,
            Self::X32 => 32,
        }
    }
}

/// The systems that run player commands through the game.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimSet {
    Apply,
}

impl From<DayPhase> for PhaseState {
    fn from(phase: DayPhase) -> Self {
        match phase {
            DayPhase::Planning => Self::Planning,
            DayPhase::Operating => Self::Operating,
            DayPhase::Review => Self::Review,
        }
    }
}

/// The `Session` resource, its messages and the systems that drive it.
pub struct SimulationPlugin;

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        let in_game = in_state(AppState::InGame).and_then(resource_exists::<Session>);
        let in_menus = in_state(AppState::MainMenu).or_else(in_state(AppState::RegionSelect));
        app.add_message::<PlayerCommand>()
            .add_message::<CommandFailed>()
            .add_message::<DomainEventMessage>()
            .add_message::<NewGameRequest>()
            .add_message::<ReturnToMenu>()
            .init_resource::<SimSpeed>()
            .init_resource::<TicksInFlight>()
            .init_resource::<Persistence>()
            .init_resource::<SessionGeneration>()
            // `main.rs` inserts the context its panic hook reads; tests get an unconnected one.
            .init_resource::<CrashContext>()
            .add_systems(
                Update,
                persistence::initialise_store
                    .before(finish_boot)
                    .before(start_new_game)
                    .before(SimSet::Apply),
            )
            .add_systems(
                Update,
                (
                    persistence::poll_persistence,
                    persistence::resume_cancel_speed,
                )
                    .chain()
                    .after(SimSet::Apply),
            )
            .require_for_boot::<Catalog>()
            .add_systems(
                Update,
                insert_catalog
                    .before(finish_boot)
                    .run_if(resource_exists::<AssetRoot>)
                    .run_if(not(resource_exists::<Catalog>))
                    .run_if(not(resource_exists::<BootError>)),
            )
            .add_systems(
                Update,
                (
                    start_new_game.run_if(in_menus.and_then(resource_exists::<Catalog>)),
                    apply_player_commands
                        .in_set(SimSet::Apply)
                        .run_if(in_game.clone()),
                    mirror_phase.after(SimSet::Apply).run_if(in_game.clone()),
                    return_to_menu.run_if(in_state(AppState::InGame)),
                ),
            )
            .add_systems(FixedUpdate, drive_ticks.run_if(in_game))
            .add_systems(OnEnter(AppState::MainMenu), persistence::refresh_menu_saves)
            .add_systems(
                OnExit(AppState::MainMenu),
                persistence::discard_menu_preload,
            )
            .add_systems(OnEnter(AppState::InGame), enter_game)
            .add_systems(OnExit(AppState::InGame), leave_game)
            .add_systems(OnEnter(PhaseState::Operating), begin_speed)
            .add_systems(OnExit(PhaseState::Operating), set_speed(SimSpeed::Paused));
    }
}

fn insert_catalog(mut commands: Commands, root: Res<AssetRoot>) {
    commands.insert_resource(Catalog(RonRegionCatalog::new(root.0.join("regions"))));
}

/// Runs every queued command through the game, in the order sent, within this one system run.
/// A command made invalid by an earlier one in the same batch (say a target change after the
/// month closed) is refused by the domain like any other: one `CommandFailed`, nothing applied.
pub fn apply_player_commands(
    mut commands: MessageReader<PlayerCommand>,
    mut session: ResMut<Session>,
    mut in_flight: ResMut<TicksInFlight>,
    mut events: MessageWriter<DomainEventMessage>,
    mut failures: MessageWriter<CommandFailed>,
    mut persistence: ResMut<Persistence>,
    generation: Res<SessionGeneration>,
) {
    persistence
        .pending
        .extend(commands.read().filter(|m| m.1 == generation.0).cloned());
    while !persistence.frozen() || persistence.draining {
        let Some(PlayerCommand(command, owner)) = persistence.pending.pop_front() else {
            break;
        };
        if owner != generation.0 {
            continue;
        }
        if matches!(command, Command::AdvanceTicks { .. }) {
            in_flight.0 = false;
        }
        match session.0.execute(command) {
            Ok(emitted) => {
                let closed = emitted
                    .iter()
                    .any(|event| matches!(event, DomainEvent::MonthClosed(_)));
                events.write_batch(
                    emitted
                        .into_iter()
                        .filter(|event| !matches!(event, DomainEvent::TickCompleted(_)))
                        .map(DomainEventMessage),
                );
                if closed {
                    persistence.month_closed(&session.0, generation.0);
                    if persistence.frozen() {
                        break;
                    }
                }
            }
            Err(error) => {
                failures.write(CommandFailed(error));
            }
        }
    }
    persistence.after_apply(&session.0, generation.0);
}

/// Copies the domain's phase into [`PhaseState`] when they differ: the only writer of
/// `NextState<PhaseState>` (the mirror is one way, domain → Bevy).
pub fn mirror_phase(
    session: Res<Session>,
    phase: Res<State<PhaseState>>,
    mut next: ResMut<NextState<PhaseState>>,
) {
    let domain = PhaseState::from(session.0.session().phase());
    if *phase.get() != domain {
        next.set(domain);
    }
}

fn return_to_menu(
    mut requests: MessageReader<ReturnToMenu>,
    mut next: ResMut<NextState<AppState>>,
) {
    if requests.read().count() > 0 {
        next.set(AppState::MainMenu);
    }
}

fn enter_game(
    mut time: ResMut<Time<Fixed>>,
    mut in_flight: ResMut<TicksInFlight>,
    mut persistence: ResMut<Persistence>,
    generation: Res<SessionGeneration>,
) {
    persistence.enter(generation.0);
    time.set_timestep_hz(FIXED_STEP_HZ);
    in_flight.0 = false;
}

fn leave_game(
    mut commands: Commands,
    mut persistence: ResMut<Persistence>,
    mut generation: ResMut<SessionGeneration>,
    mut queued: ResMut<Messages<PlayerCommand>>,
    mut speed: ResMut<SimSpeed>,
    mut in_flight: ResMut<TicksInFlight>,
    crash: Res<CrashContext>,
) {
    commands.remove_resource::<Session>();
    crash.set_game_log(None);
    persistence.abandon();
    generation.0 = generation.0.wrapping_add(1);
    queued.clear();
    *speed = SimSpeed::Paused;
    in_flight.0 = false;
}

fn begin_speed(
    mut speed: ResMut<SimSpeed>,
    persistence: Res<Persistence>,
    settings: Option<Res<crate::preferences::ClientSettings>>,
) {
    *speed = if persistence.resuming {
        SimSpeed::Paused
    } else {
        settings.map_or(SimSpeed::X1, |s| s.speed())
    };
}

fn set_speed(to: SimSpeed) -> impl FnMut(ResMut<SimSpeed>) {
    move |mut speed| *speed = to
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_are_the_ticks_per_fixed_step() {
        let ticks = [
            SimSpeed::Paused,
            SimSpeed::X1,
            SimSpeed::X2,
            SimSpeed::X4,
            SimSpeed::X8,
            SimSpeed::X16,
            SimSpeed::X32,
        ]
        .map(SimSpeed::ticks);
        assert_eq!(ticks, [0, 1, 2, 4, 8, 16, 32]);
        assert_eq!(SimSpeed::default(), SimSpeed::Paused);
    }

    #[test]
    fn day_phases_mirror_one_to_one() {
        assert_eq!(PhaseState::from(DayPhase::Planning), PhaseState::Planning);
        assert_eq!(PhaseState::from(DayPhase::Operating), PhaseState::Operating);
        assert_eq!(PhaseState::from(DayPhase::Review), PhaseState::Review);
    }
}
