//! Integration tests for `FileCommandLog`, `read_command_log` and `load_replay`.
//! Every test works in its own `TempDir`; nothing touches the cwd or environment.

use std::fs;
use std::path::{Path, PathBuf};

use application::{
    Command, CommandError, CommandLogSink, Game, LOG_FORMAT_VERSION, LogHeader, LogRecord,
    RegionFingerprint, RegionLoadError, ReplayError, replay,
};
use domain::SIM_RULES_VERSION;
use domain::ids::{CellCoord, LineId, NodeId, PlantId, RegionId};
use domain::region::PlantKind;
use domain::session::GameSettings;
use domain::time::TICKS_PER_DAY;
use domain::units::Megawatts;
use infrastructure::command_log::dto::{CommandDto, LogDtoError, LogRecordDto};
use infrastructure::command_log::{
    FileCommandLog, ReplayLoadError, load_replay, read_command_log, region_archive_path,
};
use infrastructure::region::{RonRegionCatalog, load_region_bytes};
use proptest::array::uniform24;
use proptest::prelude::*;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const MINIMAL: &[u8] = include_bytes!("support/minimal_region.ron");

fn minimal_region_bytes() -> Vec<u8> {
    MINIMAL.to_vec()
}

fn fingerprint_of(source: &[u8]) -> RegionFingerprint {
    RegionFingerprint::new(Sha256::digest(source).into())
}

fn header_for(source: &[u8], seed: u64) -> LogHeader {
    LogHeader {
        log_format_version: LOG_FORMAT_VERSION,
        sim_rules_version: SIM_RULES_VERSION,
        settings: GameSettings::default(),
        region: RegionId::new("minimal"),
        seed,
        region_fingerprint: fingerprint_of(source),
    }
}

/// A `TempDir` holding `minimal.ron` as the "shipped" region, with the log to
/// be written beside it.
struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("minimal.ron"), MINIMAL).unwrap();
        Self { dir }
    }

    fn shipped(&self) -> PathBuf {
        self.dir.path().join("minimal.ron")
    }

    fn log(&self) -> PathBuf {
        self.dir.path().join("game.log")
    }

    fn archive(&self) -> PathBuf {
        region_archive_path(&self.log()).unwrap()
    }

    fn catalog(&self) -> RonRegionCatalog {
        RonRegionCatalog::new(self.dir.path())
    }

    /// Plays one operating day on a real game logging to a `FileCommandLog`.
    fn play_a_day(&self, seed: u64) -> Game {
        let sink = FileCommandLog::create(&self.log()).unwrap();
        let mut game = Game::start_new(
            &self.catalog(),
            &RegionId::new("minimal"),
            seed,
            GameSettings::default(),
            Box::new(sink),
        )
        .unwrap();
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
            .unwrap();
        game
    }
}

fn arb_kind() -> impl Strategy<Value = PlantKind> {
    prop::sample::select(PlantKind::ALL.to_vec())
}

fn arb_command() -> impl Strategy<Value = Command> {
    let plant = any::<u32>().prop_map(PlantId);
    let node = any::<u32>().prop_map(NodeId);
    let mw = prop::num::f64::NORMAL | prop::num::f64::SUBNORMAL | prop::num::f64::ZERO;
    prop_oneof![
        (arb_kind(), any::<u16>(), any::<u16>()).prop_map(|(kind, x, y)| Command::BuildPlant {
            kind,
            cell: CellCoord { x, y }
        }),
        (node.clone(), node).prop_map(|(from, to)| Command::BuildLine { from, to }),
        (plant.clone(), uniform24(any::<bool>()))
            .prop_map(|(plant, hours)| Command::CommitPlant { plant, hours }),
        Just(Command::BeginOperating),
        (plant.clone(), mw).prop_map(|(plant, mw)| Command::SetDispatchTarget {
            plant,
            mw: Megawatts::new(mw)
        }),
        (plant.clone(), any::<bool>())
            .prop_map(|(plant, on)| Command::SetBatteryEnabled { plant, on }),
        (plant.clone(), any::<bool>()).prop_map(|(plant, on)| Command::SetAutoFollow { plant, on }),
        plant
            .clone()
            .prop_map(|plant| Command::StartPlant { plant }),
        plant.prop_map(|plant| Command::StopPlant { plant }),
        any::<u32>().prop_map(|n| Command::AdvanceTicks { n }),
        Just(Command::FinishReview),
        any::<u32>().prop_map(|id| Command::CancelLineConstruction { line: LineId(id) }),
    ]
}

fn header_record_line(header: &LogHeader) -> String {
    ron::ser::to_string(&LogRecordDto::from(&LogRecord::Header(header.clone()))).unwrap()
}

fn write_log(path: &Path, lines: &[&str]) {
    let mut text = lines.join("\n");
    text.push('\n');
    fs::write(path, text).unwrap();
}

#[test]
fn non_finite_dispatch_target_is_rejected_in_the_log() {
    // The DTO boundary refuses a NaN or infinite target before it can become
    // a `Megawatts`, which only debug-asserts finiteness.
    for mw in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let dto = CommandDto::SetDispatchTarget { plant: 1, mw };
        assert!(
            matches!(
                Command::try_from(dto),
                Err(LogDtoError::NonFiniteMegawatts(bad)) if bad.is_nan() == mw.is_nan()
            ),
            "{mw}"
        );
    }
    let finite = CommandDto::SetDispatchTarget { plant: 1, mw: -3.5 };
    assert_eq!(
        Command::try_from(finite).unwrap(),
        Command::SetDispatchTarget {
            plant: PlantId(1),
            mw: Megawatts::new(-3.5)
        }
    );

    // And a log on disk carrying one is refused at its line, as a parse error.
    let fixture = Fixture::new();
    let source = minimal_region_bytes();
    let header = header_record_line(&header_for(&source, 1));
    let bad = ron::ser::to_string(&LogRecordDto::Command(CommandDto::SetDispatchTarget {
        plant: 1,
        mw: f64::NAN,
    }))
    .unwrap();
    write_log(&fixture.log(), &[&header, &bad]);
    let err = read_command_log(&fixture.log()).unwrap_err();
    assert!(
        err.message.contains("line 2") && err.message.contains("finite"),
        "{err}"
    );
}

proptest! {
    #[test]
    fn round_trips_any_command_sequence(cmds in prop::collection::vec(arb_command(), 0..50)) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.log");
        let mut log = FileCommandLog::create(&path).unwrap();
        let source = minimal_region_bytes();
        let header = header_for(&source, 3);
        log.initialize(&header, &source).unwrap();
        let records: Vec<LogRecord> = std::iter::once(LogRecord::Header(header))
            .chain(cmds.into_iter().map(LogRecord::Command))
            .collect();
        for r in records.iter().skip(1) {
            log.append(r).unwrap();
        }
        prop_assert_eq!(read_command_log(&path).unwrap(), records);
    }
}

#[test]
fn each_record_is_one_line() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("game.log");
    let mut log = FileCommandLog::create(&path).unwrap();
    let source = minimal_region_bytes();
    log.initialize(&header_for(&source, 1), &source).unwrap();
    log.append(&LogRecord::Command(Command::BeginOperating))
        .unwrap();
    log.append(&LogRecord::Command(Command::CommitPlant {
        plant: PlantId(7),
        hours: [true; 24],
    }))
    .unwrap();
    // Flushed after every append: readable while the sink is still alive.
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 3);
    assert!(text.ends_with('\n'));
}

#[test]
fn refuses_to_overwrite_existing_log() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("game.log");
    fs::write(&path, "precious").unwrap();
    assert!(FileCommandLog::create(&path).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "precious");
    let _first = FileCommandLog::create(&dir.path().join("other.log")).unwrap();
    assert!(FileCommandLog::create(&dir.path().join("other.log")).is_err());
}

#[test]
fn file_log_replays_to_identical_session() {
    let fixture = Fixture::new();
    let game = fixture.play_a_day(42);

    let input = load_replay(&fixture.log()).unwrap();
    assert!(!input.commands.is_empty());
    let replayed = replay(input.region, &input.header, &input.commands).unwrap();
    assert_eq!(replayed.state_hash(), game.session().state_hash());
}

#[test]
fn archive_is_byte_for_byte_original_region() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("game.log");
    // Comments, odd whitespace and CRLF endings must survive untouched.
    let mut source = minimal_region_bytes();
    source.extend_from_slice(b"\r\n\t  // trailing   comment \r\n\n\n");
    let mut log = FileCommandLog::create(&path).unwrap();
    log.initialize(&header_for(&source, 5), &source).unwrap();

    let archive = region_archive_path(&path).unwrap();
    assert_eq!(archive, dir.path().join("game.log.region.ron"));
    assert_eq!(fs::read(&archive).unwrap(), source);

    let input = load_replay(&path).unwrap();
    assert_eq!(input.region.source_bytes, source);
    assert_eq!(input.region.fingerprint, fingerprint_of(&source));
}

#[test]
fn replay_survives_retuned_or_deleted_shipped_region() {
    let fixture = Fixture::new();
    let game = fixture.play_a_day(7);
    let expected = game.session().state_hash();

    let retuned = String::from_utf8(MINIMAL.to_vec()).unwrap().replacen(
        "base_load_mw: 600.0",
        "base_load_mw: 650.0",
        1,
    );
    fs::write(fixture.shipped(), retuned).unwrap();
    let input = load_replay(&fixture.log()).unwrap();
    let replayed = replay(input.region, &input.header, &input.commands).unwrap();
    assert_eq!(replayed.state_hash(), expected);

    fs::remove_file(fixture.shipped()).unwrap();
    let input = load_replay(&fixture.log()).unwrap();
    let replayed = replay(input.region, &input.header, &input.commands).unwrap();
    assert_eq!(replayed.state_hash(), expected);
}

#[test]
fn explicitly_supplied_retuned_region_fails_fingerprint_check() {
    let fixture = Fixture::new();
    fixture.play_a_day(9);
    let input = load_replay(&fixture.log()).unwrap();

    let retuned = String::from_utf8(MINIMAL.to_vec()).unwrap().replacen(
        "base_load_mw: 600.0",
        "base_load_mw: 650.0",
        1,
    );
    let supplied = load_region_bytes(retuned.into_bytes(), &RegionId::new("minimal")).unwrap();
    let err = replay(supplied, &input.header, &input.commands).unwrap_err();
    assert!(matches!(
        err,
        ReplayError::RegionFingerprintMismatch { expected, actual }
            if expected == input.header.region_fingerprint && actual != expected
    ));
}

#[test]
fn mismatched_rules_and_formats_fail_before_commands() {
    let dir = TempDir::new().unwrap();
    let source = minimal_region_bytes();
    let header = header_for(&source, 1);

    // An unsupported format may have a header shape we cannot decode, and
    // command lines we cannot either: the version envelope alone decides.
    let future = dir.path().join("future.log");
    write_log(
        &future,
        &[
            "Header((log_format_version: 99, sim_rules_version: 1, shape: \"unknown\"))",
            "Command(NotACommand(\"x\"))",
        ],
    );
    assert_eq!(
        load_replay(&future).unwrap_err(),
        ReplayLoadError::LogFormatVersionMismatch {
            recorded: 99,
            supported: LOG_FORMAT_VERSION
        }
    );
    assert!(read_command_log(&future).is_err());

    // A different rules version fails before the (here: absent) archive is
    // touched and before the (here: undecodable) commands are.
    let mut other_rules = header.clone();
    other_rules.sim_rules_version = SIM_RULES_VERSION + 1;
    let rules = dir.path().join("rules.log");
    write_log(
        &rules,
        &[&header_record_line(&other_rules), "Command(NotACommand(1))"],
    );
    assert_eq!(
        load_replay(&rules).unwrap_err(),
        ReplayLoadError::RulesVersionMismatch {
            recorded: SIM_RULES_VERSION + 1,
            supported: SIM_RULES_VERSION
        }
    );
}

#[test]
fn corrupted_or_missing_archive_never_uses_shipped_fallback() {
    let fixture = Fixture::new();
    fixture.play_a_day(11);
    assert!(fixture.shipped().exists());

    let good = fs::read(fixture.archive()).unwrap();

    // Garbage archive: a parse error, not the shipped file.
    fs::write(fixture.archive(), b"(((not ron").unwrap();
    assert!(matches!(
        load_replay(&fixture.log()).unwrap_err(),
        ReplayLoadError::Archive(RegionLoadError::Parse { .. })
    ));

    // A valid but different region: a fingerprint mismatch.
    let retuned = String::from_utf8(good.clone()).unwrap().replacen(
        "base_load_mw: 600.0",
        "base_load_mw: 650.0",
        1,
    );
    fs::write(fixture.archive(), retuned).unwrap();
    assert!(matches!(
        load_replay(&fixture.log()).unwrap_err(),
        ReplayLoadError::RegionFingerprintMismatch { .. }
    ));

    // Missing archive.
    fs::remove_file(fixture.archive()).unwrap();
    assert!(matches!(
        load_replay(&fixture.log()).unwrap_err(),
        ReplayLoadError::Archive(RegionLoadError::NotFound(_))
    ));

    // Restoring the original bytes recovers the game.
    fs::write(fixture.archive(), good).unwrap();
    assert!(load_replay(&fixture.log()).is_ok());
}

#[test]
fn archive_or_header_failure_prevents_initialisation() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("game.log");
    let source = minimal_region_bytes();
    let header = header_for(&source, 1);

    // Appending before initialisation is refused.
    let mut log = FileCommandLog::create(&path).unwrap();
    assert!(
        log.append(&LogRecord::Command(Command::BeginOperating))
            .is_err()
    );

    // Bytes that do not match the header's fingerprint: nothing is written.
    let mut wrong = source.clone();
    wrong.push(b' ');
    assert!(log.initialize(&header, &wrong).is_err());
    assert!(!region_archive_path(&path).unwrap().exists());
    assert_eq!(fs::read(&path).unwrap(), b"");

    // The failed sink stays unusable, even with the right bytes.
    assert!(log.initialize(&header, &source).is_err());
    assert!(
        log.append(&LogRecord::Command(Command::BeginOperating))
            .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), b"");

    // A game cannot start on a sink whose archive cannot be created.
    let fixture = Fixture::new();
    fs::write(fixture.archive(), "squatter").unwrap();
    let sink = FileCommandLog::create(&fixture.log()).unwrap();
    let result = Game::start_new(
        &fixture.catalog(),
        &RegionId::new("minimal"),
        1,
        GameSettings::default(),
        Box::new(sink),
    );
    assert!(matches!(result, Err(CommandError::Log(_))));
    assert_eq!(fs::read(fixture.log()).unwrap(), b"");
}

#[test]
fn refuses_to_overwrite_existing_archive() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("game.log");
    let archive = region_archive_path(&path).unwrap();
    fs::write(&archive, "do not touch").unwrap();
    let source = minimal_region_bytes();

    let mut log = FileCommandLog::create(&path).unwrap();
    assert!(log.initialize(&header_for(&source, 1), &source).is_err());
    assert_eq!(fs::read_to_string(&archive).unwrap(), "do not touch");
    assert_eq!(fs::read(&path).unwrap(), b"");
}

#[test]
fn initialises_only_once_and_never_appends_a_second_header() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("game.log");
    let source = minimal_region_bytes();
    let header = header_for(&source, 1);
    let mut log = FileCommandLog::create(&path).unwrap();
    log.initialize(&header, &source).unwrap();
    assert!(log.initialize(&header, &source).is_err());
    assert!(log.append(&LogRecord::Header(header)).is_err());
    assert_eq!(read_command_log(&path).unwrap().len(), 1);
}

#[test]
fn missing_and_duplicate_headers_are_typed_errors() {
    let dir = TempDir::new().unwrap();
    let header_line = header_record_line(&header_for(&minimal_region_bytes(), 1));

    let empty = dir.path().join("empty.log");
    fs::write(&empty, "").unwrap();
    assert_eq!(
        load_replay(&empty).unwrap_err(),
        ReplayLoadError::MissingHeader
    );
    assert!(read_command_log(&empty).is_err());

    let headless = dir.path().join("headless.log");
    write_log(&headless, &["Command(BeginOperating)"]);
    assert_eq!(
        load_replay(&headless).unwrap_err(),
        ReplayLoadError::MissingHeader
    );

    let twice = dir.path().join("twice.log");
    write_log(
        &twice,
        &[&header_line, "Command(BeginOperating)", &header_line],
    );
    assert_eq!(
        load_replay(&twice).unwrap_err(),
        ReplayLoadError::DuplicateHeader
    );
    assert!(read_command_log(&twice).is_err());
}

#[test]
fn malformed_lines_and_fingerprints_are_typed_errors() {
    let dir = TempDir::new().unwrap();
    let header_line = header_record_line(&header_for(&minimal_region_bytes(), 1));

    let garbled = dir.path().join("garbled.log");
    write_log(&garbled, &[&header_line, "Command(BeginOperating"]);
    assert!(matches!(
        load_replay(&garbled).unwrap_err(),
        ReplayLoadError::Parse { line: 2, .. }
    ));

    // Uppercase and short fingerprints are rejected, not normalised.
    let hex = header_line
        .split("region_fingerprint:\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(hex.len(), 64);
    for bad in [hex.to_uppercase(), hex[..62].to_owned(), "zz".repeat(32)] {
        let log = dir.path().join("bad_fingerprint.log");
        write_log(&log, &[&header_line.replace(&hex, &bad)]);
        assert!(matches!(
            load_replay(&log).unwrap_err(),
            ReplayLoadError::Parse { line: 1, .. }
        ));
        fs::remove_file(&log).unwrap();
    }
}

#[test]
fn missing_log_file_is_an_io_error() {
    let dir = TempDir::new().unwrap();
    assert!(matches!(
        load_replay(&dir.path().join("nope.log")).unwrap_err(),
        ReplayLoadError::Io { .. }
    ));
}
