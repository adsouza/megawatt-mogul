//! One-time A19 compatibility capture. Never run after rules 27; never overwrites files.
use application::{Command, Game, InMemoryCommandLog, SaveBundle, SaveId, SaveKind, SaveMetadata};
use domain::state::State;
use domain::{ids::RegionId, session::GameSettings};
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
fn main() {
    assert_eq!(domain::SIM_RULES_VERSION, 27);
    let root = std::path::PathBuf::from(std::env::args_os().nth(1).expect("output directory"));
    let mode = std::env::args().nth(2).unwrap_or_default();
    if mode == "continuations" {
        for phase in ["planning", "operating", "review"] {
            let bytes = std::fs::read(root.join(format!("{phase}.mgsave"))).unwrap();
            let compatibility = replay_compatibility();
            let (bundle, region) =
                infrastructure::save::decode_save(&bytes, &compatibility).unwrap();
            let mut game = bundle
                .restore(
                    region,
                    &compatibility,
                    Box::new(InMemoryCommandLog::default()),
                    &|| false,
                )
                .unwrap();
            let command = match phase {
                "planning" => Command::BeginOperating,
                "operating" => Command::AdvanceTicks { n: 60 },
                _ => Command::FinishReview,
            };
            game.execute(command).unwrap();
            let snapshot = game.checkpoint().snapshot.unwrap();
            write(
                &root,
                &format!("{phase}-continued-session.bin"),
                &snapshot.session,
            );
            write(
                &root,
                &format!("{phase}-continued-projections.bin"),
                &snapshot.projections,
            );
        }
        return;
    }
    std::fs::create_dir(&root).unwrap();
    let source = include_str!("../tests/support/economic_region_v4.ron");
    let source = if mode == "multi-island" {
        use infrastructure::region::{AreaDto, HvdcDto, RegionFileV4};
        let mut file: RegionFileV4 = ron::from_str(source).unwrap();
        file.areas = vec![
            AreaDto {
                id: 1,
                name_key: "east".into(),
                polygons: vec![vec![(6.0, 0.0), (12.0, 0.0), (12.0, 8.0), (6.0, 8.0)]],
                geography: vec![],
            },
            AreaDto {
                id: 2,
                name_key: "west".into(),
                polygons: vec![vec![(1.0, 0.0), (6.0, 0.0), (6.0, 8.0), (1.0, 8.0)]],
                geography: vec![],
            },
        ];
        file.hvdc = Some(HvdcDto {
            rating_mw: 200.0,
            converter_cost: 101,
            cost_per_cell: 7,
            converter_build_months: 2,
            ramp_mw_per_min: 20.0,
            converter_loss: 0.01,
            loss_per_cell: 0.001,
            book_life_months: 240,
            designed_hours_per_year: 4000.0,
        });
        ron::to_string(&file).unwrap()
    } else {
        source.to_owned()
    };
    write(&root, "minimal.ron", source.as_bytes());
    let mut game = Game::start_new(
        &RonRegionCatalog::new(&root),
        &RegionId::new("minimal"),
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    if mode == "multi-island" {
        let cell = domain::ids::CellCoord { x: 4, y: 4 };
        game.execute(Command::BuildPlant {
            kind: domain::region::PlantKind::Ccgt,
            cell,
        })
        .unwrap();
        let node = game
            .session()
            .plant_views()
            .into_iter()
            .find(|p| p.cell == cell)
            .unwrap()
            .node;
        game.execute(Command::BuildLink {
            from: game.session().anchor_node(),
            to: node,
        })
        .unwrap();
    }
    for phase in ["planning", "operating", "review"] {
        match phase {
            "operating" => {
                game.execute(Command::BeginOperating).unwrap();
                game.execute(Command::AdvanceTicks { n: 517 }).unwrap();
            }
            "review" => {
                game.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
            }
            _ => {}
        }
        let mut checkpoint = game.checkpoint();
        if mode == "schema4" {
            let snapshot = checkpoint.snapshot.as_mut().unwrap();
            snapshot.schema = 4;
            snapshot.session = game.session().snapshot_state_schema(4);
            snapshot.projections = game.projections().state_bytes_schema(4);
            checkpoint.state_hash = snapshot.state_hash();
        }
        let bundle = SaveBundle {
            compatibility: replay_compatibility(),
            metadata: SaveMetadata {
                slot: SaveId::new("a".repeat(32)).unwrap(),
                run: SaveId::new("b".repeat(32)).unwrap(),
                name: phase.into(),
                kind: SaveKind::Manual,
                saved_at: 0,
            },
            checkpoint,
        };
        write(
            &root,
            &format!("{phase}.mgsave"),
            &encode_save(&bundle).unwrap(),
        );
    }
}
