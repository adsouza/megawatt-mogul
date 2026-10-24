use domain::events::DomainEvent;
use domain::session::GameSession;

use crate::command::{Command, CommandHandler};
use crate::error::CommandError;

/// After every successful command, checks the session's invariants. A
/// violation is a simulation bug, never player error, so it panics.
pub struct InvariantCheckLayer<H> {
    inner: H,
}

impl<H> InvariantCheckLayer<H> {
    pub fn new(inner: H) -> Self {
        Self { inner }
    }
}

impl<H: CommandHandler> CommandHandler for InvariantCheckLayer<H> {
    fn handle(
        &mut self,
        session: &mut GameSession,
        command: &Command,
    ) -> Result<Vec<DomainEvent>, CommandError> {
        let events = self.inner.handle(session, command)?;
        if let Err(violation) = session.check_invariants() {
            panic!("invariant violated after {command:?}: {violation}");
        }
        Ok(events)
    }
}
