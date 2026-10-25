//! Checks on the regions shipped in `assets/regions`: which regions exist,
//! that their geography puts real places on the right side of the
//! interconnection seams, that the start fleets cover their territory, that
//! a month of play on each is reliable (also from a late-year start), that
//! Western hydro rides the winter evening peak on realistic flows, and that
//! a hands-off beginner keeps the licence through year one. The assets are
//! read in place, read-only.

use std::sync::Arc;

use application::{
    Command, Game, InMemoryCommandLog, LoadedRegion, RegionCatalog, RegionListing, RegionLoadError,
    RegionSummary,
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
fn both_regions_allow_battery_discharge_from_four_pm() {
    for id in [EAST, WEST] {
        let region = region(id);
        let battery = region.spec(PlantKind::Battery).battery.unwrap();
        assert_eq!(battery.discharge_start_hour, 16, "{id}");
        assert_eq!(battery.charge_end_hour, 18, "{id}");
    }
}

#[test]
fn both_regions_carry_the_frequency_protection_and_performance_defaults() {
    for id in [EAST, WEST] {
        let region = region(id);
        let (f, r) = (region.frequency(), region.regulator());
        assert_eq!(f.trip_above, Hertz::new(1.5), "{id} trip_above");
        assert_eq!(f.trip_after_ticks, 5, "{id} trip_after_ticks");
        assert_eq!(
            f.shed_stage_spacing,
            Hertz::new(0.2),
            "{id} shed_stage_spacing"
        );
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
    let ids: Vec<_> = listed.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(ids, [EAST, WEST]);
    for listing in &listed {
        assert!(listing.entry.is_ok(), "{listing:?}");
    }
}

#[test]
fn shipped_home_cities_anchor_the_existing_city_nodes() {
    for (id, name_key) in [(EAST, "city-columbus"), (WEST, "city-seattle")] {
        let game = new_game(&catalog(), id, 0);
        let session = game.session();
        let cities = session.city_views();
        let home = cities
            .iter()
            .find(|city| city.id == session.home_city())
            .unwrap();
        assert_eq!(home.name_key, name_key);
        assert!(home.connected);
        assert_eq!(home.node, session.anchor_node());
        assert_eq!(
            session.power().network().nodes().count(),
            cities.len() + session.plant_views().len()
        );
        assert_eq!(
            session.line_views().len(),
            session.region().start().connected_cities.len() - 1 + session.plant_views().len()
        );
        session.check_invariants().unwrap();
    }
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
    fn list(&self) -> Result<Vec<RegionListing>, RegionLoadError> {
        Ok(vec![RegionListing {
            id: self.0.definition.id().clone(),
            entry: Ok(RegionSummary {
                id: self.0.definition.id().clone(),
                name_key: self.0.definition.name_key().to_owned(),
            }),
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

/// Starts a game on `id` from `catalog` and issues no commands: the start
/// fleet runs the previous operator's schedule (spec §16.3).
fn new_game(catalog: &dyn RegionCatalog, id: &str, seed: u64) -> Game {
    Game::start_new(
        catalog,
        &RegionId::new(id),
        seed,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .expect("game starts")
}

/// Starts a game on `id` from `catalog` with the hands-off plan in place.
fn hands_off_game(catalog: &dyn RegionCatalog, id: &str, seed: u64) -> Game {
    let mut game = new_game(catalog, id, seed);
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

/// Spec §16.2: a new game with no player commands plays its first day
/// without a forecast shortfall, on both shipped regions at seeds 0-7.
#[test]
fn shipped_regions_start_without_a_shortfall() {
    for id in [EAST, WEST] {
        for seed in 0..8 {
            let game = new_game(&catalog(), id, seed);
            assert_eq!(
                game.session().capacity_shortfall_check(),
                None,
                "{id} seed {seed}"
            );
        }
    }
}

/// Spec §14.5 beginner acceptance: with the hands-off plan (every
/// dispatchable plant committed all day under AGC, no other commands), each
/// shipped region is still `Active` after twelve months on seeds 0-7.
#[test]
fn hands_off_plan_survives_year_one() {
    assert_survives_year_one(hands_off_game);
}

/// Spec §16.5: the beginner acceptance check also passes with no commands at
/// all, on the previous operator's schedule.
#[test]
fn no_commands_survive_year_one() {
    assert_survives_year_one(new_game);
}

/// Plays twelve months on both regions at seeds 0-7, each game started by
/// `start` and given no commands beyond the phase changes, and asserts the
/// game is still `Active` at every close. Before every month the forecast
/// shows no shortfall, so a hands-off player never sees the shortfall prompt
/// (spec §16.4).
fn assert_survives_year_one(start: fn(&dyn RegionCatalog, &str, u64) -> Game) {
    for id in [EAST, WEST] {
        for seed in 0..8 {
            let mut game = start(&catalog(), id, seed);
            for month in 1..=12 {
                assert_eq!(
                    game.session().capacity_shortfall_check(),
                    None,
                    "{id} seed {seed} month {month}"
                );
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

/// Spec §15.7: the shipped start fleets cover their forecasts, so the
/// hands-off plan never orders a rotating outage. Over the hands-off year
/// on seeds 0-7, every month of both regions closes with no planned outage;
/// a failure lists each month that had one, with the hours it ran and its
/// size.
#[test]
fn shipped_regions_never_order_outages_hands_off() {
    let mut outages = Vec::new();
    for id in [EAST, WEST] {
        for seed in 0..8 {
            let mut game = hands_off_game(&catalog(), id, seed);
            let mut months_played = 0;
            for _ in 1..=12 {
                let played = play_month(&mut game);
                months_played += 1;
                let report = &played.report;
                if report.rotating_outage.value() != 0.0 {
                    let hours: std::collections::BTreeSet<u32> = played
                        .ticks
                        .iter()
                        .filter(|summary| summary.outage > Ratio::ZERO)
                        .map(|summary| summary.tick / TICKS_PER_HOUR)
                        .collect();
                    outages.push(format!(
                        "{id} seed {seed} {:?}: {:?} planned (peak share {:?}) in hours {hours:?}",
                        report.month, report.rotating_outage, report.peak_outage_share,
                    ));
                }
                if game.session().status() != SessionStatus::Active {
                    break;
                }
                game.execute(Command::FinishReview).expect("finish review");
            }
            // An early game over would end the year with fewer months checked.
            assert_eq!(months_played, 12, "{id} seed {seed}: months played");
        }
    }
    assert!(
        outages.is_empty(),
        "rotating outages:\n{}",
        outages.join("\n")
    );
}

/// Gameplay pacing is a contract: basic builds finish within a few played days,
/// while nuclear and hydro remain longer-term investments, within two played years.
#[test]
fn shipped_projects_fit_tycoon_pacing() {
    for id in [EAST, WEST] {
        let region = region(id);
        for (kind, longest) in [
            (PlantKind::Peaker, 3),
            (PlantKind::Solar, 3),
            (PlantKind::Ccgt, 6),
            (PlantKind::Coal, 12),
            (PlantKind::Hydro, 18),
            (PlantKind::Nuclear, 24),
        ] {
            assert!(
                region.spec(kind).build_months <= longest,
                "{id}: {kind:?} takes {} played months",
                region.spec(kind).build_months
            );
        }
    }
}

fn assert_first_expansion(id: &str, seed: u64) {
    use domain::{grid::LineStatus, units::Money};
    use std::collections::BTreeSet;
    let mut game = new_game(&catalog(), id, seed);
    let region = Arc::clone(game.session().region());
    let starting_cash = game.session().company().cash();
    let city_key = if id == EAST {
        "city-cleveland"
    } else {
        "city-vancouver"
    };
    let target = game
        .session()
        .city_views()
        .into_iter()
        .find(|city| city.name_key == city_key)
        .unwrap();
    assert!(!target.connected);
    let future_base: f64 = region
        .cities()
        .iter()
        .filter(|city| region.start().connected_cities.contains(&city.id) || city.id == target.id)
        .map(|city| city.base_load.value())
        .sum();
    let firm: f64 = region
        .start()
        .fleet
        .iter()
        .map(|plant| {
            let spec = region.spec(plant.kind);
            spec.capacity_credit.of(spec.capacity).value()
        })
        .sum();
    let gas = region.spec(PlantKind::Ccgt);
    // Cover the entire year's peak plus 10% operating headroom, using the same capacity
    // credits as the starting-fleet acceptance check. This includes existing spare capacity.
    let count = ((future_base * max_shape_factor(&region) * 1.10 - firm)
        / gas.capacity_credit.of(gas.capacity).value())
    .ceil()
    .max(1.0) as usize;
    assert!(
        count <= 3,
        "{id}: first expansion needs {count} new gas plants"
    );
    let anchor = game.session().anchor_node();
    let home = region
        .cities()
        .iter()
        .find(|city| city.id == region.start().home_city)
        .unwrap()
        .cell;
    let occupied: BTreeSet<_> = game
        .session()
        .plant_views()
        .iter()
        .map(|plant| plant.cell)
        .chain(game.session().city_views().iter().map(|city| city.cell))
        .collect();
    let mut sites: Vec<_> = (0..region.terrain().height())
        .flat_map(|y| (0..region.terrain().width()).map(move |x| domain::ids::CellCoord { x, y }))
        .filter(|cell| {
            !occupied.contains(cell) && region.site_check(PlantKind::Ccgt, *cell).is_ok()
        })
        .collect();
    sites.sort_by_key(|cell| {
        (
            i32::from(cell.x).abs_diff(i32::from(home.x)).pow(2)
                + i32::from(cell.y).abs_diff(i32::from(home.y)).pow(2),
            *cell,
        )
    });
    let mut lines = Vec::new();
    let mut total = Money::ZERO;
    for cell in sites.into_iter().take(count) {
        let events = game
            .execute(Command::BuildPlant {
                kind: PlantKind::Ccgt,
                cell,
            })
            .unwrap();
        let plant = events
            .iter()
            .find_map(|event| match event {
                DomainEvent::PlantConstructionStarted { plant, .. } => Some(*plant),
                _ => None,
            })
            .unwrap();
        let node = game
            .session()
            .plant_views()
            .iter()
            .find(|view| view.id == plant)
            .unwrap()
            .node;
        let quote = game.session().line_quote(anchor, node).unwrap();
        total += gas.capital_cost + quote.capital;
        lines.push((anchor, node, quote));
    }
    let (from, quote) = game
        .session()
        .city_views()
        .into_iter()
        .filter(|city| city.connected)
        .map(|city| {
            (
                city.node,
                game.session().line_quote(city.node, target.node).unwrap(),
            )
        })
        .min_by_key(|(node, quote)| (quote.cells, *node))
        .unwrap();
    total += quote.capital;
    lines.push((from, target.node, quote));
    assert!(
        total.cents() <= starting_cash.cents() / 2,
        "{id}: expansion costs {total:?} against {starting_cash:?}"
    );
    let ready = lines
        .iter()
        .map(|(_, _, quote)| quote.months)
        .max()
        .unwrap()
        .max(gas.build_months);
    assert!(
        ready <= 12,
        "{id}: first expansion waits {ready} played months"
    );
    for month in 0..12 {
        for (from, to, quote) in &lines {
            if month + quote.months == ready {
                game.execute(Command::BuildLine {
                    from: *from,
                    to: *to,
                })
                .unwrap();
            }
        }
        auto_follow_all(&mut game);
        assert_eq!(
            game.session().capacity_shortfall_check(),
            None,
            "{id} seed {seed} month {month}"
        );
        let played = play_month(&mut game);
        assert_eq!(
            game.session().status(),
            SessionStatus::Active,
            "{id} seed {seed} month {month}: {:?}",
            played.report
        );
        assert!(
            played.report.reliability.value() >= 0.999,
            "{id} seed {seed} month {month}: reliability {:?}",
            played.report.reliability
        );
        assert!(
            game.session().company().cash().cents() >= starting_cash.cents() / 3,
            "{id}: cash buffer exhausted at month {month}"
        );
        let connected = game
            .session()
            .city_views()
            .iter()
            .find(|city| city.id == target.id)
            .unwrap()
            .connected;
        assert_eq!(
            connected,
            month + 1 >= ready,
            "city must connect when its generation is ready"
        );
        if month + 1 >= ready {
            assert!(
                game.session()
                    .line_views()
                    .iter()
                    .all(|line| !matches!(line.status, LineStatus::UnderConstruction { .. }))
            );
        }
        game.execute(Command::FinishReview).unwrap();
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config { cases: 4, failure_persistence: None, ..Default::default() })]
    #[test]
    fn shipped_first_city_expansion_is_affordable_and_reliable(seed in proptest::prelude::any::<u64>()) {
        for id in [EAST, WEST] {
            assert_first_expansion(id, seed);
        }
    }
}

/// Extra idle storage and its lines cannot beat a baseline across three annual reviews.
/// Separate cases let nextest parallelize independent seeds and name any failure.
mod gold_plated_batteries_do_not_raise_cash_over_three_reviews {
    use super::*;
    use application::pipeline::{CoreHandler, InvariantCheckLayer};
    use application::{CommandError, CommandHandler};
    use domain::company::AssetRef;
    use domain::error::DomainError;
    use domain::generation::BatteryTotals;
    use domain::ids::CellCoord;
    use domain::session::GameSession;

    #[derive(Clone, Copy)]
    enum Driver {
        Domain,
        Application,
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum StorageUse {
        Idle,
        Enabled,
    }

    fn idle_commissioned_storage(company: &mut FinancialCompany, batteries: &[PlantId]) {
        let enabled: Vec<_> = batteries
            .iter()
            .copied()
            .filter(|id| {
                let plant = company.session().power().plant(*id).unwrap();
                !matches!(plant.status(), PlantStatus::UnderConstruction { .. })
                    && plant.battery().unwrap().enabled()
            })
            .collect();
        for plant in enabled {
            company
                .execute(Command::SetBatteryEnabled { plant, on: false })
                .unwrap();
        }
    }

    #[test]
    fn idle_storage_preserves_regulatory_capacity_without_dispatch() {
        for region in [EAST, WEST] {
            let mut company = FinancialCompany::new(region, 7, Driver::Domain);
            let batteries = add_excess_storage(&mut company);
            for _ in 0..2 {
                company.play_month();
                company.execute(Command::FinishReview).unwrap();
            }
            let firm = company
                .session()
                .power()
                .firm_capacity(&company.session().region().regulator().storage_accreditation);
            let capital = company.session().company().rate_base().clone();
            idle_commissioned_storage(&mut company, &batteries);
            for id in &batteries {
                assert!(
                    !company
                        .session()
                        .power()
                        .plant(*id)
                        .unwrap()
                        .battery()
                        .unwrap()
                        .enabled(),
                    "{region}: financial regressions must keep commissioned storage idle"
                );
            }
            assert_eq!(company.session().company().rate_base(), &capital);
            assert_eq!(
                company
                    .session()
                    .power()
                    .firm_capacity(&company.session().region().regulator().storage_accreditation,),
                firm
            );
            let events = company.play_month();
            let report = events
                .iter()
                .find_map(|e| match e {
                    DomainEvent::MonthClosed(report) => Some(report.as_ref()),
                    _ => None,
                })
                .unwrap();
            for id in batteries {
                assert_eq!(report.storage.get(&id), Some(&BatteryTotals::default()));
            }
        }
    }

    /// The broad financial matrix needs the real commands and invariant checks, but not charts,
    /// ramp warnings or a recording. The two application cases retain that wider integration.
    enum FinancialCompany {
        Domain(Box<GameSession>),
        Application(Box<Game>),
    }

    impl FinancialCompany {
        fn new(region: &str, seed: u64, driver: Driver) -> Self {
            let mut company = match driver {
                Driver::Domain => {
                    let loaded = catalog().load(&RegionId::new(region)).unwrap();
                    Self::Domain(Box::new(GameSession::new(
                        loaded.definition,
                        seed,
                        GameSettings::default(),
                    )))
                }
                Driver::Application => {
                    Self::Application(Box::new(new_game(&catalog(), region, seed)))
                }
            };
            let plants: Vec<_> = company
                .session()
                .plant_views()
                .into_iter()
                .filter(|p| {
                    p.kind.is_dispatchable()
                        && !matches!(p.status, PlantStatus::UnderConstruction { .. })
                })
                .map(|p| p.id)
                .collect();
            for plant in plants {
                company
                    .execute(Command::CommitPlant {
                        plant,
                        hours: [true; 24],
                    })
                    .unwrap();
                company
                    .execute(Command::SetAutoFollow { plant, on: true })
                    .unwrap();
            }
            company
        }

        fn session(&self) -> &GameSession {
            match self {
                Self::Domain(session) => session,
                Self::Application(game) => game.session(),
            }
        }

        fn execute(&mut self, command: Command) -> Result<Vec<DomainEvent>, CommandError> {
            match self {
                Self::Domain(session) => {
                    InvariantCheckLayer::new(CoreHandler).handle(session, &command)
                }
                Self::Application(game) => game.execute(command),
            }
        }

        fn play_month(&mut self) -> Vec<DomainEvent> {
            self.execute(Command::BeginOperating).unwrap();
            self.execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
                .unwrap()
        }
    }

    #[test]
    fn financial_driver_matches_application_through_commissioning() {
        for region in [EAST, WEST] {
            let mut direct = FinancialCompany::new(region, 7, Driver::Domain);
            let mut application = FinancialCompany::new(region, 7, Driver::Application);
            let batteries = add_excess_storage(&mut direct);
            assert_eq!(batteries, add_excess_storage(&mut application));
            for month in 0..3 {
                direct.play_month();
                application.play_month();
                assert!(
                    direct.session().snapshot_state() == application.session().snapshot_state(),
                    "{region} month {month}: projections must not change simulation state"
                );
                if month < 2 {
                    direct.execute(Command::FinishReview).unwrap();
                    application.execute(Command::FinishReview).unwrap();
                }
            }
            assert!(batteries.iter().all(|id| !matches!(
                direct.session().power().plant(*id).unwrap().status(),
                PlantStatus::UnderConstruction { .. }
            )));
        }
    }

    fn add_excess_storage(gold: &mut FinancialCompany) -> Vec<PlantId> {
        let home = gold.session().anchor_node();
        let home_cell = gold.session().power().network().node(home).unwrap().cell;
        let other = gold.session().plant_views()[0].node;
        let terrain = gold.session().region().terrain();
        let mut sites: Vec<_> = (0..terrain.height())
            .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
            .filter(|cell| {
                gold.session()
                    .plant_site_check(PlantKind::Battery, *cell)
                    .is_ok()
            })
            .collect();
        sites.sort_by_key(|cell| {
            (
                i32::from(cell.x).abs_diff(i32::from(home_cell.x)).pow(2)
                    + i32::from(cell.y).abs_diff(i32::from(home_cell.y)).pow(2),
                *cell,
            )
        });
        assert!(sites.len() >= 10);
        let mut batteries = Vec::new();
        for cell in sites.into_iter().take(10) {
            let events = gold
                .execute(Command::BuildPlant {
                    kind: PlantKind::Battery,
                    cell,
                })
                .unwrap();
            let plant = events
                .iter()
                .find_map(|e| {
                    if let DomainEvent::PlantConstructionStarted { plant, .. } = e {
                        Some(*plant)
                    } else {
                        None
                    }
                })
                .unwrap();
            let node = gold
                .session()
                .plant_views()
                .iter()
                .find(|p| p.id == plant)
                .unwrap()
                .node;
            gold.execute(Command::BuildLine {
                from: home,
                to: node,
            })
            .unwrap();
            assert!(matches!(
                gold.session().line_quote(node, other),
                Err(DomainError::LineNotNeeded { .. })
            ));
            batteries.push(plant);
        }
        batteries
    }

    fn compare(region_id: &str, seed: u64, driver: Driver, storage_use: StorageUse) {
        let mut baseline = FinancialCompany::new(region_id, seed, driver);
        let mut gold = FinancialCompany::new(region_id, seed, driver);
        let batteries = add_excess_storage(&mut gold);
        let mut first_december_held = false;
        let mut first_commission = None;
        let mut storage_operated = false;
        for month in 0..36 {
            if storage_use == StorageUse::Idle {
                idle_commissioned_storage(&mut gold, &batteries);
            }
            baseline.play_month();
            let events = gold.play_month();
            let report = events
                .iter()
                .find_map(|e| match e {
                    DomainEvent::MonthClosed(report) => Some(report.as_ref()),
                    _ => None,
                })
                .unwrap();
            let dispatched = report
                .storage
                .values()
                .any(|totals| totals.input.value() > 0.0 || totals.output.value() > 0.0);
            storage_operated |= dispatched;
            if storage_use == StorageUse::Idle {
                assert!(
                    report
                        .storage
                        .values()
                        .all(|totals| *totals == BatteryTotals::default()),
                    "{region_id} seed {seed} month {month}: idle storage must have no flows"
                );
            }
            let now = gold.session().clock().month();
            if events.iter().any(|e| matches!(e, DomainEvent::PlantCommissioned { plant, .. } if batteries.contains(plant))) { first_commission.get_or_insert(now); }
            if now.month() == 12 && first_commission.is_some_and(|first| first.year() == now.year())
            {
                first_december_held = events.iter().any(|e| matches!(e, DomainEvent::CapitalHeld { asset: AssetRef::Plant(plant), .. } if batteries.contains(plant)));
            }
            if month < 35 {
                baseline.execute(Command::FinishReview).unwrap();
                gold.execute(Command::FinishReview).unwrap();
            }
        }
        assert!(
            first_december_held,
            "{region_id} seed {seed}: first review must hold excess storage"
        );
        assert_eq!(
            storage_operated,
            storage_use == StorageUse::Enabled,
            "{region_id} seed {seed}: exercise the intended {storage_use:?} storage mode"
        );
        assert!(
            gold.session().company_view().cash <= baseline.session().company_view().cash,
            "{region_id} seed {seed}: gold {:?} > baseline {:?}",
            gold.session().company_view().cash,
            baseline.session().company_view().cash
        );
    }

    macro_rules! case {
        ($name:ident, $region:expr, $seed:expr) => {
            #[test]
            fn $name() {
                compare($region, $seed, Driver::Domain, StorageUse::Idle);
            }
        };
    }
    case!(eastern_0, EAST, 0);
    case!(eastern_1, EAST, 1);
    case!(eastern_2, EAST, 2);
    case!(eastern_3, EAST, 3);
    case!(eastern_4, EAST, 4);
    case!(eastern_5, EAST, 5);
    case!(eastern_6, EAST, 6);
    case!(eastern_7, EAST, 7);
    case!(western_0, WEST, 0);
    case!(western_1, WEST, 1);
    case!(western_2, WEST, 2);
    case!(western_3, WEST, 3);
    case!(western_4, WEST, 4);
    case!(western_5, WEST, 5);
    case!(western_6, WEST, 6);
    case!(western_7, WEST, 7);

    #[test]
    fn application_eastern_enabled() {
        compare(EAST, 7, Driver::Application, StorageUse::Enabled);
    }

    #[test]
    fn application_western_enabled() {
        compare(WEST, 7, Driver::Application, StorageUse::Enabled);
    }
}
