//! Starting a new game: the command log on disk, then `Game::start_new`.
//!
//! The wall clock is allowed here, and only here in the simulation bridge: it names the log file
//! and seeds the default seed. Neither reaches the domain except as the seed the player confirms.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(not(target_arch = "wasm32"))]
use application::{CommandError, RegionCatalog};
use application::{CommandLogError, Game};
use bevy::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use domain::ids::RegionId;
use domain::session::GameSettings;
use infrastructure::command_log::{FileCommandLog, region_archive_path};

use super::{Catalog, CommandFailed, NewGameRequest, Session};
#[cfg(not(target_arch = "wasm32"))]
use crate::crash::CrashContext;
#[cfg(not(target_arch = "wasm32"))]
use crate::paths::LogDir;
use crate::state::AppState;

/// How many `-2`, `-3`, … suffixes to try before giving up on a log name.
const MAX_LOG_NAME_ATTEMPTS: u32 = 1_000;

/// A seed for the menu to offer, from the wall clock's nanoseconds (the low 64 bits). The player
/// can edit it; the game only ever sees the seed they confirm.
pub fn default_seed() -> u64 {
    // Truncating to the low 64 bits is the intent: they are the fast-changing ones.
    since_epoch().as_nanos() as u64
}

#[cfg(not(target_arch = "wasm32"))]
fn since_epoch() -> Duration {
    // A clock before 1970 is a broken clock; the epoch itself still names a valid file.
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

/// Starts the first `NewGameRequest` that succeeds and enters `InGame`; each one that fails
/// becomes a `CommandFailed`, and the app stays in the menu. Requests after a success are dropped.
#[allow(clippy::too_many_arguments)] // Independent ECS resources and message channels.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn start_new_game(
    mut requests: MessageReader<NewGameRequest>,
    catalog: Res<Catalog>,
    log_dir: Res<LogDir>,
    mut commands: Commands,
    mut generation: ResMut<super::SessionGeneration>,
    mut persistence: ResMut<super::Persistence>,
    mut next: ResMut<NextState<AppState>>,
    mut failures: MessageWriter<CommandFailed>,
    crash: Res<CrashContext>,
) {
    for request in requests.read() {
        match new_game(&catalog.0, &log_dir.0, request, since_epoch().as_secs()) {
            Ok((game, log)) => {
                crash.set_game_log(Some(log));
                generation.0 = generation.0.wrapping_add(1);
                persistence.abandon();
                persistence.enter(generation.0);
                commands.insert_resource(Session(game));
                next.set(AppState::InGame);
                break;
            }
            Err(error) => {
                warn!("cannot start a new game: {error}");
                failures.write(CommandFailed(error));
            }
        }
    }
    requests.clear();
}

/// Creates the log under `log_dir` (named for `unix_secs`, the region and seed) and starts the
/// game on it. Returns the game and its log's path.
#[cfg(not(target_arch = "wasm32"))]
fn new_game(
    catalog: &dyn RegionCatalog,
    log_dir: &Path,
    request: &NewGameRequest,
    unix_secs: u64,
) -> Result<(Game, PathBuf), CommandError> {
    let stem = log_stem(unix_secs, &request.region, request.seed);
    let (path, sink) = create_log(log_dir, &stem)?;
    let started = Game::start_new(
        catalog,
        &request.region,
        request.seed,
        GameSettings::default(),
        Box::new(sink),
    );
    match &started {
        Ok(_) => info!("new game, recording to {}", path.display()),
        // A region that fails to load fails before the log is written: remove the empty file
        // this call created, so a typo'd region leaves nothing behind. A log that failed part-way
        // is kept, as evidence.
        Err(CommandError::Region(_)) => {
            if let Err(error) = fs::remove_file(&path) {
                warn!("cannot remove the unused log {}: {error}", path.display());
            }
        }
        Err(_) => {}
    }
    started.map(|game| (game, path))
}

/// `<YYYYMMDD-HHMMSS>-<region>-<seed>`, in UTC. Characters a file name shouldn't carry become `_`.
#[cfg(not(target_arch = "wasm32"))]
fn log_stem(unix_secs: u64, region: &RegionId, seed: u64) -> String {
    let region: String = region
        .as_str()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{}-{region}-{seed}", utc_stamp(unix_secs))
}

/// Creates `<dir>/<stem>.log`, making `dir` if needed. If that name (or its region archive) is
/// taken, tries `<stem>-2.log`, `<stem>-3.log`, …: two games started in the same second with the
/// same region and seed both get a log.
pub(super) fn create_log(
    dir: &Path,
    stem: &str,
) -> Result<(PathBuf, FileCommandLog), CommandLogError> {
    fs::create_dir_all(dir).map_err(|error| CommandLogError {
        message: format!("cannot create log directory {}: {error}", dir.display()),
    })?;
    for attempt in 1..=MAX_LOG_NAME_ATTEMPTS {
        let name = match attempt {
            1 => format!("{stem}.log"),
            n => format!("{stem}-{n}.log"),
        };
        let path = dir.join(name);
        let taken = path.exists() || region_archive_path(&path).is_some_and(|a| a.exists());
        if !taken {
            // Creation is still exclusive, so a name taken since the check fails, not overwrites.
            return FileCommandLog::create(&path).map(|log| (path, log));
        }
    }
    Err(CommandLogError {
        message: format!(
            "{MAX_LOG_NAME_ATTEMPTS} logs named {stem} already exist in {}",
            dir.display()
        ),
    })
}

/// `YYYYMMDD-HHMMSS` for a Unix time, in UTC.
pub fn utc_stamp(unix_secs: u64) -> String {
    const SECS_PER_DAY: u64 = 86_400;
    let (year, month, day) = civil_from_days(unix_secs / SECS_PER_DAY);
    let secs = unix_secs % SECS_PER_DAY;
    let (hour, minute, second) = (secs / 3_600, secs % 3_600 / 60, secs % 60);
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

/// Human-readable UTC date and time for save cards; keeps filename stamps unchanged.
pub fn utc_date_time(unix_secs: u64) -> String {
    let (year, month, day) = civil_from_days(unix_secs / 86_400);
    let secs = unix_secs % 86_400;
    let (hour, minute, second) = (secs / 3_600, secs % 3_600 / 60, secs % 60);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}

/// The proleptic Gregorian (year, month, day) of a count of days since 1970-01-01. Howard
/// Hinnant's `civil_from_days`, for days on or after the epoch: eras are 400-year cycles of
/// 146,097 days, counted from 0000-03-01 so the leap day ends each year.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468; // days since 0000-03-01
    let era = z / 146_097;
    let day_of_era = z % 146_097; // [0, 146096]
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365; // [0, 399]
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100); // [0, 365]
    let shifted_month = (5 * day_of_year + 2) / 153; // [0, 11], March = 0
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = era * 400 + year_of_era + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn civil_dates_for_known_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(59), (1970, 3, 1));
        // 2000 is a leap year (divisible by 400), 2100 is not (by 100 only).
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
        assert_eq!(civil_from_days(47_540), (2100, 2, 28));
        assert_eq!(civil_from_days(47_541), (2100, 3, 1));
        assert_eq!(civil_from_days(20_818), (2026, 12, 31));
        assert_eq!(civil_from_days(20_819), (2027, 1, 1));
    }

    #[test]
    fn utc_stamps_for_known_timestamps() {
        assert_eq!(utc_stamp(0), "19700101-000000");
        assert_eq!(utc_stamp(951_782_400), "20000229-000000");
        assert_eq!(utc_stamp(1_234_567_890), "20090213-233130");
        assert_eq!(utc_stamp(1_700_000_000), "20231114-221320");
        assert_eq!(utc_stamp(4_102_444_799), "20991231-235959");
    }

    #[test]
    fn display_timestamps_are_readable_and_keep_the_exact_saved_second() {
        assert_eq!(utc_date_time(0), "1970-01-01 00:00:00");
        assert_eq!(utc_date_time(951_782_400), "2000-02-29 00:00:00");
        assert_eq!(utc_date_time(1_700_000_000), "2023-11-14 22:13:20");
    }

    #[test]
    fn the_log_stem_is_time_region_seed() {
        let region = RegionId::new("eastern_interconnection");
        assert_eq!(
            log_stem(1_700_000_000, &region, 42),
            "20231114-221320-eastern_interconnection-42"
        );
        assert_eq!(
            log_stem(0, &RegionId::new("../a b"), 1),
            "19700101-000000-___a_b-1"
        );
    }

    #[test]
    fn a_log_is_created_in_a_missing_directory() {
        let root = TempDir::new().unwrap();
        let dir = root.path().join("logs").join("deep");

        let (path, _log) = create_log(&dir, "stem").unwrap();

        assert_eq!(path, dir.join("stem.log"));
        assert!(path.is_file());
    }

    #[test]
    fn a_taken_log_name_gets_the_next_suffix() {
        let root = TempDir::new().unwrap();
        let dir = root.path();

        let (first, _a) = create_log(dir, "stem").unwrap();
        let (second, _b) = create_log(dir, "stem").unwrap();
        let (third, _c) = create_log(dir, "stem").unwrap();

        assert_eq!(first, dir.join("stem.log"));
        assert_eq!(second, dir.join("stem-2.log"));
        assert_eq!(third, dir.join("stem-3.log"));
    }

    #[test]
    fn a_leftover_archive_also_takes_the_name() {
        let root = TempDir::new().unwrap();
        let dir = root.path();
        fs::write(dir.join("stem.log.region.ron"), "someone else's archive").unwrap();

        let (path, _log) = create_log(dir, "stem").unwrap();

        assert_eq!(path, dir.join("stem-2.log"));
        assert_eq!(
            fs::read_to_string(dir.join("stem.log.region.ron")).unwrap(),
            "someone else's archive"
        );
    }

    #[test]
    fn an_unusable_log_directory_is_a_log_error() {
        let root = TempDir::new().unwrap();
        let file = root.path().join("not-a-dir");
        fs::write(&file, "").unwrap();

        let error = create_log(&file, "stem").unwrap_err();

        assert!(error.message.contains("not-a-dir"), "{}", error.message);
    }
}

#[cfg(target_arch = "wasm32")]
fn since_epoch() -> Duration {
    Duration::from_millis(js_sys::Date::now().max(0.0) as u64)
}
#[cfg(target_arch = "wasm32")]
pub(super) fn start_new_game(
    mut requests: MessageReader<NewGameRequest>,
    catalog: Res<Catalog>,
    mut commands: Commands,
    mut generation: ResMut<super::SessionGeneration>,
    mut persistence: ResMut<super::Persistence>,
    mut next: ResMut<NextState<AppState>>,
    mut failures: MessageWriter<CommandFailed>,
) {
    for request in requests.read() {
        match Game::start_new(
            &catalog.0,
            &request.region,
            request.seed,
            GameSettings::default(),
            Box::<application::InMemoryCommandLog>::default(),
        ) {
            Ok(game) => {
                generation.0 = generation.0.wrapping_add(1);
                persistence.abandon();
                persistence.enter(generation.0);
                commands.insert_resource(Session(game));
                next.set(AppState::InGame);
                break;
            }
            Err(error) => {
                failures.write(CommandFailed(error));
            }
        }
    }
    requests.clear();
}
