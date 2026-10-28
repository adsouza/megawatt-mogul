//! A19 application pipeline: batched ticks, persistence and hostile commands.
#[allow(dead_code)]
mod support;
use application::{Command, Game, InMemoryCommandLog};
use domain::{
    ids::{CellCoord, PlantId},
    region::{
        PlantKind,
        fixtures::{geothermal_acceptance_parts, tiny_region_parts},
    },
    state::State,
};
use support::{game_on, loaded_region};
#[test]
fn geothermal_batching_speed_and_repeated_queries_preserve_complete_state_and_projections() {
    let mut parts = tiny_region_parts();
    let p = parts.geothermal.as_mut().unwrap();
    p.success_min = domain::units::Ratio::ONE;
    p.success_max = domain::units::Ratio::ONE;
    let (mut a, _) = game_on(parts.clone(), 7);
    let (mut b, _) = game_on(parts, 7);
    for game in [&mut a, &mut b] {
        game.execute(Command::BuildPlant {
            kind: PlantKind::Geothermal,
            cell: CellCoord { x: 4, y: 4 },
        })
        .unwrap();
    }
    for month in 0..4 {
        for game in [&mut a, &mut b] {
            if month > 0 {
                game.execute(Command::FinishReview).unwrap();
            }
            game.execute(Command::BeginOperating).unwrap();
        }
        for _ in 0..1440 {
            a.execute(Command::AdvanceTicks { n: 1 }).unwrap();
        }
        for _ in 0..45 {
            let before = b.session().snapshot_state();
            let _ = b.session().supply_forecast();
            assert_eq!(before, b.session().snapshot_state());
            b.execute(Command::AdvanceTicks { n: 32 }).unwrap();
        }
        assert_eq!(a.session().snapshot_state(), b.session().snapshot_state());
        assert_eq!(a.projections().state_bytes(), b.projections().state_bytes());
    }
}
#[test]
fn geothermal_notice_commands_and_mid_makeup_checkpoint_resume_through_actual_pipeline() {
    let parts = geothermal_acceptance_parts();
    let loaded = loaded_region(parts.clone());
    let (mut game, _) = game_on(parts, 7);
    let id = game
        .session()
        .plant_views()
        .into_iter()
        .find(|p| p.kind == PlantKind::Geothermal)
        .unwrap()
        .id;
    for month in 0..3 {
        if month > 0 {
            game.execute(Command::FinishReview).unwrap();
        }
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
    }
    game.execute(Command::FinishReview).unwrap();
    game.execute(Command::DismissFieldNotice { plant: id })
        .unwrap();
    let checkpoint = game.checkpoint();
    let mut restored = Game::restore(
        loaded.clone(),
        checkpoint.clone(),
        Box::new(InMemoryCommandLog::default()),
        &|| false,
    )
    .unwrap();
    assert_eq!(restored.checkpoint(), checkpoint);
    let before = game.checkpoint();
    assert!(
        game.execute(Command::DismissFieldNotice { plant: id })
            .is_err()
    );
    assert_eq!(before, game.checkpoint());
    for g in [&mut game, &mut restored] {
        g.execute(Command::DrillMakeupWells { plant: id }).unwrap();
        g.execute(Command::BeginOperating).unwrap();
        g.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
    }
    let checkpoint = restored.checkpoint();
    let mut resumed = Game::restore(
        loaded,
        checkpoint,
        Box::new(InMemoryCommandLog::default()),
        &|| false,
    )
    .unwrap();
    for _ in 0..2 {
        for g in [&mut game, &mut resumed] {
            g.execute(Command::FinishReview).unwrap();
            g.execute(Command::BeginOperating).unwrap();
            g.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
        }
    }
    assert_eq!(game.checkpoint(), resumed.checkpoint());
    assert_eq!(
        game.session()
            .power()
            .plant(id)
            .unwrap()
            .geothermal()
            .unwrap()
            .rounds,
        1
    );
    for command in [
        Command::DrillMakeupWells {
            plant: PlantId(u32::MAX),
        },
        Command::DismissFieldNotice {
            plant: PlantId(u32::MAX),
        },
    ] {
        assert_eq!(
            Command::from_state_bytes(&command.state_bytes()).unwrap(),
            command
        );
        for schema in 1..=5 {
            assert!(Command::from_state_bytes_schema(&command.state_bytes(), schema).is_err());
        }
        let before = game.checkpoint();
        assert!(game.execute(command).is_err());
        assert_eq!(before, game.checkpoint());
    }
}
