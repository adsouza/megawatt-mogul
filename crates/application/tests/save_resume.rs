//! Complete command checkpoints, independent of diagnostic recording health.
#[allow(dead_code)]
mod support;

use application::{
    Command, CommandLogError, CommandLogSink, Game, GameCheckpoint, InMemoryCommandLog, LogHeader,
    LogRecord, ReplayCompatibility, RestoreError, SaveBundle, SaveError, SaveId, SaveKind,
    SaveMetadata,
};
use domain::ids::PlantId;
use domain::region::fixtures::tiny_region_parts;
use domain::session::{DayPhase, GameSettings};
use domain::time::TICKS_PER_DAY;
use proptest::prelude::*;
use support::{StaticCatalog, game_on, tiny_loaded_region};

fn restore(game: &Game) -> Game {
    Game::restore(
        tiny_loaded_region(),
        game.checkpoint(),
        Box::new(InMemoryCommandLog::default()),
        &|| false,
    )
    .unwrap()
}

fn same(a: &Game, b: &Game) {
    assert_eq!(a.session().state_hash(), b.session().state_hash());
    assert_eq!(a.session().plant_views(), b.session().plant_views());
    assert_eq!(a.session().city_views(), b.session().city_views());
    assert_eq!(a.session().grid_snapshot(), b.session().grid_snapshot());
    assert_eq!(a.projections(), b.projections());
    assert_eq!(a.checkpoint(), b.checkpoint());
}

#[test]
fn cancelled_line_stays_cancelled_in_snapshots_and_command_replay() {
    use domain::state::State;
    let (mut game, _) = game_on(tiny_region_parts(), 8);
    let to = game
        .session()
        .city_views()
        .into_iter()
        .find(|city| !city.connected)
        .unwrap()
        .node;
    game.execute(Command::BuildLine {
        from: game.session().anchor_node(),
        to,
    })
    .unwrap();
    let line = game
        .session()
        .line_views()
        .into_iter()
        .find(|line| {
            matches!(
                line.status,
                domain::grid::LineStatus::UnderConstruction { .. }
            )
        })
        .unwrap()
        .id;
    let command = Command::CancelLineConstruction { line };
    assert_eq!(
        Command::from_state_bytes(&command.state_bytes()).unwrap(),
        command
    );
    game.execute(command).unwrap();
    same(&game, &restore(&game));
    let mut checkpoint = game.checkpoint();
    checkpoint.snapshot = None;
    checkpoint.state_hash = game.session().state_hash();
    let mut replayed = Game::restore(
        tiny_loaded_region(),
        checkpoint,
        Box::new(InMemoryCommandLog::default()),
        &|| false,
    )
    .unwrap();
    same(&game, &replayed);
    for game in [&mut game, &mut replayed] {
        assert!(
            !game
                .session()
                .line_views()
                .iter()
                .any(|view| view.id == line)
        );
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
            .unwrap();
        assert!(
            !game
                .session()
                .line_views()
                .iter()
                .any(|view| view.id == line)
        );
        same(game, &restore(game));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn completed_supply_history_survives_later_ticks_batching_and_resume(seed in 0u64..100, first in 1u32..TICKS_PER_DAY) {
        let (mut game, _) = game_on(tiny_region_parts(), seed);
        let month = game.session().clock().month();
        let forecast = game.session().hourly_forecast();
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: first }).unwrap();
        prop_assert!(game.projections().timeline.completed_days().is_empty());
        game.execute(Command::AdvanceTicks { n: TICKS_PER_DAY - first }).unwrap();
        let archived = game.projections().timeline.completed_days()[0].clone();
        prop_assert_eq!(archived.month, month);
        prop_assert_eq!(archived.forecast, forecast);
        prop_assert_eq!(archived.points.len(), TICKS_PER_DAY as usize);
        prop_assert_eq!(archived.points.iter().map(|p| p.tick).collect::<Vec<_>>(), (0..TICKS_PER_DAY).collect::<Vec<_>>());
        let report = game.projections().monthly_reports.latest().unwrap();
        let energy = archived.points.iter().map(|p| p.demand.value()).sum::<f64>() / 60.0 * f64::from(month.days_in_month());
        prop_assert!((energy - report.demand.value()).abs() < 1e-6);
        same(&game, &restore(&game));
        game.execute(Command::FinishReview).unwrap();
        prop_assert_eq!(&game.projections().timeline.completed_days()[0], &archived);
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: first }).unwrap();
        prop_assert_eq!(&game.projections().timeline.completed_days()[0], &archived);
        same(&game, &restore(&game));
        game.execute(Command::AdvanceTicks { n: TICKS_PER_DAY - first }).unwrap();
        let history = game.projections().timeline.completed_days();
        prop_assert_eq!(history.len(), 2);
        prop_assert_eq!(&history[0], &archived);
        prop_assert_eq!(history[1].month, month.next());
        same(&game, &restore(&game));
    }
}

#[test]
fn completed_supply_history_keeps_the_forecast_from_before_month_close_commissioning() {
    let mut parts = tiny_region_parts();
    parts.lines.cells_per_month = 100;
    let (mut game, _) = game_on(parts, 8);
    let city = game
        .session()
        .city_views()
        .into_iter()
        .find(|c| !c.connected)
        .unwrap();
    game.execute(Command::BuildLine {
        from: game.session().anchor_node(),
        to: city.node,
    })
    .unwrap();
    let forecast = game.session().hourly_forecast();
    game.execute(Command::BeginOperating).unwrap();
    game.execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
        .unwrap();
    assert!(
        game.session()
            .city_views()
            .iter()
            .find(|c| c.id == city.id)
            .unwrap()
            .connected
    );
    assert_ne!(game.session().hourly_forecast(), forecast);
    assert_eq!(
        game.projections().timeline.completed_days()[0].forecast,
        forecast
    );
}

#[test]
fn save_resume_preserves_each_phase_and_original_batches() {
    let (mut game, _) = game_on(tiny_region_parts(), 8);
    same(&game, &restore(&game));
    game.execute(Command::BeginOperating).unwrap();
    for n in [0, 1, 67, 109] {
        game.execute(Command::AdvanceTicks { n }).unwrap();
        same(&game, &restore(&game));
    }
    assert!(
        game.checkpoint()
            .commands
            .contains(&Command::AdvanceTicks { n: 0 })
    );
    game.execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
        .unwrap();
    assert_eq!(game.session().phase(), DayPhase::Review);
    same(&game, &restore(&game));
    if game.session().status() == domain::session::SessionStatus::Active {
        game.execute(Command::FinishReview).unwrap();
        same(&game, &restore(&game));
    }
}

struct FailingSink {
    accepted: usize,
}
impl CommandLogSink for FailingSink {
    fn initialize(&mut self, _: &LogHeader, _: &[u8]) -> Result<(), CommandLogError> {
        Ok(())
    }
    fn append(&mut self, _: &LogRecord) -> Result<(), CommandLogError> {
        self.accepted += 1;
        if self.accepted > 2 {
            Err(CommandLogError {
                message: "disk full".into(),
            })
        } else {
            Ok(())
        }
    }
}

#[test]
fn save_resume_keeps_history_after_mid_run_recording_failure_and_excludes_refusals() {
    let loaded = tiny_loaded_region();
    let mut game = Game::start_new(
        &StaticCatalog(vec![loaded.clone()]),
        loaded.definition.id(),
        4,
        GameSettings::default(),
        Box::new(FailingSink { accepted: 0 }),
    )
    .unwrap();
    game.execute(Command::BeginOperating).unwrap();
    for n in [2, 3, 4, 0] {
        game.execute(Command::AdvanceTicks { n }).unwrap();
    }
    assert!(game.log_failure().is_some());
    let before = game.checkpoint();
    assert!(
        game.execute(Command::StopPlant {
            plant: PlantId(u32::MAX)
        })
        .is_err()
    );
    assert_eq!(game.checkpoint(), before);
    assert_eq!(before.commands.len(), 5);
    same(&game, &restore(&game));
}

#[test]
fn save_resume_seeds_a_fresh_branch_once() {
    let (mut game, _) = game_on(tiny_region_parts(), 10);
    game.execute(Command::BeginOperating).unwrap();
    game.execute(Command::AdvanceTicks { n: 61 }).unwrap();
    let checkpoint = game.checkpoint();
    let log = InMemoryCommandLog::default();
    let mut branch = Game::restore(
        tiny_loaded_region(),
        checkpoint.clone(),
        Box::new(log.clone()),
        &|| false,
    )
    .unwrap();
    assert_eq!(log.records().len(), checkpoint.commands.len() + 1);
    branch.execute(Command::AdvanceTicks { n: 5 }).unwrap();
    assert_eq!(
        branch.checkpoint().commands.len(),
        checkpoint.commands.len() + 1
    );
    same(&branch, &restore(&branch));
    assert!(matches!(
        Game::restore(
            tiny_loaded_region(),
            checkpoint,
            Box::new(InMemoryCommandLog::default()),
            &|| true
        ),
        Err(RestoreError::Cancelled)
    ));
}

fn running() -> ReplayCompatibility {
    ReplayCompatibility {
        build: "current".into(),
        platform: "here".into(),
    }
}

fn older() -> ReplayCompatibility {
    ReplayCompatibility {
        build: "older".into(),
        platform: "elsewhere".into(),
    }
}

/// A save of `checkpoint` as the build `compatibility` recorded it.
fn stamped(compatibility: ReplayCompatibility, checkpoint: GameCheckpoint) -> SaveBundle {
    SaveBundle {
        compatibility,
        metadata: SaveMetadata {
            slot: SaveId::new("a".repeat(32)).unwrap(),
            run: SaveId::new("b".repeat(32)).unwrap(),
            kind: SaveKind::Manual,
            name: "Checkpoint".into(),
            saved_at: 0,
        },
        checkpoint,
    }
}

/// The build fingerprint is diagnostics: a save from any build or platform restores when its
/// replay reproduces the recorded boundary here.
#[test]
fn a_checkpoint_stamped_by_another_build_restores_when_its_replay_matches() {
    let (mut live, _) = game_on(tiny_region_parts(), 12);
    live.execute(Command::BeginOperating).unwrap();
    live.execute(Command::AdvanceTicks { n: 333 }).unwrap();
    for recorded in [
        older(),
        ReplayCompatibility {
            platform: "elsewhere".into(),
            ..running()
        },
    ] {
        let log = InMemoryCommandLog::default();
        let mut resumed = stamped(recorded, live.checkpoint())
            .restore(
                tiny_loaded_region(),
                &running(),
                Box::new(log.clone()),
                &|| false,
            )
            .unwrap();
        same(&live, &resumed);
        assert_eq!(log.records().len(), live.checkpoint().commands.len() + 1);
        let mut uninterrupted = restore(&live);
        uninterrupted
            .execute(Command::AdvanceTicks { n: 90 })
            .unwrap();
        resumed.execute(Command::AdvanceTicks { n: 90 }).unwrap();
        same(&uninterrupted, &resumed);
    }
}

/// A replay that doesn't reproduce the boundary fails explicitly and returns no game, and its
/// fresh branch log records nothing. On another build the error says the builds differed.
#[test]
fn a_checkpoint_whose_replay_diverges_fails_verification_and_installs_nothing() {
    let (mut game, _) = game_on(tiny_region_parts(), 10);
    game.execute(Command::BeginOperating).unwrap();
    game.execute(Command::AdvanceTicks { n: 61 }).unwrap();
    let checkpoint = game.checkpoint();
    for change in 0..4 {
        let mut bad = checkpoint.clone();
        match change {
            0 => bad.command_count += 1,
            1 => bad.state_hash ^= 1,
            2 => bad.summary.tick += 1,
            _ => bad.commands[0] = Command::FinishReview,
        }
        let expected = Game::restore(
            tiny_loaded_region(),
            bad.clone(),
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )
        .err()
        .unwrap();
        assert!(expected.is_verification_failure(), "{expected:?}");
        assert!(matches!(
            (change, &expected),
            (0, RestoreError::CommandCountMismatch)
                | (1, RestoreError::StateHashMismatch)
                | (2, RestoreError::SummaryMismatch)
                | (3, RestoreError::CommandCountMismatch)
        ));
        for (recorded, error) in [
            (running(), SaveError::Restore(expected.clone())),
            (
                older(),
                SaveError::OtherVersionDiverged {
                    recorded: older(),
                    error: expected.clone(),
                },
            ),
        ] {
            let target = InMemoryCommandLog::default();
            let result = stamped(recorded, bad.clone()).restore(
                tiny_loaded_region(),
                &running(),
                Box::new(target.clone()),
                &|| false,
            );
            assert_eq!(result.err(), Some(error));
            assert!(target.records().is_empty());
        }
    }
}

/// Only a failed verification blames the other build. Other refusals keep their own cause.
#[test]
fn a_checkpoint_from_another_build_keeps_its_cause_when_it_fails_before_verification() {
    let (mut game, _) = game_on(tiny_region_parts(), 3);
    game.execute(Command::BeginOperating).unwrap();
    // Enough commands for `FailingSink` to fail while the branch log is seeded.
    for n in [1, 2, 3] {
        game.execute(Command::AdvanceTicks { n }).unwrap();
    }
    let mut wrong = game.checkpoint();
    wrong.header.region = domain::ids::RegionId::new("other");
    assert_eq!(
        stamped(older(), wrong)
            .restore(
                tiny_loaded_region(),
                &running(),
                Box::new(InMemoryCommandLog::default()),
                &|| false,
            )
            .err(),
        Some(SaveError::Restore(RestoreError::RegionIdMismatch))
    );
    assert_eq!(
        stamped(older(), game.checkpoint())
            .restore(
                tiny_loaded_region(),
                &running(),
                Box::new(InMemoryCommandLog::default()),
                &|| true,
            )
            .err(),
        Some(SaveError::Restore(RestoreError::Cancelled))
    );
    assert_eq!(
        stamped(older(), game.checkpoint())
            .restore(
                tiny_loaded_region(),
                &running(),
                Box::new(FailingSink { accepted: 0 }),
                &|| false,
            )
            .err()
            .map(|error| matches!(error, SaveError::Restore(RestoreError::Log(_)))),
        Some(true)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn save_resume_arbitrary_prefix_and_continuation(seed in any::<u64>(), batches in prop::collection::vec(0u32..60, 0..20), extra in 0u32..120) {
        let (mut live, _) = game_on(tiny_region_parts(), seed);
        live.execute(Command::BeginOperating).unwrap();
        for n in batches {
            let before = live.checkpoint();
            prop_assert!(live.execute(Command::FinishReview).is_err());
            prop_assert_eq!(live.checkpoint(), before);
            live.execute(Command::AdvanceTicks { n }).unwrap();
        }
        let mut resumed = restore(&live);
        same(&live, &resumed);
        live.execute(Command::AdvanceTicks { n: extra }).unwrap();
        resumed.execute(Command::AdvanceTicks { n: extra }).unwrap();
        same(&live, &resumed);
    }
}

#[test]
fn save_resume_reconstructs_year_end_reports_construction_and_a_terminal_review() {
    use domain::ids::CellCoord;
    use domain::region::PlantKind;
    use domain::session::SessionStatus;
    use domain::units::Money;
    let mut parts = tiny_region_parts();
    parts.start.month = domain::time::SimMonth::new(2027, 11).unwrap();
    let loaded = support::loaded_region(parts.clone());
    let mut live = Game::start_new(
        &StaticCatalog(vec![loaded.clone()]),
        loaded.definition.id(),
        2,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    live.execute(Command::BuildPlant {
        kind: PlantKind::Peaker,
        cell: CellCoord { x: 9, y: 4 },
    })
    .unwrap();
    for _ in 0..4 {
        live.execute(Command::BeginOperating).unwrap();
        live.execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
            .unwrap();
        let resumed = Game::restore(
            loaded.clone(),
            live.checkpoint(),
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )
        .unwrap();
        same(&live, &resumed);
        if live.session().status() != SessionStatus::Active {
            break;
        }
        live.execute(Command::FinishReview).unwrap();
    }
    assert!(live.projections().monthly_reports.history().len() >= 2);
    assert!(
        live.projections()
            .monthly_reports
            .history()
            .iter()
            .any(|r| r.month.month() == 12 && r.rate_review.is_some())
    );
    parts.start.cash = Money::from_cents(1);
    let loaded = support::loaded_region(parts);
    let mut ended = Game::start_new(
        &StaticCatalog(vec![loaded.clone()]),
        loaded.definition.id(),
        2,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    // No generation means no tariff revenue, but the inherited fleet still incurs fixed O&M.
    support::empty_start(&mut ended);
    for _ in 0..3 {
        ended.execute(Command::BeginOperating).unwrap();
        ended
            .execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
            .unwrap();
        if ended.session().status() != SessionStatus::Active {
            break;
        }
        ended.execute(Command::FinishReview).unwrap();
    }
    assert!(matches!(
        ended.session().status(),
        SessionStatus::GameOver(_)
    ));
    let mut resumed = Game::restore(
        loaded,
        ended.checkpoint(),
        Box::new(InMemoryCommandLog::default()),
        &|| false,
    )
    .unwrap();
    same(&ended, &resumed);
    assert!(resumed.execute(Command::FinishReview).is_err());
}

#[test]
fn save_resume_historical_errors_are_indexed_and_branch_log_failure_returns_no_game() {
    let (mut game, _) = game_on(tiny_region_parts(), 3);
    game.execute(Command::BeginOperating).unwrap();
    let mut checkpoint = game.checkpoint();
    // The version-1 command-only path still reports the precise failing historical command.
    checkpoint.snapshot = None;
    checkpoint.state_hash = game.session().state_hash();
    checkpoint.commands.push(Command::FinishReview);
    checkpoint.command_count += 1;
    assert!(matches!(
        Game::restore(
            tiny_loaded_region(),
            checkpoint,
            Box::new(InMemoryCommandLog::default()),
            &|| false
        ),
        Err(RestoreError::ReplayCommandFailed { index: 1, .. })
    ));
    for n in [1, 2, 3] {
        game.execute(Command::AdvanceTicks { n }).unwrap();
    }
    assert!(matches!(
        Game::restore(
            tiny_loaded_region(),
            game.checkpoint(),
            Box::new(FailingSink { accepted: 0 }),
            &|| false
        ),
        Err(RestoreError::Log(_))
    ));
    let mut wrong = game.checkpoint();
    wrong.header.region = domain::ids::RegionId::new("other");
    assert!(matches!(
        Game::restore(
            tiny_loaded_region(),
            wrong,
            Box::new(InMemoryCommandLog::default()),
            &|| false
        ),
        Err(RestoreError::RegionIdMismatch)
    ));
}

#[test]
fn a_preloaded_game_attaches_a_new_recording_without_replaying_or_losing_projections() {
    let (mut game, _) = game_on(tiny_region_parts(), 9);
    game.execute(Command::BeginOperating).unwrap();
    game.execute(Command::AdvanceTicks { n: 121 }).unwrap();
    let candidate = restore(&game);
    let log = InMemoryCommandLog::default();
    let mut resumed = candidate
        .with_recording(Box::new(log.clone()), &|| false)
        .unwrap();
    same(&game, &resumed);
    let checkpoint = game.checkpoint();
    assert_eq!(log.region_source(), checkpoint.region_source);
    assert_eq!(log.records()[0], LogRecord::Header(checkpoint.header));
    assert_eq!(
        log.records()[1..],
        checkpoint
            .commands
            .into_iter()
            .map(LogRecord::Command)
            .collect::<Vec<_>>()
    );
    for command in [
        Command::AdvanceTicks { n: 63 },
        Command::AdvanceTicks { n: 40 },
    ] {
        game.execute(command.clone()).unwrap();
        resumed.execute(command.clone()).unwrap();
        same(&game, &resumed);
        assert_eq!(log.records().last(), Some(&LogRecord::Command(command)));
    }
    assert!(matches!(
        restore(&game).with_recording(Box::new(InMemoryCommandLog::default()), &|| true),
        Err(RestoreError::Cancelled)
    ));
    assert!(matches!(
        restore(&game).with_recording(Box::new(FailingSink { accepted: 2 }), &|| false),
        Err(RestoreError::Log(_))
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]
    #[test]
    fn battery_checkpoint_replays_charged_disabled_and_evening_states(seed in 0u64..32, cutoff in 600u32..1300) {
        use domain::region::{PlantKind,StartPlantDef};
        use domain::ids::CellCoord;
        let mut parts=tiny_region_parts();
        parts.start.fleet.push(StartPlantDef{kind:PlantKind::Battery,cell:CellCoord{x:5,y:1}});
        if seed % 2 == 0 { parts.start.fleet.push(StartPlantDef{kind:PlantKind::Battery,cell:CellCoord{x:5,y:2}}); }
        let loaded=support::loaded_region(parts.clone());
        let (mut live,_)=game_on(parts,seed);
        let id=live.session().plant_views().iter().find(|p|p.kind==PlantKind::Battery).unwrap().id;
        live.execute(Command::BeginOperating).unwrap();
        live.execute(Command::AdvanceTicks{n:cutoff}).unwrap();
        let mut resumed=Game::restore(loaded.clone(),live.checkpoint(),Box::new(InMemoryCommandLog::default()),&||false).unwrap();
        same(&live,&resumed);
        for on in [false,true,false,true] {
            let command=Command::SetBatteryEnabled{plant:id,on};
            live.execute(command.clone()).unwrap();resumed.execute(command).unwrap();
            same(&live,&resumed);
        }
        live.execute(Command::SetBatteryEnabled{plant:id,on:false}).unwrap();
        resumed=Game::restore(loaded.clone(),live.checkpoint(),Box::new(InMemoryCommandLog::default()),&||false).unwrap();
        same(&live,&resumed);
        for command in [Command::SetBatteryEnabled{plant:id,on:true},Command::AdvanceTicks{n:TICKS_PER_DAY-cutoff}] {
            live.execute(command.clone()).unwrap();resumed.execute(command).unwrap();
        }
        same(&live,&resumed);
        let report=live.projections().monthly_reports.latest().unwrap();
        prop_assert!(report.storage[&id].input.value()>0.0);
        prop_assert_eq!(live.session().plant_views().iter().find(|p|p.id==id).unwrap().battery.as_ref().unwrap().stored,domain::units::MegawattHours::ZERO);
        prop_assert_eq!(live.projections().timeline.completed_days()[0].points.len(),1440);
        resumed=Game::restore(loaded,live.checkpoint(),Box::new(InMemoryCommandLog::default()),&||false).unwrap();
        same(&live,&resumed);
    }
}

#[test]
fn storage_flow_report_and_inventory_are_independent_of_tick_batching() {
    use domain::ids::CellCoord;
    use domain::region::{PlantKind, StartPlantDef};
    let mut parts = tiny_region_parts();
    parts.start.fleet.push(StartPlantDef {
        kind: PlantKind::Battery,
        cell: CellCoord { x: 5, y: 1 },
    });
    let (mut one, _) = game_on(parts.clone(), 7);
    let (mut batches, _) = game_on(parts, 7);
    one.execute(Command::BeginOperating).unwrap();
    batches.execute(Command::BeginOperating).unwrap();
    one.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
    for n in [1, 31, 32, 128, 512, 736] {
        batches.execute(Command::AdvanceTicks { n }).unwrap();
    }
    assert_eq!(one.session().state_hash(), batches.session().state_hash());
    assert_eq!(one.projections().timeline, batches.projections().timeline);
    assert_eq!(
        one.projections().monthly_reports,
        batches.projections().monthly_reports
    );
    assert_eq!(
        one.projections().plant_history,
        batches.projections().plant_history
    );
}

#[test]
fn portable_snapshots_allow_rule_upgrades_and_cover_history_without_debug_hashes() {
    assert!(application::PortableState::rules_compatible(8, 9));
    assert!(application::PortableState::rules_compatible(8, 10));
    assert!(!application::PortableState::rules_compatible(9, 8));
    let (mut game, _) = game_on(tiny_region_parts(), 3);
    game.execute(Command::BeginOperating).unwrap();
    game.execute(Command::AdvanceTicks { n: 120 }).unwrap();
    let checkpoint = game.checkpoint();
    assert_eq!(
        checkpoint.state_hash,
        checkpoint.snapshot.as_ref().unwrap().state_hash()
    );
    let mut bad = checkpoint.clone();
    bad.snapshot.as_mut().unwrap().projections[0] ^= 1;
    assert!(matches!(
        Game::restore(
            tiny_loaded_region(),
            bad,
            Box::new(InMemoryCommandLog::default()),
            &|| false
        ),
        Err(RestoreError::StateHashMismatch)
    ));
    // The old rules-8 command-only checkpoint remains supported too.
    let mut legacy = checkpoint;
    legacy.snapshot = None;
    legacy.state_hash = game.session().state_hash();
    let restored = Game::restore(
        tiny_loaded_region(),
        legacy,
        Box::new(InMemoryCommandLog::default()),
        &|| false,
    )
    .unwrap();
    assert_eq!(restored.session().state_hash(), game.session().state_hash());
    assert_eq!(restored.projections(), game.projections());
}

#[test]
fn rules_ten_snapshot_keeps_its_state_and_blocks_new_builds_on_occupied_cells() {
    use domain::region::PlantKind;
    let (game, _) = game_on(tiny_region_parts(), 7);
    let mut checkpoint = game.checkpoint();
    checkpoint.header.sim_rules_version = 10;
    let mut restored = Game::restore(
        tiny_loaded_region(),
        checkpoint,
        Box::new(InMemoryCommandLog::default()),
        &|| false,
    )
    .unwrap();
    assert_eq!(restored.session().state_hash(), game.session().state_hash());
    assert_eq!(restored.projections(), game.projections());
    let at = restored.session().city_views()[0].cell;
    let before = restored.checkpoint();
    assert!(
        restored
            .execute(Command::BuildPlant {
                kind: PlantKind::Peaker,
                cell: at
            })
            .is_err()
    );
    assert_eq!(restored.checkpoint(), before);
}

#[test]
fn pending_recording_owns_the_frozen_boundary_until_activation() {
    let (mut game, _) = game_on(tiny_region_parts(), 17);
    game.execute(Command::BeginOperating).unwrap();
    game.execute(Command::AdvanceTicks { n: 12 }).unwrap();
    let expected = game.checkpoint();
    let pending = game.prepare_recording();
    assert_eq!(pending.checkpoint(), &expected);
    let game = pending
        .activate(Box::new(InMemoryCommandLog::default()), &|| false)
        .unwrap();
    assert_eq!(game.checkpoint(), expected);
    assert!(matches!(
        game.prepare_recording()
            .activate(Box::new(InMemoryCommandLog::default()), &|| true),
        Err(RestoreError::Cancelled)
    ));
}

#[test]
fn monthly_chart_archive_reconstructs_exact_projection_bytes_across_phase_boundaries() {
    use domain::state::State;
    let (mut game, _) = game_on(tiny_region_parts(), 17);
    for command in [
        Command::BeginOperating,
        Command::AdvanceTicks { n: 60 },
        Command::AdvanceTicks { n: 1440 },
        Command::FinishReview,
        Command::BeginOperating,
        Command::AdvanceTicks { n: 20 },
    ] {
        game.execute(command).unwrap();
        let original = game.projections().clone();
        let (rest, charts) = original
            .clone()
            .split_charts(game.session().clock().month())
            .unwrap();
        assert!(rest.timeline.completed_days().is_empty());
        assert_eq!(rest.timeline.points().count(), 0);
        let restored = rest.restore_charts(charts).unwrap();
        assert_eq!(restored.state_bytes(), original.state_bytes());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]
    #[test]
    fn monthly_chart_split_is_lossless_at_arbitrary_boundaries(ns in prop::collection::vec(0u32..1441,0..8),seed in 0u64..20) {
        use domain::state::State;
        let (mut game,_) = game_on(tiny_region_parts(),seed);
        for n in ns {
            if game.session().status()!=domain::session::SessionStatus::Active{break;}
            if game.session().phase()==DayPhase::Review{game.execute(Command::FinishReview).unwrap();}
            if game.session().phase()==DayPhase::Planning{game.execute(Command::BeginOperating).unwrap();}
            game.execute(Command::AdvanceTicks{n}).unwrap();
            let original=game.projections().clone();
            let (rest,charts)=original.clone().split_charts(game.session().clock().month()).unwrap();
            prop_assert_eq!(rest.restore_charts(charts).unwrap().state_bytes(),original.state_bytes());
        }
    }
}
