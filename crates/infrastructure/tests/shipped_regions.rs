//! Checks on the regions shipped in `assets/regions`: which regions exist,
//! that their geography puts real places on the right side of the
//! interconnection seams, that the start fleets cover their territory, that
//! a month of play on each is reliable (also from a late-year start), that
//! Western hydro rides the winter evening peak on realistic flows, and that
//! a hands-off beginner keeps the licence through year one. The assets are
//! read in place, read-only.

use std::sync::Arc;

use application::{
    Command, Game, InMemoryCommandLog, LoadedRegion, RegionCatalog, RegionLoadError, RegionSummary,
};
use domain::demand::shape_factor;
use domain::events::DomainEvent;
use domain::generation::PlantStatus;
use domain::grid::TickSummary;
use domain::ids::{PlantId, RegionId};
use domain::region::{PlantKind, RegionDefinition};
use domain::session::{GameSettings, MonthlyReport, SessionStatus};
use domain::time::{SimMonth, TICKS_PER_DAY, TICKS_PER_HOUR};
use domain::units::{Hertz, Ratio};
use infrastructure::region::{RegionFile, RonRegionCatalog, map_region, to_cell};

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
fn both_regions_carry_the_frequency_protection_and_performance_defaults() {
    for id in [EAST, WEST] {
        let region = region(id);
        let (f, r) = (region.frequency(), region.regulator());
        assert_eq!(f.trip_above, Hertz::new(1.5), "{id} trip_above");
        assert_eq!(f.trip_after_ticks, 5, "{id} trip_after_ticks");
        assert_eq!(r.frequency_band, Hertz::new(0.2), "{id} frequency_band");
        assert_eq!(
            r.frequency_violation_share,
            Ratio::new(0.05).unwrap(),
            "{id} frequency_violation_share"
        );
    }
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

/// A catalog holding one shipped region whose start month is overridden
/// through its file DTO (the shipped files all start in January).
///
/// The archived source bytes are the unmodified file, so a log written by
/// such a game would not replay to the overridden start; these tests never
/// replay.
struct StartMonthOverride(LoadedRegion);

impl StartMonthOverride {
    fn new(id: &str, month: u8) -> Self {
        let mut loaded = catalog()
            .load(&RegionId::new(id))
            .expect("shipped region loads");
        let mut file = region_file(id);
        file.start.month = month;
        loaded.definition = Arc::new(map_region(file).expect("overridden region maps"));
        Self(loaded)
    }
}

impl RegionCatalog for StartMonthOverride {
    fn list(&self) -> Result<Vec<RegionSummary>, RegionLoadError> {
        Ok(vec![RegionSummary {
            id: self.0.definition.id().clone(),
            name_key: self.0.definition.name_key().to_owned(),
        }])
    }

    fn load(&self, id: &RegionId) -> Result<LoadedRegion, RegionLoadError> {
        if self.0.definition.id() == id {
            Ok(self.0.clone())
        } else {
            Err(RegionLoadError::NotFound(id.clone()))
        }
    }
}

/// Starts a game on `id` from `catalog` with the hands-off plan in place.
fn hands_off_game(catalog: &dyn RegionCatalog, id: &str, seed: u64) -> Game {
    let mut game = Game::start_new(
        catalog,
        &RegionId::new(id),
        seed,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .expect("game starts");
    auto_follow_all(&mut game);
    game
}

/// One played month: what the hydro fleet held when the day began, every
/// tick of the day, and the month's report.
struct PlayedMonth {
    hydro_start_budget: Vec<(PlantId, f64)>,
    ticks: Vec<TickSummary>,
    report: MonthlyReport,
}

/// Plays the current month's day (`BeginOperating`, then one day of ticks)
/// and returns it. The game is left in review: the caller decides whether to
/// `FinishReview`.
fn play_month(game: &mut Game) -> PlayedMonth {
    game.execute(Command::BeginOperating)
        .expect("begin operating");
    let hydro_start_budget = game
        .session()
        .plant_views()
        .iter()
        .filter(|plant| plant.kind == PlantKind::Hydro)
        .map(|plant| {
            let budget = plant.hydro_budget.map_or(0.0, |b| b.value());
            (plant.id, budget)
        })
        .collect();
    let events = game
        .execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
        .expect("advance the day");
    let mut ticks = Vec::new();
    let mut report = None;
    for event in events {
        match event {
            DomainEvent::TickCompleted(summary) => ticks.push(summary),
            DomainEvent::MonthClosed(closed) => report = Some(*closed),
            _ => {}
        }
    }
    PlayedMonth {
        hydro_start_budget,
        ticks,
        report: report.expect("the month closed"),
    }
}

/// Spec §12.7 "end to end": every hydro plant that began the day with water
/// keeps producing through the evening peak, 20:00 to 21:00 inclusive.
fn assert_no_evening_hydro_cliff(label: &str, played: &PlayedMonth) {
    let evening = 20 * TICKS_PER_HOUR..=21 * TICKS_PER_HOUR;
    let watered: Vec<PlantId> = played
        .hydro_start_budget
        .iter()
        .filter(|(_, budget)| *budget > 0.0)
        .map(|(plant, _)| *plant)
        .collect();
    assert!(!watered.is_empty(), "{label}: no hydro began with water");
    for summary in played.ticks.iter().filter(|s| evening.contains(&s.tick)) {
        for plant in &watered {
            let output = summary.by_plant.get(plant).map_or(0.0, |mw| mw.value());
            assert!(
                output > 0.0,
                "{label}: hydro {plant:?} at {output} MW on tick {}",
                summary.tick
            );
        }
    }
}

#[test]
fn west_january_hydro_has_no_evening_cliff() {
    let january = region(WEST).hydro().monthly_flow[0];
    assert!(
        january <= 0.9,
        "West January flow {january} is above a realistic winter flow"
    );
    for seed in 0..8 {
        let mut game = hands_off_game(&catalog(), WEST, seed);
        assert_eq!(
            game.session().clock().month().month(),
            1,
            "starts in January"
        );
        let played = play_month(&mut game);
        assert_no_evening_hydro_cliff(&format!("West January, seed {seed}"), &played);
    }
}

#[test]
fn west_october_smoke_run_is_reliable() {
    let mut game = hands_off_game(&StartMonthOverride::new(WEST, 10), WEST, 7);
    assert_eq!(
        game.session().clock().month().month(),
        10,
        "starts in October"
    );
    let played = play_month(&mut game);
    assert!(
        played.report.reliability.value() >= 0.99,
        "West October: reliability {:?}",
        played.report.reliability
    );
    assert_eq!(game.session().status(), SessionStatus::Active);
    assert_no_evening_hydro_cliff("West October", &played);
}

/// Spec §14.5 beginner acceptance: with the hands-off plan (every
/// dispatchable plant committed all day under AGC, no other commands), each
/// shipped region is still `Active` after twelve months on seeds 0-7.
#[test]
fn hands_off_plan_survives_year_one() {
    for id in [EAST, WEST] {
        for seed in 0..8 {
            let mut game = hands_off_game(&catalog(), id, seed);
            for month in 1..=12 {
                let played = play_month(&mut game);
                assert_eq!(
                    game.session().status(),
                    SessionStatus::Active,
                    "{id} seed {seed}: game over at the close of month {month} ({:?}, \
                     out of band {:.3}, reliability {:.4})",
                    played.report.month,
                    played.report.out_of_band_share.value(),
                    played.report.reliability.value(),
                );
                game.execute(Command::FinishReview).expect("finish review");
            }
        }
    }
}
