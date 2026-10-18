//! Checks on the regions shipped in `assets/regions`: which regions exist,
//! that their geography puts real places on the right side of the
//! interconnection seams, that the start fleets cover their territory, and
//! that a month of play on each is reliable. The assets are read in place,
//! read-only.

use application::{Command, Game, InMemoryCommandLog, RegionCatalog};
use domain::demand::shape_factor;
use domain::events::DomainEvent;
use domain::generation::PlantStatus;
use domain::ids::{PlantId, RegionId};
use domain::region::RegionDefinition;
use domain::session::{GameSettings, MonthlyReport, SessionStatus};
use domain::time::{SimMonth, TICKS_PER_DAY, TICKS_PER_HOUR};
use infrastructure::region::{RegionFile, RonRegionCatalog, to_cell};

const REGIONS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/regions");
const EAST: &str = "eastern_interconnection";
const WEST: &str = "western_interconnection";

fn catalog() -> RonRegionCatalog {
    RonRegionCatalog::new(REGIONS_DIR)
}

fn region(id: &str) -> RegionDefinition {
    catalog()
        .load(&RegionId::new(id))
        .expect("shipped region loads")
        .definition
        .as_ref()
        .clone()
}

/// The raw file, for mapping (lon, lat) to cells exactly as loading does.
fn region_file(id: &str) -> RegionFile {
    let text = std::fs::read_to_string(format!("{REGIONS_DIR}/{id}.ron")).expect("file exists");
    ron::de::from_str(&text).expect("file parses")
}

fn total_base_load(region: &RegionDefinition) -> f64 {
    region.cities().iter().map(|c| c.base_load.value()).sum()
}

/// Whether the cell containing (lon, lat) is land.
fn is_land(id: &str, lon: f64, lat: f64) -> bool {
    let file = region_file(id);
    let cell = to_cell(&file, lon, lat);
    region(id)
        .terrain()
        .get(cell)
        .expect("point lies inside the grid")
        .land
}

/// The highest demand multiplier of the year: every month's shape at every
/// hour boundary (interpolation between hours never exceeds the endpoints).
fn max_shape_factor(region: &RegionDefinition) -> f64 {
    (1..=12)
        .flat_map(|month| {
            let month = SimMonth::new(2027, month).expect("valid month");
            (0..24).map(move |hour| shape_factor(region.demand(), month, hour * TICKS_PER_HOUR))
        })
        .fold(f64::MIN, f64::max)
}

#[test]
fn ships_exactly_two_regions() {
    let listed = catalog().list().expect("shipped regions list");
    let ids: Vec<_> = listed.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, [EAST, WEST]);
}

#[test]
fn east_is_three_to_five_times_west() {
    let ratio = total_base_load(&region(EAST)) / total_base_load(&region(WEST));
    assert!((3.0..=5.0).contains(&ratio), "East / West load was {ratio}");
}

#[test]
fn cities_are_named_metros() {
    for id in [EAST, WEST] {
        let cities = region(id).cities().to_vec();
        assert!(
            (18..=25).contains(&cities.len()),
            "{id} has {} cities",
            cities.len()
        );
        for city in &cities {
            assert!(
                city.name_key.starts_with("city-"),
                "{id}: {}",
                city.name_key
            );
            assert!(city.base_load.value() > 0.0, "{id}: {}", city.name_key);
        }
    }
}

#[test]
fn landmarks_fall_on_the_right_side_of_boundaries() {
    // East: Ontario is in, the Texas Panhandle is in, ERCOT and Quebec are out.
    assert!(is_land(EAST, -79.38, 43.65), "Toronto");
    assert!(is_land(EAST, -101.83, 35.22), "Amarillo");
    assert!(!is_land(EAST, -97.74, 30.27), "Austin");
    assert!(!is_land(EAST, -73.57, 45.50), "Montreal");
    // West: Baja California and El Paso are in.
    assert!(is_land(WEST, -117.04, 32.51), "Tijuana");
    assert!(is_land(WEST, -106.44, 31.76), "El Paso");
    assert!(is_land(WEST, -122.33, 47.61), "Seattle");
}

#[test]
fn start_fleet_covers_connected_peak_with_margin() {
    for id in [EAST, WEST] {
        let region = region(id);
        let start = region.start();
        let firm: f64 = start
            .fleet
            .iter()
            .map(|plant| {
                let spec = region.spec(plant.kind);
                spec.capacity_credit.of(spec.capacity).value()
            })
            .sum();
        let connected: f64 = start
            .connected_cities
            .iter()
            .map(|&city| region.city(city).expect("known city").base_load.value())
            .sum();
        let peak = connected * max_shape_factor(&region);
        assert!(
            firm >= 1.10 * peak,
            "{id}: firm {firm} MW against a connected peak of {peak} MW"
        );
    }
}

/// Commits every dispatchable, commissioned plant for hours 0-23 and turns
/// AGC on for it (a local copy of the application tests' helper, which this
/// crate cannot see).
fn auto_follow_all(game: &mut Game) {
    let plants: Vec<PlantId> = game
        .session()
        .plant_views()
        .iter()
        .filter(|plant| {
            plant.kind.is_dispatchable()
                && !matches!(plant.status, PlantStatus::UnderConstruction { .. })
        })
        .map(|plant| plant.id)
        .collect();
    for plant in plants {
        game.execute(Command::CommitPlant {
            plant,
            hours: [true; 24],
        })
        .expect("commit plant all day");
        game.execute(Command::SetAutoFollow { plant, on: true })
            .expect("turn AGC on");
    }
}

#[test]
fn one_month_smoke_run_is_reliable() {
    for id in [EAST, WEST] {
        let mut game = Game::start_new(
            &catalog(),
            &RegionId::new(id),
            7,
            GameSettings::default(),
            Box::new(InMemoryCommandLog::default()),
        )
        .expect("game starts");
        auto_follow_all(&mut game);
        game.execute(Command::BeginOperating)
            .expect("begin operating");
        let events = game
            .execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
            .expect("advance the day");
        let report: &MonthlyReport = events
            .iter()
            .find_map(|event| match event {
                DomainEvent::MonthClosed(report) => Some(report.as_ref()),
                _ => None,
            })
            .expect("the month closed");
        assert!(
            report.reliability.value() >= 0.99,
            "{id}: reliability {:?}",
            report.reliability
        );
        assert_eq!(game.session().status(), SessionStatus::Active, "{id}");
        game.execute(Command::FinishReview).expect("finish review");
    }
}
