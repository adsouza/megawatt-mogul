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
use infrastructure::region::{
    ParsedRegionFile, RegionFileV4, RonRegionCatalog, map_region_v4, parse_region_bundle,
};

const REGIONS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/regions");
const EAST: &str = "eastern_interconnection";
const WEST: &str = "western_interconnection";
const SOUTHERN_CHINA: &str = "southern_china";
const BRAZIL: &str = "brazil_sin";
const EUROPE_MAGHREB: &str = "continental_europe_maghreb";
const INDIA: &str = "india_nepal_bhutan";
const IPS: &str = "ips_ups";
const SGCC: &str = "sgcc";

/// The research note whose row tables the region files cite (`[P7-XX-NNN]`).
const RESEARCH_NOTE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/research/2026-10-25-plan-7-regional-economics.md"
);

/// Every shipped region ID, sorted like `RonRegionCatalog::list`. Each Plan 7 region task
/// inserts its ID here and in the tables below; every region-agnostic test iterates them.
const SHIPPED: &[&str] = &[
    BRAZIL,
    EUROPE_MAGHREB,
    EAST,
    INDIA,
    IPS,
    SGCC,
    SOUTHERN_CHINA,
    WEST,
];

/// The IDs Plan 7 adds, a subset of `SHIPPED`. The provenance, scale and squareness tests
/// iterate it: the Eastern and Western files predate those rules.
const NEW_REGIONS: &[&str] = &[BRAZIL, EUROPE_MAGHREB, INDIA, IPS, SOUTHERN_CHINA];

/// Each region's real system capacity in GW, from the research note: the scale reference for
/// `new_region_load_scales_with_capacity`.
const RESEARCH_CAPACITY_GW: &[(&str, f64)] = &[
    (BRAZIL, 250.0),
    (EUROPE_MAGHREB, 1350.0),
    (EAST, 980.0),
    (INDIA, 520.0),
    (IPS, 345.0),
    (SGCC, 3170.0),
    (SOUTHERN_CHINA, 500.0),
    (WEST, 340.0),
];

/// Each region's start city (`start.home_city`), by name key.
const HOME_CITY: &[(&str, &str)] = &[
    (BRAZIL, "city-sao-paulo"),
    (EUROPE_MAGHREB, "city-seville"),
    (EAST, "city-columbus"),
    (INDIA, "city-delhi"),
    (IPS, "city-moscow"),
    (SGCC, "city-beijing"),
    (SOUTHERN_CHINA, "city-guangzhou"),
    (WEST, "city-seattle"),
];

/// The city each region's first-expansion acceptance test (spec §18.3) connects, by name key.
const EXPANSION_CITY: &[(&str, &str)] = &[
    (BRAZIL, "city-sorocaba"),
    (EUROPE_MAGHREB, "city-cordoba"),
    (EAST, "city-cleveland"),
    (INDIA, "city-faridabad"),
    (IPS, "city-kaluga"),
    (SGCC, "city-baoding"),
    (SOUTHERN_CHINA, "city-shenzhen"),
    (WEST, "city-vancouver"),
];

/// Each region's nominal grid frequency in hertz.
const NOMINAL_HZ: &[(&str, f64)] = &[
    (BRAZIL, 60.0),
    (EUROPE_MAGHREB, 50.0),
    (EAST, 60.0),
    (INDIA, 50.0),
    (IPS, 50.0),
    (SGCC, 50.0),
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

/// The file with its sidecars inlined, for mapping (lon, lat) to cells exactly as loading does.
fn region_file(id: &str) -> RegionFileV4 {
    let id = RegionId::new(id);
    let bundle = catalog()
        .load(&id)
        .expect("shipped region loads")
        .source_bytes;
    match parse_region_bundle(&bundle, &id).expect("bundle parses") {
        ParsedRegionFile::V4(file) => *file,
        ParsedRegionFile::Legacy(_) => panic!("{} is schema 4", id.as_str()),
    }
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
    exclude: Vec<Vec<(f64, f64)>>,
}

impl ExcludedExtent {
    fn contains(&self, lon: f64, lat: f64) -> bool {
        self.exclude
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

/// Exact annual peak of the requested cities, respecting each city's climate and clock.
fn peak_demand(region: &RegionDefinition, cities: &[domain::ids::CityId]) -> f64 {
    (1..=12)
        .flat_map(|month| {
            let month = SimMonth::new(2027, month).expect("valid month");
            (0..TICKS_PER_DAY).map(move |tick| {
                cities
                    .iter()
                    .map(|id| {
                        region.city(*id).expect("known city").base_load.value()
                            * region.load_factor(*id, month, tick)
                    })
                    .sum::<f64>()
            })
        })
        .fold(0.0, f64::max)
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
fn every_shipped_region_allows_battery_discharge_from_six_am_and_charge_until_ten_pm() {
    for &id in SHIPPED {
        let region = region(id);
        let battery = region.spec(PlantKind::Battery).battery.unwrap();
        assert_eq!(battery.discharge_start_hour, 6, "{id}");
        assert_eq!(battery.charge_end_hour, 22, "{id}");
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
            session.region().start().connected_cities.len()
                - session.region().start().island_homes.len().max(1)
                + session.plant_views().len()
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
        let peak = peak_demand(&region, &start.connected_cities);
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

/// The same eight seeds for each region, as independent tests so a large region does not leave
/// one worker playing eight games while the other workers are idle.
macro_rules! scenario_seeds {
    ($scenario:ident, $id:expr) => {
        scenario_seeds!(@cases $scenario, $id;
            (seed_0, 0), (seed_1, 1), (seed_2, 2), (seed_3, 3),
            (seed_4, 4), (seed_5, 5), (seed_6, 6), (seed_7, 7));
    };
    (@cases $scenario:ident, $id:expr; $(($case:ident, $seed:expr)),+) => {
        $(
            #[test]
            fn $case() {
                $scenario($id, $seed);
            }
        )+
    };
}

/// One test group per region and scenario, so nextest runs regions/seeds in parallel and names a failure.
/// Spec §14.5 beginner acceptance: with the hands-off plan (every dispatchable plant committed
/// all day under AGC, no other commands), each shipped region is still `Active` after twelve
/// months on seeds 0-7. Spec §16.5: the beginner acceptance check also passes with no commands
/// at all, on the previous operator's schedule. Each region task adds one line here.
macro_rules! shipped_scenario_cases {
    ($(($hands_off:ident, $no_commands:ident, $expansion:ident, $outages:ident, $id:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $hands_off() {
                assert_survives_year_one(hands_off_game, $id);
            }

            #[test]
            fn $no_commands() {
                assert_survives_year_one(new_game, $id);
            }

            mod $expansion {
                use super::*;
                scenario_seeds!(assert_first_expansion, $id);
            }

            mod $outages {
                use super::*;
                scenario_seeds!(assert_no_outages_year_one, $id);
            }
        )+

        /// All four scenario families cover exactly this same region matrix.
        const SCENARIO_REGIONS: &[&str] = &[$($id),+];
    };
}

shipped_scenario_cases! {
    (
        year_one_hands_off_brazil_sin,
        year_one_no_commands_brazil_sin,
        first_expansion_brazil_sin,
        no_outages_brazil_sin,
        BRAZIL
    ),
    (
        year_one_hands_off_continental_europe_maghreb,
        year_one_no_commands_continental_europe_maghreb,
        first_expansion_continental_europe_maghreb,
        no_outages_continental_europe_maghreb,
        EUROPE_MAGHREB
    ),
    (
        year_one_hands_off_eastern_interconnection,
        year_one_no_commands_eastern_interconnection,
        first_expansion_eastern_interconnection,
        no_outages_eastern_interconnection,
        EAST
    ),
    (
        year_one_hands_off_india_nepal_bhutan,
        year_one_no_commands_india_nepal_bhutan,
        first_expansion_india_nepal_bhutan,
        no_outages_india_nepal_bhutan,
        INDIA
    ),
    (
        year_one_hands_off_ips_ups,
        year_one_no_commands_ips_ups,
        first_expansion_ips_ups,
        no_outages_ips_ups,
        IPS
    ),
    (
        year_one_hands_off_sgcc,
        year_one_no_commands_sgcc,
        first_expansion_sgcc,
        no_outages_sgcc,
        SGCC
    ),
    (
        year_one_hands_off_southern_china,
        year_one_no_commands_southern_china,
        first_expansion_southern_china,
        no_outages_southern_china,
        SOUTHERN_CHINA
    ),
    (
        year_one_hands_off_western_interconnection,
        year_one_no_commands_western_interconnection,
        first_expansion_western_interconnection,
        no_outages_western_interconnection,
        WEST
    ),
}

#[test]
fn scenario_cases_cover_every_shipped_region() {
    assert_eq!(SCENARIO_REGIONS, SHIPPED);
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
fn assert_no_outages_year_one(id: &str, seed: u64) {
    let mut outages = Vec::new();
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
    let future_cities: Vec<_> = region
        .cities()
        .iter()
        .filter(|city| {
            (region.start().connected_cities.contains(&city.id) || city.id == target.id)
                && region.area_at(city.cell) == region.area_at(target.cell)
        })
        .map(|city| city.id)
        .collect();
    let firm: f64 = region
        .start()
        .fleet
        .iter()
        .filter(|plant| region.area_at(plant.cell) == region.area_at(target.cell))
        .map(|plant| {
            let spec = region.spec(plant.kind);
            spec.capacity_credit.of(spec.capacity).value()
        })
        .sum();
    let gas = region.spec(PlantKind::Ccgt);
    // Cover the entire year's peak plus 10% operating headroom, using the same capacity
    // credits as the starting-fleet acceptance check. This includes existing spare capacity.
    let count = ((peak_demand(&region, &future_cities) * 1.10 - firm)
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
            !occupied.contains(cell)
                && region.area_at(*cell) == region.area_at(home)
                && region.site_check(PlantKind::Ccgt, *cell).is_ok()
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
        .filter(|city| city.connected && region.area_at(city.cell) == region.area_at(target.cell))
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
        // A14's immutable sidecars preserve strict legacy ranks before A16's new scale.
        let mut migrated = region_file(id);
        migrated.solar.calibration = infrastructure::region::SolarCalibrationDto::Legacy;
        migrated.irradiance = ron::from_str(
            &std::fs::read_to_string(format!(
                "{}/tests/support/global-foundations/{id}-irradiance-v4.ron",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap(),
        )
        .unwrap();
        let after = map_region_v4(migrated).unwrap();
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
        // A14's immutable sidecars preserve strict legacy ranks before A16's new scale.
        let mut migrated = region_file(id);
        migrated.solar.calibration = infrastructure::region::SolarCalibrationDto::Legacy;
        migrated.irradiance = ron::from_str(
            &std::fs::read_to_string(format!(
                "{}/tests/support/global-foundations/{id}-irradiance-v4.ron",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap(),
        )
        .unwrap();
        let after = map_region_v4(migrated).unwrap();
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

/// Catalog entries whose ramp comes from a sourced ramp row rather than ruling 24's per-MW
/// scaling, as (region, kind). None exists today.
const SOURCED_RAMPS: &[(&str, infrastructure::region::PlantKindDto)] = &[];

/// The ramp per MW of capacity of each catalog kind with a nonzero ramp, as (kind, ratio).
fn ramp_per_mw(id: &str) -> Vec<(infrastructure::region::PlantKindDto, f64)> {
    region_file(id)
        .catalog
        .iter()
        .filter(|spec| spec.ramp_mw_per_min != 0.0)
        .map(|spec| (spec.kind, spec.ramp_mw_per_min / spec.capacity_mw))
        .collect()
}

/// Each entry of `local` whose ramp per MW differs from the reference's for that kind by more
/// than 1% (3-significant-figure rounding), unless `exempt` lists it.
fn ramp_mismatches(
    label: &str,
    local: &[(infrastructure::region::PlantKindDto, f64)],
    reference: &[(infrastructure::region::PlantKindDto, f64)],
    exempt: &[infrastructure::region::PlantKindDto],
) -> Vec<String> {
    let mut out = Vec::new();
    for (kind, reference_ratio) in reference {
        if exempt.contains(kind) {
            continue;
        }
        match local.iter().find(|(k, _)| k == kind) {
            Some((_, ratio)) if ((ratio - reference_ratio) / reference_ratio).abs() <= 0.01 => {}
            Some((_, ratio)) => out.push(format!(
                "{label} {kind:?}: {ratio:.5} MW/min per MW against North America's \
                 {reference_ratio:.5}"
            )),
            None => out.push(format!("{label} {kind:?}: no ramp")),
        }
    }
    out
}

/// Ruling 24: a new region's unit ramps scale per MW from the North American catalog, so each
/// kind's ramp per MW equals the Eastern Interconnection's (within 1%, for rounding) unless a
/// sourced ramp row backs it.
#[test]
fn new_region_ramps_scale_per_mw_from_north_america() {
    let reference = ramp_per_mw(EAST);
    assert!(reference.len() >= 6, "{reference:?}");
    let mut mismatches = Vec::new();
    for &id in NEW_REGIONS {
        let exempt: Vec<_> = SOURCED_RAMPS
            .iter()
            .filter(|(region, _)| *region == id)
            .map(|(_, kind)| *kind)
            .collect();
        mismatches.extend(ramp_mismatches(id, &ramp_per_mw(id), &reference, &exempt));
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn the_ramp_check_allows_rounding_and_flags_the_rest() {
    use infrastructure::region::PlantKindDto::{Coal, Nuclear};
    let reference = [(Coal, 6.0 / 600.0), (Nuclear, 5.0 / 1100.0)];
    // 5 x 700 / 1,100 = 3.1818 rounds to 3.18: within 1%.
    let scaled = [(Coal, 8.0 / 800.0), (Nuclear, 3.18 / 700.0)];
    assert!(ramp_mismatches("scaled", &scaled, &reference, &[]).is_empty());
    let unscaled = [(Coal, 10.0 / 600.0), (Nuclear, 3.18 / 700.0)];
    let flagged = ramp_mismatches("unscaled", &unscaled, &reference, &[]);
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    assert!(flagged[0].contains("Coal"), "{flagged:?}");
    assert!(ramp_mismatches("exempt", &unscaled, &reference, &[Coal]).is_empty());
    let missing = ramp_mismatches("missing", &[(Coal, 0.01)], &reference, &[]);
    assert_eq!(missing, ["missing Nuclear: no ramp"]);
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

#[test]
fn shipped_regions_do_not_overlap() {
    let maps: Vec<(&str, LandMap)> = SHIPPED.iter().map(|&id| (id, LandMap::load(id))).collect();
    for (a_id, a) in &maps {
        for (b_id, b) in &maps {
            if a_id == b_id {
                continue;
            }
            let shared = a.centres_on_land_in(b);
            assert!(
                shared.is_empty(),
                "{a_id} has {} land cells whose centre is playable in {b_id}, first {:?}",
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
    assert!(!extent.exclude.is_empty());
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
            ("Wenshan", 104.24, 23.37),
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

// ---- Region checks: IPS/UPS ------------------------------------------------------------------

mod ips_ups {
    use super::*;

    const ID: &str = IPS;

    fn month(m: u8) -> SimMonth {
        SimMonth::new(2027, m).expect("valid month")
    }

    fn city_named(region: &RegionDefinition, key: &str) -> domain::region::CityDef {
        region
            .cities()
            .iter()
            .find(|city| city.name_key == key)
            .unwrap_or_else(|| panic!("{key} is a city"))
            .clone()
    }

    /// The footprint is Russia's first synchronous zone with Belarus, Kazakhstan, Uzbekistan,
    /// mainland Azerbaijan, Kyrgyzstan, main Tajikistan and Georgia (Abkhazia and South Ossetia
    /// included by the grid-operation rule); Kaliningrad, the Far East, the isolated Norilsk
    /// system, Armenia, Iran, Mongolia, Nakhchivan, the Pamir and Crimea are out (spec section
    /// 28.3).
    #[test]
    fn ips_landmarks() {
        let map = LandMap::load(ID);
        for (name, lon, lat) in [
            ("Moscow", 37.62, 55.76),
            ("Minsk", 27.56, 53.90),
            ("Almaty", 76.95, 43.24),
            ("Tashkent", 69.28, 41.31),
            ("Tbilisi", 44.79, 41.72),
            ("Baku", 49.83, 40.38),
            ("Novosibirsk", 82.95, 55.05),
            ("Irkutsk", 104.28, 52.29),
            ("Chita", 113.47, 52.05),
            ("Murmansk", 33.08, 68.97),
            ("Sukhumi", 41.02, 43.00),
            ("Tskhinvali", 43.96, 42.23),
        ] {
            assert!(map.land_at(lon, lat), "{name} is playable land");
        }
        for (name, lon, lat) in [
            ("Kaliningrad", 20.51, 54.71),
            ("Vladivostok", 131.89, 43.12),
            ("Khabarovsk", 135.07, 48.48),
            ("Yerevan", 44.51, 40.18),
            ("Tehran", 51.39, 35.69),
            ("Ulaanbaatar", 106.92, 47.92),
            ("Sevastopol", 33.52, 44.60),
            ("Simferopol", 34.10, 44.95),
            ("Norilsk", 88.20, 69.35),
            ("Nakhchivan", 45.41, 39.21),
            ("Khorugh", 71.55, 37.49),
            ("Mirny", 113.96, 62.53),
        ] {
            assert!(!map.land_at(lon, lat), "{name} is outside the footprint");
        }
    }

    /// Rulings 10 and 11: the canonical Crimea and Sevastopol extent, shared with
    /// Europe-Maghreb, appears verbatim among the file's exclusions.
    #[test]
    fn ips_exclude_contains_the_canonical_extent() {
        let file = region_file(ID);
        let extent = excluded_extent();
        assert!(!extent.exclude.is_empty());
        for (i, polygon) in extent.exclude.iter().enumerate() {
            assert!(
                file.exclude.iter().any(|ring| ring == polygon),
                "canonical polygon {i} is not in exclude unchanged"
            );
        }
    }

    /// Spec sections 28.5 and 28.12: above the Arctic Circle a Solar plant at Murmansk sees no
    /// sun at any December tick, and in June it still produces at Murmansk's local midnight.
    #[test]
    fn murmansk_polar_night_and_midnight_sun() {
        let region = region(ID);
        let murmansk = city_named(&region, "city-murmansk");
        let solar = region.spec(PlantKind::Solar);
        let irradiance = region
            .terrain()
            .get(murmansk.cell)
            .expect("Murmansk's cell is in the grid")
            .irradiance;
        assert!(irradiance.value() > 0.0);
        assert!(region.site_check(PlantKind::Solar, murmansk.cell).is_ok());
        let output = |m: u8, tick: u32, clearness: f64| {
            region
                .solar_available(solar.capacity, murmansk.cell, month(m), tick, clearness)
                .value()
        };
        for tick in 0..TICKS_PER_DAY {
            assert_eq!(output(12, tick, 1.0), 0.0, "December tick {tick}");
        }
        let midnight = (0..TICKS_PER_DAY)
            .find(|&tick| region.local_minute(murmansk.id, month(6), tick) == 0)
            .expect("Murmansk has a local midnight");
        let clearness = region.solar().clearness_mean[5];
        assert!(
            output(6, midnight, clearness) > 0.0,
            "June output at local midnight (tick {midnight})"
        );
    }

    /// Spec section 28.12: Novosibirsk, four hours east of Moscow, sees sunrise earlier on the
    /// MSK system clock in every month.
    #[test]
    fn novosibirsk_sunrise_precedes_moscow_on_msk() {
        let region = region(ID);
        assert_eq!(region.system_zone().offset_minutes(month(1)), 180);
        let sunrise = |key: &str, m: u8| {
            let cell = city_named(&region, key).cell;
            (0..TICKS_PER_DAY)
                .find(|&tick| region.sun(cell, month(m), tick) > 0.0)
                .unwrap_or_else(|| panic!("{key} has a sunrise in month {m}"))
        };
        for m in 1..=12 {
            let (novosibirsk, moscow) = (sunrise("city-novosibirsk", m), sunrise("city-moscow", m));
            assert!(
                novosibirsk < moscow,
                "month {m}: Novosibirsk rises at tick {novosibirsk}, Moscow at {moscow}"
            );
        }
    }

    /// Peak over mean of a day's values.
    fn peak_to_mean(values: &[f64]) -> f64 {
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        values.iter().copied().fold(f64::MIN, f64::max) / mean
    }

    /// Spec section 28.3's six-time-zone lesson: with every city connected, Siberia and Central
    /// Asia reach their evening peaks hours before Moscow on the MSK clock, so the aggregate
    /// forecast is flatter than Moscow's own demand from September to May, the heating season
    /// that sets the year's peak. (From June to August the southern and Central Asian
    /// air-conditioning afternoons line up with Moscow's midday plateau instead.)
    #[test]
    fn siberian_cities_flatten_the_system_peak() {
        for m in [1, 2, 3, 4, 5, 9, 10, 11, 12] {
            let mut loaded = catalog()
                .load(&RegionId::new(ID))
                .expect("shipped region loads");
            let mut file = region_file(ID);
            file.start.month = m;
            file.start.connected_cities = file.cities.iter().map(|city| city.id).collect();
            loaded.definition = Arc::new(map_region_v4(file).expect("all-cities region maps"));
            let catalog = StartMonthOverride(loaded);
            let game = new_game(&catalog, ID, 0);
            let session = game.session();
            assert!(session.city_views().iter().all(|city| city.connected));
            let aggregate: Vec<f64> = (0..TICKS_PER_DAY)
                .map(|tick| session.forecast_net_load(tick).value())
                .collect();
            let region = session.region();
            let moscow = city_named(region, "city-moscow");
            let own: Vec<f64> = (0..TICKS_PER_DAY)
                .map(|tick| {
                    moscow.base_load.value() * region.load_factor(moscow.id, month(m), tick)
                })
                .collect();
            let (system, city) = (peak_to_mean(&aggregate), peak_to_mean(&own));
            assert!(
                system < city,
                "month {m}: the system peaks at {system:.4} of its mean, Moscow at {city:.4}"
            );
        }
    }
}

// ---- Region checks: Continental Europe-Maghreb ------------------------------------------------

mod continental_europe_maghreb {
    use super::*;
    use domain::ids::{CellCoord, NodeId};

    const ID: &str = EUROPE_MAGHREB;

    fn city_named(region: &RegionDefinition, key: &str) -> domain::region::CityDef {
        region
            .cities()
            .iter()
            .find(|city| city.name_key == key)
            .unwrap_or_else(|| panic!("{key} is a city"))
            .clone()
    }

    /// The footprint is the Continental Europe synchronous area with DK1, Turkiye, connected
    /// Ukraine and Moldova, the Baltic states, Sicily, Malta and Crete, plus Morocco, northern
    /// Algeria and Tunisia; the Nordic area, DK2, Great Britain, Ireland, Sardinia, Corsica, the
    /// Balearics, isolated southern Algeria and Crimea are out (spec section 28.3).
    #[test]
    fn europe_maghreb_landmarks() {
        let map = LandMap::load(ID);
        for (name, lon, lat) in [
            ("Seville", -5.99, 37.39),
            ("Tangier", -5.81, 35.76),
            ("Lisbon", -9.14, 38.72),
            ("Aarhus", 10.20, 56.16),
            ("Palermo", 13.36, 38.12),
            ("Valletta", 14.51, 35.90),
            ("Heraklion", 25.14, 35.34),
            ("Istanbul", 28.98, 41.01),
            ("Kyiv", 30.52, 50.45),
            ("Donetsk", 37.80, 48.00),
            ("Chisinau", 28.84, 47.02),
            ("Tunis", 10.18, 36.81),
            ("Algiers", 3.06, 36.75),
            // Grid-operation rulings recorded in the region file.
            ("Tiraspol (Transnistria)", 29.63, 46.84),
            ("Pristina (Kosovo)", 21.17, 42.66),
            ("Laayoune (ONEE grid)", -13.20, 27.15),
            ("Smara (ONEE grid since 2000)", -11.67, 26.74),
            ("Ghardaia (Algeria's interconnected north)", 3.67, 32.49),
            ("Tallinn", 24.75, 59.44),
        ] {
            assert!(map.land_at(lon, lat), "{name} is playable land");
        }
        for (name, lon, lat) in [
            ("Copenhagen", 12.57, 55.68),
            ("London", -0.13, 51.51),
            ("Dublin", -6.26, 53.35),
            ("Oslo", 10.75, 59.91),
            ("Cagliari", 9.11, 39.22),
            ("Palma", 2.65, 39.57),
            ("Ajaccio", 8.74, 41.93),
            ("Tamanrasset", 5.52, 22.79),
            ("Sevastopol", 33.52, 44.60),
            ("Simferopol", 34.10, 44.95),
            // Inside the bounds, south of the schematic 30 N line or otherwise excluded.
            ("Adrar", -0.29, 27.87),
            ("In Salah", 2.47, 27.19),
            ("Timimoun", 0.23, 29.26),
            ("Tifariti (east of the berm)", -10.6, 26.1),
            ("Coast south of Dakhla (no grid source)", -15.9, 23.2),
            ("Minsk", 27.56, 53.90),
            ("Kaliningrad", 20.51, 54.71),
            ("Bornholm (DK2)", 14.92, 55.13),
            ("Nicosia", 33.36, 35.17),
            ("Algeria south of 30 N at the Morocco border", -6.05, 29.64),
            ("Libya at the Ghadames tripoint", 9.55, 30.24),
        ] {
            assert!(!map.land_at(lon, lat), "{name} is outside the footprint");
        }
    }

    /// Rulings 10 and 11: the canonical Crimea and Sevastopol extent, shared with IPS/UPS,
    /// appears verbatim among the file's exclusions.
    #[test]
    fn europe_maghreb_exclude_contains_the_canonical_extent() {
        let file = region_file(ID);
        let extent = excluded_extent();
        assert!(!extent.exclude.is_empty());
        for (i, polygon) in extent.exclude.iter().enumerate() {
            assert!(
                file.exclude.iter().any(|ring| ring == polygon),
                "canonical polygon {i} is not in exclude unchanged"
            );
        }
    }

    /// The nearest buildable land cell to (lon, lat) among those whose centre `side` accepts,
    /// by distance in degrees with longitude scaled by the latitude's cosine; ties go to the
    /// lower cell.
    fn nearest_site(
        region: &RegionDefinition,
        occupied: &[CellCoord],
        lon: f64,
        lat: f64,
        side: impl Fn(f64) -> bool,
    ) -> CellCoord {
        let file = region_file(ID);
        let (width, height) = (file.grid.width, file.grid.height);
        let b = &file.bounds;
        let centre = |cell: CellCoord| {
            (
                b.west + (f64::from(cell.x) + 0.5) / f64::from(width) * (b.east - b.west),
                b.north - (f64::from(cell.y) + 0.5) / f64::from(height) * (b.north - b.south),
            )
        };
        let cos = lat.to_radians().cos();
        (0..height)
            .flat_map(|y| (0..width).map(move |x| CellCoord { x, y }))
            .filter(|cell| {
                !occupied.contains(cell)
                    && region.site_check(PlantKind::Peaker, *cell).is_ok()
                    && side(centre(*cell).0)
            })
            .min_by(|a, b| {
                let d = |cell: &CellCoord| {
                    let (x, y) = centre(*cell);
                    ((x - lon) * cos).powi(2) + (y - lat).powi(2)
                };
                d(a).total_cmp(&d(b)).then(a.cmp(b))
            })
            .expect("a buildable cell exists")
    }

    /// Builds a Peaker at `cell` and returns its node.
    fn build_node(game: &mut Game, cell: CellCoord) -> NodeId {
        let events = game
            .execute(Command::BuildPlant {
                kind: PlantKind::Peaker,
                cell,
            })
            .expect("the plant is ordered");
        let plant = events
            .iter()
            .find_map(|event| match event {
                DomainEvent::PlantConstructionStarted { plant, .. } => Some(*plant),
                _ => None,
            })
            .expect("a plant was started");
        game.session()
            .plant_views()
            .into_iter()
            .find(|view| view.id == plant)
            .expect("the plant has a view")
            .node
    }

    /// A landfall: (lon, lat) and which cell-centre longitudes count as its side of the water.
    type Landfall = (f64, f64, fn(f64) -> bool);

    /// Spec section 28.8: from a new game a player can quote a line across the Strait of
    /// Messina (between the Calabrian side east of 15.6 E and the Sicilian side west of it), from
    /// Syracuse to Malta and from the Peloponnese (Molaoi, the Crete link's landfall) to
    /// Heraklion; each straight path crosses water, so each is a submarine cable.
    #[test]
    fn messina_malta_and_crete_are_reachable_by_line() {
        let mut game = new_game(&catalog(), ID, 0);
        let region = Arc::clone(game.session().region());
        let mut occupied: Vec<CellCoord> = game
            .session()
            .plant_views()
            .iter()
            .map(|plant| plant.cell)
            .chain(game.session().city_views().iter().map(|city| city.cell))
            .collect();
        let crossings: [(&str, Landfall, Landfall); 3] = [
            (
                "Messina",
                (15.65, 38.11, |lon| lon > 15.6),
                (15.55, 38.19, |lon| lon < 15.6),
            ),
            ("Malta", (15.29, 37.07, |_| true), (14.51, 35.90, |_| true)),
            ("Crete", (22.85, 36.80, |_| true), (25.14, 35.34, |_| true)),
        ];
        for (name, (alon, alat, aside), (blon, blat, bside)) in crossings {
            let a = nearest_site(&region, &occupied, alon, alat, aside);
            occupied.push(a);
            let b = nearest_site(&region, &occupied, blon, blat, bside);
            occupied.push(b);
            let (from, to) = (build_node(&mut game, a), build_node(&mut game, b));
            let quote = game
                .session()
                .line_quote(from, to)
                .unwrap_or_else(|error| panic!("{name}: the crossing is quoted: {error:?}"));
            assert!(quote.cells > 0, "{name}: {quote:?}");
            let water = region.terrain().water_cells_on_segment(a, b);
            assert!(
                !water.is_empty(),
                "{name}: the path {a:?} to {b:?} crosses water"
            );
        }
    }

    /// Task 2's offline rasteriser regenerates the shipped economic-zone cells sidecar, byte
    /// for byte, from the committed authoring polygons (spec section 28.11.1).
    #[test]
    fn europe_maghreb_zone_cells_are_reproducible() {
        #[derive(serde::Deserialize)]
        struct Authoring {
            zones: Vec<infrastructure::region::ZonePolygons>,
        }
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/authoring/continental_europe_maghreb_economic_zones.ron"
        );
        let source = std::fs::read(path).expect("authoring file exists");
        let authoring: Authoring = ron::de::from_bytes(&source).expect("authoring file parses");
        let file = region_file(ID);
        let cells = infrastructure::region::economic_zone_cells(&file, &authoring.zones)
            .expect("zone cells rasterise");
        let sidecar = infrastructure::region::economic_zone_cells_sidecar(
            &format!("{ID}.ron"),
            &format!("{ID}_economic_zones.ron"),
            &source,
            &cells,
            file.grid.width,
        );
        let shipped =
            std::fs::read_to_string(format!("{REGIONS_DIR}/{ID}/economic_zone_cells.ron"))
                .expect("sidecar exists");
        assert!(
            sidecar == shipped,
            "regenerate it with author_economic_zone_cells"
        );
        assert!(region(ID).has_economic_zones());
    }

    /// The regions' copies of the Crimea and Sevastopol extent are the canonical file (ruling 11).
    #[test]
    fn region_copies_of_the_ukraine_extent_are_the_canonical_file() {
        let canonical = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/authoring/ukraine_ips_operating_extent.ron"
        ))
        .expect("canonical extent exists");
        for region in [ID, "ips_ups"] {
            let copy = std::fs::read(format!(
                "{REGIONS_DIR}/{region}/ukraine_ips_operating_extent.ron"
            ))
            .expect("region copy exists");
            assert!(copy == canonical, "{region}");
        }
    }

    /// Spec section 28.7.1 and the contrast of record (the 2024-25 pairing, user decision
    /// 2026-10-26): onshore wind at Seville (Spain) and Tangier (Morocco) differs by at least
    /// 10% in capital, in either direction, and by at least one game month in build time, and
    /// both stay within the pacing ceilings.
    #[test]
    fn spain_and_morocco_differ_in_cost_and_duration() {
        let region = region(ID);
        let seville = city_named(&region, "city-seville").cell;
        let tangier = city_named(&region, "city-tangier").cell;
        let (spain, morocco) = (
            region.economic_zone_at(seville).unwrap(),
            region.economic_zone_at(tangier).unwrap(),
        );
        assert_ne!(spain, morocco);
        assert_eq!(
            region.economic_zone(spain).unwrap().name_key,
            "economic-zone-spain"
        );
        assert_eq!(
            region.economic_zone(morocco).unwrap().name_key,
            "economic-zone-morocco"
        );
        let es = region.plant_spec_at(PlantKind::Wind, seville).unwrap();
        let ma = region.plant_spec_at(PlantKind::Wind, tangier).unwrap();
        let (es_cost, ma_cost) = (es.capital_cost.cents(), ma.capital_cost.cents());
        let gap = (ma_cost - es_cost).abs() as f64 / es_cost.min(ma_cost) as f64;
        assert!(gap >= 0.10, "wind capital {es_cost} vs {ma_cost}: {gap:.3}");
        // The 2024-25 pairing of record: Spain 845 and Morocco 751 EUR/kW paced, so Morocco
        // is cheaper. The 2021 pair would also pass the gap check above.
        assert_eq!(
            (es.capital_cost, ma.capital_cost),
            (
                domain::units::Money::from_dollars(253_500_000),
                domain::units::Money::from_dollars(225_300_000)
            )
        );
        assert!(ma_cost < es_cost, "Morocco's wind is cheaper");
        assert!(
            es.build_months.abs_diff(ma.build_months) >= 1,
            "wind months {} vs {}",
            es.build_months,
            ma.build_months
        );
        for (label, cell) in [("Seville", seville), ("Tangier", tangier)] {
            let overshoots = pacing_overshoots(label, |kind| {
                region.plant_spec_at(kind, cell).unwrap().build_months
            });
            assert!(overshoots.is_empty(), "{overshoots:?}");
        }
    }

    /// Isolated southern Algeria is out (the schematic 30 N boundary), so no Algerian
    /// construction-zone cell has its centre south of 30 N.
    #[test]
    fn algeria_zone_stays_north_of_30n() {
        let file = region_file(ID);
        let region = region(ID);
        let algeria = domain::region::EconomicZoneId(3);
        assert_eq!(
            region.economic_zone(algeria).unwrap().name_key,
            "economic-zone-algeria"
        );
        let (north, south) = (file.bounds.north, file.bounds.south);
        let height = f64::from(file.grid.height);
        let terrain = region.terrain();
        let southern: Vec<CellCoord> = (0..terrain.height())
            .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
            .filter(|cell| region.economic_zone_at(*cell).unwrap() == algeria)
            .filter(|cell| north - (f64::from(cell.y) + 0.5) / height * (north - south) < 30.0)
            .collect();
        assert!(southern.is_empty(), "{southern:?}");
    }

    /// The research note's "Algeria and Tunisia zone proposal (follow-up 2026-10-26)": Algiers
    /// and Tunis resolve solar and CCGT construction from their zones, and every other kind,
    /// wind included, and the AC line terms inherit the regional default, zone 0.
    #[test]
    fn algeria_and_tunisia_use_their_profiles() {
        use domain::units::Money;
        let region = region(ID);
        for (city, name_key, solar, ccgt) in [
            (
                "city-algiers",
                "economic-zone-algeria",
                (78_900_000, 3),
                (287_000_000, 6),
            ),
            (
                "city-tunis",
                "economic-zone-tunisia",
                (92_700_000, 3),
                (235_500_000, 6),
            ),
        ] {
            let cell = city_named(&region, city).cell;
            let zone = region
                .economic_zone(region.economic_zone_at(cell).unwrap())
                .unwrap_or_else(|| panic!("{city} is in an authored zone"));
            assert_eq!(zone.name_key, name_key, "{city}");
            assert_eq!(
                (zone.ac_lines.cost_per_cell, zone.ac_lines.cells_per_month),
                (None, None),
                "{city}: AC lines stay regional"
            );
            for kind in PlantKind::ALL {
                let spec = region.plant_spec_at(kind, cell).unwrap();
                let expected = match kind {
                    PlantKind::Solar => (Money::from_dollars(solar.0), solar.1),
                    PlantKind::Ccgt => (Money::from_dollars(ccgt.0), ccgt.1),
                    _ => (
                        region.spec(kind).capital_cost,
                        region.spec(kind).build_months,
                    ),
                };
                assert_eq!(
                    (spec.capital_cost, spec.build_months),
                    expected,
                    "{city} {kind:?}"
                );
            }
        }
    }

    /// Spec section 28.8: a new game serves Seville, Malaga and Tangier; Tangier's starter line
    /// from Seville is the named Spain-Morocco interconnector, crosses the strait, and is valued
    /// in the rate base by the route's economic-zone terms.
    #[test]
    fn gibraltar_starts_served_named_and_valued_by_route() {
        use domain::company::AssetRef;
        let game = new_game(&catalog(), ID, 0);
        let session = game.session();
        let region = session.region();
        let served: Vec<String> = session
            .city_views()
            .into_iter()
            .filter(|city| city.connected)
            .map(|city| city.name_key)
            .collect();
        assert_eq!(served, ["city-seville", "city-malaga", "city-tangier"]);
        let seville = city_named(region, "city-seville");
        let tangier = city_named(region, "city-tangier");
        assert_eq!(
            region.start_line_name(tangier.id),
            Some("line-spain-morocco")
        );
        let tangier_node = session
            .city_views()
            .into_iter()
            .find(|city| city.id == tangier.id)
            .unwrap()
            .node;
        let line = session
            .line_views()
            .into_iter()
            .find(|line| {
                (line.from, line.to) == (session.anchor_node(), tangier_node)
                    || (line.to, line.from) == (session.anchor_node(), tangier_node)
            })
            .expect("Tangier's starter line runs from Seville");
        assert!(
            !region
                .terrain()
                .water_cells_on_segment(seville.cell, tangier.cell)
                .is_empty(),
            "the interconnector crosses the strait"
        );
        let terms = region
            .line_terms(
                seville.cell,
                tangier.cell,
                domain::grid::distance_cells(seville.cell, tangier.cell),
            )
            .unwrap();
        let original = session
            .company()
            .rate_base()
            .assets()
            .find(|asset| asset.asset == AssetRef::Line(line.id))
            .expect("the starter line is in the rate base")
            .original;
        assert_eq!(original, terms.capital);
        let spain = region.economic_zone(region.economic_zone_at(seville.cell).unwrap());
        let morocco = region.economic_zone(region.economic_zone_at(tangier.cell).unwrap());
        let ac = |zone: Option<&domain::region::EconomicZone>| {
            zone.and_then(|z| z.ac_lines.cost_per_cell)
                .unwrap_or(region.lines().cost_per_cell)
        };
        if ac(spain) != ac(morocco) {
            assert_ne!(
                original.cents(),
                i64::from(terms.cells) * region.lines().cost_per_cell.cents()
            );
        }
        // Every zone the route samples is Spain, Morocco or the regional default (water).
        assert!(
            terms
                .zones
                .iter()
                .all(|work| work.zone.0 <= 2 && work.cells > 0),
            "{terms:?}"
        );
    }
}

mod sgcc {
    use super::*;
    #[test]
    fn starter_zones_have_distinct_shapes_and_regionally_characteristic_fleets() {
        use std::collections::BTreeMap;
        let game = new_game(&catalog(), SGCC, 7);
        let session = game.session();
        let mut fleets = Vec::new();
        let mut zones = Vec::new();
        for (&id, nodes) in session.power().island_nodes() {
            let fleet = session
                .plant_views()
                .into_iter()
                .filter(|p| nodes.contains(&p.node))
                .fold(BTreeMap::<_, usize>::new(), |mut counts, p| {
                    *counts.entry(p.kind).or_default() += 1;
                    counts
                });
            let area = session
                .region()
                .area_at(session.power().network().node(id).unwrap().cell)
                .0;
            match area {
                1 => assert!(
                    fleet.contains_key(&PlantKind::Coal) && fleet.contains_key(&PlantKind::Ccgt)
                ),
                2 => assert!(
                    fleet.contains_key(&PlantKind::Nuclear)
                        && fleet.contains_key(&PlantKind::Solar),
                    "coastal East inherits nuclear and solar"
                ),
                3 => assert!(
                    fleet.contains_key(&PlantKind::Coal)
                        && fleet.contains_key(&PlantKind::Wind)
                        && fleet.contains_key(&PlantKind::Solar),
                    "Northwest inherits thermal-backed wind and solar"
                ),
                4 => assert!(
                    fleet.contains_key(&PlantKind::Coal)
                        && fleet.contains_key(&PlantKind::Wind)
                        && !fleet.contains_key(&PlantKind::Solar),
                    "Northeast inherits coal and wind"
                ),
                5 => assert!(
                    fleet.get(&PlantKind::Hydro).copied().unwrap_or(0) >= 2
                        && fleet.contains_key(&PlantKind::Ccgt),
                    "Southwest inherits hydro and dry-season backup"
                ),
                _ => panic!("unknown SGCC area"),
            }
            assert!(
                !fleets.contains(&fleet),
                "starter fleet composition must be distinct"
            );
            fleets.push(fleet);
            zones.push(id);
        }
        for month in 1..=12 {
            let date = SimMonth::new(2027, month).unwrap();
            let profiles: Vec<_> = zones
                .iter()
                .map(|id| {
                    let nodes = &session.power().island_nodes()[id];
                    let expected: Vec<_> = (0..24)
                        .map(|h| {
                            session
                                .city_views()
                                .iter()
                                .filter(|c| nodes.contains(&c.node))
                                .map(|c| {
                                    session.region().city(c.id).unwrap().base_load.value()
                                        * session.region().load_factor(
                                            c.id,
                                            date,
                                            h * TICKS_PER_HOUR,
                                        )
                                })
                                .sum::<f64>()
                        })
                        .collect();
                    let peak = expected.iter().copied().fold(0.0, f64::max);
                    expected.into_iter().map(|v| v / peak).collect::<Vec<_>>()
                })
                .collect();
            for (i, a) in profiles.iter().enumerate() {
                for b in profiles.iter().skip(i + 1) {
                    let difference = a
                        .iter()
                        .zip(b)
                        .map(|(a, b)| (a - b).abs())
                        .fold(0.0, f64::max);
                    assert!(
                        difference >= 0.05,
                        "month {month}: shapes must differ, not just load scale: {difference}"
                    );
                }
            }
        }
    }

    #[test]
    fn starter_regional_sources_generate_and_retrospectives_are_distinct() {
        let mut game = new_game(&catalog(), SGCC, 7);
        let played = play_month(&mut game);
        let mut demands = Vec::new();
        for (id, nodes) in game.session().power().island_nodes() {
            let area = game
                .session()
                .region()
                .area_at(game.session().power().network().node(*id).unwrap().cell)
                .0;
            let demand: Vec<_> = played.ticks.iter().map(|p| p.islands[id].demand).collect();
            assert!(!demands.contains(&demand), "starter histories must differ");
            demands.push(demand);
            let output = |kind| {
                played
                    .ticks
                    .iter()
                    .map(|p| {
                        p.islands[id]
                            .by_kind
                            .get(&kind)
                            .map_or(0.0, |mw| mw.value())
                    })
                    .sum::<f64>()
            };
            match area {
                2 => assert!(output(PlantKind::Nuclear) > 0.0 && output(PlantKind::Solar) > 0.0),
                3 => assert!(output(PlantKind::Wind) > 0.0 && output(PlantKind::Solar) > 0.0),
                4 => assert!(output(PlantKind::Coal) > 0.0 && output(PlantKind::Wind) > 0.0),
                5 => {
                    let total: f64 = played
                        .ticks
                        .iter()
                        .map(|p| p.islands[id].generation.value())
                        .sum();
                    assert!(
                        output(PlantKind::Hydro) > total * 0.5,
                        "Southwest is hydro-led, not merely labelled hydro"
                    );
                }
                _ => {}
            }
            assert!(
                game.session()
                    .plant_views()
                    .iter()
                    .any(|p| nodes.contains(&p.node))
            );
        }
    }

    #[test]
    fn each_sgcc_starter_grid_covers_its_own_full_year_without_imports() {
        for seed in 0..8 {
            let mut game = new_game(&catalog(), SGCC, seed);
            // This scenario still proves each inherited fleet can operate independently.
            let links: Vec<_> = game
                .session()
                .power()
                .network()
                .links()
                .map(|l| l.id)
                .collect();
            for link in links {
                game.execute(Command::SetLinkAutoFollow { link, on: false })
                    .unwrap();
            }
            for month in 1..=12 {
                assert!(
                    game.session()
                        .power()
                        .network()
                        .links()
                        .all(|link| link.schedule == [0.0; 24] && link.flow.value() == 0.0)
                );
                assert_eq!(game.session().power().island_nodes().len(), 5);
                for id in game.session().power().island_nodes().keys() {
                    let local = game.session().island_session(*id).unwrap();
                    assert!(
                        local.capacity_shortfall_check().is_none(),
                        "seed {seed}, month {month}, island {id:?}: shortfall"
                    );
                    assert!(
                        local.minimum_generation_check().is_none(),
                        "seed {seed}, month {month}, island {id:?}: minimum excess"
                    );
                }
                let played = play_month(&mut game);
                assert_eq!(played.report.islands.len(), 5);
                assert_eq!(
                    played.report.over_frequency_trips, 0,
                    "seed {seed}, month {month}: inherited fleets must not trip on surplus"
                );
                for (id, local) in &played.report.islands {
                    let share = f64::from(local.minutes_outside_band) / f64::from(TICKS_PER_DAY);
                    assert!(
                        share
                            <= game
                                .session()
                                .region()
                                .regulator()
                                .frequency_violation_share
                                .value(),
                        "seed {seed}, month {month}, zone {id:?}: frequency must be viable locally ({share})"
                    );
                }
                for tick in &played.ticks {
                    assert_eq!(tick.islands.len(), 5);
                    for (id, local) in &tick.islands {
                        assert_eq!(local.imports.value(), 0.0);
                        assert_eq!(local.exports.value(), 0.0);
                        assert_eq!(
                            local.outage,
                            Ratio::ZERO,
                            "seed {seed}, month {month}, island {id:?}, tick {}",
                            tick.tick
                        );
                        assert!(local.served.value() > 0.0);
                        assert!(
                            local.unserved.value() < 1e-6,
                            "seed {seed}, month {month}, island {id:?}: unserved load at tick {}",
                            tick.tick
                        );
                    }
                }
                assert_eq!(game.session().status(), SessionStatus::Active);
                game.execute(Command::FinishReview).unwrap();
            }
        }
    }

    #[test]
    fn starter_service_survives_restore_and_a_shortage_in_beijing_stays_local() {
        let mut baseline = new_game(&catalog(), SGCC, 7);
        let mut changed = new_game(&catalog(), SGCC, 7);
        for game in [&mut baseline, &mut changed] {
            let links: Vec<_> = game
                .session()
                .power()
                .network()
                .links()
                .map(|l| l.id)
                .collect();
            for link in links {
                game.execute(Command::SetLinkAutoFollow { link, on: false })
                    .unwrap();
            }
        }
        let home = changed.session().anchor_node();
        let plants: Vec<_> = changed
            .session()
            .plant_views()
            .into_iter()
            .filter(|p| changed.session().power().island_nodes()[&home].contains(&p.node))
            .map(|p| p.id)
            .collect();
        for plant in plants {
            changed
                .execute(Command::CommitPlant {
                    plant,
                    hours: [false; 24],
                })
                .unwrap();
        }
        baseline.execute(Command::BeginOperating).unwrap();
        changed.execute(Command::BeginOperating).unwrap();
        baseline.execute(Command::AdvanceTicks { n: 120 }).unwrap();
        changed.execute(Command::AdvanceTicks { n: 120 }).unwrap();
        let normal = baseline.session().grid_snapshot().last_tick.unwrap();
        let shortage = changed.session().grid_snapshot().last_tick.unwrap();
        for (id, local) in &shortage.islands {
            if *id == home {
                assert!(local.unserved.value() > 0.0);
            } else {
                assert_eq!(
                    local, &normal.islands[id],
                    "Beijing cannot burden island {id:?}"
                );
            }
        }
        let checkpoint = changed.checkpoint();
        let loaded = catalog().load(&RegionId::new(SGCC)).unwrap();
        let mut restored = Game::restore(
            loaded,
            checkpoint.clone(),
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )
        .unwrap();
        assert_eq!(restored.checkpoint(), checkpoint);
        for _ in 0..3 {
            changed.execute(Command::AdvanceTicks { n: 100 }).unwrap();
            restored.execute(Command::AdvanceTicks { n: 100 }).unwrap();
            assert_eq!(changed.checkpoint(), restored.checkpoint());
        }
        let replayed = application::replay(
            catalog().load(&RegionId::new(SGCC)).unwrap(),
            &checkpoint.header,
            &checkpoint.commands,
        )
        .unwrap();
        assert_eq!(
            replayed.snapshot_state(),
            Game::restore(
                catalog().load(&RegionId::new(SGCC)).unwrap(),
                checkpoint,
                Box::new(InMemoryCommandLog::default()),
                &|| false
            )
            .unwrap()
            .session()
            .snapshot_state()
        );
    }

    fn automatic_hvdc_year(seed: u64) {
        let mut game = new_game(&catalog(), SGCC, seed);
        let mut transferred = 0.0;
        for month in 1..=12 {
            let before = game.checkpoint();
            let schedules = game.session().link_schedules();
            assert!(
                schedules
                    .values()
                    .flatten()
                    .all(|mw| mw.is_finite() && mw.abs() <= 1000.0 + 1e-7)
            );
            assert_eq!(before, game.checkpoint(), "planning must be pure");
            let played = play_month(&mut game);
            assert_eq!(
                played.report.over_frequency_trips, 0,
                "seed {seed}, month {month}"
            );
            for local in played.report.islands.values() {
                assert!(
                    f64::from(local.minutes_outside_band) / f64::from(TICKS_PER_DAY)
                        <= game
                            .session()
                            .region()
                            .regulator()
                            .frequency_violation_share
                            .value()
                );
            }
            for tick in &played.ticks {
                assert!(
                    tick.unserved.value() < 1e-6,
                    "month {month}, tick {}: {} MW unserved",
                    tick.tick,
                    tick.unserved.value()
                );
                assert!(
                    tick.islands
                        .values()
                        .all(|i| i.mode == domain::grid::GridMode::Normal),
                    "month {month}, tick {}",
                    tick.tick
                );
                let imports: f64 = tick.islands.values().map(|i| i.imports.value()).sum();
                let exports: f64 = tick.islands.values().map(|i| i.exports.value()).sum();
                let losses: f64 = tick.islands.values().map(|i| i.losses.value()).sum();
                assert!((exports - imports - losses).abs() < 1e-5);
                transferred += exports;
            }
            assert_eq!(game.session().status(), SessionStatus::Active);
            game.execute(Command::FinishReview).unwrap();
        }
        assert!(transferred > 0.0);
        let checkpoint = game.checkpoint();
        let replayed = application::replay(
            catalog().load(&RegionId::new(SGCC)).unwrap(),
            &checkpoint.header,
            &checkpoint.commands,
        )
        .unwrap();
        assert_eq!(replayed.snapshot_state(), game.session().snapshot_state());
    }

    #[test]
    fn automatic_hvdc_balances_shipped_grids_for_a_year_and_replays() {
        automatic_hvdc_year(7);
    }
    macro_rules! automatic_year_seeds {
        ($($name:ident: $seed:literal),* $(,)?) => { $(#[test] fn $name() { automatic_hvdc_year($seed); })* };
    }
    automatic_year_seeds!(
        automatic_hvdc_year_seed_0: 0, automatic_hvdc_year_seed_1: 1,
        automatic_hvdc_year_seed_2: 2, automatic_hvdc_year_seed_3: 3,
        automatic_hvdc_year_seed_4: 4, automatic_hvdc_year_seed_5: 5,
        automatic_hvdc_year_seed_6: 6,
    );

    #[test]
    fn new_sgcc_has_five_active_starter_grids_with_neighbouring_hvdc() {
        let game = new_game(&catalog(), "sgcc", 7);
        let s = game.session();
        assert_eq!(s.power().island_nodes().len(), 5);
        assert_eq!(s.power().network().links().count(), 5);
        let network = s.power().network();
        let pairs: std::collections::BTreeSet<_> = network
            .links()
            .map(|link| {
                assert!(link.auto_follow);
                assert_eq!(link.status, domain::grid::LineStatus::InService);
                assert_eq!(link.schedule, [0.0; 24]);
                assert_eq!(link.flow.value(), 0.0);
                let a = network.node(link.from_grid()).unwrap().cell;
                let b = network.node(link.to_grid()).unwrap().cell;
                let asset = s
                    .company()
                    .rate_base()
                    .assets()
                    .find(|asset| asset.asset == domain::company::AssetRef::Link(link.id))
                    .unwrap();
                assert_eq!(
                    asset.original,
                    s.region()
                        .link_terms(a, b, link.length_cells)
                        .unwrap()
                        .capital
                );
                assert!(asset.legacy);
                let (a, b) = (s.region().area_at(a).0, s.region().area_at(b).0);
                (a.min(b), a.max(b))
            })
            .collect();
        assert_eq!(pairs, [(1, 2), (1, 3), (1, 4), (1, 5), (3, 5)].into());
        // Every pair of areas sharing a land-cell edge has one direct inherited corridor.
        let terrain = s.region().terrain();
        let mut neighbours = std::collections::BTreeSet::new();
        for y in 0..terrain.height() {
            for x in 0..terrain.width() {
                let a = s.region().area_at(domain::ids::CellCoord { x, y }).0;
                for (x, y) in [(x + 1, y), (x, y + 1)] {
                    let b = s.region().area_at(domain::ids::CellCoord { x, y }).0;
                    if a != 0 && b != 0 && a != b {
                        neighbours.insert((a.min(b), a.max(b)));
                    }
                }
            }
        }
        assert_eq!(pairs, neighbours);
        assert_eq!(s.company().cash(), s.region().start().cash);
        let mut areas = std::collections::BTreeSet::new();
        for (id, nodes) in s.power().island_nodes() {
            assert!(s.power().island_active(*id));
            let area = s
                .region()
                .area_at(s.power().network().node(*id).unwrap().cell);
            assert!(areas.insert(area));
            assert!(nodes.iter().all(|n| {
                s.region()
                    .area_at(s.power().network().node(*n).unwrap().cell)
                    == area
            }));
            let local = s.island_session(*id).unwrap();
            assert!(local.hourly_forecast().iter().any(|p| p.mean.value() > 0.0));
            assert!(local.capacity_shortfall_check().is_none());
            assert!(local.minimum_generation_check().is_none());
        }
        s.check_invariants().unwrap();
    }

    #[test]
    fn inherited_hvdc_transfers_and_survives_archive_restore_and_journal_replay() {
        use application::{SaveBundle, SaveId, SaveKind, SaveMetadata};
        use infrastructure::save::{decode_archive, encode_archive, replay_compatibility};
        let mut game = new_game(&catalog(), SGCC, 7);
        let link = game.session().power().network().links().next().unwrap().id;
        game.execute(Command::SetLinkSchedule {
            link,
            mw: Box::new([100.0; 24]),
        })
        .unwrap();
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: 60 }).unwrap();
        let dc = game.session().power().network().link(link).unwrap();
        assert!(dc.flow.value() > 0.0);
        assert!(dc.loss.value() > 0.0);
        assert!(dc.sent.value() > 0.0);
        let bundle = SaveBundle {
            compatibility: replay_compatibility(),
            metadata: SaveMetadata {
                slot: SaveId::new("a".repeat(32)).unwrap(),
                run: SaveId::new("b".repeat(32)).unwrap(),
                kind: SaveKind::Manual,
                name: "inherited HVDC".into(),
                saved_at: 0,
            },
            checkpoint: game.checkpoint(),
        };
        let bytes = encode_archive(&bundle, &bundle.compatibility).unwrap();
        let (decoded, loaded) = decode_archive(&bytes, &bundle.compatibility, &|| false).unwrap();
        let mut restored = decoded
            .restore(
                loaded,
                &bundle.compatibility,
                Box::new(InMemoryCommandLog::default()),
                &|| false,
            )
            .unwrap();
        assert_eq!(restored.checkpoint(), game.checkpoint());
        let replayed = application::replay(
            catalog().load(&RegionId::new(SGCC)).unwrap(),
            &bundle.checkpoint.header,
            &bundle.checkpoint.commands,
        )
        .unwrap();
        assert_eq!(replayed.snapshot_state(), game.session().snapshot_state());
        for command in [
            Command::AdvanceTicks { n: 60 },
            Command::AdvanceTicks { n: 1320 },
        ] {
            assert_eq!(game.execute(command.clone()), restored.execute(command));
            assert_eq!(restored.checkpoint(), game.checkpoint());
        }
        restored.session().check_invariants().unwrap();
    }

    #[test]
    fn sgcc_loads_as_five_areas_with_beijing_starter_and_dc_catalog() {
        let r = region("sgcc");
        assert_eq!(r.areas().unwrap().areas.len(), 5);
        assert!(r.hvdc().is_some());
        assert_eq!(
            r.city(r.start().home_city).unwrap().name_key,
            "city-beijing"
        );
        let mut game = new_game(&catalog(), "sgcc", 7);
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: 120 }).unwrap();
        game.session().check_invariants().unwrap();
    }
}

/// Every current region opts into the same clear-sky scale, with weather applied once.
#[test]
fn solar_calibration_uses_one_scale_in_all_shipped_regions() {
    for id in SHIPPED {
        let r = region(id);
        assert!(
            r.solar().annual_site_clearness,
            "{id} still uses legacy double attenuation"
        );
        let mut source = region_file(id);
        let calibration = source.solar.calibration;
        source.solar.calibration = infrastructure::region::SolarCalibrationDto::Legacy;
        let raw = map_region_v4(source).unwrap();
        for y in 0..r.terrain().height() {
            for x in 0..r.terrain().width() {
                let cell = domain::ids::CellCoord { x, y };
                let input = raw.terrain().get(cell).unwrap().irradiance.value();
                let expected = match calibration {
                    infrastructure::region::SolarCalibrationDto::AngstromPrescott => {
                        (input / 0.75).min(1.0)
                    }
                    infrastructure::region::SolarCalibrationDto::ClearSkyFraction => input,
                    infrastructure::region::SolarCalibrationDto::Legacy => {
                        panic!("{id} has no calibration")
                    }
                };
                assert!(
                    (r.terrain().get(cell).unwrap().irradiance.value() - expected).abs() < 1e-12,
                    "{id} {cell:?}"
                );
            }
        }
    }
}

#[test]
fn solar_calibration_puts_spain_and_morocco_on_the_north_american_scale() {
    let europe = region(EUROPE_MAGHREB);
    let west = region(WEST);
    let potential = |r: &RegionDefinition, name: &str| {
        r.solar_potential(r.cities().iter().find(|c| c.name_key == name).unwrap().cell)
            .value()
    };
    let seville = potential(&europe, "city-seville");
    let file = region_file(EUROPE_MAGHREB);
    let (w, h) = (europe.terrain().width(), europe.terrain().height());
    let at = domain::ids::CellCoord {
        x: ((-6.9 - file.bounds.west) / (file.bounds.east - file.bounds.west) * f64::from(w))
            as u16,
        y: ((file.bounds.north - 30.93) / (file.bounds.north - file.bounds.south) * f64::from(h))
            as u16,
    };
    let ouarzazate = europe.solar_potential(at).value();
    let phoenix = potential(&west, "city-phoenix");
    assert!(
        seville > 0.65 && ouarzazate > 0.73,
        "{seville} {ouarzazate}"
    );
    assert!(
        phoenix > seville && phoenix - ouarzazate < 0.12,
        "{phoenix} {ouarzazate}"
    );
}

/// Independent NASA POWER annual-energy receipts, not the runtime conversion formula.
#[test]
fn solar_clearness_matches_independent_nasa_anchor_energies() {
    let regions: std::collections::BTreeMap<_, _> = [WEST, EAST, SGCC]
        .into_iter()
        .map(|id| (id, (region(id), region_file(id))))
        .collect();
    for (id, lon, lat, all_sky, clear_sky) in [
        ("western_interconnection", -112.074, 33.448, 5.8476, 6.3326),
        ("western_interconnection", -122.332, 47.606, 3.4178, 5.0875),
        ("western_interconnection", -106.485, 31.762, 5.8987, 6.5273),
        ("western_interconnection", -123.12, 49.283, 3.2184, 4.95),
        ("western_interconnection", -122.419, 37.775, 4.7354, 5.9237),
        ("western_interconnection", -114.071, 51.045, 3.5086, 4.9553),
        ("western_interconnection", -119.814, 39.53, 5.2282, 6.0595),
        ("western_interconnection", -111.891, 40.761, 4.8036, 6.0358),
        ("western_interconnection", -116.202, 43.615, 4.5629, 5.6002),
        ("western_interconnection", -117.426, 47.659, 3.8357, 5.1936),
        ("eastern_interconnection", -80.192, 25.762, 5.0112, 6.2542),
        ("eastern_interconnection", -81.694, 41.499, 3.6271, 5.3326),
        ("eastern_interconnection", -79.383, 43.653, 3.6518, 5.2229),
        ("eastern_interconnection", -101.831, 35.222, 5.2246, 6.1507),
        ("eastern_interconnection", -104.619, 50.445, 3.755, 4.8888),
        ("eastern_interconnection", -87.63, 41.878, 3.8441, 5.3381),
        ("eastern_interconnection", -90.072, 29.951, 4.7405, 6.0103),
        ("eastern_interconnection", -71.059, 42.36, 3.8134, 5.322),
        ("eastern_interconnection", -84.388, 33.749, 4.5134, 5.8596),
        ("eastern_interconnection", -97.138, 49.895, 3.6953, 4.919),
        ("sgcc", 116.407, 39.904, 3.953, 4.8427),
        ("sgcc", 87.617, 43.825, 4.3032, 5.4086),
        ("sgcc", 104.067, 30.573, 3.1493, 5.293),
        ("sgcc", 91.172, 29.652, 5.7055, 7.1225),
        ("sgcc", 126.535, 45.803, 3.8506, 4.9553),
        ("sgcc", 121.474, 31.23, 3.7433, 5.4221),
        ("sgcc", 111.75, 40.842, 4.6476, 5.6681),
        ("sgcc", 114.305, 30.593, 3.5107, 5.1259),
        ("sgcc", 102.718, 25.038, 4.4657, 6.3984),
        ("sgcc", 108.94, 34.342, 3.8381, 5.3638),
    ] {
        let (r, file) = &regions[id];
        let cell = domain::ids::CellCoord {
            x: ((lon - file.bounds.west) / (file.bounds.east - file.bounds.west)
                * f64::from(r.terrain().width())) as u16,
            y: ((file.bounds.north - lat) / (file.bounds.north - file.bounds.south)
                * f64::from(r.terrain().height())) as u16,
        };
        let actual = r.terrain().get(cell).unwrap().irradiance.value();
        assert!(
            (actual - all_sky / clear_sky).abs() < 1e-12,
            "{id} ({lon}, {lat}): {actual}"
        );
    }
}

/// New-game economics must allow growth without financing several reactors from monthly sales.
#[test]
fn south_china_profitability_has_headroom_without_parallel_reactor_windfalls() {
    for seed in 0..8 {
        let mut game = hands_off_game(&catalog(), SOUTHERN_CHINA, seed);
        let nuclear = game.session().region().spec(PlantKind::Nuclear);
        let capital = nuclear.capital_cost.cents();
        let payment = nuclear.capital_cost.split_even(nuclear.build_months)[0].cents();
        let mut year_profit = 0;
        let mut january_profit = 0;
        for index in 0..13 {
            let played = play_month(&mut game);
            let net = played.report.net.cents();
            assert!(
                net > 0,
                "seed {seed}, month {index}: starter economy loses money"
            );
            assert!(
                net < 2 * payment,
                "seed {seed}, month {index}: operating profit {net} finances multiple reactors ({payment} each)"
            );
            if index == 0 {
                january_profit = net;
            }
            if index < 12 {
                year_profit += net;
            }
            if index == 12 {
                assert!(
                    net >= january_profit / 2 && net <= january_profit * 3 / 2,
                    "seed {seed}: first review creates a cash-flow cliff: {january_profit} -> {net}"
                );
            }
            assert_eq!(game.session().status(), SessionStatus::Active);
            game.execute(Command::FinishReview).unwrap();
        }
        assert!(
            year_profit < capital,
            "seed {seed}: year-one profit {year_profit} exceeds reactor cost {capital}"
        );
    }
}

#[test]
fn south_china_three_parallel_reactor_orders_consume_cash() {
    use domain::ids::CellCoord;
    for seed in 0..8 {
        let mut game = hands_off_game(&catalog(), SOUTHERN_CHINA, seed);
        let nuclear = game.session().region().spec(PlantKind::Nuclear);
        let payment = nuclear.capital_cost.split_even(nuclear.build_months)[0];
        let starting_cash = game.session().company_view().cash;
        for _ in 0..3 {
            let r = game.session().region();
            let cell = (0..r.terrain().height())
                .flat_map(|y| (0..r.terrain().width()).map(move |x| CellCoord { x, y }))
                .find(|c| game.session().plant_quote(PlantKind::Nuclear, *c).is_ok())
                .unwrap();
            game.execute(Command::BuildPlant {
                kind: PlantKind::Nuclear,
                cell,
            })
            .unwrap();
        }
        let played = play_month(&mut game);
        assert_eq!(played.report.construction, -(payment * 3));
        assert!(
            played.report.net.cents() < 0,
            "seed {seed}: triple reactor construction is funded by operating profit"
        );
        assert!(game.session().company_view().cash < starting_cash);
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(16))]
    #[test]
    fn south_china_profitability_is_bounded_for_other_seeds_and_months(seed in proptest::prelude::any::<u64>(), month in 1u8..=12) {
        let override_catalog = StartMonthOverride::new(SOUTHERN_CHINA, month);
        let mut game = hands_off_game(&override_catalog, SOUTHERN_CHINA, seed);
        let nuclear = game.session().region().spec(PlantKind::Nuclear);
        let payment = nuclear.capital_cost.split_even(nuclear.build_months)[0].cents();
        let played = play_month(&mut game);
        proptest::prop_assert!(played.report.net.cents()>0);
        proptest::prop_assert!(played.report.net.cents()<2*payment);
        proptest::prop_assert_eq!(game.session().status(), SessionStatus::Active);
    }
}
