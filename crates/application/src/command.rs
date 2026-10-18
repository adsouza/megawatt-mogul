//! Player commands and the handler trait every pipeline layer implements.

use domain::events::DomainEvent;
use domain::ids::{CellCoord, NodeId, PlantId};
use domain::region::PlantKind;
use domain::session::GameSession;
use domain::units::Megawatts;

use crate::error::CommandError;

/// Everything the player (or a replay) can ask the game to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    BuildPlant {
        kind: PlantKind,
        cell: CellCoord,
    },
    BuildLine {
        from: NodeId,
        to: NodeId,
    },
    CommitPlant {
        plant: PlantId,
        hours: [bool; 24],
    },
    BeginOperating,
    SetDispatchTarget {
        plant: PlantId,
        mw: Megawatts,
    },
    SetAutoFollow {
        plant: PlantId,
        on: bool,
    },
    StartPlant {
        plant: PlantId,
    },
    StopPlant {
        plant: PlantId,
    },
    /// Runs up to `n` ticks, stopping early once the day is no longer being
    /// operated (the month closed). `n = 0` does nothing.
    AdvanceTicks {
        n: u32,
    },
    FinishReview,
}

/// One stage of the command pipeline. Layers wrap an inner handler and the
/// innermost one applies the command to the session.
pub trait CommandHandler {
    fn handle(
        &mut self,
        session: &mut GameSession,
        command: &Command,
    ) -> Result<Vec<DomainEvent>, CommandError>;
}

impl<H: CommandHandler + ?Sized> CommandHandler for Box<H> {
    fn handle(
        &mut self,
        session: &mut GameSession,
        command: &Command,
    ) -> Result<Vec<DomainEvent>, CommandError> {
        (**self).handle(session, command)
    }
}
