use domain::events::DomainEvent;
use domain::session::{DayPhase, GameSession};

use crate::command::{Command, CommandHandler};
use crate::error::CommandError;

/// Applies each command to the session; the innermost pipeline stage.
#[derive(Debug, Default, Clone, Copy)]
pub struct CoreHandler;

impl CommandHandler for CoreHandler {
    #[tracing::instrument(skip(self, session))]
    fn handle(
        &mut self,
        session: &mut GameSession,
        command: &Command,
    ) -> Result<Vec<DomainEvent>, CommandError> {
        let events = match command {
            Command::BuildPlant { kind, cell } => session.build_plant(*kind, *cell)?.1,
            Command::BuildLine { from, to } => session.build_line(*from, *to)?.1,
            Command::CancelLineConstruction { line } => session.cancel_line_construction(*line)?,
            Command::CommitPlant { plant, hours } => session.commit_plant(*plant, *hours)?,
            Command::BeginOperating => session.begin_operating()?,
            Command::SetDispatchTarget { plant, mw } => session.set_dispatch_target(*plant, *mw)?,
            Command::SetBatteryEnabled { plant, on } => session.set_battery_enabled(*plant, *on)?,
            Command::SetAutoFollow { plant, on } => session.set_auto_follow(*plant, *on)?,
            Command::StartPlant { plant } => session.start_plant(*plant)?,
            Command::StopPlant { plant } => session.stop_plant(*plant)?,
            Command::AdvanceTicks { n } => advance_ticks(session, *n)?,
            Command::FinishReview => session.finish_review()?,
        };
        Ok(events)
    }
}

/// Ticks up to `n` times, stopping as soon as the phase leaves `Operating`.
/// The game must be active and operating, checked up front so `n = 0` is
/// validated like any other count. After that check no tick can fail, so an
/// `Err` always leaves the session untouched.
fn advance_ticks(session: &mut GameSession, n: u32) -> Result<Vec<DomainEvent>, CommandError> {
    session.require_operating()?;
    let mut events = Vec::new();
    for _ in 0..n {
        events.extend(session.tick()?);
        if session.phase() != DayPhase::Operating {
            break;
        }
    }
    Ok(events)
}
