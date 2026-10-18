use domain::events::DomainEvent;
use domain::session::GameSession;

use crate::command::{Command, CommandHandler};
use crate::error::CommandError;
use crate::ports::{CommandLogSink, LogRecord};

/// Records each successfully handled command. A failing sink is reported but
/// never fails the command: the player's action already happened.
pub struct CommandLogLayer<H> {
    inner: H,
    sink: Box<dyn CommandLogSink>,
}

impl<H> CommandLogLayer<H> {
    pub fn new(inner: H, sink: Box<dyn CommandLogSink>) -> Self {
        Self { inner, sink }
    }
}

impl<H: CommandHandler> CommandHandler for CommandLogLayer<H> {
    fn handle(
        &mut self,
        session: &mut GameSession,
        command: &Command,
    ) -> Result<Vec<DomainEvent>, CommandError> {
        let events = self.inner.handle(session, command)?;
        if let Err(error) = self.sink.append(&LogRecord::Command(command.clone())) {
            tracing::error!(%error, ?command, "failed to append command to log");
        }
        Ok(events)
    }
}
