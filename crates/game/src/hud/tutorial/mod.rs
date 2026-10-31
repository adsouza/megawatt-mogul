//! The first-game tutorial (first-game tutorial spec §3, §4): the step table and pure model
//! ([`steps`]), the anchors panels mark for it ([`anchors`]) and the run's lifecycle.
//!
//! A run starts with a new game when the player's progress says the tutorial is owed, belongs to
//! that one game, and finishes when the player leaves the first Review. Loading a save never
//! starts one, and leaving the game part-way drops it without clearing `tutorial_pending`.

mod anchors;
mod steps;

use bevy::prelude::*;
use domain::session::DayPhase;

pub use self::anchors::{anchor_rect, mark_anchor};
pub use self::steps::{
    Done, StepDef, StepKind, TUTORIAL_STEPS, TutorialAnchor, TutorialState, TutorialStep,
    active_step, construction_key,
};
use crate::progress::ClientProgressStore;
use crate::sim::{NewGameStarted, Session, SessionGeneration, SimSet};
use crate::state::AppState;

/// The running tutorial, if any.
#[derive(Resource, Debug, Default)]
pub struct TutorialRun(pub Option<TutorialState>);

/// The tutorial run's lifecycle. `HudPlugin` adds it.
///
/// These systems run in `Update`: they are bookkeeping on the game's state, not clicks, so they
/// write [`TutorialRun`] and [`ClientProgressStore`] directly rather than through `dispatch`.
pub struct TutorialPlugin;

impl Plugin for TutorialPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TutorialRun>()
            .add_systems(
                Update,
                (
                    // The new game's `Session` is inserted by command: order after it so the
                    // starter fleet can be read in the same frame.
                    start_run.after(crate::sim::start_new_game),
                    track_run.after(SimSet::Apply),
                )
                    .chain(),
            )
            .add_systems(OnExit(AppState::InGame), clear_run);
    }
}

/// Starts a run for the new game when the tutorial is owed (and progress has loaded, so the owed
/// flag is the player's, not the in-memory default); otherwise there is none.
fn start_run(
    mut started: MessageReader<NewGameStarted>,
    session: Option<Res<Session>>,
    progress: Res<ClientProgressStore>,
    mut run: ResMut<TutorialRun>,
) {
    let Some(started) = started.read().last() else {
        return;
    };
    let owed = progress.loaded && progress.value.tutorial_pending;
    run.0 = match session {
        Some(session) if owed => Some(TutorialState::new(started.generation, &session.0)),
        _ => None,
    };
}

/// Ends a run whose game is gone (a load or another new game changes the session generation),
/// notes the first Review, and finishes the tutorial when the player leaves it.
fn track_run(
    session: Option<Res<Session>>,
    generation: Res<SessionGeneration>,
    mut progress: ResMut<ClientProgressStore>,
    mut run: ResMut<TutorialRun>,
) {
    let Some(state) = run.0.as_mut() else {
        return;
    };
    let Some(session) = session.filter(|_| state.generation == generation.0) else {
        run.0 = None;
        return;
    };
    match session.0.session().phase() {
        DayPhase::Review => state.seen_review = true,
        DayPhase::Planning if state.seen_review => {
            progress.value.tutorial_pending = false;
            run.0 = None;
        }
        _ => {}
    }
}

fn clear_run(mut run: ResMut<TutorialRun>) {
    run.0 = None;
}
