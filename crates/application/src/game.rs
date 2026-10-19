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

/// One running game: the session and the pipeline every command goes through.
pub struct Game {
    session: GameSession,
    handler: Box<dyn CommandHandler>,
    log_health: LogHealth,
    projections: Projections,
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
        })
    }

    pub fn execute(&mut self, command: Command) -> Result<Vec<DomainEvent>, CommandError> {
        let events = self.handler.handle(&mut self.session, &command)?;
        self.projections.apply_all(&events, &self.session);
        Ok(events)
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
    /// longer reproduces it, so the player should be told their save is
    /// incomplete.
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
