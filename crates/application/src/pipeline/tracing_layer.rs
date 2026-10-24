use domain::events::DomainEvent;
use domain::session::GameSession;
use tracing::field::Empty;

use crate::command::{Command, CommandHandler};
use crate::error::CommandError;

/// Wraps each command in a `command` span recording its outcome.
pub struct TracingLayer<H> {
    inner: H,
}

impl<H> TracingLayer<H> {
    pub fn new(inner: H) -> Self {
        Self { inner }
    }
}

impl<H: CommandHandler> CommandHandler for TracingLayer<H> {
    fn handle(
        &mut self,
        session: &mut GameSession,
        command: &Command,
    ) -> Result<Vec<DomainEvent>, CommandError> {
        let span = tracing::info_span!("command", ?command, ok = Empty, events = Empty);
        let _entered = span.enter();
        let result = self.inner.handle(session, command);
        match &result {
            Ok(events) => {
                span.record("ok", true);
                span.record("events", events.len());
            }
            Err(error) => {
                span.record("ok", false);
                tracing::warn!(%error, "command failed");
            }
        }
        result
    }
}
