use std::sync::Arc;

use application::{
    Command, CommandError, CommandLogError, CommandLogSink, Game, InMemoryCommandLog,
    LOG_FORMAT_VERSION, LoadedRegion, LogHeader, LogRecord, RegionCatalog, RegionFingerprint,
    RegionLoadError, RegionSummary, ReplayError, auto_select_region, replay,
};
use domain::SIM_RULES_VERSION;
use domain::error::DomainError;
use domain::events::DomainEvent;
use domain::ids::{CellCoord, PlantId, RegionId};
use domain::region::PlantKind;
use domain::region::fixtures::tiny_region;
use domain::session::{DayPhase, GameSettings};

const TINY_SOURCE: &[u8] = b"fake tiny region source bytes";
const TINY_FINGERPRINT: [u8; 32] = [0x11; 32];

/// The same fixture serves live play and replay.
fn tiny_loaded_region() -> LoadedRegion {
    LoadedRegion {
        definition: Arc::new(tiny_region()),
        source_bytes: TINY_SOURCE.to_vec(),
        fingerprint: RegionFingerprint::new(TINY_FINGERPRINT),
    }
}

struct StaticCatalog;

impl RegionCatalog for StaticCatalog {
    fn list(&self) -> Result<Vec<RegionSummary>, RegionLoadError> {
        Ok(vec![RegionSummary {
            id: RegionId::new("tiny"),
            name_key: "region-tiny".to_owned(),
        }])
    }

    fn load(&self, id: &RegionId) -> Result<LoadedRegion, RegionLoadError> {
        if id.as_str() == "tiny" {
            Ok(tiny_loaded_region())
        } else {
            Err(RegionLoadError::NotFound(id.clone()))
        }
    }
}

fn tiny_header(seed: u64) -> LogHeader {
    LogHeader {
        log_format_version: LOG_FORMAT_VERSION,
        sim_rules_version: SIM_RULES_VERSION,
        settings: GameSettings::default(),
        region: RegionId::new("tiny"),
        seed,
        region_fingerprint: RegionFingerprint::new(TINY_FINGERPRINT),
    }
}

fn game_with_log(log: InMemoryCommandLog) -> Game {
    Game::start_new(
        &StaticCatalog,
        &RegionId::new("tiny"),
        1,
        GameSettings::default(),
        Box::new(log),
    )
    .unwrap()
}

fn game() -> Game {
    game_with_log(InMemoryCommandLog::default())
}

/// The commands logged after the header.
fn logged_commands(log: &InMemoryCommandLog) -> Vec<Command> {
    log.records()
        .into_iter()
        .filter_map(|record| match record {
            LogRecord::Command(command) => Some(command),
            LogRecord::Header(_) => None,
        })
        .collect()
}

fn plant_of(g: &Game, kind: PlantKind) -> PlantId {
    g.session()
        .plant_views()
        .iter()
        .find(|p| p.kind == kind)
        .expect("plant of kind")
        .id
}

#[test]
fn advance_ticks_stops_at_month_close() {
    let mut g = game();
    g.execute(Command::BeginOperating).unwrap();
    let events = g.execute(Command::AdvanceTicks { n: 5_000 }).unwrap();
    assert_eq!(g.session().phase(), DayPhase::Review);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(
                e,
                DomainEvent::PhaseChanged {
                    to: DayPhase::Review,
                    ..
                }
            ))
            .count(),
        1
    );
}

#[test]
fn log_records_header_and_successful_commands_only() {
    let log = InMemoryCommandLog::default();
    let mut g = game_with_log(log.clone());
    g.execute(Command::AdvanceTicks { n: 1 }).unwrap_err(); // wrong phase
    g.execute(Command::BeginOperating).unwrap();
    assert_eq!(
        log.records(),
        vec![
            LogRecord::Header(tiny_header(1)),
            LogRecord::Command(Command::BeginOperating)
        ]
    );
}

#[test]
fn replay_reproduces_live_session() {
    let log = InMemoryCommandLog::default();
    let mut g = game_with_log(log.clone());
    for kind in [PlantKind::Coal, PlantKind::Ccgt, PlantKind::Peaker] {
        let plant = plant_of(&g, kind);
        g.execute(Command::StartPlant { plant }).unwrap();
        g.execute(Command::SetAutoFollow { plant, on: true })
            .unwrap();
    }
    g.execute(Command::BuildPlant {
        kind: PlantKind::Ccgt,
        cell: CellCoord { x: 4, y: 4 },
    })
    .unwrap();
    g.execute(Command::BeginOperating).unwrap();
    g.execute(Command::AdvanceTicks { n: 700 }).unwrap();
    g.execute(Command::AdvanceTicks { n: 5_000 }).unwrap();
    g.execute(Command::FinishReview).unwrap();
    assert_eq!(g.session().phase(), DayPhase::Planning);

    let header = match log.records().remove(0) {
        LogRecord::Header(header) => header,
        other => panic!("first record must be the header, got {other:?}"),
    };
    let replayed = replay(tiny_loaded_region(), &header, &logged_commands(&log)).unwrap();
    assert_eq!(replayed.state_hash(), g.session().state_hash());
}

#[test]
fn header_records_all_replay_inputs() {
    let log = InMemoryCommandLog::default();
    let _game = Game::start_new(
        &StaticCatalog,
        &RegionId::new("tiny"),
        42,
        GameSettings::default(),
        Box::new(log.clone()),
    )
    .unwrap();
    let records = log.records();
    assert_eq!(records, vec![LogRecord::Header(tiny_header(42))]);
    let LogRecord::Header(header) = &records[0] else {
        unreachable!()
    };
    assert_eq!(header.log_format_version, LOG_FORMAT_VERSION);
    assert_eq!(header.sim_rules_version, SIM_RULES_VERSION);
    assert_eq!(header.settings, GameSettings::default());
    assert_eq!(header.seed, 42);
    assert_eq!(header.region, RegionId::new("tiny"));
    assert_eq!(header.region_fingerprint.as_bytes(), &TINY_FINGERPRINT);
    assert_eq!(log.region_source(), TINY_SOURCE);
}

#[test]
fn version_or_region_mismatch_prevents_any_command() {
    // The first command is invalid in Planning, so running it would fail with a
    // domain error; the compatibility error must win.
    let commands = [Command::AdvanceTicks { n: 1 }];
    let other_fingerprint = RegionFingerprint::new([0x22; 32]);

    let mut bad_format = tiny_header(1);
    bad_format.log_format_version += 1;
    assert_eq!(
        replay(tiny_loaded_region(), &bad_format, &commands).unwrap_err(),
        ReplayError::LogFormatVersionMismatch {
            recorded: LOG_FORMAT_VERSION + 1,
            supported: LOG_FORMAT_VERSION,
        }
    );

    let mut bad_rules = tiny_header(1);
    bad_rules.sim_rules_version += 1;
    assert_eq!(
        replay(tiny_loaded_region(), &bad_rules, &commands).unwrap_err(),
        ReplayError::RulesVersionMismatch {
            recorded: SIM_RULES_VERSION + 1,
            supported: SIM_RULES_VERSION,
        }
    );

    let mut other_region = tiny_loaded_region();
    other_region.fingerprint = other_fingerprint;
    assert_eq!(
        replay(other_region.clone(), &tiny_header(1), &commands).unwrap_err(),
        ReplayError::RegionFingerprintMismatch {
            expected: RegionFingerprint::new(TINY_FINGERPRINT),
            actual: other_fingerprint,
        }
    );

    // Checks run in order: format, then rules, then fingerprint.
    let mut everything_wrong = tiny_header(1);
    everything_wrong.log_format_version += 1;
    everything_wrong.sim_rules_version += 1;
    assert!(matches!(
        replay(other_region.clone(), &everything_wrong, &commands),
        Err(ReplayError::LogFormatVersionMismatch { .. })
    ));
    let mut rules_and_region_wrong = tiny_header(1);
    rules_and_region_wrong.sim_rules_version += 1;
    assert!(matches!(
        replay(other_region, &rules_and_region_wrong, &commands),
        Err(ReplayError::RulesVersionMismatch { .. })
    ));

    // With compatible inputs the same commands do run, and fail in the domain.
    assert_eq!(
        replay(tiny_loaded_region(), &tiny_header(1), &commands).unwrap_err(),
        ReplayError::Command(CommandError::Domain(DomainError::WrongPhase {
            actual: DayPhase::Planning
        }))
    );
}

/// A sink whose operations can be made to fail.
struct FailingSink {
    fail_initialize: bool,
    fail_append: bool,
}

impl CommandLogSink for FailingSink {
    fn initialize(&mut self, _: &LogHeader, _: &[u8]) -> Result<(), CommandLogError> {
        if self.fail_initialize {
            Err(CommandLogError {
                message: "disk full".to_owned(),
            })
        } else {
            Ok(())
        }
    }

    fn append(&mut self, _: &LogRecord) -> Result<(), CommandLogError> {
        if self.fail_append {
            Err(CommandLogError {
                message: "disk full".to_owned(),
            })
        } else {
            Ok(())
        }
    }
}

#[test]
fn archive_or_header_initialisation_failure_prevents_new_game() {
    let result = Game::start_new(
        &StaticCatalog,
        &RegionId::new("tiny"),
        1,
        GameSettings::default(),
        Box::new(FailingSink {
            fail_initialize: true,
            fail_append: false,
        }),
    );
    assert_eq!(
        result.err(),
        Some(CommandError::Log(CommandLogError {
            message: "disk full".to_owned()
        }))
    );
}

#[test]
fn unknown_region_prevents_new_game() {
    let missing = RegionId::new("nowhere");
    let result = Game::start_new(
        &StaticCatalog,
        &missing,
        1,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    );
    assert_eq!(
        result.err(),
        Some(CommandError::Region(RegionLoadError::NotFound(missing)))
    );
}

#[test]
fn a_failing_log_append_never_fails_the_command() {
    let mut g = Game::start_new(
        &StaticCatalog,
        &RegionId::new("tiny"),
        1,
        GameSettings::default(),
        Box::new(FailingSink {
            fail_initialize: false,
            fail_append: true,
        }),
    )
    .unwrap();
    g.execute(Command::BeginOperating).unwrap();
    assert_eq!(g.session().phase(), DayPhase::Operating);
}

#[test]
fn in_memory_log_rejects_append_before_initialize() {
    let mut log = InMemoryCommandLog::default();
    assert!(
        log.append(&LogRecord::Command(Command::BeginOperating))
            .is_err()
    );
    assert!(log.records().is_empty());
}

#[test]
fn in_memory_log_accepts_only_one_header() {
    let mut log = InMemoryCommandLog::default();
    log.initialize(&tiny_header(1), TINY_SOURCE).unwrap();
    assert!(log.initialize(&tiny_header(2), b"other").is_err());
    assert_eq!(log.records(), vec![LogRecord::Header(tiny_header(1))]);
    assert_eq!(log.region_source(), TINY_SOURCE);
}

#[test]
fn domain_errors_pass_through_unchanged() {
    let mut g = game();
    assert_eq!(
        g.execute(Command::AdvanceTicks { n: 1 }).unwrap_err(),
        CommandError::Domain(DomainError::WrongPhase {
            actual: DayPhase::Planning
        })
    );
}

#[test]
fn auto_select_only_with_exactly_one_region() {
    let summary = |id: &str| RegionSummary {
        id: RegionId::new(id),
        name_key: format!("region-{id}"),
    };
    assert_eq!(auto_select_region(&[]), None);
    let one = [summary("eastern")];
    assert_eq!(auto_select_region(&one), Some(&RegionId::new("eastern")));
    assert_eq!(
        auto_select_region(&[summary("eastern"), summary("western")]),
        None
    );
}

#[test]
fn game_updates_projections_after_execute() {
    let mut g = game();
    assert_eq!(g.projections().timeline.points().count(), 0);
    g.execute(Command::BeginOperating).unwrap();
    g.execute(Command::AdvanceTicks { n: 10 }).unwrap();
    assert_eq!(g.projections().timeline.points().count(), 10);
}

#[test]
fn game_updates_forward_looking_and_lesson_projections() {
    let mut g = game();
    assert_eq!(
        g.projections().rate_review.estimate(),
        Some(&g.session().rate_review_estimate())
    );
    assert!(g.projections().lessons.latest().is_empty());

    g.execute(Command::BeginOperating).unwrap();
    // The fleet is offline, so the morning demand ramp outruns it.
    g.execute(Command::AdvanceTicks { n: 366 }).unwrap();
    assert!(g.projections().ramp_warnings.current().is_some());

    g.execute(Command::AdvanceTicks { n: 2_000 }).unwrap();
    assert_eq!(g.session().phase(), DayPhase::Review);
    assert!(!g.projections().lessons.latest().is_empty());
    assert_eq!(
        g.projections().rate_review.estimate(),
        Some(&g.session().rate_review_estimate())
    );
}

#[test]
fn failed_command_leaves_projections_unchanged() {
    let mut g = game();
    g.execute(Command::AdvanceTicks { n: 10 }).unwrap_err(); // wrong phase
    assert_eq!(g.projections().timeline.points().count(), 0);
}
