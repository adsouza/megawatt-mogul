//! Serde mirrors of the log's domain and application types. The domain and
//! application crates stay free of serde; this module owns the on-disk shape.

use application::{Command, LogHeader, LogRecord, RegionFingerprint};
use domain::grid::NetworkModel;
use domain::ids::{CellCoord, LineId, LinkId, NodeId, PlantId, RegionId};
use domain::region::PlantKind;
use domain::session::{GameSettings, SimulationFidelity};
use domain::units::Megawatts;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A log record that cannot be mapped back to application types.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum LogDtoError {
    #[error("region fingerprint must be 64 lowercase hex digits, got {0:?}")]
    InvalidFingerprint(String),
    #[error("dispatch target must be finite, got {0}")]
    NonFiniteMegawatts(f64),
}

const FINGERPRINT_BYTES: usize = 32;

fn fingerprint_to_hex(fingerprint: &RegionFingerprint) -> String {
    fingerprint
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

fn fingerprint_from_hex(text: &str) -> Result<RegionFingerprint, LogDtoError> {
    let invalid = || LogDtoError::InvalidFingerprint(text.to_owned());
    let digits = text.as_bytes();
    if digits.len() != FINGERPRINT_BYTES * 2 {
        return Err(invalid());
    }
    let mut bytes = [0u8; FINGERPRINT_BYTES];
    let (pairs, _) = digits.as_chunks::<2>();
    for (byte, [high, low]) in bytes.iter_mut().zip(pairs) {
        let high = hex_digit(*high).ok_or_else(invalid)?;
        let low = hex_digit(*low).ok_or_else(invalid)?;
        *byte = high << 4 | low;
    }
    Ok(RegionFingerprint::new(bytes))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlantKindDto {
    Nuclear,
    Coal,
    Ccgt,
    Peaker,
    Solar,
    Hydro,
    Battery,
    Wind,
}

impl From<PlantKind> for PlantKindDto {
    fn from(kind: PlantKind) -> Self {
        match kind {
            PlantKind::Nuclear => Self::Nuclear,
            PlantKind::Coal => Self::Coal,
            PlantKind::Ccgt => Self::Ccgt,
            PlantKind::Peaker => Self::Peaker,
            PlantKind::Solar => Self::Solar,
            PlantKind::Hydro => Self::Hydro,
            PlantKind::Battery => Self::Battery,
            PlantKind::Wind => Self::Wind,
        }
    }
}

impl From<PlantKindDto> for PlantKind {
    fn from(kind: PlantKindDto) -> Self {
        match kind {
            PlantKindDto::Nuclear => Self::Nuclear,
            PlantKindDto::Coal => Self::Coal,
            PlantKindDto::Ccgt => Self::Ccgt,
            PlantKindDto::Peaker => Self::Peaker,
            PlantKindDto::Solar => Self::Solar,
            PlantKindDto::Hydro => Self::Hydro,
            PlantKindDto::Battery => Self::Battery,
            PlantKindDto::Wind => Self::Wind,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellDto {
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkModelDto {
    CopperPlate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FidelityDto {
    pub network: NetworkModelDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameSettingsDto {
    pub fidelity: FidelityDto,
}

impl From<&GameSettings> for GameSettingsDto {
    fn from(settings: &GameSettings) -> Self {
        let network = match settings.fidelity.network {
            NetworkModel::CopperPlate => NetworkModelDto::CopperPlate,
        };
        Self {
            fidelity: FidelityDto { network },
        }
    }
}

impl From<GameSettingsDto> for GameSettings {
    fn from(dto: GameSettingsDto) -> Self {
        let network = match dto.fidelity.network {
            NetworkModelDto::CopperPlate => NetworkModel::CopperPlate,
        };
        GameSettings {
            fidelity: SimulationFidelity { network },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogHeaderDto {
    pub log_format_version: u32,
    pub sim_rules_version: u32,
    pub settings: GameSettingsDto,
    pub region: String,
    pub seed: u64,
    /// SHA-256 of the region source bytes as 64 lowercase hex digits.
    pub region_fingerprint: String,
}

impl From<&LogHeader> for LogHeaderDto {
    fn from(header: &LogHeader) -> Self {
        Self {
            log_format_version: header.log_format_version,
            sim_rules_version: header.sim_rules_version,
            settings: (&header.settings).into(),
            region: header.region.as_str().to_owned(),
            seed: header.seed,
            region_fingerprint: fingerprint_to_hex(&header.region_fingerprint),
        }
    }
}

impl TryFrom<LogHeaderDto> for LogHeader {
    type Error = LogDtoError;

    fn try_from(dto: LogHeaderDto) -> Result<Self, LogDtoError> {
        Ok(Self {
            log_format_version: dto.log_format_version,
            sim_rules_version: dto.sim_rules_version,
            settings: dto.settings.into(),
            region: RegionId::new(&dto.region),
            seed: dto.seed,
            region_fingerprint: fingerprint_from_hex(&dto.region_fingerprint)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum CommandDto {
    ActivateService {
        island: u32,
        accept_shortfall: bool,
        expected_revision: u64,
    },
    BuildLink {
        from: u32,
        to: u32,
    },
    CancelLinkConstruction {
        link: u32,
    },
    SetLinkSchedule {
        link: u32,
        mw: [f64; 24],
    },
    SetLinkTarget {
        link: u32,
        mw: f64,
    },
    ClearLinkOverride {
        link: u32,
    },
    BuildPlant {
        kind: PlantKindDto,
        cell: CellDto,
    },
    BuildLine {
        from: u32,
        to: u32,
    },
    CommitPlant {
        plant: u32,
        hours: [bool; 24],
    },
    BeginOperating,
    SetDispatchTarget {
        plant: u32,
        mw: f64,
    },
    SetAutoFollow {
        plant: u32,
        on: bool,
    },
    SetBatteryEnabled {
        plant: u32,
        on: bool,
    },
    StartPlant {
        plant: u32,
    },
    StopPlant {
        plant: u32,
    },
    AdvanceTicks {
        n: u32,
    },
    FinishReview,
    CancelLineConstruction {
        line: u32,
    },
}

impl From<&Command> for CommandDto {
    fn from(command: &Command) -> Self {
        match *command {
            Command::ActivateService {
                island,
                accept_shortfall,
                expected_revision,
            } => Self::ActivateService {
                island: island.0,
                accept_shortfall,
                expected_revision,
            },
            Command::BuildLink { from, to } => Self::BuildLink {
                from: from.0,
                to: to.0,
            },
            Command::CancelLinkConstruction { link } => {
                Self::CancelLinkConstruction { link: link.0 }
            }
            Command::SetLinkSchedule { link, ref mw } => Self::SetLinkSchedule {
                link: link.0,
                mw: **mw,
            },
            Command::SetLinkTarget { link, mw } => Self::SetLinkTarget { link: link.0, mw },
            Command::ClearLinkOverride { link } => Self::ClearLinkOverride { link: link.0 },
            Command::BuildPlant { kind, cell } => Self::BuildPlant {
                kind: kind.into(),
                cell: CellDto {
                    x: cell.x,
                    y: cell.y,
                },
            },
            Command::BuildLine { from, to } => Self::BuildLine {
                from: from.0,
                to: to.0,
            },
            Command::CommitPlant { plant, hours } => Self::CommitPlant {
                plant: plant.0,
                hours,
            },
            Command::BeginOperating => Self::BeginOperating,
            Command::SetDispatchTarget { plant, mw } => Self::SetDispatchTarget {
                plant: plant.0,
                mw: mw.value(),
            },
            Command::SetBatteryEnabled { plant, on } => {
                Self::SetBatteryEnabled { plant: plant.0, on }
            }
            Command::SetAutoFollow { plant, on } => Self::SetAutoFollow { plant: plant.0, on },
            Command::StartPlant { plant } => Self::StartPlant { plant: plant.0 },
            Command::StopPlant { plant } => Self::StopPlant { plant: plant.0 },
            Command::AdvanceTicks { n } => Self::AdvanceTicks { n },
            Command::FinishReview => Self::FinishReview,
            Command::CancelLineConstruction { line } => {
                Self::CancelLineConstruction { line: line.0 }
            }
        }
    }
}

impl TryFrom<CommandDto> for Command {
    type Error = LogDtoError;

    fn try_from(dto: CommandDto) -> Result<Self, LogDtoError> {
        Ok(match dto {
            CommandDto::ActivateService {
                island,
                accept_shortfall,
                expected_revision,
            } => Self::ActivateService {
                island: NodeId(island),
                accept_shortfall,
                expected_revision,
            },
            CommandDto::BuildLink { from, to } => Self::BuildLink {
                from: NodeId(from),
                to: NodeId(to),
            },
            CommandDto::CancelLinkConstruction { link } => {
                Self::CancelLinkConstruction { link: LinkId(link) }
            }
            CommandDto::SetLinkSchedule { link, mw } => {
                if let Some(value) = mw.iter().find(|v| !v.is_finite()) {
                    return Err(LogDtoError::NonFiniteMegawatts(*value));
                }
                Self::SetLinkSchedule {
                    link: LinkId(link),
                    mw: Box::new(mw),
                }
            }
            CommandDto::SetLinkTarget { link, mw } => {
                if !mw.is_finite() {
                    return Err(LogDtoError::NonFiniteMegawatts(mw));
                }
                Self::SetLinkTarget {
                    link: LinkId(link),
                    mw,
                }
            }
            CommandDto::ClearLinkOverride { link } => {
                Self::ClearLinkOverride { link: LinkId(link) }
            }
            CommandDto::BuildPlant { kind, cell } => Self::BuildPlant {
                kind: kind.into(),
                cell: CellCoord {
                    x: cell.x,
                    y: cell.y,
                },
            },
            CommandDto::BuildLine { from, to } => Self::BuildLine {
                from: NodeId(from),
                to: NodeId(to),
            },
            CommandDto::CommitPlant { plant, hours } => Self::CommitPlant {
                plant: PlantId(plant),
                hours,
            },
            CommandDto::BeginOperating => Self::BeginOperating,
            CommandDto::SetDispatchTarget { plant, mw } => {
                if !mw.is_finite() {
                    return Err(LogDtoError::NonFiniteMegawatts(mw));
                }
                Self::SetDispatchTarget {
                    plant: PlantId(plant),
                    mw: Megawatts::new(mw),
                }
            }
            CommandDto::SetBatteryEnabled { plant, on } => Self::SetBatteryEnabled {
                plant: PlantId(plant),
                on,
            },
            CommandDto::SetAutoFollow { plant, on } => Self::SetAutoFollow {
                plant: PlantId(plant),
                on,
            },
            CommandDto::StartPlant { plant } => Self::StartPlant {
                plant: PlantId(plant),
            },
            CommandDto::StopPlant { plant } => Self::StopPlant {
                plant: PlantId(plant),
            },
            CommandDto::AdvanceTicks { n } => Self::AdvanceTicks { n },
            CommandDto::FinishReview => Self::FinishReview,
            CommandDto::CancelLineConstruction { line } => {
                Self::CancelLineConstruction { line: LineId(line) }
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LogRecordDto {
    Header(LogHeaderDto),
    Command(CommandDto),
}

impl From<&LogRecord> for LogRecordDto {
    fn from(record: &LogRecord) -> Self {
        match record {
            LogRecord::Header(header) => Self::Header(header.into()),
            LogRecord::Command(command) => Self::Command(command.into()),
        }
    }
}

impl TryFrom<LogRecordDto> for LogRecord {
    type Error = LogDtoError;

    fn try_from(dto: LogRecordDto) -> Result<Self, LogDtoError> {
        Ok(match dto {
            LogRecordDto::Header(header) => Self::Header(header.try_into()?),
            LogRecordDto::Command(command) => Self::Command(command.try_into()?),
        })
    }
}
