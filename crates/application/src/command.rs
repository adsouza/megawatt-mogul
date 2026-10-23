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
    SetBatteryEnabled {
        plant: PlantId,
        on: bool,
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
    /// operated (the month closed). Like any tick it needs an active game in
    /// `Operating`, even for `n = 0`, which then changes nothing.
    AdvanceTicks {
        n: u32,
    },
    FinishReview,
}

/// One stage of the command pipeline. Layers wrap an inner handler and the
/// innermost one applies the command to the session. `Send + Sync` so a
/// `Game` holding the pipeline can live in a Bevy `Resource`.
pub trait CommandHandler: Send + Sync {
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

impl domain::state::State for Command {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::BuildPlant { kind, cell } => {
                domain::state::State::write(&0u8, out);
                domain::state::State::write(kind, out);
                domain::state::State::write(cell, out);
            }
            Self::BuildLine { from, to } => {
                domain::state::State::write(&1u8, out);
                domain::state::State::write(from, out);
                domain::state::State::write(to, out);
            }
            Self::CommitPlant { plant, hours } => {
                domain::state::State::write(&2u8, out);
                domain::state::State::write(plant, out);
                domain::state::State::write(hours, out);
            }
            Self::BeginOperating => {
                domain::state::State::write(&3u8, out);
            }
            Self::SetDispatchTarget { plant, mw } => {
                domain::state::State::write(&4u8, out);
                domain::state::State::write(plant, out);
                domain::state::State::write(mw, out);
            }
            Self::SetBatteryEnabled { plant, on } => {
                domain::state::State::write(&5u8, out);
                domain::state::State::write(plant, out);
                domain::state::State::write(on, out);
            }
            Self::SetAutoFollow { plant, on } => {
                domain::state::State::write(&6u8, out);
                domain::state::State::write(plant, out);
                domain::state::State::write(on, out);
            }
            Self::StartPlant { plant } => {
                domain::state::State::write(&7u8, out);
                domain::state::State::write(plant, out);
            }
            Self::StopPlant { plant } => {
                domain::state::State::write(&8u8, out);
                domain::state::State::write(plant, out);
            }
            Self::AdvanceTicks { n } => {
                domain::state::State::write(&9u8, out);
                domain::state::State::write(n, out);
            }
            Self::FinishReview => {
                domain::state::State::write(&10u8, out);
            }
        }
    }
    fn read(input: &mut domain::state::StateReader<'_>) -> Result<Self, domain::state::StateError> {
        Ok(match <u8 as domain::state::State>::read(input)? {
            0 => Self::BuildPlant {
                kind: domain::state::State::read(input)?,
                cell: domain::state::State::read(input)?,
            },
            1 => Self::BuildLine {
                from: domain::state::State::read(input)?,
                to: domain::state::State::read(input)?,
            },
            2 => Self::CommitPlant {
                plant: domain::state::State::read(input)?,
                hours: domain::state::State::read(input)?,
            },
            3 => Self::BeginOperating,
            4 => Self::SetDispatchTarget {
                plant: domain::state::State::read(input)?,
                mw: domain::state::State::read(input)?,
            },
            5 => Self::SetBatteryEnabled {
                plant: domain::state::State::read(input)?,
                on: domain::state::State::read(input)?,
            },
            6 => Self::SetAutoFollow {
                plant: domain::state::State::read(input)?,
                on: domain::state::State::read(input)?,
            },
            7 => Self::StartPlant {
                plant: domain::state::State::read(input)?,
            },
            8 => Self::StopPlant {
                plant: domain::state::State::read(input)?,
            },
            9 => Self::AdvanceTicks {
                n: domain::state::State::read(input)?,
            },
            10 => Self::FinishReview,
            _ => return Err(domain::state::StateError::Invalid),
        })
    }
}
