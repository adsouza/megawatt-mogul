//! One-time pre-Plan-6a oracle. Refuses newer rules and never overwrites captures.
use application::{
    Command, Game, InMemoryCommandLog, ReplayCompatibility, SaveBundle, SaveId, SaveKind,
    SaveMetadata,
};
use domain::{
    ids::{CellCoord, RegionId},
    region::PlantKind,
    session::GameSettings,
};
use infrastructure::{region::RonRegionCatalog, save::encode_save};
use std::{fmt::Write as _, io::Write as _, path::Path};
fn write(path: &Path, bytes: &[u8]) {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}
fn main() {
    assert_eq!(
        domain::SIM_RULES_VERSION,
        16,
        "capture only with the pre-change engine"
    );
    let output = std::path::PathBuf::from(std::env::args_os().nth(1).expect("output directory"));
    std::fs::create_dir(&output).unwrap();
    let source = include_bytes!("../tests/support/minimal_region_v4.ron");
    write(&output.join("minimal.ron"), source);
    let catalog = RonRegionCatalog::new(&output);
    let mut game = Game::start_new(
        &catalog,
        &RegionId::new("minimal"),
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    let cell = CellCoord { x: 4, y: 4 };
    game.execute(Command::BuildPlant {
        kind: PlantKind::Wind,
        cell,
    })
    .unwrap();
    let plant = game
        .session()
        .plant_views()
        .into_iter()
        .find(|p| p.cell == cell)
        .unwrap();
    let home = game
        .session()
        .city_views()
        .into_iter()
        .find(|c| c.id == game.session().home_city())
        .unwrap()
        .node;
    let quote = game.session().line_quote(home, plant.node).unwrap();
    game.execute(Command::BuildLine {
        from: home,
        to: plant.node,
    })
    .unwrap();
    game.execute(Command::BeginOperating).unwrap();
    game.execute(Command::AdvanceTicks { n: 60 }).unwrap();
    let checkpoint = game.checkpoint();
    let snapshot = checkpoint.snapshot.as_ref().unwrap();
    write(&output.join("session-schema3.bin"), &snapshot.session);
    write(
        &output.join("projections-schema3.bin"),
        &snapshot.projections,
    );
    let mut expected = format!(
        "rules=16\nregion_schema=4\nstate_schema={}\norigin=macos-aarch64\nboundary_hash={}\nplant={}\nline_quote={quote:?}\n",
        snapshot.schema, checkpoint.state_hash, plant.id.0
    );
    let bundle = SaveBundle {
        compatibility: ReplayCompatibility {
            build: "plan-6a-preflight-rules16".into(),
            platform: "aarch64-apple-darwin".into(),
        },
        metadata: SaveMetadata {
            slot: SaveId::new("d".repeat(32)).unwrap(),
            run: SaveId::new("e".repeat(32)).unwrap(),
            kind: SaveKind::Manual,
            name: "Immutable pre-economic construction".into(),
            saved_at: 0,
        },
        checkpoint,
    };
    write(
        &output.join("in-flight-rules16.mgsave"),
        &encode_save(&bundle).unwrap(),
    );
    for index in 1..=3 {
        if index > 1 {
            game.execute(Command::FinishReview).unwrap();
            game.execute(Command::BeginOperating).unwrap();
        }
        game.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
        let report = game
            .projections()
            .monthly_reports
            .history()
            .last()
            .unwrap()
            .clone();
        writeln!(&mut expected, "close_{index}_hash={}\nclose_{index}_construction={}\nclose_{index}_cash={}\nclose_{index}_plant={:?}\nclose_{index}_rate_base={:?}", game.checkpoint().state_hash, report.construction.cents(), game.session().company_view().cash.cents(), game.session().plant_views().into_iter().find(|p| p.id == plant.id).unwrap().status, game.session().company().rate_base()).unwrap();
    }
    write(&output.join("expected.txt"), expected.as_bytes());
}
