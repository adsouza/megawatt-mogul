use std::sync::{Arc, Mutex, PoisonError};

use domain::events::DomainEvent;
use domain::session::GameSession;

use crate::command::{Command, CommandHandler};
use crate::error::CommandError;
use crate::ports::{CommandLogError, CommandLogSink, LogRecord};

/// Whether the command log is still being written: a cloneable handle on the
/// first append failure, shared between the layer that records it and
/// whoever shows it to the player.
///
/// `Send + Sync`, so the `Game` that holds one can live in a Bevy `Resource`.
#[derive(Debug, Clone, Default)]
pub struct LogHealth {
    first_failure: Arc<Mutex<Option<CommandLogError>>>,
}

impl LogHealth {
    /// The first append error, or `None` while every command has been logged.
    /// Later errors are not kept: the first is the moment the log stopped
    /// being a complete record of the game.
    pub fn failure(&self) -> Option<CommandLogError> {
        self.first_failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn record(&self, error: &CommandLogError) {
        self.first_failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(|| error.clone());
    }
}

/// Records each successfully handled command. A failing sink is reported
/// (traced, and kept in the layer's [`LogHealth`]) but never fails the
/// command: the player's action already happened.
pub struct CommandLogLayer<H> {
    inner: H,
    sink: Box<dyn CommandLogSink>,
    health: LogHealth,
}

impl<H> CommandLogLayer<H> {
    pub fn new(inner: H, sink: Box<dyn CommandLogSink>) -> Self {
        Self {
            inner,
            sink,
            health: LogHealth::default(),
        }
    }

    /// A handle on this layer's log health.
    pub fn health(&self) -> LogHealth {
        self.health.clone()
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
            self.health.record(&error);
        }
        Ok(events)
    }
}
