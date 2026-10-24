#[allow(dead_code)]
mod support;

use application::{
    Command, CommandError, CommandHandler, CommandLogError, CommandLogSink, Game,
    InMemoryCommandLog, LOG_FORMAT_VERSION, LogHeader, LogRecord, RegionFingerprint, RegionListing,
    RegionLoadError, RegionSummary, ReplayError, auto_select_region, build_pipeline, replay,
};
use domain::SIM_RULES_VERSION;
use domain::error::DomainError;
use domain::events::DomainEvent;
use domain::ids::{CellCoord, PlantId, RegionId};
use domain::region::PlantKind;
use domain::session::{DayPhase, GameSettings, SessionStatus};
use support::{StaticCatalog, empty_start, logged_commands, tiny_loaded_region};

fn tiny_catalog() -> StaticCatalog {
    StaticCatalog(vec![tiny_loaded_region()])
}

fn tiny_fingerprint() -> RegionFingerprint {
    tiny_loaded_region().fingerprint
}

fn tiny_header(seed: u64) -> LogHeader {
    LogHeader {
        log_format_version: LOG_FORMAT_VERSION,
        sim_rules_version: SIM_RULES_VERSION,
        settings: GameSettings::default(),
        region: RegionId::new("tiny"),
        seed,
        region_fingerprint: tiny_fingerprint(),
    }
}

fn game_with_log(log: InMemoryCommandLog) -> Game {
    Game::start_new(
        &tiny_catalog(),
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

#[test]
fn a_redundant_line_refusal_preserves_the_complete_checkpoint_and_journal() {
    let mut g = game();
    let from = g.session().city_views()[1].node;
    let to = g.session().plant_views()[0].node;
    let before = g.checkpoint();
    assert!(matches!(
        g.execute(Command::BuildLine { from, to }),
        Err(CommandError::Domain(DomainError::LineNotNeeded { .. }))
    ));
    assert_eq!(g.checkpoint(), before);
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
        &tiny_catalog(),
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
    assert_eq!(header.region_fingerprint, tiny_fingerprint());
    assert_eq!(log.region_source(), tiny_loaded_region().source_bytes);
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
            expected: tiny_fingerprint(),
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
        &tiny_catalog(),
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
        &tiny_catalog(),
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
        &tiny_catalog(),
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
    let source = tiny_loaded_region().source_bytes;
    log.initialize(&tiny_header(1), &source).unwrap();
    assert!(log.initialize(&tiny_header(2), b"other").is_err());
    assert_eq!(log.records(), vec![LogRecord::Header(tiny_header(1))]);
    assert_eq!(log.region_source(), source);
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
fn auto_select_requires_exactly_one_available_entry() {
    let available = |id: &str| RegionListing {
        id: RegionId::new(id),
        entry: Ok(RegionSummary {
            id: RegionId::new(id),
            name_key: format!("region-{id}"),
        }),
    };
    let broken = |id: &str| RegionListing {
        id: RegionId::new(id),
        entry: Err(RegionLoadError::Parse {
            region: RegionId::new(id),
            message: "1:1: unexpected token".to_owned(),
        }),
    };
    assert_eq!(auto_select_region(&[]), None);
    // One ok: the only choice.
    let one = [available("eastern")];
    assert_eq!(auto_select_region(&one), Some(&RegionId::new("eastern")));
    // One ok plus one broken: the player should hear about the broken one
    // rather than be silently sent into the survivor.
    assert_eq!(
        auto_select_region(&[available("eastern"), broken("western")]),
        None
    );
    assert_eq!(
        auto_select_region(&[broken("western"), available("eastern")]),
        None
    );
    // Only a broken one: nothing to enter.
    assert_eq!(auto_select_region(&[broken("western")]), None);
    // Two ok: a real choice.
    assert_eq!(
        auto_select_region(&[available("eastern"), available("western")]),
        None
    );
}

/// A sink that accepts its header and then fails every `fail_every`-th
/// `append` (the 2nd, 4th, ... for 2), with a message naming the call.
struct FlakySink {
    appends: u32,
    fail_every: u32,
}

impl CommandLogSink for FlakySink {
    fn initialize(
        &mut self,
        _header: &LogHeader,
        _region_source: &[u8],
    ) -> Result<(), CommandLogError> {
        Ok(())
    }

    fn append(&mut self, _record: &LogRecord) -> Result<(), CommandLogError> {
        self.appends += 1;
        if self.appends.is_multiple_of(self.fail_every) {
            Err(CommandLogError {
                message: format!("append {} failed", self.appends),
            })
        } else {
            Ok(())
        }
    }
}

#[test]
fn a_failing_sink_is_reported_and_the_game_continues() {
    let mut g = Game::start_new(
        &tiny_catalog(),
        &RegionId::new("tiny"),
        1,
        GameSettings::default(),
        Box::new(FlakySink {
            appends: 0,
            fail_every: 2,
        }),
    )
    .unwrap();
    assert_eq!(g.log_failure(), None, "a fresh log is healthy");

    g.execute(Command::BeginOperating).unwrap();
    assert_eq!(g.log_failure(), None, "the first append succeeded");

    // The 2nd append fails: the command still succeeds and is applied.
    g.execute(Command::AdvanceTicks { n: 3 }).unwrap();
    assert_eq!(g.projections().timeline.points().count(), 3);
    let failure = g.log_failure().expect("the 2nd append failed");
    assert_eq!(failure.message, "append 2 failed");

    // Play goes on; the 3rd append works, the 4th fails again, and the first
    // failure stays the one reported.
    g.execute(Command::AdvanceTicks { n: 3 }).unwrap();
    g.execute(Command::AdvanceTicks { n: 3 }).unwrap();
    assert_eq!(g.projections().timeline.points().count(), 9);
    assert_eq!(g.log_failure(), Some(failure));
}

#[test]
fn log_health_is_shared_by_the_pipeline_that_reports_into_it() {
    let (mut handler, health) = build_pipeline(Box::new(FlakySink {
        appends: 0,
        fail_every: 1,
    }));
    let observer = health.clone();
    assert_eq!(observer.failure(), None);
    let mut session = domain::session::GameSession::new(
        std::sync::Arc::new(domain::region::fixtures::tiny_region()),
        1,
        GameSettings::default(),
    );
    handler
        .handle(&mut session, &Command::BeginOperating)
        .unwrap();
    assert_eq!(observer.failure(), health.failure());
    assert_eq!(observer.failure().unwrap().message, "append 1 failed");
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
    empty_start(&mut g);
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

#[test]
fn zero_ticks_in_planning_is_the_wrong_phase_and_not_logged() {
    let log = InMemoryCommandLog::default();
    let mut g = game_with_log(log.clone());
    assert_eq!(
        g.execute(Command::AdvanceTicks { n: 0 }).unwrap_err(),
        CommandError::Domain(DomainError::WrongPhase {
            actual: DayPhase::Planning
        })
    );
    assert_eq!(log.records(), vec![LogRecord::Header(tiny_header(1))]);
}

#[test]
fn zero_ticks_after_game_over_is_refused_and_not_logged() {
    // The start fleet sits offline every month, so strikes soon end the game.
    let log = InMemoryCommandLog::default();
    let mut g = game_with_log(log.clone());
    empty_start(&mut g);
    for _ in 0..12 {
        if g.session().status() != SessionStatus::Active {
            break;
        }
        if g.session().phase() == DayPhase::Review {
            g.execute(Command::FinishReview).unwrap();
        }
        g.execute(Command::BeginOperating).unwrap();
        g.execute(Command::AdvanceTicks { n: 1_440 }).unwrap();
    }
    let SessionStatus::GameOver(reason) = g.session().status() else {
        panic!("an offline fleet keeps its licence for a year");
    };
    let logged = log.records().len();
    let hash = g.session().state_hash();
    assert_eq!(
        g.execute(Command::AdvanceTicks { n: 0 }).unwrap_err(),
        CommandError::Domain(DomainError::GameOver { reason })
    );
    assert_eq!(log.records().len(), logged);
    assert_eq!(g.session().state_hash(), hash);
}
