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
use domain::events::DomainEvent;
use domain::generation::PlantStatus;
use domain::grid::TickSummary;
use domain::ids::{PlantId, RegionId};
use domain::region::{PlantKind, RegionDefinition};
use domain::session::{GameSettings, MonthlyReport, SessionStatus};
use domain::time::{SimMonth, TICKS_PER_DAY, TICKS_PER_HOUR};
use domain::units::{Hertz, Ratio};
use infrastructure::region::{RegionFileV4, RonRegionCatalog, map_region_v4};

const REGIONS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/regions");
const EAST: &str = "eastern_interconnection";
const WEST: &str = "western_interconnection";
const SOUTHERN_CHINA: &str = "southern_china";
const BRAZIL: &str = "brazil_sin";
const INDIA: &str = "india_nepal_bhutan";

/// The research note whose row tables the region files cite (`[P7-XX-NNN]`).
const RESEARCH_NOTE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/research/2026-10-25-plan-7-regional-economics.md"
);

/// Every shipped region ID, sorted like `RonRegionCatalog::list`. Each Plan 7 region task
/// inserts its ID here and in the tables below; every region-agnostic test iterates them.
const SHIPPED: &[&str] = &[BRAZIL, EAST, INDIA, SOUTHERN_CHINA, WEST];

/// The IDs Plan 7 adds, a subset of `SHIPPED`. The provenance, scale and squareness tests
/// iterate it: the Eastern and Western files predate those rules.
const NEW_REGIONS: &[&str] = &[BRAZIL, INDIA, SOUTHERN_CHINA];

/// Each region's real system capacity in GW, from the research note: the scale reference for
/// `new_region_load_scales_with_capacity`.
const RESEARCH_CAPACITY_GW: &[(&str, f64)] = &[
    (BRAZIL, 250.0),
    (EAST, 980.0),
    (INDIA, 520.0),
    (SOUTHERN_CHINA, 500.0),
    (WEST, 340.0),
];

/// Each region's start city (`start.home_city`), by name key.
const HOME_CITY: &[(&str, &str)] = &[
    (BRAZIL, "city-sao-paulo"),
    (EAST, "city-columbus"),
    (INDIA, "city-delhi"),
    (SOUTHERN_CHINA, "city-guangzhou"),
    (WEST, "city-seattle"),
];

/// The city each region's first-expansion acceptance test (spec §18.3) connects, by name key.
const EXPANSION_CITY: &[(&str, &str)] = &[
    (BRAZIL, "city-sorocaba"),
    (EAST, "city-cleveland"),
    (INDIA, "city-faridabad"),
    (SOUTHERN_CHINA, "city-shenzhen"),
    (WEST, "city-vancouver"),
];

/// Each region's nominal grid frequency in hertz.
const NOMINAL_HZ: &[(&str, f64)] = &[
    (BRAZIL, 60.0),
    (EAST, 60.0),
    (INDIA, 50.0),
    (SOUTHERN_CHINA, 50.0),
    (WEST, 60.0),
];

/// The row for `id` in one of the per-region tables above.
fn table_value<T: Copy>(table: &[(&str, T)], id: &str) -> T {
    table
        .iter()
        .find(|(key, _)| *key == id)
        .map(|(_, value)| *value)
        .unwrap_or_else(|| panic!("{id} has no row in a shipped-region table"))
}

fn table_keys<'a, T>(table: &[(&'a str, T)]) -> Vec<&'a str> {
    table.iter().map(|(key, _)| *key).collect()
}

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
fn region_file(id: &str) -> RegionFileV4 {
    let text = std::fs::read_to_string(format!("{REGIONS_DIR}/{id}.ron")).expect("file exists");
    ron::de::from_str(&text).expect("file parses")
}

fn total_base_load(region: &RegionDefinition) -> f64 {
    region.cities().iter().map(|c| c.base_load.value()).sum()
}

/// Whether the cell containing (lon, lat) is land. Points outside the grid count as its
/// nearest edge cell.
fn is_land(id: &str, lon: f64, lat: f64) -> bool {
    LandMap::load(id).land_at_clamped(lon, lat)
}

/// A region's land mask with its geographic bounds, loaded once so that whole-grid scans
/// don't re-read the file for every cell.
struct LandMap {
    lon: (f64, f64),
    lat: (f64, f64),
    grid: (u16, u16),
    /// Row-major from the northwest cell.
    land: Vec<bool>,
}

impl LandMap {
    fn from_cells(lon: (f64, f64), lat: (f64, f64), grid: (u16, u16), land: Vec<bool>) -> Self {
        assert_eq!(land.len(), usize::from(grid.0) * usize::from(grid.1));
        Self {
            lon,
            lat,
            grid,
            land,
        }
    }

    /// The shipped region `id`, mapped exactly as loading does.
    fn load(id: &str) -> Self {
        let file = region_file(id);
        let region = region(id);
        let terrain = region.terrain();
        let land = (0..terrain.height())
            .flat_map(|y| (0..terrain.width()).map(move |x| domain::ids::CellCoord { x, y }))
            .map(|cell| terrain.get(cell).expect("cell is in the grid").land)
            .collect();
        Self::from_cells(
            (file.bounds.west, file.bounds.east),
            (file.bounds.south, file.bounds.north),
            (file.grid.width, file.grid.height),
            land,
        )
    }

    fn land_at_clamped(&self, lon: f64, lat: f64) -> bool {
        let (width, height) = (f64::from(self.grid.0), f64::from(self.grid.1));
        let x = ((lon - self.lon.0) / (self.lon.1 - self.lon.0) * width)
            .floor()
            .clamp(0.0, width - 1.0) as usize;
        let y = ((self.lat.1 - lat) / (self.lat.1 - self.lat.0) * height)
            .floor()
            .clamp(0.0, height - 1.0) as usize;
        self.land[y * usize::from(self.grid.0) + x]
    }

    /// Whether (lon, lat) lies inside the bounds and in a land cell.
    fn land_at(&self, lon: f64, lat: f64) -> bool {
        (self.lon.0..=self.lon.1).contains(&lon)
            && (self.lat.0..=self.lat.1).contains(&lat)
            && self.land_at_clamped(lon, lat)
    }

    /// The (lon, lat) centre of every land cell.
    fn land_centres(&self) -> Vec<(f64, f64)> {
        let (width, height) = (usize::from(self.grid.0), usize::from(self.grid.1));
        let mut centres = Vec::new();
        for y in 0..height {
            for x in 0..width {
                if self.land[y * width + x] {
                    centres.push((
                        self.lon.0 + (x as f64 + 0.5) * (self.lon.1 - self.lon.0) / width as f64,
                        self.lat.1 - (y as f64 + 0.5) * (self.lat.1 - self.lat.0) / height as f64,
                    ));
                }
            }
        }
        centres
    }

    /// The centres of this map's land cells that are also land in `other`.
    fn centres_on_land_in(&self, other: &LandMap) -> Vec<(f64, f64)> {
        self.land_centres()
            .into_iter()
            .filter(|&(lon, lat)| other.land_at(lon, lat))
            .collect()
    }
}

/// Crimea and Sevastopol: no region may offer a playable cell there.
#[derive(serde::Deserialize)]
struct ExcludedExtent {
    polygons: Vec<Vec<(f64, f64)>>,
}

impl ExcludedExtent {
    fn contains(&self, lon: f64, lat: f64) -> bool {
        self.polygons
            .iter()
            .any(|ring| point_in_ring(lon, lat, ring))
    }
}

fn excluded_extent() -> ExcludedExtent {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/authoring/ukraine_ips_operating_extent.ron"
    );
    ron::de::from_str(&std::fs::read_to_string(path).expect("extent file exists"))
        .expect("extent file parses")
}

/// Even-odd ray casting: whether (lon, lat) lies inside the (lon, lat) ring.
fn point_in_ring(lon: f64, lat: f64, ring: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let mut previous = ring.len() - 1;
    for (i, &(xi, yi)) in ring.iter().enumerate() {
        let (xj, yj) = ring[previous];
        if (yi > lat) != (yj > lat) && lon < (xj - xi) * (lat - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        previous = i;
    }
    inside
}

/// Degrees to kilometres: the research note's spherical-Earth convention (R = 6,371 km).
const KM_PER_DEGREE: f64 = 111.195;
/// The longest cell edge over the shortest, at the region's middle latitude.
const MAX_CELL_ASPECT: f64 = 1.25;
const MAX_GRID_WIDTH: u16 = 256;
const MAX_GRID_HEIGHT: u16 = 128;

/// A cell's east-west and north-south lengths in km at the middle latitude of `lat`.
fn cell_size_km(lon: (f64, f64), lat: (f64, f64), grid: (u16, u16)) -> (f64, f64) {
    let middle = (lat.0 + lat.1) / 2.0;
    let w_km = (lon.1 - lon.0) / f64::from(grid.0) * KM_PER_DEGREE * middle.to_radians().cos();
    let h_km = (lat.1 - lat.0) / f64::from(grid.1) * KM_PER_DEGREE;
    (w_km, h_km)
}

/// The longer edge over the shorter: 1.0 for a square cell.
fn cell_aspect(w_km: f64, h_km: f64) -> f64 {
    w_km.max(h_km) / w_km.min(h_km)
}

fn grid_fits(width: u16, height: u16) -> bool {
    width <= MAX_GRID_WIDTH && height <= MAX_GRID_HEIGHT
}

/// The share of the Eastern Interconnection's load per GW that a region carries: 40,850 MW
/// at 980 GW. New regions stay within a quarter of it.
const LOAD_SCALE_BAND: std::ops::RangeInclusive<f64> = 0.75..=1.25;

fn load_scale(total_base_load_mw: f64, capacity_gw: f64) -> f64 {
    total_base_load_mw / (40_850.0 * capacity_gw / 980.0)
}

/// The longest build (in played months) each kind may take, for the tycoon pacing contract.
const PACING_CEILINGS: [(PlantKind, u32); 6] = [
    (PlantKind::Peaker, 3),
    (PlantKind::Solar, 3),
    (PlantKind::Ccgt, 6),
    (PlantKind::Coal, 12),
    (PlantKind::Hydro, 18),
    (PlantKind::Nuclear, 24),
];

/// Each kind whose `build_months` exceeds its ceiling, described under `label`.
fn pacing_overshoots(label: &str, build_months: impl Fn(PlantKind) -> u32) -> Vec<String> {
    PACING_CEILINGS
        .iter()
        .filter(|&&(kind, longest)| build_months(kind) > longest)
        .map(|&(kind, longest)| {
            format!(
                "{label}: {kind:?} takes {} played months, over {longest}",
                build_months(kind)
            )
        })
        .collect()
}

/// A provenance citation looks like `[P7-SC-007]`: two or three capitals, then three digits.
/// Returns the ID at the very start of `text`, if it has that shape.
fn leading_row_id(text: &str) -> Option<&str> {
    let body = text.strip_prefix("P7-")?;
    let letters = body.bytes().take_while(u8::is_ascii_uppercase).count();
    if !(2..=3).contains(&letters) {
        return None;
    }
    let digits = body[letters..].strip_prefix('-')?;
    if digits.bytes().take_while(u8::is_ascii_digit).count() != 3 {
        return None;
    }
    Some(&text[.."P7-".len() + letters + "-".len() + 3])
}

/// Every `[P7-XX-NNN]` citation in `text`, in order.
fn cited_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("[P7-") {
        rest = &rest[at + 1..];
        if let Some(id) = leading_row_id(rest)
            && rest[id.len()..].starts_with(']')
        {
            ids.push(id.to_owned());
        }
    }
    ids
}

/// The IDs that head a `| P7-XX-NNN |` row of the research note's tables.
fn research_row_ids(note: &str) -> std::collections::BTreeSet<String> {
    note.lines()
        .filter_map(|line| {
            let cell = line.trim_start().strip_prefix("| ")?;
            let id = leading_row_id(cell)?;
            cell[id.len()..]
                .starts_with([' ', '|'])
                .then(|| id.to_owned())
        })
        .collect()
}

/// Whether the entry at `lines[at]` has a comment block directly above it that cites a row.
/// An entry written over several lines starts at its opening `(` (and `id:` line).
fn has_citation_above(lines: &[&str], at: usize) -> bool {
    let mut top = at;
    while top > 0 && {
        let above = lines[top - 1].trim();
        above == "(" || above.starts_with("id:")
    } {
        top -= 1;
    }
    let mut i = top;
    while i > 0 && lines[i - 1].trim_start().starts_with("//") {
        i -= 1;
        if !cited_ids(lines[i]).is_empty() {
            return true;
        }
    }
    false
}

/// The entries a region file must source: every plant kind in the catalog, every economic zone,
/// the `lines` block, the starting tariff and the starting cash. Each is described by its line
/// number and text when no comment directly above it cites a research row.
fn uncited_entries(ron: &str) -> Vec<String> {
    let lines: Vec<&str> = ron.lines().collect();
    let mut catalog_indent = None;
    let mut found = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.starts_with("//") {
            continue;
        }
        match catalog_indent {
            Some(open) if indent <= open && trimmed.starts_with(']') => {
                catalog_indent = None;
                continue;
            }
            None if trimmed.starts_with("catalog: [") => {
                catalog_indent = Some(indent);
                continue;
            }
            _ => {}
        }
        let plant = catalog_indent.is_some()
            && (trimmed.starts_with("(kind:")
                || (trimmed.starts_with("kind:") && n > 0 && lines[n - 1].trim() == "("));
        let sourced = plant
            || trimmed.contains("name_key: \"economic-zone-")
            || trimmed.starts_with("lines:")
            || trimmed.starts_with("starting_tariff_per_mwh:")
            || trimmed.starts_with("cash:");
        if sourced && !has_citation_above(&lines, n) {
            found.push(format!("line {}: {trimmed}", n + 1));
        }
    }
    found
}

/// The cited IDs of `ron` that are not rows of the research note, sorted and without repeats.
fn unknown_citations(ron: &str, rows: &std::collections::BTreeSet<String>) -> Vec<String> {
    let cited: std::collections::BTreeSet<String> = cited_ids(ron).into_iter().collect();
    cited.into_iter().filter(|id| !rows.contains(id)).collect()
}

/// The highest demand multiplier of the year: every month's shape at every
/// hour boundary (interpolation between hours never exceeds the endpoints).
fn max_shape_factor(region: &RegionDefinition) -> f64 {
    (1..=12)
        .flat_map(|month| {
            let month = SimMonth::new(2027, month).expect("valid month");
            (0..24).map(move |hour| {
                region.load_factor(region.cities()[0].id, month, hour * TICKS_PER_HOUR)
            })
        })
        .fold(f64::MIN, f64::max)
}

/// Civil clocks must shift demand by the city's authored local hour, including
/// the Saskatchewan/Arizona summer exceptions. A wrong zone changes these loads.
#[test]
fn shipped_city_clocks_shift_local_demand_and_sun() {
    use domain::region::SolarModel;
    use domain::units::Currency;
    for (id, system_offset, groups) in [
        (
            EAST,
            -300,
            vec![
                (
                    -300,
                    true,
                    "new-york washington philadelphia toronto miami charlotte atlanta tampa boston detroit pittsburgh cincinnati columbus cleveland",
                ),
                (
                    -360,
                    true,
                    "chicago nashville new-orleans st-louis minneapolis kansas-city oklahoma-city winnipeg amarillo",
                ),
                (-360, false, "regina"),
                (-240, true, "halifax"),
            ],
        ),
        (
            WEST,
            -480,
            vec![
                (
                    -480,
                    true,
                    "los-angeles riverside san-diego san-francisco san-jose sacramento fresno las-vegas seattle portland spokane vancouver tijuana mexicali",
                ),
                (
                    -420,
                    true,
                    "salt-lake-city denver albuquerque el-paso boise calgary edmonton",
                ),
                (-420, false, "phoenix tucson"),
            ],
        ),
    ] {
        let r = region(id);
        assert!(matches!(r.solar().model, SolarModel::Astronomical), "{id}");
        assert_eq!(r.currency(), Currency::Usd);
        let legacy: infrastructure::region::RegionFile = ron::from_str(
            &std::fs::read_to_string(format!(
                "{}/tests/support/global-foundations/{id}-v3.ron",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap(),
        )
        .unwrap();
        let mut covered = 0;
        for (offset, dst, names) in groups {
            for name in names.split_whitespace() {
                let c = r
                    .cities()
                    .iter()
                    .find(|c| c.name_key == format!("city-{name}"))
                    .unwrap();
                covered += 1;
                for m in 1..=12 {
                    let month = SimMonth::new(2027, m).unwrap();
                    let summer_clock = (3..=10).contains(&m);
                    let system = system_offset + if summer_clock { 60 } else { 0 };
                    let local = offset + if dst && summer_clock { 60 } else { 0 };
                    assert_eq!(r.system_zone().offset_minutes(month), system);
                    assert_eq!(
                        r.city_zone(c.id).offset_minutes(month),
                        local,
                        "{id} {name} {m}"
                    );
                    assert_eq!(r.season(month), month.season());
                    let shape = match m {
                        12 | 1 | 2 => &legacy.demand.winter,
                        6..=8 => &legacy.demand.summer,
                        _ => &legacy.demand.shoulder,
                    };
                    for tick in [0, 59, 480, 960, 1439] {
                        let minute = (tick as i32 + local - system).rem_euclid(1440) as usize;
                        assert_eq!(r.local_minute(c.id, month, tick), minute as u32);
                        let hour = minute / 60;
                        let want = (shape[hour]
                            + (shape[(hour + 1) % 24] - shape[hour]) * (minute % 60) as f64 / 60.0)
                            * legacy.demand.monthly[usize::from(m - 1)];
                        assert!(
                            (r.load_factor(c.id, month, tick) - want).abs() < 1e-12,
                            "{id} {name} {m} {tick}"
                        );
                    }
                }
            }
        }
        assert_eq!(covered, r.cities().len());
        let home = r.city(r.start().home_city).unwrap();
        let june = SimMonth::new(2027, 6).unwrap();
        assert_eq!(r.sun(home.cell, june, 0), 0.0);
        assert!(r.sun(home.cell, june, 13 * 60) > 0.8);
        assert!(r.sun_at_home(june, 13 * 60).is_some());
    }
}

#[test]
fn shipped_first_expansion_acceptance_seeds_zero_through_seven() {
    for &id in SHIPPED {
        for seed in 0..8 {
            assert_first_expansion(id, seed);
        }
    }
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
fn shipped_regions_carry_the_frequency_protection_and_performance_defaults() {
    for &id in SHIPPED {
        let region = region(id);
        let (f, r) = (region.frequency(), region.regulator());
        assert_eq!(
            f.nominal,
            Hertz::new(table_value(NOMINAL_HZ, id)),
            "{id} nominal_hz"
        );
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
fn ships_exactly_the_listed_regions() {
    let listed = catalog().list().expect("shipped regions list");
    let ids: Vec<_> = listed.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(ids, SHIPPED);
    for listing in &listed {
        assert!(listing.entry.is_ok(), "{listing:?}");
    }
}

#[test]
fn shipped_home_cities_anchor_the_existing_city_nodes() {
    for &id in SHIPPED {
        let name_key = table_value(HOME_CITY, id);
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
    for &id in SHIPPED {
        let cities = region(id).cities().to_vec();
        assert!(
            (18..=40).contains(&cities.len()),
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
    for &id in SHIPPED {
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
    for &id in SHIPPED {
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
        loaded.definition = Arc::new(map_region_v4(file).expect("overridden region maps"));
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
/// without a forecast shortfall, on every shipped region at seeds 0-7.
#[test]
fn shipped_regions_start_without_a_shortfall() {
    for &id in SHIPPED {
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

/// One test per region and mode, so nextest runs the regions in parallel and names a failure.
/// Spec §14.5 beginner acceptance: with the hands-off plan (every dispatchable plant committed
/// all day under AGC, no other commands), each shipped region is still `Active` after twelve
/// months on seeds 0-7. Spec §16.5: the beginner acceptance check also passes with no commands
/// at all, on the previous operator's schedule. Each region task adds one line here.
macro_rules! year_one_cases {
    ($(($hands_off:ident, $no_commands:ident, $id:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $hands_off() {
                assert_survives_year_one(hands_off_game, $id);
            }

            #[test]
            fn $no_commands() {
                assert_survives_year_one(new_game, $id);
            }
        )+

        /// The regions the year-one cases cover.
        const YEAR_ONE_REGIONS: &[&str] = &[$($id),+];
    };
}

year_one_cases! {
    (
        year_one_hands_off_brazil_sin,
        year_one_no_commands_brazil_sin,
        BRAZIL
    ),
    (
        year_one_hands_off_eastern_interconnection,
        year_one_no_commands_eastern_interconnection,
        EAST
    ),
    (
        year_one_hands_off_india_nepal_bhutan,
        year_one_no_commands_india_nepal_bhutan,
        INDIA
    ),
    (
        year_one_hands_off_southern_china,
        year_one_no_commands_southern_china,
        SOUTHERN_CHINA
    ),
    (
        year_one_hands_off_western_interconnection,
        year_one_no_commands_western_interconnection,
        WEST
    ),
}

#[test]
fn year_one_cases_cover_every_shipped_region() {
    assert_eq!(YEAR_ONE_REGIONS, SHIPPED);
}

/// Plays twelve months on region `id` at seeds 0-7, each game started by `start` and given no
/// commands beyond the phase changes, and asserts the game is still `Active` at every close.
/// Before every month the forecast shows no shortfall, so a hands-off player never sees the
/// shortfall prompt (spec §16.4).
fn assert_survives_year_one(start: fn(&dyn RegionCatalog, &str, u64) -> Game, id: &str) {
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

/// Spec §15.7: the shipped start fleets cover their forecasts, so the
/// hands-off plan never orders a rotating outage. Over the hands-off year
/// on seeds 0-7, every month of every shipped region closes with no planned outage;
/// a failure lists each month that had one, with the hours it ran and its
/// size.
#[test]
fn shipped_regions_never_order_outages_hands_off() {
    let mut outages = Vec::new();
    for &id in SHIPPED {
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
/// Economic-zone overrides keep the same ceilings.
#[test]
fn shipped_projects_fit_tycoon_pacing() {
    for &id in SHIPPED {
        let region = region(id);
        let overshoots = pacing_overshoots(id, |kind| region.spec(kind).build_months);
        assert!(overshoots.is_empty(), "{overshoots:?}");
        // One cell per economic zone stands for it: the zone's overrides resolve the same
        // everywhere inside it.
        let mut zone_cells = std::collections::BTreeMap::new();
        for y in 0..region.terrain().height() {
            for x in 0..region.terrain().width() {
                let cell = domain::ids::CellCoord { x, y };
                let zone = region.economic_zone_at(cell).expect("cell is in the grid");
                zone_cells.entry(zone).or_insert(cell);
            }
        }
        for (zone, cell) in zone_cells {
            let overshoots = pacing_overshoots(&format!("{id} zone {}", zone.0), |kind| {
                region
                    .plant_spec_at(kind, cell)
                    .expect("cell is in the grid")
                    .build_months
            });
            assert!(overshoots.is_empty(), "{overshoots:?}");
        }
    }
}

#[test]
fn the_pacing_check_flags_exactly_the_kinds_over_their_ceiling() {
    let at_ceiling = |kind| {
        PACING_CEILINGS
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or(1, |(_, longest)| *longest)
    };
    assert!(pacing_overshoots("at the ceilings", at_ceiling).is_empty());
    for (kind, longest) in PACING_CEILINGS {
        let slow = pacing_overshoots("one slow kind", |k| at_ceiling(k) + u32::from(k == kind));
        assert_eq!(slow.len(), 1, "{kind:?}: {slow:?}");
        assert!(
            slow[0].contains(&format!("{kind:?} takes {}", longest + 1)),
            "{slow:?}"
        );
    }
    // Wind and Battery have no ceiling.
    assert!(
        pacing_overshoots("unbounded kinds", |k| match k {
            PlantKind::Wind | PlantKind::Battery => 99,
            other => at_ceiling(other),
        })
        .is_empty()
    );
}

fn assert_first_expansion(id: &str, seed: u64) {
    use domain::{grid::LineStatus, units::Money};
    use std::collections::BTreeSet;
    let mut game = new_game(&catalog(), id, seed);
    let region = Arc::clone(game.session().region());
    let starting_cash = game.session().company().cash();
    let city_key = table_value(EXPANSION_CITY, id);
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
        for &id in SHIPPED {
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
    use domain::company::{AssetProfile, AssetRef};
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

    fn assert_every_storage_asset_is_held(
        company: &FinancialCompany,
        batteries: &[PlantId],
        events: &[DomainEvent],
    ) {
        let rate_base = company.session().company().rate_base();
        for id in batteries {
            let plant = company.session().power().plant(*id).unwrap();
            let capital = rate_base.asset(AssetRef::Plant(*id)).unwrap();
            assert!(
                capital.held() > domain::units::Money::ZERO,
                "battery {id:?}"
            );
            assert!(events.contains(&DomainEvent::CapitalHeld {
                asset: capital.asset,
                amount: capital.held(),
            }));
            let fraction = capital.held_slice().unwrap().fraction();
            let lines: Vec<_> = rate_base
                .assets()
                .filter(|asset| {
                    !asset.legacy
                        && matches!(asset.profile,
                    AssetProfile::Line { from, to } if from == plant.node() || to == plant.node())
                })
                .collect();
            assert_eq!(lines.len(), 1, "one dedicated line for battery {id:?}");
            for line in lines {
                assert!(line.held() > domain::units::Money::ZERO);
                assert!(events.contains(&DomainEvent::CapitalHeld {
                    asset: line.asset,
                    amount: line.held(),
                }));
                let slice = line.held_slice().unwrap();
                assert_eq!(slice.follows(), Some(*id));
                assert_eq!(slice.fraction(), fraction);
            }
        }
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
            if events.iter().any(|e| {
                matches!(e,
                DomainEvent::PlantCommissioned { plant, .. } if batteries.contains(plant))
            }) {
                first_commission.get_or_insert(now);
            }
            if now.month() == 12 && first_commission.is_some_and(|first| first.year() == now.year())
            {
                assert_every_storage_asset_is_held(&gold, &batteries, &events);
                first_december_held = true;
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

/// Compare the new annual overlay with the frozen old irradiance field, across
/// every authored blob centre on land, starter solar plant, and named example.
#[test]
fn solar_migration_preserves_authored_site_rankings() {
    use infrastructure::region::{RegionFile, map_region, to_cell};
    for (id, names) in [
        (EAST, "miami amarillo regina toronto"),
        (WEST, "phoenix tucson mexicali el-paso seattle vancouver"),
    ] {
        let old: RegionFile = ron::from_str(
            &std::fs::read_to_string(format!(
                "{}/tests/support/global-foundations/{id}-v3.ron",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap(),
        )
        .unwrap();
        let before = map_region(old.clone()).unwrap();
        let after = region(id);
        let mut sites = std::collections::BTreeMap::new();
        for b in &old.irradiance.blobs {
            let cell = to_cell(&old, b.lon, b.lat);
            if before.terrain().get(cell).unwrap().land {
                sites.insert(cell, format!("blob({},{})", b.lon, b.lat));
            } else {
                eprintln!("{id}: excluded water blob({},{})", b.lon, b.lat);
            }
        }
        for plant in &before.start().fleet {
            if plant.kind == PlantKind::Solar {
                sites.insert(plant.cell, "starter solar".into());
            }
        }
        for name in names.split_whitespace() {
            let c = before
                .cities()
                .iter()
                .find(|c| c.name_key == format!("city-{name}"))
                .unwrap();
            sites.entry(c.cell).or_insert(c.name_key.clone());
        }
        let values: Vec<_> = sites
            .iter()
            .map(|(cell, name)| {
                let old = before.solar_potential(*cell).value();
                let new = after.solar_potential(*cell).value();
                eprintln!("{id} {name}: {old:.6} -> {new:.6}");
                (name, old, new)
            })
            .collect();
        let mut inversions = Vec::new();
        for (i, (a, oa, na)) in values.iter().enumerate() {
            for (b, ob, nb) in &values[i + 1..] {
                if oa != ob && oa.total_cmp(ob) != na.total_cmp(nb) {
                    inversions.push(format!("{a} vs {b}"));
                }
            }
        }
        assert!(
            inversions.is_empty(),
            "{id} authored-site inversions: {inversions:?}"
        );
    }
}

/// Neighboring old-value groups suffice to prove every strict pair's order:
/// the largest new value in one group must precede the smallest in the next.
#[test]
fn solar_migration_preserves_every_playable_strict_site_ranking() {
    use infrastructure::region::{RegionFile, map_region};
    for id in [EAST, WEST] {
        let source = std::fs::read_to_string(format!(
            "{}/tests/support/global-foundations/{id}-v3.ron",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let before = map_region(ron::from_str::<RegionFile>(&source).unwrap()).unwrap();
        let after = region(id);
        let mut cells: Vec<_> = (0..before.terrain().height())
            .flat_map(|y| {
                (0..before.terrain().width()).map(move |x| domain::ids::CellCoord { x, y })
            })
            .filter(|c| before.terrain().get(*c).unwrap().land)
            .map(|c| {
                (
                    before.solar_potential(c).value(),
                    after.solar_potential(c).value(),
                    c,
                )
            })
            .collect();
        cells.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.cmp(&b.2)));
        let groups: Vec<_> = cells.chunk_by(|a, b| a.0 == b.0).collect();
        for pair in groups.windows(2) {
            let lower = pair[0].iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
            let higher = pair[1].iter().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
            assert!(
                lower.1 < higher.1,
                "{id}: strict old order collapsed or reversed: {lower:?} vs {higher:?}"
            );
        }
        eprintln!(
            "{id}: all strict pairs preserved over {} playable cells / {} distinct old-value groups",
            cells.len(),
            groups.len()
        );
    }
}

// ---- Shared acceptance for every shipped region ----------------------------------------------
//
// Plan 7 adds regions one file at a time. Each region task inserts its ID into the tables at the
// top of this file, and every test below that iterates them then covers the new region.

#[test]
fn shipped_region_tables_cover_every_shipped_region() {
    assert!(SHIPPED.windows(2).all(|pair| pair[0] < pair[1]), "sorted");
    for id in [EAST, WEST] {
        assert!(SHIPPED.contains(&id), "{id} still ships");
    }
    for id in NEW_REGIONS {
        assert!(SHIPPED.contains(id), "new region {id} is not in SHIPPED");
        assert!(
            ![EAST, WEST].contains(id),
            "{id} is North American, not new"
        );
    }
    for (table, keys) in [
        ("HOME_CITY", table_keys(HOME_CITY)),
        ("EXPANSION_CITY", table_keys(EXPANSION_CITY)),
        ("NOMINAL_HZ", table_keys(NOMINAL_HZ)),
        ("RESEARCH_CAPACITY_GW", table_keys(RESEARCH_CAPACITY_GW)),
    ] {
        assert_eq!(
            keys, SHIPPED,
            "{table} must hold one row per shipped region"
        );
    }
}

#[test]
fn new_region_cells_are_nearly_square() {
    for &id in NEW_REGIONS {
        let file = region_file(id);
        let (w_km, h_km) = cell_size_km(
            (file.bounds.west, file.bounds.east),
            (file.bounds.south, file.bounds.north),
            (file.grid.width, file.grid.height),
        );
        let aspect = cell_aspect(w_km, h_km);
        assert!(
            aspect <= MAX_CELL_ASPECT,
            "{id}: cells are {w_km:.1} x {h_km:.1} km (aspect {aspect:.3})"
        );
        assert!(
            grid_fits(file.grid.width, file.grid.height),
            "{id}: grid {} x {} exceeds {MAX_GRID_WIDTH} x {MAX_GRID_HEIGHT}",
            file.grid.width,
            file.grid.height
        );
    }
}

#[test]
fn cell_geometry_helpers_follow_the_research_convention() {
    // 10 degrees square, 100 x 100 cells, centred on the equator: w = h = 11.1195 km.
    let (w, h) = cell_size_km((0.0, 10.0), (-5.0, 5.0), (100, 100));
    assert!(
        (w - 11.1195).abs() < 1e-3 && (h - 11.1195).abs() < 1e-3,
        "{w} {h}"
    );
    assert!((cell_aspect(w, h) - 1.0).abs() < 1e-9);
    // At 60 degrees north an east-west degree is half as long, so the same grid is 2:1.
    let (w, h) = cell_size_km((0.0, 10.0), (55.0, 65.0), (100, 100));
    assert!((cell_aspect(w, h) - 1.0 / 60.0_f64.to_radians().cos()).abs() < 1e-6);
    assert!(cell_aspect(w, h) > MAX_CELL_ASPECT, "{w} x {h}");
    // Doubling the width halves the east-west length: the aspect flips side but keeps its size.
    let (w2, h2) = cell_size_km((0.0, 10.0), (-5.0, 5.0), (200, 100));
    assert!((cell_aspect(w2, h2) - 2.0).abs() < 1e-9);
    // The limit is inclusive on both sides.
    assert!(grid_fits(256, 128));
    assert!(!grid_fits(257, 128));
    assert!(!grid_fits(256, 129));
}

#[test]
fn new_region_load_scales_with_capacity() {
    for &id in NEW_REGIONS {
        let capacity_gw = table_value(RESEARCH_CAPACITY_GW, id);
        let total = total_base_load(&region(id));
        let scale = load_scale(total, capacity_gw);
        assert!(
            LOAD_SCALE_BAND.contains(&scale),
            "{id}: {total:.0} MW against {capacity_gw} GW is {scale:.3} of the Eastern ratio"
        );
    }
}

#[test]
fn load_scale_is_relative_to_the_eastern_interconnection() {
    // The Eastern Interconnection (980 GW) carries 40,850 MW, the anchor of the ratio.
    assert!((load_scale(40_850.0, 980.0) - 1.0).abs() < 1e-12);
    assert!((load_scale(40_850.0 * 1350.0 / 980.0, 1350.0) - 1.0).abs() < 1e-12);
    assert!((load_scale(81_700.0, 980.0) - 2.0).abs() < 1e-12);
    assert!(LOAD_SCALE_BAND.contains(&0.75) && LOAD_SCALE_BAND.contains(&1.25));
    assert!(!LOAD_SCALE_BAND.contains(&0.7499) && !LOAD_SCALE_BAND.contains(&1.2501));
}

const CITED: &str = "// Pumped hydro, [P7-SC-007] and [P7-SC-009].";

#[test]
fn new_region_entries_cite_research_rows() {
    let note = std::fs::read_to_string(RESEARCH_NOTE).expect("research note exists");
    let rows = research_row_ids(&note);
    assert!(
        rows.len() > 100,
        "the research note's row table parsed: {}",
        rows.len()
    );
    for &id in NEW_REGIONS {
        let text = std::fs::read_to_string(format!("{REGIONS_DIR}/{id}.ron")).expect("file exists");
        let uncited = uncited_entries(&text);
        assert!(
            uncited.is_empty(),
            "{id}: entries without a citation: {uncited:#?}"
        );
        let unknown = unknown_citations(&text, &rows);
        assert!(
            unknown.is_empty(),
            "{id}: citations the research note lacks: {unknown:?}"
        );
    }
}

#[test]
fn the_research_row_parser_reads_row_tables_only() {
    let note = "\
| ID | Area |
|---|---|
| P7-US-001 | Pacing reference | US | Wind |
| P7-IPS-011 | IPS/UPS | Russia |
Prose citing P7-SC-001 and a bracketed [P7-BR-002] is not a row.
    | P7-DE-005 | indented rows still count |
| p7-xx-001 | lower case is not an ID |
| P7-X-001 | one letter is not an area |
| P7-ABCD-001 | four letters are not an area |
| P7-US-01 | two digits are not a row |
";
    let rows: Vec<_> = research_row_ids(note).into_iter().collect();
    assert_eq!(rows, ["P7-DE-005", "P7-IPS-011", "P7-US-001"]);
}

#[test]
fn the_citation_finder_reads_bracketed_ids_only() {
    assert_eq!(cited_ids(CITED), ["P7-SC-007", "P7-SC-009"]);
    assert_eq!(cited_ids("// see [P7-IPS-011]"), ["P7-IPS-011"]);
    for not_a_citation in [
        "// P7-SC-007 without brackets",
        "// [P7-S-007] one letter",
        "// [P7-ABCD-007] four letters",
        "// [P7-SC-07] two digits",
        "// [P7-SC-0071] four digits",
        "// [p7-sc-007] lower case",
        "// [P7-SC-007",
    ] {
        assert!(cited_ids(not_a_citation).is_empty(), "{not_a_citation}");
    }
}

/// A fixture region file with every kind of entry the provenance rule covers.
const CITED_REGION: &str = "\
(
    // Pacing: [P7-US-001].
    lines: (cost_per_cell: 1, cells_per_month: 4, book_life_months: 600),
    catalog: [
        // Wind: [P7-SC-001].
        (kind: Wind, capacity_mw: 300.0),
        // Peaker: [P7-SC-002]
        // arithmetic: 2 * 3.
        (
            kind: Peaker,
            capacity_mw: 100.0,
        ),
    ],
    economic_zones: (
        zones: [
            // North: [P7-SC-003].
            (id: 1, name_key: \"economic-zone-north\", plants: []),
            // South: [P7-SC-004].
            (
                id: 2,
                name_key: \"economic-zone-south\",
            ),
        ],
    ),
    regulator: (
        // Tariff: [P7-SC-005].
        starting_tariff_per_mwh: 95.0,
    ),
    start: (
        // Cash: [P7-SC-006].
        cash: 5000000000,
    ),
)
";

#[test]
fn an_entry_with_a_citation_directly_above_it_passes() {
    assert_eq!(uncited_entries(CITED_REGION), Vec::<String>::new());
    let rows: std::collections::BTreeSet<String> = (1..=6)
        .map(|n| format!("P7-SC-{n:03}"))
        .chain(["P7-US-001".to_owned()])
        .collect();
    assert_eq!(unknown_citations(CITED_REGION, &rows), Vec::<String>::new());
}

#[test]
fn an_entry_without_a_citation_directly_above_it_fails() {
    for (name, text) in [
        (
            "a one-line plant",
            "(\n    catalog: [\n        (kind: Wind, capacity_mw: 300.0),\n    ],\n)",
        ),
        (
            "a plant in the multi-line form",
            "(\n    catalog: [\n        (\n            kind: Peaker,\n        ),\n    ],\n)",
        ),
        (
            "a one-line economic zone",
            "(\n    zones: [\n        (id: 1, name_key: \"economic-zone-north\"),\n    ],\n)",
        ),
        (
            "a multi-line economic zone",
            "(\n    zones: [\n        (\n            id: 2,\n            name_key: \"economic-zone-south\",\n        ),\n    ],\n)",
        ),
        ("the lines block", "(\n    lines: (cost_per_cell: 1),\n)"),
        (
            "the starting tariff",
            "(\n    starting_tariff_per_mwh: 95.0,\n)",
        ),
        ("the starting cash", "(\n    cash: 5000000000,\n)"),
        (
            "a comment two lines above",
            "(\n    // Cash: [P7-SC-006].\n\n    cash: 5000000000,\n)",
        ),
        (
            "a comment without an ID",
            "(\n    // Cash, no source.\n    cash: 5000000000,\n)",
        ),
        (
            "a comment that only cites a malformed ID",
            "(\n    // Cash: [P7-S-006].\n    cash: 5000000000,\n)",
        ),
    ] {
        assert_eq!(uncited_entries(text).len(), 1, "{name}: {text}");
    }
    // The rule covers a catalog's kinds, the zone names and the named fields, nothing else.
    let unrelated = "(\n    atlas: (kind: Wind),\n    name_key: \"region-x\",\n    start_line_names: [],\n    cash_note: 1,\n    catalog: [\n    ],\n)";
    assert!(uncited_entries(unrelated).is_empty());
    // A `kind:` after the catalog closes is not a catalog entry.
    let after = "(\n    catalog: [\n        // Wind: [P7-SC-001].\n        (kind: Wind),\n    ],\n    other: (kind: Solar),\n)";
    assert!(uncited_entries(after).is_empty());
}

#[test]
fn a_cited_id_missing_from_the_research_table_fails() {
    let rows: std::collections::BTreeSet<String> = ["P7-US-001".to_owned()].into();
    let unknown = unknown_citations(CITED_REGION, &rows);
    assert_eq!(unknown.len(), 6, "{unknown:?}");
    assert!(
        unknown.iter().all(|id| id.starts_with("P7-SC-")),
        "{unknown:?}"
    );
    assert!(unknown_citations("// [P7-US-001]", &rows).is_empty());
}

/// Pairs `(a, b, n)` whose grids already share `n` cells along their real border: `n` land
/// cells of `a` have their centre in a land cell of `b`. The Eastern and Western grids (0.51 and
/// 0.39 degrees wide) don't line up along the seam through the Oklahoma panhandle and New
/// Mexico, so three cells each way straddle it. The count is exact: a new overlap fails, and so
/// does a fix that leaves this entry behind. No other pair may share a cell.
const KNOWN_SEAM_OVERLAPS: &[(&str, &str, usize)] = &[(EAST, WEST, 3), (WEST, EAST, 3)];

#[test]
fn shipped_regions_do_not_overlap() {
    let maps: Vec<(&str, LandMap)> = SHIPPED.iter().map(|&id| (id, LandMap::load(id))).collect();
    for (a_id, a) in &maps {
        for (b_id, b) in &maps {
            if a_id == b_id {
                continue;
            }
            let allowed = KNOWN_SEAM_OVERLAPS
                .iter()
                .find(|(x, y, _)| x == a_id && y == b_id)
                .map_or(0, |&(_, _, cells)| cells);
            let shared = a.centres_on_land_in(b);
            assert_eq!(
                shared.len(),
                allowed,
                "{a_id} has {} land cells whose centre is playable in {b_id} (allowed {allowed}), \
                 first {:?}",
                shared.len(),
                shared.first()
            );
        }
    }
}

#[test]
fn the_overlap_check_sees_shared_land_and_nothing_else() {
    //  2 x 1 cells over lon 0..2: land on both.
    let both = LandMap::from_cells((0.0, 2.0), (0.0, 1.0), (2, 1), vec![true, true]);
    // The same square sharing only the western cell's centre (0.5, 0.5) with `both`.
    let west_cell = LandMap::from_cells((0.0, 1.0), (0.0, 1.0), (1, 1), vec![true]);
    // East of `both`: no shared coordinates at all.
    let apart = LandMap::from_cells((5.0, 6.0), (0.0, 1.0), (1, 1), vec![true]);
    // Water where `both` has land.
    let water = LandMap::from_cells((0.0, 2.0), (0.0, 1.0), (2, 1), vec![false, false]);
    assert_eq!(both.centres_on_land_in(&west_cell), vec![(0.5, 0.5)]);
    assert_eq!(both.centres_on_land_in(&apart), Vec::<(f64, f64)>::new());
    assert_eq!(both.centres_on_land_in(&water), Vec::<(f64, f64)>::new());
    assert_eq!(water.centres_on_land_in(&both), Vec::<(f64, f64)>::new());
    // Strictly outside the other map's bounds is not land, even when its edge cell is.
    assert!(!west_cell.land_at(1.5, 0.5));
    assert!(west_cell.land_at(0.5, 0.5));
    // A land cell centre must be land in itself.
    assert_eq!(both.centres_on_land_in(&both).len(), 2);
}

#[test]
fn excluded_extent_is_unplayable_everywhere() {
    let extent = excluded_extent();
    for &id in SHIPPED {
        let map = LandMap::load(id);
        let inside: Vec<_> = map
            .land_centres()
            .into_iter()
            .filter(|&(lon, lat)| extent.contains(lon, lat))
            .collect();
        assert!(
            inside.is_empty(),
            "{id} has {} playable cells inside the excluded extent, first {:?}",
            inside.len(),
            inside.first()
        );
    }
}

#[test]
fn the_excluded_extent_covers_crimea_and_sevastopol_only() {
    let extent = excluded_extent();
    assert!(!extent.polygons.is_empty());
    for (name, lon, lat) in [
        ("Simferopol", 34.10, 44.95),
        ("Sevastopol (Balaklava side)", 33.55, 44.52),
        ("Kerch", 36.47, 45.36),
        ("Yalta", 34.17, 44.50),
    ] {
        assert!(extent.contains(lon, lat), "{name} is in the extent");
    }
    for (name, lon, lat) in [
        ("Kherson", 32.62, 46.64),
        ("Melitopol", 35.37, 46.85),
        ("Kyiv", 30.52, 50.45),
        ("Rostov-on-Don", 39.72, 47.24),
        ("Bucharest", 26.10, 44.43),
    ] {
        assert!(!extent.contains(lon, lat), "{name} is outside the extent");
    }
}

#[test]
fn point_in_ring_handles_concavity_and_edges() {
    // A U shape: the notch between the arms is outside.
    let u = [
        (0.0, 0.0),
        (3.0, 0.0),
        (3.0, 3.0),
        (2.0, 3.0),
        (2.0, 1.0),
        (1.0, 1.0),
        (1.0, 3.0),
        (0.0, 3.0),
    ];
    assert!(point_in_ring(0.5, 2.0, &u), "left arm");
    assert!(point_in_ring(2.5, 2.0, &u), "right arm");
    assert!(point_in_ring(1.5, 0.5, &u), "base");
    assert!(!point_in_ring(1.5, 2.0, &u), "notch");
    assert!(!point_in_ring(-1.0, 1.0, &u), "left of the shape");
    assert!(!point_in_ring(5.0, 1.0, &u), "right of the shape");
    assert!(!point_in_ring(1.5, 4.0, &u), "above");
}

#[test]
fn home_lighting_is_finite_for_every_shipped_region() {
    for &id in SHIPPED {
        let region = region(id);
        for month in 1..=12 {
            let month = SimMonth::new(2027, month).expect("valid month");
            for tick in 0..TICKS_PER_DAY {
                if let Some(sun) = region.sun_at_home(month, tick) {
                    assert!(
                        sun.elevation_deg.is_finite() && sun.azimuth_deg.is_finite(),
                        "{id} {month:?} tick {tick}: {sun:?}"
                    );
                }
            }
        }
    }
}

// ---- Region checks: Southern China -----------------------------------------------------------

mod southern_china {
    use super::*;

    const ID: &str = SOUTHERN_CHINA;

    /// The footprint holds Guangdong, Guangxi, Guizhou, Hainan, Hong Kong and Macau, and leaves
    /// out Yunnan, Vietnam and Hunan (spec §28.3).
    #[test]
    fn southern_china_landmarks() {
        let map = LandMap::load(ID);
        for (name, lon, lat) in [
            ("Guangzhou", 113.26, 23.13),
            ("Hong Kong", 114.17, 22.32),
            ("Macao", 113.54, 22.20),
            ("Guiyang", 106.63, 26.65),
            ("Nanning", 108.37, 22.82),
            ("Haikou", 110.35, 20.02),
        ] {
            assert!(map.land_at(lon, lat), "{name} is playable land");
        }
        for (name, lon, lat) in [
            ("Kunming", 102.83, 24.88),
            ("Hanoi", 105.85, 21.03),
            ("Changsha", 112.94, 28.23),
        ] {
            assert!(!map.land_at(lon, lat), "{name} is outside the footprint");
        }
    }

    /// Spec §28.8: a player-ordered line may cross the Qiongzhou Strait to Hainan, and its
    /// straight path crosses water, so it is drawn as a submarine cable.
    #[test]
    fn hainan_line_crosses_water_as_a_submarine_cable() {
        let game = new_game(&catalog(), ID, 0);
        let session = game.session();
        let city = |key: &str| {
            session
                .city_views()
                .into_iter()
                .find(|city| city.name_key == key)
                .unwrap_or_else(|| panic!("{key} is a city"))
        };
        let (zhanjiang, haikou) = (city("city-zhanjiang"), city("city-haikou"));
        let quote = session
            .line_quote(zhanjiang.node, haikou.node)
            .expect("the strait crossing is quoted");
        assert!(quote.cells > 0, "{quote:?}");
        let water = session
            .region()
            .terrain()
            .water_cells_on_segment(zhanjiang.cell, haikou.cell);
        assert!(!water.is_empty(), "the straight path crosses the strait");
    }
}

// ---- Region checks: Brazil -------------------------------------------------------------------

mod brazil_sin {
    use super::*;
    use domain::time::Season;

    const ID: &str = BRAZIL;

    fn month(m: u8) -> SimMonth {
        SimMonth::new(2027, m).expect("valid month")
    }

    /// The footprint is mainland Brazil, Roraima included, and leaves out Paraguay, Uruguay and
    /// Venezuela (spec section 28.3).
    #[test]
    fn brazil_landmarks() {
        let map = LandMap::load(ID);
        for (name, lon, lat) in [
            ("Sao Paulo", -46.63, -23.55),
            ("Boa Vista", -60.67, 2.82),
            ("Manaus", -60.02, -3.12),
            ("Rio Branco", -67.81, -9.97),
            ("Fortaleza", -38.54, -3.72),
        ] {
            assert!(map.land_at(lon, lat), "{name} is playable land");
        }
        for (name, lon, lat) in [
            ("Asuncion", -57.58, -25.26),
            ("Montevideo", -56.19, -34.90),
            ("Caracas", -66.90, 10.49),
        ] {
            assert!(!map.land_at(lon, lat), "{name} is outside the footprint");
        }
    }

    /// Spec section 28.6: Brazil's calendar is southern, so January is summer, Sao Paulo's
    /// January demand follows its summer shape, and December days are longer than June's.
    #[test]
    fn brazil_january_is_summer() {
        let region = region(ID);
        assert_eq!(region.season(month(1)), Season::Summer);
        let file = region_file(ID);
        let home = file
            .cities
            .iter()
            .find(|city| city.name_key == "city-sao-paulo")
            .expect("Sao Paulo is a city");
        let climate = file
            .climates
            .iter()
            .find(|climate| climate.id == home.climate)
            .expect("Sao Paulo's climate");
        assert_eq!(climate.month_shapes[0], "summer");
        let summer = &climate
            .shapes
            .iter()
            .find(|(name, _)| name == "summer")
            .expect("a summer shape")
            .1;
        let city = region
            .cities()
            .iter()
            .find(|city| city.name_key == "city-sao-paulo")
            .unwrap();
        for hour in 0..24 {
            let tick = hour * TICKS_PER_HOUR;
            let local = region.local_minute(city.id, month(1), tick) / TICKS_PER_HOUR;
            let want = summer[local as usize] * climate.monthly[0];
            assert!(
                (region.load_factor(city.id, month(1), tick) - want).abs() < 1e-12,
                "hour {hour}"
            );
        }
        let daylight = |m| {
            (0..TICKS_PER_DAY)
                .filter(|&tick| region.sun(city.cell, month(m), tick) > 0.0)
                .count()
        };
        assert!(
            daylight(12) > daylight(6),
            "December {} against June {} minutes",
            daylight(12),
            daylight(6)
        );
    }

    /// Spec section 28.6: the wet season runs from December to April, and the northeastern
    /// wind peaks from August to October.
    #[test]
    fn brazil_wet_season_and_northeast_wind() {
        let region = region(ID);
        let flow = region.hydro().monthly_flow;
        let mean = |months: &[usize]| {
            months.iter().map(|&m| flow[m - 1]).sum::<f64>() / months.len() as f64
        };
        let (wet, dry) = (mean(&[12, 1, 2, 3, 4]), mean(&[6, 7, 8, 9, 10]));
        assert!(wet > dry, "wet {wet} against dry {dry}: {flow:?}");
        let wind = region.wind().monthly;
        let windiest = (0..12)
            .max_by(|&a, &b| wind[a].total_cmp(&wind[b]))
            .unwrap()
            + 1;
        assert!(
            (8..=10).contains(&windiest),
            "the windiest month is {windiest}: {wind:?}"
        );
    }

    /// Roraima lies north of the equator, so the picker card reads both hemispheres.
    #[test]
    fn brazil_card_reads_both_hemispheres() {
        let id = RegionId::new(ID);
        let loaded = catalog().load(&id).expect("Brazil loads");
        let outline =
            infrastructure::region::region_outline(&loaded.source_bytes, &id).expect("outline");
        assert_eq!(outline.hemisphere, infrastructure::region::Hemisphere::Both);
    }

    /// Spec section 28.5: in the southern summer the noon sun over Sao Paulo stands north.
    #[test]
    fn brazil_home_lighting_points_north_at_noon() {
        let region = region(ID);
        let noon = (0..TICKS_PER_DAY)
            .filter_map(|tick| region.sun_at_home(month(1), tick))
            .max_by(|a, b| a.elevation_deg.total_cmp(&b.elevation_deg))
            .expect("the home sun is computed");
        let from_north = noon.azimuth_deg.min(360.0 - noon.azimuth_deg);
        assert!(from_north <= 20.0, "solar noon in January: {noon:?}");
    }
}

// ---- Region checks: India–Nepal–Bhutan -------------------------------------------------------

mod india_nepal_bhutan {
    use super::*;
    use domain::time::Season;

    const ID: &str = INDIA;

    fn month(m: u8) -> SimMonth {
        SimMonth::new(2027, m).expect("valid month")
    }

    /// The footprint is mainland India, the Indian-operated north included, with Nepal and
    /// Bhutan; Bangladesh, Sri Lanka and the Andaman, Nicobar and Lakshadweep islands are out
    /// (spec section 28.3).
    #[test]
    fn india_landmarks() {
        let map = LandMap::load(ID);
        for (name, lon, lat) in [
            ("Delhi", 77.21, 28.61),
            ("Leh", 77.58, 34.15),
            ("Kathmandu", 85.32, 27.72),
            ("Thimphu", 89.64, 27.47),
            ("Chennai", 80.27, 13.08),
        ] {
            assert!(map.land_at(lon, lat), "{name} is playable land");
        }
        for (name, lon, lat) in [
            ("Dhaka", 90.41, 23.81),
            ("Colombo", 79.86, 6.93),
            ("Port Blair", 92.73, 11.62),
            ("Kavaratti", 72.64, 10.57),
        ] {
            assert!(!map.land_at(lon, lat), "{name} is outside the footprint");
        }
    }

    /// Spec section 28.6: the HUD season label comes from the region calendar, which follows
    /// the IMD scheme, so July reads Monsoon and the winter run is two months long.
    #[test]
    fn india_labels_follow_the_imd_calendar() {
        let region = region(ID);
        assert_eq!(region.season(month(7)), Season::Monsoon);
        assert_eq!(region.calendar().run_length(month(1)), 2);
        use Season::*;
        assert_eq!(
            region.calendar().seasons,
            [
                Winter,
                Winter,
                Summer,
                Summer,
                Summer,
                Monsoon,
                Monsoon,
                Monsoon,
                Monsoon,
                PostMonsoon,
                PostMonsoon,
                PostMonsoon
            ]
        );
    }

    /// Spec section 28.6: during the monsoon (June to September) wind and river flow rise above
    /// their year means while clearness falls below its own.
    #[test]
    fn india_monsoon_resources() {
        let region = region(ID);
        let monsoon_against_year = |values: [f64; 12]| {
            let monsoon = values[5..9].iter().sum::<f64>() / 4.0;
            let year = values.iter().sum::<f64>() / 12.0;
            (monsoon, year)
        };
        let (monsoon, year) = monsoon_against_year(region.wind().monthly);
        assert!(
            monsoon > year,
            "wind: monsoon {monsoon} against year {year}"
        );
        let (monsoon, year) = monsoon_against_year(region.hydro().monthly_flow);
        assert!(
            monsoon > year,
            "flow: monsoon {monsoon} against year {year}"
        );
        let (monsoon, year) = monsoon_against_year(region.solar().clearness_mean);
        assert!(
            monsoon < year,
            "clearness: monsoon {monsoon} against year {year}"
        );
    }

    /// Spec section 28.4: Nepal (+5:45) and Bhutan (+6) put their local-hour bends at :15 and
    /// :00 of the IST hour, so planning samples minutes 30 and 45 as well as the endpoints.
    #[test]
    fn nepal_and_bhutan_add_quarter_hour_samples() {
        let region = region(ID);
        let city = |key: &str| {
            region
                .cities()
                .iter()
                .find(|city| city.name_key == key)
                .unwrap_or_else(|| panic!("{key} is a city"))
                .id
        };
        let (delhi, kathmandu, thimphu) = (
            city("city-delhi"),
            city("city-kathmandu"),
            city("city-thimphu"),
        );
        assert_eq!(
            region.hour_sample_minutes([delhi, kathmandu, thimphu], month(1)),
            std::collections::BTreeSet::from([0, 30, 45, 59])
        );
        assert_eq!(
            region.hour_sample_minutes([delhi], month(1)),
            std::collections::BTreeSet::from([0, 59])
        );
    }
}
