//! `FileCommandLog`: the command-log sink on disk, and reading a log back.
//!
//! A log `game.log` is a text file with one RON record per line, the header
//! first. Beside it, `game.log.region.ron` holds the exact bytes of the region
//! file the game started on. The archive's name is derived from the log's path
//! alone, never from anything the log says, so a log cannot point replay at
//! some other file.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use application::{
    Command, CommandLogError, CommandLogSink, LOG_FORMAT_VERSION, LoadedRegion, LogHeader,
    LogRecord, RegionFingerprint, RegionLoadError,
};
use domain::SIM_RULES_VERSION;
use serde::Deserialize;
use serde::de::IgnoredAny;
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::dto::LogRecordDto;
use crate::region::load_region_bytes;

const ARCHIVE_SUFFIX: &str = ".region.ron";

/// Where the region archive for the log at `log_path` lives: the log's file
/// name with `.region.ron` appended, in the same directory. `None` when the
/// path has no file name.
pub fn region_archive_path(log_path: &Path) -> Option<PathBuf> {
    let mut name: OsString = log_path.file_name()?.to_owned();
    name.push(ARCHIVE_SUFFIX);
    Some(log_path.with_file_name(name))
}

/// Format-2 recordings retain a verified base checkpoint beside their command stream.
pub fn checkpoint_archive_path(path: &Path) -> Option<PathBuf> {
    let mut name = path.file_name()?.to_owned();
    name.push(".checkpoint");
    Some(path.with_file_name(name))
}

fn log_error(message: impl Into<String>) -> CommandLogError {
    CommandLogError {
        message: message.into(),
    }
}

fn io_error(what: &str, path: &Path, error: &io::Error) -> CommandLogError {
    log_error(format!("{what} {}: {error}", path.display()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// The log file exists; the archive and header are not yet written.
    Created,
    /// The archive and header are on disk; appends are accepted.
    Ready,
    /// Initialisation or an append failed part-way. The files on disk may be
    /// incomplete, so nothing more is written.
    Failed,
}

/// A command log written to a file, one flushed RON record per line.
#[derive(Debug)]
pub struct FileCommandLog {
    log: File,
    archive_path: PathBuf,
    checkpoint_path: PathBuf,
    state: State,
    prepared: Option<PreparedRecording>,
}

impl FileCommandLog {
    /// Creates the log file at `path`, failing if it already exists. The sink
    /// accepts no commands until `initialize` has succeeded.
    pub fn create(path: &Path) -> Result<Self, CommandLogError> {
        let archive_path = region_archive_path(path)
            .ok_or_else(|| log_error(format!("{} has no file name", path.display())))?;
        let log = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| io_error("cannot create log", path, &e))?;
        Ok(Self {
            log,
            archive_path,
            checkpoint_path: checkpoint_archive_path(path)
                .ok_or_else(|| log_error("checkpoint path"))?,
            state: State::Created,
            prepared: None,
        })
    }

    /// Attach bytes prepared speculatively in memory. Initialization still binds them to the
    /// exact verified checkpoint and writes nothing until the player activates the game.
    pub fn with_prepared_recording(mut self, prepared: PreparedRecording) -> Self {
        self.prepared = Some(prepared);
        self
    }

    fn write_archive(&self, source: &[u8]) -> Result<(), CommandLogError> {
        let mut archive = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.archive_path)
            .map_err(|e| io_error("cannot create region archive", &self.archive_path, &e))?;
        archive
            .write_all(source)
            .and_then(|()| archive.flush())
            .and_then(|()| archive.sync_all())
            .map_err(|e| io_error("cannot write region archive", &self.archive_path, &e))
    }

    fn write_record(&mut self, record: &LogRecord) -> Result<(), CommandLogError> {
        let mut line = ron::ser::to_string(&LogRecordDto::from(record))
            .map_err(|e| log_error(format!("cannot encode record: {e}")))?;
        debug_assert!(!line.contains('\n'), "a record must stay on one line");
        line.push('\n');
        self.log
            .write_all(line.as_bytes())
            .and_then(|()| self.log.flush())
            .map_err(|e| log_error(format!("cannot write record: {e}")))
    }
}

impl CommandLogSink for FileCommandLog {
    fn initialize(
        &mut self,
        header: &LogHeader,
        region_source: &[u8],
    ) -> Result<(), CommandLogError> {
        if self.state != State::Created {
            return Err(log_error("log already initialised or failed"));
        }
        // Whatever happens next, this sink is not usable unless it reaches the end.
        self.state = State::Failed;

        let actual = RegionFingerprint::new(Sha256::digest(region_source).into());
        if actual != header.region_fingerprint {
            return Err(log_error(
                "region source does not match the header fingerprint",
            ));
        }
        self.write_archive(region_source)?;
        self.write_record(&LogRecord::Header(header.clone()))?;
        self.log
            .sync_all()
            .map_err(|e| log_error(format!("cannot sync header: {e}")))?;
        self.state = State::Ready;
        Ok(())
    }

    fn initialize_checkpoint(
        &mut self,
        checkpoint: &application::GameCheckpoint,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), CommandLogError> {
        if self.state != State::Created {
            return Err(log_error("log already initialised or failed"));
        }
        let prepared = match self.prepared.take() {
            Some(prepared) if &prepared.checkpoint == checkpoint => prepared,
            Some(_) => {
                self.state = State::Failed;
                return Err(log_error("prepared recording differs from checkpoint"));
            }
            None => PreparedRecording::new(checkpoint.clone(), cancelled)?,
        };
        if cancelled() {
            self.state = State::Failed;
            return Err(log_error("recording cancelled"));
        }
        let mut header = checkpoint.header.clone();
        if let Some(bytes) = prepared.snapshot {
            // Until initialization completes, every failed write leaves an unusable sink.
            self.state = State::Failed;
            crate::save::monthly::publish_fresh(&self.checkpoint_path, &bytes, cancelled)
                .map_err(|e| log_error(e.to_string()))?;
            header.log_format_version = 2;
            self.state = State::Created;
        }
        self.initialize(&header, &checkpoint.region_source)?;
        self.state = State::Failed;
        write_prepared(&mut self.log, &prepared.commands, cancelled)?;
        self.log.sync_all().map_err(|e| log_error(e.to_string()))?;
        self.state = State::Ready;
        Ok(())
    }

    fn append(&mut self, record: &LogRecord) -> Result<(), CommandLogError> {
        if self.state != State::Ready {
            return Err(log_error("log not initialised"));
        }
        if matches!(record, LogRecord::Header(_)) {
            return Err(log_error("the header is written by initialize only"));
        }
        let result = self.write_record(record);
        if result.is_err() {
            // A failed write may have left half a line behind.
            self.state = State::Failed;
        }
        result
    }
}

/// Immutable diagnostic base prepared without filesystem access. Retained only while Resume
/// remains a possible menu choice. Preparation includes snapshot compression and prefix encoding.
#[derive(Debug)]
pub struct PreparedRecording {
    checkpoint: application::GameCheckpoint,
    snapshot: Option<crate::save::monthly::EncodedArchive>,
    commands: Vec<u8>,
}
impl PreparedRecording {
    pub fn new(
        checkpoint: application::GameCheckpoint,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, CommandLogError> {
        if cancelled() {
            return Err(log_error("recording cancelled"));
        }
        let snapshot = if checkpoint.snapshot.is_some() {
            let bundle = application::SaveBundle {
                compatibility: crate::save::replay_compatibility(),
                checkpoint: checkpoint.clone(),
                metadata: application::SaveMetadata {
                    slot: application::SaveId::new("0".repeat(32))
                        .map_err(|e| log_error(e.to_string()))?,
                    run: application::SaveId::new("0".repeat(32))
                        .map_err(|e| log_error(e.to_string()))?,
                    kind: application::SaveKind::Manual,
                    name: "diagnostic-checkpoint".into(),
                    saved_at: 0,
                },
            };
            Some(
                crate::save::monthly::encode(&bundle, &bundle.compatibility)
                    .map_err(|e| log_error(e.to_string()))?,
            )
        } else {
            None
        };
        let mut commands = Vec::new();
        for command in &checkpoint.commands {
            if cancelled() {
                return Err(log_error("recording cancelled"));
            }
            let line = ron::to_string(&LogRecordDto::from(&LogRecord::Command(command.clone())))
                .map_err(|e| log_error(e.to_string()))?;
            commands.extend_from_slice(line.as_bytes());
            commands.push(b'\n');
        }
        if cancelled() {
            return Err(log_error("recording cancelled"));
        }
        Ok(Self {
            checkpoint,
            snapshot,
            commands,
        })
    }
}
fn write_prepared(
    file: &mut File,
    bytes: &[u8],
    cancelled: &dyn Fn() -> bool,
) -> Result<(), CommandLogError> {
    for chunk in bytes.chunks(64 * 1024) {
        if cancelled() {
            return Err(log_error("recording cancelled"));
        }
        file.write_all(chunk)
            .map_err(|e| log_error(e.to_string()))?;
    }
    Ok(())
}

/// Why a log could not be read or turned into replay inputs.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ReplayLoadError {
    #[error("i/o error: {0}")]
    Io(String),
    #[error("line {line}: {message}")]
    Parse { line: usize, message: String },
    #[error("recording checkpoint could not be verified: {0}")]
    Checkpoint(String),
    #[error("log has no header as its first record")]
    MissingHeader,
    #[error("log has more than one header")]
    DuplicateHeader,
    #[error("log format version {recorded} unsupported (supported {supported})")]
    LogFormatVersionMismatch { recorded: u32, supported: u32 },
    #[error("simulation rules version {recorded} incompatible (engine {supported})")]
    RulesVersionMismatch { recorded: u32, supported: u32 },
    #[error("region archive fingerprint mismatch: expected {expected:?}, actual {actual:?}")]
    RegionFingerprintMismatch {
        expected: RegionFingerprint,
        actual: RegionFingerprint,
    },
    #[error("region archive unusable: {0}")]
    Archive(#[from] RegionLoadError),
}

/// Everything `application::replay` needs, read from a log and its archive.
#[derive(Debug, Clone)]
pub struct ReplayInput {
    pub header: LogHeader,
    pub region: LoadedRegion,
    pub commands: Vec<Command>,
    pub checkpoint: Option<application::GameCheckpoint>,
}
impl ReplayInput {
    /// Replay from the recording's own verified base, including migrated games.
    pub fn replay(self) -> Result<domain::session::GameSession, ReplayLoadError> {
        if let Some(base) = self.checkpoint {
            let mut game = application::Game::restore(
                self.region,
                base,
                Box::new(application::InMemoryCommandLog::default()),
                &|| false,
            )
            .map_err(|e| ReplayLoadError::Checkpoint(e.to_string()))?;
            for command in self.commands {
                game.execute(command)
                    .map_err(|e| ReplayLoadError::Checkpoint(e.to_string()))?;
            }
            Ok(game.session().clone())
        } else {
            application::replay(self.region, &self.header, &self.commands)
                .map_err(|e| ReplayLoadError::Checkpoint(e.to_string()))
        }
    }
}

/// The part of a header every format version shares. Decoding only this lets a
/// log in an unknown format be refused before its records are interpreted.
#[derive(Debug, Deserialize)]
struct VersionEnvelope {
    log_format_version: u32,
    sim_rules_version: u32,
}

#[derive(Debug, Deserialize)]
enum FirstRecordEnvelope {
    Header(VersionEnvelope),
    Command(IgnoredAny),
}

fn read_text(path: &Path) -> Result<String, ReplayLoadError> {
    fs::read_to_string(path).map_err(|e| ReplayLoadError::Io(format!("{}: {e}", path.display())))
}

fn parse_error(line: usize, error: impl ToString) -> ReplayLoadError {
    ReplayLoadError::Parse {
        line,
        message: error.to_string(),
    }
}

fn peek_versions(text: &str) -> Result<VersionEnvelope, ReplayLoadError> {
    let first = text.lines().next().ok_or(ReplayLoadError::MissingHeader)?;
    match ron::de::from_str(first).map_err(|e| parse_error(1, e))? {
        FirstRecordEnvelope::Header(versions) => Ok(versions),
        FirstRecordEnvelope::Command(_) => Err(ReplayLoadError::MissingHeader),
    }
}

fn check_format(versions: &VersionEnvelope) -> Result<(), ReplayLoadError> {
    if [LOG_FORMAT_VERSION, 2].contains(&versions.log_format_version) {
        return Ok(());
    }
    Err(ReplayLoadError::LogFormatVersionMismatch {
        recorded: versions.log_format_version,
        supported: LOG_FORMAT_VERSION,
    })
}

fn check_rules(versions: &VersionEnvelope) -> Result<(), ReplayLoadError> {
    if versions.sim_rules_version == SIM_RULES_VERSION {
        return Ok(());
    }
    Err(ReplayLoadError::RulesVersionMismatch {
        recorded: versions.sim_rules_version,
        supported: SIM_RULES_VERSION,
    })
}

/// Checks the version envelope, then decodes every record: the header first
/// and exactly once, then commands. With `check_rules`, an incompatible rules
/// version is refused before anything past the envelope is decoded.
fn parse_log(text: &str, check_rules_version: bool) -> Result<Vec<LogRecord>, ReplayLoadError> {
    let versions = peek_versions(text)?;
    check_format(&versions)?;
    if check_rules_version {
        check_rules(&versions)?;
    }
    let mut records = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let dto: LogRecordDto = ron::de::from_str(line).map_err(|e| parse_error(number, e))?;
        let record = LogRecord::try_from(dto).map_err(|e| parse_error(number, e))?;
        if index > 0 && matches!(record, LogRecord::Header(_)) {
            return Err(ReplayLoadError::DuplicateHeader);
        }
        records.push(record);
    }
    Ok(records)
}

/// Reads every record of the log at `path`: the header, then the commands.
/// Fails on an absent or duplicate header, an unsupported format version, or
/// any line that does not decode.
pub fn read_command_log(path: &Path) -> Result<Vec<LogRecord>, CommandLogError> {
    let text = read_text(path).map_err(|e| log_error(e.to_string()))?;
    parse_log(&text, false).map_err(|e| log_error(e.to_string()))
}

/// Loads a log together with its archived region, checking the format version,
/// the rules version and the region fingerprint, in that order, before any
/// region data is mapped or any command is returned. The region always comes
/// from the archive beside the log; there is no fallback to installed data.
pub fn load_replay(path: &Path) -> Result<ReplayInput, ReplayLoadError> {
    let text = read_text(path)?;
    let mut records = parse_log(&text, true)?.into_iter();
    let Some(LogRecord::Header(header)) = records.next() else {
        return Err(ReplayLoadError::MissingHeader);
    };
    // `parse_log` guarantees every later record is a command.
    let mut commands: Vec<Command> = records
        .filter_map(|record| match record {
            LogRecord::Command(command) => Some(command),
            LogRecord::Header(_) => None,
        })
        .collect();

    let archive_path = region_archive_path(path)
        .ok_or_else(|| ReplayLoadError::Io(format!("{} has no file name", path.display())))?;
    let source = fs::read(&archive_path).map_err(|e| {
        ReplayLoadError::Archive(match e.kind() {
            io::ErrorKind::NotFound => RegionLoadError::NotFound(header.region.clone()),
            _ => RegionLoadError::Io {
                region: header.region.clone(),
                message: format!("{}: {e}", archive_path.display()),
            },
        })
    })?;
    let region = load_region_bytes(source, &header.region)?;
    if region.fingerprint != header.region_fingerprint {
        return Err(ReplayLoadError::RegionFingerprintMismatch {
            expected: header.region_fingerprint,
            actual: region.fingerprint,
        });
    }
    let checkpoint = if header.log_format_version == 2 {
        let path = checkpoint_archive_path(path)
            .ok_or_else(|| ReplayLoadError::Checkpoint("missing checkpoint path".into()))?;
        let (bundle, _) = if path.is_dir() {
            crate::save::monthly::read(&path, &crate::save::replay_compatibility(), &|| false)
                .map_err(|e| ReplayLoadError::Checkpoint(e.to_string()))?
        } else {
            let bytes = crate::save::file_store::read_bounded(&path, crate::save::MAX_SAVE_BYTES)
                .map_err(|e| ReplayLoadError::Checkpoint(e.to_string()))?;
            crate::save::decode_save(&bytes, &crate::save::replay_compatibility())
                .map_err(|e| ReplayLoadError::Checkpoint(e.to_string()))?
        };
        let base = bundle.checkpoint;
        let mut expected = header.clone();
        expected.log_format_version = LOG_FORMAT_VERSION;
        if base.header != expected
            || base.region_source != region.source_bytes
            || commands.get(..base.commands.len()) != Some(base.commands.as_slice())
        {
            return Err(ReplayLoadError::Checkpoint(
                "recording prefix differs from checkpoint".into(),
            ));
        }
        commands.drain(..base.commands.len());
        Some(base)
    } else {
        None
    };
    Ok(ReplayInput {
        header,
        region,
        commands,
        checkpoint,
    })
}
