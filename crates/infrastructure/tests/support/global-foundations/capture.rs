use application::*;
use domain::{events::DomainEvent, ids::{CellCoord, RegionId}, region::PlantKind, session::GameSettings};
use infrastructure::{region::RonRegionCatalog, save::encode_save};
use std::{io::Write, path::PathBuf};
fn main() {
    let out = PathBuf::from(std::env::args_os().nth(1).unwrap());
    let catalog = RonRegionCatalog::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/regions"));
    for id in ["eastern_interconnection", "western_interconnection"] {
        let mut game = Game::start_new(&catalog, &RegionId::new(id), 7, GameSettings::default(), Box::new(InMemoryCommandLog::default())).unwrap();
        let s = game.session();
        let terrain = s.region().terrain();
        let home = s.region().cities().iter().find(|c| c.id == s.home_city()).unwrap().cell;
        let mut sites: Vec<_> = (0..terrain.height()).flat_map(|y| (0..terrain.width()).map(move |x| CellCoord{x,y})).filter(|c| s.plant_site_check(PlantKind::Battery, *c).is_ok()).collect();
        sites.sort_by_key(|c| (c.x.abs_diff(home.x).pow(2) + c.y.abs_diff(home.y).pow(2), *c));
        let events = game.execute(Command::BuildPlant{kind:PlantKind::Battery,cell:sites[0]}).unwrap();
        let plant = events.iter().find_map(|e| if let DomainEvent::PlantConstructionStarted{plant,..}=e {Some(*plant)} else {None}).unwrap();
        let node = game.session().plant_views().iter().find(|p| p.id==plant).unwrap().node;
        let from = game.session().anchor_node();
        game.execute(Command::BuildLine{from,to:node}).unwrap();
        for _ in 0..2 {
            game.execute(Command::BeginOperating).unwrap();
            game.execute(Command::AdvanceTicks{n:1440}).unwrap();
            game.execute(Command::FinishReview).unwrap();
        }
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks{n:900}).unwrap();
        let battery = game.session().plant_views().into_iter().find(|p|p.id==plant).unwrap().battery.unwrap();
        assert!(battery.stored.value()>0.0, "{id}: battery not charged");
        let b = SaveBundle{compatibility:ReplayCompatibility{build:"plan6-baseline-rules15".into(),platform:"baseline".into()},metadata:SaveMetadata{slot:SaveId::new("a".repeat(32)).unwrap(),run:SaveId::new("b".repeat(32)).unwrap(),kind:SaveKind::Manual,name:format!("{id} before global foundations"),saved_at:0},checkpoint:game.checkpoint()};
        assert_eq!(b.checkpoint.snapshot.as_ref().unwrap().schema,3);
        let raw = encode_save(&b).unwrap();
        let mut zip = flate2::write::ZlibEncoder::new(Vec::new(),flate2::Compression::best());
        zip.write_all(&raw).unwrap();
        let bytes=zip.finish().unwrap();
        std::fs::write(out.join(format!("{id}-schema3.mgsave.z")), &bytes).unwrap();
        game.execute(Command::AdvanceTicks{n:60}).unwrap();
        let after60=game.checkpoint().state_hash;
        game.execute(Command::AdvanceTicks{n:480}).unwrap();
        let review=game.checkpoint().state_hash;
        std::fs::write(out.join(format!("{id}-expected.txt")),format!("{after60}\n{review}\n")).unwrap();
        println!("{id}: {} compressed bytes; after60={after60}, review={review}; charged={} MWh",bytes.len(),battery.stored.value());
    }
}
