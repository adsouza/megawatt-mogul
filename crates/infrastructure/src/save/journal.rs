//! Save format 3 compresses only adjacent identical tick batches. Decoding restores the original
//! command boundaries before count/hash checks or replay; it never adds the tick sizes together.
use super::codec::{MAX_SAVE_COMMANDS, malformed};
use crate::command_log::dto::CommandDto;
use application::{Command, RestoreError, SaveError};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) enum JournalRecord {
    Command(CommandDto),
    TickRun { n: u32, repeat: u32 },
}
impl From<CommandDto> for JournalRecord {
    fn from(command: CommandDto) -> Self {
        Self::Command(command)
    }
}
pub(super) fn encode(commands: &[Command]) -> Vec<JournalRecord> {
    let mut records = Vec::new();
    let mut at = 0;
    while at < commands.len() {
        if let Command::AdvanceTicks { n } = commands[at] {
            let mut end = at + 1;
            while end < commands.len() && commands[end] == commands[at] {
                end += 1;
            }
            if end - at > 1 {
                records.push(JournalRecord::TickRun {
                    n,
                    repeat: (end - at) as u32,
                });
                at = end;
                continue;
            }
        }
        records.push(JournalRecord::Command((&commands[at]).into()));
        at += 1;
    }
    records
}
pub(super) fn expand<C: Into<JournalRecord>>(
    records: Vec<C>,
    expected: u64,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<Command>, SaveError> {
    if expected > MAX_SAVE_COMMANDS as u64 || records.len() > MAX_SAVE_COMMANDS {
        return Err(SaveError::SaveTooLarge);
    }
    // Validate the expanded length before allocating. Small malicious inputs cannot request an
    // unbounded expansion, overflow a length or hide discarded/extra commands.
    let mut expanded = 0usize;
    let mut checked = Vec::with_capacity(records.len());
    for record in records {
        if cancelled() {
            return Err(SaveError::Cancelled);
        }
        let record = record.into();
        let count = match &record {
            JournalRecord::Command(_) => 1,
            JournalRecord::TickRun { repeat: 0, .. } => return Err(malformed("empty tick run")),
            JournalRecord::TickRun { repeat, .. } => *repeat as usize,
        };
        expanded = expanded
            .checked_add(count)
            .filter(|n| *n <= MAX_SAVE_COMMANDS)
            .ok_or(SaveError::SaveTooLarge)?;
        checked.push(record);
    }
    if expanded as u64 != expected {
        return Err(RestoreError::CommandCountMismatch.into());
    }
    let mut commands = Vec::with_capacity(expanded);
    for record in checked {
        let (command, repeat) = match record {
            JournalRecord::Command(command) => (command.try_into().map_err(malformed)?, 1),
            JournalRecord::TickRun { n, repeat } => (Command::AdvanceTicks { n }, repeat),
        };
        for i in 0..repeat {
            if i % 1024 == 0 && cancelled() {
                return Err(SaveError::Cancelled);
            }
            commands.push(command.clone());
        }
    }
    Ok(commands)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn combined_runs_cannot_exceed_the_expansion_limit() {
        let runs = vec![
            JournalRecord::TickRun {
                n: 8,
                repeat: 600_000,
            },
            JournalRecord::TickRun {
                n: 8,
                repeat: 600_000,
            },
        ];
        assert_eq!(
            expand(runs, MAX_SAVE_COMMANDS as u64, &|| false),
            Err(SaveError::SaveTooLarge)
        );
    }
    #[test]
    fn large_run_expansion_remains_cancellable() {
        let calls = std::cell::Cell::new(0);
        let runs = vec![JournalRecord::TickRun {
            n: 32,
            repeat: 100_000,
        }];
        let cancelled = || {
            let n = calls.get() + 1;
            calls.set(n);
            n >= 4
        };
        assert_eq!(expand(runs, 100_000, &cancelled), Err(SaveError::Cancelled));
    }
}
