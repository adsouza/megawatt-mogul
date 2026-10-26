//! Crash reports. A panic anywhere in the game writes `crash-<UTC stamp>.txt` into the log
//! directory, beside the command logs. The report names the running game's command log, so the
//! backtrace comes with a replay of the game that crashed. The binary keeps its symbol table for
//! this: release builds have no debug info, so function names are all the backtrace can show, but
//! they are enough to find the code.
//!
//! [`install`] is process setup, called from `main.rs` before the app is built. Bevy's own panic
//! hooks chain to whichever hook was already installed, so this one keeps running.

use std::backtrace::Backtrace;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::panic::{self, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::Resource;

use crate::sim::utc_stamp;

/// After `crash-<stamp>.txt`, `-2` … up to this suffix: a burst of panics on several threads
/// within one second all get reports.
const MAX_REPORT_NAME_ATTEMPTS: u32 = 100;

/// What the panic hook knows about the app: the running game's command log. A resource whose
/// clones share one value, so the simulation updates the copy the hook holds. Set when a new or
/// loaded game becomes the session; cleared when the player leaves it.
#[derive(Resource, Debug, Clone, Default)]
pub struct CrashContext(Arc<Mutex<Option<PathBuf>>>);

impl CrashContext {
    pub fn set_game_log(&self, log: Option<PathBuf>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = log;
    }

    pub fn game_log(&self) -> Option<PathBuf> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Everything a crash report records, captured inside the panic hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashReport {
    pub version: String,
    pub unix_secs: u64,
    pub thread: String,
    pub message: String,
    /// `file:line:column` of the panic, when the runtime knows it.
    pub location: Option<String>,
    /// The running game's command log; `None` outside a game.
    pub game_log: Option<PathBuf>,
    pub backtrace: String,
}

impl CrashReport {
    /// Captures the panic `info` describes, with a backtrace whether or not `RUST_BACKTRACE` is set.
    pub fn capture(info: &PanicHookInfo<'_>, game_log: Option<PathBuf>) -> Self {
        let unix_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        Self {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            unix_secs,
            thread: std::thread::current()
                .name()
                .unwrap_or("<unnamed>")
                .to_owned(),
            message: info
                .payload_as_str()
                .unwrap_or("<non-string panic payload>")
                .to_owned(),
            location: info.location().map(ToString::to_string),
            game_log,
            backtrace: Backtrace::force_capture().to_string(),
        }
    }

    pub fn render(&self) -> String {
        format!(
            "Megawatt Mogul {} panicked at {} UTC\nthread: {}\nmessage: {}\nlocation: {}\n\
             game log: {}\n\nbacktrace:\n{}",
            self.version,
            utc_stamp(self.unix_secs),
            self.thread,
            self.message,
            self.location.as_deref().unwrap_or("unknown"),
            self.game_log.as_deref().map_or_else(
                || "none (no game running)".into(),
                |log| log.display().to_string()
            ),
            self.backtrace,
        )
    }

    /// Writes `<dir>/crash-<stamp>.txt`, making `dir` if needed. An existing report is never
    /// overwritten: a name already taken moves on to `crash-<stamp>-2.txt`, `-3`, ….
    pub fn write_to(&self, dir: &Path) -> io::Result<PathBuf> {
        fs::create_dir_all(dir)?;
        let stem = format!("crash-{}", utc_stamp(self.unix_secs));
        for attempt in 1..=MAX_REPORT_NAME_ATTEMPTS {
            let name = match attempt {
                1 => format!("{stem}.txt"),
                n => format!("{stem}-{n}.txt"),
            };
            let path = dir.join(name);
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    file.write_all(self.render().as_bytes())?;
                    return Ok(path);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{MAX_REPORT_NAME_ATTEMPTS} crash reports named {stem} already exist"),
        ))
    }
}

/// Wraps the current panic hook so every panic also writes a [`CrashReport`] into `log_dir`, and
/// returns the context the hook reads, to insert as a resource. The previous hook runs first, so
/// stderr output is unchanged. A report that can't be written is noted on stderr: the hook itself
/// must not panic.
pub fn install(log_dir: PathBuf) -> CrashContext {
    let context = CrashContext::default();
    let hook_context = context.clone();
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        previous(info);
        let report = CrashReport::capture(info, hook_context.game_log());
        match report.write_to(&log_dir) {
            Ok(path) => eprintln!("crash report written to {}", path.display()),
            Err(error) => eprintln!(
                "cannot write a crash report to {}: {error}",
                log_dir.display()
            ),
        }
    }));
    context
}

/// The hidden `--crash` flag: panics on purpose, so tests and the release smoke can check that
/// a build writes a report whose backtrace names this function.
#[inline(never)]
pub fn requested_crash() -> ! {
    panic!("crash requested by --crash")
}
