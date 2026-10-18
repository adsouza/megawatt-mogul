//! The `Game` service: a session plus its command pipeline, and replay.

use domain::SIM_RULES_VERSION;
use domain::events::DomainEvent;
use domain::ids::RegionId;
use domain::session::{GameSession, GameSettings};

use crate::command::{Command, CommandHandler};
use crate::error::{CommandError, ReplayError};
use crate::pipeline::{CoreHandler, build_pipeline};
use crate::ports::{
    CommandLogSink, LOG_FORMAT_VERSION, LoadedRegion, LogHeader, RegionCatalog, RegionSummary,
};

/// One running game: the session and the pipeline every command goes through.
pub struct Game {
    session: GameSession,
    handler: Box<dyn CommandHandler>,
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
        Ok(Game {
            session: GameSession::new(loaded.definition, seed, settings),
            handler: build_pipeline(sink),
        })
    }

    pub fn execute(&mut self, command: Command) -> Result<Vec<DomainEvent>, CommandError> {
        self.handler.handle(&mut self.session, &command)
    }

    pub fn session(&self) -> &GameSession {
        &self.session
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
    let mut session = GameSession::new(region.definition, header.seed, header.settings);
    let mut handler = CoreHandler;
    for command in commands {
        handler.handle(&mut session, command)?;
    }
    Ok(session)
}

/// The region to enter without asking, when there is exactly one to choose.
pub fn auto_select_region(regions: &[RegionSummary]) -> Option<&RegionId> {
    match regions {
        [only] => Some(&only.id),
        _ => None,
    }
}
