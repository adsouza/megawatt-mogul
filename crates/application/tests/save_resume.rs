//! Complete command checkpoints, independent of diagnostic recording health.
#[allow(dead_code)]
mod support;

use application::{
    Command, CommandLogError, CommandLogSink, Game, InMemoryCommandLog, LogHeader, LogRecord,
    RestoreError,
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
fn save_resume_verifies_boundary_and_seeds_a_fresh_branch_once() {
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
    for change in 0..4 {
        let mut bad = checkpoint.clone();
        match change {
            0 => bad.command_count += 1,
            1 => bad.state_hash ^= 1,
            2 => bad.summary.tick += 1,
            _ => bad.commands[0] = Command::FinishReview,
        }
        let target = InMemoryCommandLog::default();
        assert!(
            Game::restore(tiny_loaded_region(), bad, Box::new(target.clone()), &|| {
                false
            })
            .is_err()
        );
        assert!(target.records().is_empty());
    }
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
        cell: CellCoord { x: 8, y: 4 },
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
