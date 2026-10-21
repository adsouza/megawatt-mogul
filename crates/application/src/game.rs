//! The `Game` service: a session plus its command pipeline, and replay.

use domain::SIM_RULES_VERSION;
use domain::events::DomainEvent;
use domain::ids::RegionId;
use domain::session::{GameSession, GameSettings};

use crate::command::{Command, CommandHandler};
use crate::error::{CommandError, ReplayError};
use crate::pipeline::{CoreHandler, LogHealth, build_pipeline};
use crate::ports::{
    CommandLogError, CommandLogSink, LOG_FORMAT_VERSION, LoadedRegion, LogHeader, RegionCatalog,
    RegionListing,
};
use crate::projections::Projections;
use crate::{CheckpointSummary, GameCheckpoint, LogRecord, RestoreError};

/// One running game: the session and the pipeline every command goes through.
pub struct Game {
    session: GameSession,
    handler: Box<dyn CommandHandler>,
    log_health: LogHealth,
    projections: Projections,
    header: LogHeader,
    region_source: Vec<u8>,
    journal: Vec<Command>,
}

impl Game {
    /// Loads `region` once, writes the log header and region archive through
    /// `sink`, and only then creates the session. Any failure along the way
    /// means no game: a game without its replay inputs is never returned.
    pub fn start_new(
        catalog: &dyn RegionCatalog,
        region: &RegionId,
        seed: u64,
        settings: GameSettings,
        mut sink: Box<dyn CommandLogSink>,
    ) -> Result<Game, CommandError> {
        let loaded = catalog.load(region)?;
        let header = LogHeader {
            log_format_version: LOG_FORMAT_VERSION,
            sim_rules_version: SIM_RULES_VERSION,
            settings,
            region: loaded.definition.id().clone(),
            seed,
            region_fingerprint: loaded.fingerprint,
        };
        sink.initialize(&header, &loaded.source_bytes)?;
        let session = GameSession::new(loaded.definition, seed, settings);
        let projections = Projections::new(&session);
        let (handler, log_health) = build_pipeline(sink);
        Ok(Game {
            session,
            handler,
            log_health,
            projections,
            header,
            region_source: loaded.source_bytes,
            journal: Vec::new(),
        })
    }

    pub fn execute(&mut self, command: Command) -> Result<Vec<DomainEvent>, CommandError> {
        let events = self.handler.handle(&mut self.session, &command)?;
        self.projections.apply_all(&events, &self.session);
        self.journal.push(command);
        Ok(events)
    }

    pub fn checkpoint(&self) -> GameCheckpoint {
        GameCheckpoint {
            header: self.header.clone(),
            region_source: self.region_source.clone(),
            commands: self.journal.clone(),
            command_count: self.journal.len() as u64,
            state_hash: self.session.state_hash(),
            summary: CheckpointSummary::of(&self.session),
        }
    }

    /// Build and verify a candidate before recording the historical prefix in a fresh sink.
    /// Cancellation never exposes a partially replayed game to the caller.
    pub fn restore(
        region: LoadedRegion,
        checkpoint: GameCheckpoint,
        mut sink: Box<dyn CommandLogSink>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, RestoreError> {
        validate_replay(&region, &checkpoint.header)?;
        if region.definition.id() != &checkpoint.header.region {
            return Err(RestoreError::RegionIdMismatch);
        }
        if region.source_bytes != checkpoint.region_source {
            return Err(RestoreError::RegionSourceMismatch);
        }
        if checkpoint.command_count != checkpoint.commands.len() as u64 {
            return Err(RestoreError::CommandCountMismatch);
        }
        let (session, projections) =
            fold_replay(region, &checkpoint.header, &checkpoint.commands, cancelled)?;
        if session.state_hash() != checkpoint.state_hash {
            return Err(RestoreError::StateHashMismatch);
        }
        if CheckpointSummary::of(&session) != checkpoint.summary {
            return Err(RestoreError::SummaryMismatch);
        }
        if cancelled() {
            return Err(RestoreError::Cancelled);
        }
        sink.initialize(&checkpoint.header, &checkpoint.region_source)?;
        for command in &checkpoint.commands {
            if cancelled() {
                return Err(RestoreError::Cancelled);
            }
            sink.append(&LogRecord::Command(command.clone()))?;
        }
        let (handler, log_health) = build_pipeline(sink);
        Ok(Self {
            session,
            projections,
            handler,
            log_health,
            header: checkpoint.header,
            region_source: checkpoint.region_source,
            journal: checkpoint.commands,
        })
    }

    pub fn session(&self) -> &GameSession {
        &self.session
    }

    /// The read models, updated after every successful command.
    pub fn projections(&self) -> &Projections {
        &self.projections
    }

    /// The first time the command log failed to record a command, if ever.
    /// The game carries on regardless, but from that command on the log no
    /// longer reproduces it. The in-memory checkpoint journal stays complete.
    pub fn log_failure(&self) -> Option<CommandLogError> {
        self.log_health.failure()
    }
}

/// Rebuilds a session from a log. The format version, rules version and region
/// fingerprint are verified, in that order, before any session exists or any
/// command runs; commands then go through the core handler only.
pub fn replay(
    region: LoadedRegion,
    header: &LogHeader,
    commands: &[Command],
) -> Result<GameSession, ReplayError> {
    validate_replay(&region, header)?;
    fold_replay(region, header, commands, &|| false)
        .map(|(session, _)| session)
        .map_err(|error| match error {
            RestoreError::ReplayCommandFailed { error, .. } => ReplayError::Command(error),
            RestoreError::Replay(error) => error,
            _ => unreachable!("uncancelled raw replay only validates and executes commands"),
        })
}

fn validate_replay(region: &LoadedRegion, header: &LogHeader) -> Result<(), ReplayError> {
    if header.log_format_version != LOG_FORMAT_VERSION {
        return Err(ReplayError::LogFormatVersionMismatch {
            recorded: header.log_format_version,
            supported: LOG_FORMAT_VERSION,
        });
    }
    if header.sim_rules_version != SIM_RULES_VERSION {
        return Err(ReplayError::RulesVersionMismatch {
            recorded: header.sim_rules_version,
            supported: SIM_RULES_VERSION,
        });
    }
    if header.region_fingerprint != region.fingerprint {
        return Err(ReplayError::RegionFingerprintMismatch {
            expected: header.region_fingerprint,
            actual: region.fingerprint,
        });
    }
    Ok(())
}

fn fold_replay(
    region: LoadedRegion,
    header: &LogHeader,
    commands: &[Command],
    cancelled: &dyn Fn() -> bool,
) -> Result<(GameSession, Projections), RestoreError> {
    if cancelled() {
        return Err(RestoreError::Cancelled);
    }
    let mut session = GameSession::new(region.definition, header.seed, header.settings);
    let mut projections = Projections::new(&session);
    let mut handler = CoreHandler;
    for (index, command) in commands.iter().enumerate() {
        if cancelled() {
            return Err(RestoreError::Cancelled);
        }
        let events = handler
            .handle(&mut session, command)
            .map_err(|error| RestoreError::ReplayCommandFailed { index, error })?;
        projections.apply_all(&events, &session);
    }
    Ok((session, projections))
}

/// The region to enter without asking: only when there is exactly one
/// listing and it is available. A broken region next to a good one is left to
/// the picker, which can say what is wrong with it.
pub fn auto_select_region(listing: &[RegionListing]) -> Option<&RegionId> {
    match listing {
        [only] => only.entry.as_ref().ok().map(|summary| &summary.id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    /// A Bevy `Resource` must be `Send + Sync`; this fails to compile otherwise.
    #[test]
    fn game_and_its_parts_are_send_and_sync() {
        assert_send_sync::<Game>();
        assert_send_sync::<GameSession>();
        assert_send_sync::<Projections>();
        assert_send_sync::<crate::pipeline::LogHealth>();
    }
}
