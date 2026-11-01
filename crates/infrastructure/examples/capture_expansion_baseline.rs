//! One-time capture of the pre-expansion-tracking compatibility saves (state schema 7). Run only
//! from the baseline before `ExpansionProjection` (schema 8); it never overwrites files.
//!
//! `capture_expansion_baseline <new output directory>` writes `minimal.ron` and two Planning saves
//! of the minimal region: `expanded.mgsave`, whose game has connected city 3 (outside the starter
//! territory), and `starter.mgsave`, whose game serves only its starter cities.
use application::{Command, Game, InMemoryCommandLog, SaveBundle, SaveId, SaveKind, SaveMetadata};
use domain::session::{DayPhase, GameSettings};
use domain::{ids::RegionId, time::TICKS_PER_DAY};
use infrastructure::{
    region::RonRegionCatalog,
    save::{encode_save, replay_compatibility},
};
use std::{io::Write, path::Path};

fn write(root: &Path, name: &str, bytes: &[u8]) {
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(root.join(name))
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

fn save(root: &Path, name: &str, game: &Game) {
    assert_eq!(game.session().phase(), DayPhase::Planning);
    let checkpoint = game.checkpoint();
    assert_eq!(checkpoint.snapshot.as_ref().unwrap().schema, 7);
    let bundle = SaveBundle {
        compatibility: replay_compatibility(),
        metadata: SaveMetadata {
            slot: SaveId::new("a".repeat(32)).unwrap(),
            run: SaveId::new("b".repeat(32)).unwrap(),
            name: name.into(),
            kind: SaveKind::Manual,
            saved_at: 0,
        },
        checkpoint,
    };
    write(
        root,
        &format!("{name}.mgsave"),
        &encode_save(&bundle).unwrap(),
    );
}

fn main() {
    assert_eq!(
        domain::state::CURRENT_STATE_SCHEMA,
        7,
        "capture from the schema-7 baseline only"
    );
    let root = std::path::PathBuf::from(std::env::args_os().nth(1).expect("output directory"));
    std::fs::create_dir(&root).unwrap();
    let source = include_str!("../tests/support/economic_region_v4.ron");
    write(&root, "minimal.ron", source.as_bytes());
    let start = || {
        Game::start_new(
            &RonRegionCatalog::new(&root),
            &RegionId::new("minimal"),
            7,
            GameSettings::default(),
            Box::new(InMemoryCommandLog::default()),
        )
        .unwrap()
    };

    save(&root, "starter", &start());

    let mut game = start();
    let starters = game.session().region().start().connected_cities.clone();
    let extra = game
        .session()
        .city_views()
        .into_iter()
        .find(|city| !city.connected && !starters.contains(&city.id))
        .expect("a city outside the starter territory");
    game.execute(Command::BuildLine {
        from: game.session().anchor_node(),
        to: extra.node,
    })
    .unwrap();
    let connected = |game: &Game| {
        game.session()
            .city_views()
            .iter()
            .any(|city| city.id == extra.id && city.connected)
    };
    for _ in 0..12 {
        if connected(&game) {
            break;
        }
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
            .unwrap();
        game.execute(Command::FinishReview).unwrap();
    }
    assert!(connected(&game), "the line to the extra city is built");
    save(&root, "expanded", &game);
}
