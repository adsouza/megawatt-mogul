//! Isolated, accelerated A19 acceptance data. Never edits shipped assets or personal saves.
//! Usage: geothermal_acceptance <new-output-directory>. Prints native reservoir trace.
use application::{
    Command, Game, InMemoryCommandLog, RegionCatalog, SaveBundle, SaveId, SaveKind, SaveMetadata,
};
use domain::{
    ids::{CellCoord, RegionId},
    region::PlantKind,
    session::{DayPhase, GameSettings},
};
use infrastructure::{
    region::{Blob, Field, PlantKindDto, RegionFileV4, RonRegionCatalog},
    save::{FileSaveStore, decode_archive, encode_archive, encode_save, replay_compatibility},
};
use std::{error::Error, path::Path};
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
fn trace(game: &Game) -> String {
    let mut rows = vec![format!("dry={}", game.session().dry_cells().len())];
    for p in game.session().power().plants().values() {
        if let Some(g) = p.geothermal() {
            rows.push(format!(
                "{}:{}:{:016x}:{}:{}:{:?}",
                p.id().0,
                g.exploration_left,
                g.field.value().to_bits(),
                g.rounds,
                g.makeup.as_ref().map_or(0, |m| m.months_left),
                g.notice
            ));
        }
    }
    rows.join("|")
}
fn main() -> Result<(), Box<dyn Error>> {
    let output = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("new output directory required")?,
    );
    std::fs::create_dir(&output)?;
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    copy_tree(&assets, &output.join("assets"))?;
    let id = RegionId::new("western_interconnection");
    let original = RonRegionCatalog::new(assets.join("regions")).load(&id)?;
    let r = &original.definition;
    let t = r.terrain();
    let sites: Vec<_> = (0..t.height())
        .flat_map(|y| (0..t.width()).map(move |x| CellCoord { x, y }))
        .filter(|at| {
            t.get(*at).unwrap().land
                && !r.cities().iter().any(|c| c.cell == *at)
                && !r.start().fleet.iter().any(|p| p.cell == *at)
        })
        .collect();
    let home = r.city(r.start().home_city).unwrap().cell;
    let good = sites
        .iter()
        .copied()
        .min_by_key(|at| at.x.abs_diff(home.x) + at.y.abs_diff(home.y))
        .unwrap();
    let second = sites
        .iter()
        .copied()
        .find(|at| *at != good && at.x.abs_diff(good.x) + at.y.abs_diff(good.y) <= 2)
        .unwrap();
    let dry = sites
        .iter()
        .copied()
        .find(|at| at.x.abs_diff(good.x) + at.y.abs_diff(good.y) > 25)
        .unwrap();
    let path = output.join("assets/regions/western_interconnection.ron");
    let mut file: RegionFileV4 = ron::from_str(&std::fs::read_to_string(&path)?)?;
    let (lat, lon) = r.cell_lat_lon(good);
    // Acceptance-only acceleration: an inherited proven plant, definite wet/dry sites,
    // 1+3 construction months and 4% monthly decline. Production data is unchanged.
    file.start.fleet.push((PlantKindDto::Geothermal, lon, lat));
    file.start.cash = 100_000_000_000;
    file.geothermal_potential = Some(Field {
        base: 0.3,
        blobs: vec![Blob {
            lon,
            lat,
            radius_deg: 0.5,
            value: 10.0,
        }],
    });
    let params = file.geothermal.as_mut().unwrap();
    params.success_min = 0.0;
    params.success_max = 1.0;
    params.exploration_months = 1;
    params.monthly_decline = 0.04;
    file.catalog
        .iter_mut()
        .find(|s| s.kind == PlantKindDto::Geothermal)
        .unwrap()
        .build_months = 4;
    std::fs::write(path, ron::to_string(&file)?)?;
    let mut game = Game::start_new(
        &RonRegionCatalog::new(output.join("assets/regions")),
        &id,
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )?;
    for cell in [second, dry] {
        game.execute(Command::BuildPlant {
            kind: PlantKind::Geothermal,
            cell,
        })?;
    }
    let bundle = SaveBundle {
        compatibility: replay_compatibility(),
        metadata: SaveMetadata {
            slot: SaveId::new("c".repeat(32))?,
            run: SaveId::new("d".repeat(32))?,
            kind: SaveKind::Manual,
            name: "A19 accelerated acceptance".into(),
            saved_at: 0,
        },
        checkpoint: game.checkpoint(),
    };
    std::fs::create_dir(output.join("saves"))?;
    let bytes = encode_save(&bundle)?;
    std::fs::write(
        output.join("saves/cccccccccccccccccccccccccccccccc.mgsave"),
        &bytes,
    )?;
    let mut traces = Vec::new();
    for _ in 0..8 {
        if game.session().phase() == DayPhase::Review {
            game.execute(Command::FinishReview)?;
        }
        let drill: Vec<_> = game
            .session()
            .power()
            .plants()
            .values()
            .filter(|p| {
                p.geothermal().is_some_and(|g| g.notice.is_some())
                    && game.session().makeup_quote(p.id()).is_ok()
            })
            .map(|p| p.id())
            .collect();
        for plant in drill {
            game.execute(Command::DrillMakeupWells { plant })?;
        }
        game.execute(Command::BeginOperating)?;
        game.execute(Command::AdvanceTicks { n: 1440 })?;
        traces.push(trace(&game));
        // Check every lifecycle boundary through the same split-chart archive as browser saves.
        let saved = SaveBundle {
            checkpoint: game.checkpoint(),
            ..bundle.clone()
        };
        let archive = encode_archive(&saved, &saved.compatibility)?;
        let (decoded, source) = decode_archive(&archive, &saved.compatibility, &|| false)?;
        assert_eq!(decoded, saved);
        let restored = decoded.restore(
            source,
            &saved.compatibility,
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )?;
        assert_eq!(restored.checkpoint(), game.checkpoint());
        // Native directory autosaves use the same checkpoint, with separate publication code.
        use application::{SaveStore, SaveTarget};
        let store = FileSaveStore::new(output.join(format!("archive-{}", traces.len())))
            .with_monthly_charts();
        store.write(&saved, &SaveTarget::Create, &|| false)?;
        assert_eq!(store.read(&saved.metadata.slot)?.0, saved);
    }
    std::fs::write(output.join("native-trace.txt"), traces.join("\n"))?;
    println!("{}", traces.join("\n"));
    Ok(())
}
