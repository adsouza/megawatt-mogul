//! Turns the domain's automatic commitment plan into the commands a player would send.

use domain::session::DayPhase;

use crate::{Command, Game};

/// One `CommitPlant` per plant whose planned pattern differs from its current one, in plant
/// order. Empty outside Planning, where commitments can't change.
pub fn automatic_commitment_commands(game: &Game) -> Vec<Command> {
    let session = game.session();
    if session.phase() != DayPhase::Planning {
        return Vec::new();
    }
    let current: std::collections::BTreeMap<_, _> = session
        .plant_views()
        .into_iter()
        .map(|view| (view.id, view.commitment))
        .collect();
    session
        .automatic_commitment_plan()
        .into_iter()
        .filter(|(plant, schedule)| current.get(plant) != Some(&schedule.hours()))
        .map(|(plant, schedule)| Command::CommitPlant {
            plant,
            hours: schedule.hours(),
        })
        .collect()
}
