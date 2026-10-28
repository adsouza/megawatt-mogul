//! Player commands and the handler trait every pipeline layer implements.

use domain::events::DomainEvent;
use domain::ids::{CellCoord, LineId, LinkId, NodeId, PlantId};
use domain::region::PlantKind;
use domain::session::GameSession;
use domain::units::Megawatts;

use crate::error::CommandError;

/// Everything the player (or a replay) can ask the game to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    DrillMakeupWells {
        plant: PlantId,
    },
    DismissFieldNotice {
        plant: PlantId,
    },
    ActivateService {
        island: NodeId,
        accept_shortfall: bool,
        expected_revision: u64,
    },
    BuildLink {
        from: NodeId,
        to: NodeId,
    },
    CancelLinkConstruction {
        link: LinkId,
    },
    SetLinkSchedule {
        link: LinkId,
        mw: Box<[f64; 24]>,
    },
    SetLinkTarget {
        link: LinkId,
        mw: f64,
    },
    ClearLinkOverride {
        link: LinkId,
    },
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
    CancelLineConstruction {
        line: LineId,
    },
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
            Self::DrillMakeupWells { plant } => {
                domain::state::State::write(&18u8, out);
                domain::state::State::write(plant, out);
            }
            Self::DismissFieldNotice { plant } => {
                domain::state::State::write(&19u8, out);
                domain::state::State::write(plant, out);
            }
            Self::ActivateService {
                island,
                accept_shortfall,
                expected_revision,
            } => {
                domain::state::State::write(&17u8, out);
                domain::state::State::write(island, out);
                domain::state::State::write(accept_shortfall, out);
                domain::state::State::write(expected_revision, out);
            }
            Self::BuildLink { from, to } => {
                domain::state::State::write(&12u8, out);
                domain::state::State::write(from, out);
                domain::state::State::write(to, out);
            }
            Self::CancelLinkConstruction { link } => {
                domain::state::State::write(&13u8, out);
                domain::state::State::write(link, out);
            }
            Self::SetLinkSchedule { link, mw } => {
                domain::state::State::write(&14u8, out);
                domain::state::State::write(link, out);
                domain::state::State::write(mw, out);
            }
            Self::SetLinkTarget { link, mw } => {
                domain::state::State::write(&15u8, out);
                domain::state::State::write(link, out);
                domain::state::State::write(mw, out);
            }
            Self::ClearLinkOverride { link } => {
                domain::state::State::write(&16u8, out);
                domain::state::State::write(link, out);
            }
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
            Self::CancelLineConstruction { line } => {
                domain::state::State::write(&11u8, out);
                domain::state::State::write(line, out);
            }
        }
    }
    fn read(input: &mut domain::state::StateReader<'_>) -> Result<Self, domain::state::StateError> {
        Ok(match <u8 as domain::state::State>::read(input)? {
            18 if input.schema >= 6 => Self::DrillMakeupWells {
                plant: domain::state::State::read(input)?,
            },
            19 if input.schema >= 6 => Self::DismissFieldNotice {
                plant: domain::state::State::read(input)?,
            },
            17 => Self::ActivateService {
                island: domain::state::State::read(input)?,
                accept_shortfall: domain::state::State::read(input)?,
                expected_revision: domain::state::State::read(input)?,
            },
            12 => Self::BuildLink {
                from: domain::state::State::read(input)?,
                to: domain::state::State::read(input)?,
            },
            13 => Self::CancelLinkConstruction {
                link: domain::state::State::read(input)?,
            },
            14 => Self::SetLinkSchedule {
                link: domain::state::State::read(input)?,
                mw: domain::state::State::read(input)?,
            },
            15 => Self::SetLinkTarget {
                link: domain::state::State::read(input)?,
                mw: domain::state::State::read(input)?,
            },
            16 => Self::ClearLinkOverride {
                link: domain::state::State::read(input)?,
            },
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
            11 => Self::CancelLineConstruction {
                line: domain::state::State::read(input)?,
            },
            _ => return Err(domain::state::StateError::Invalid),
        })
    }
}
