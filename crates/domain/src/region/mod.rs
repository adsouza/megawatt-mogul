//! A region definition: terrain, plant catalog, cities, resource and rule
//! parameters and the starting scenario. Constructing one validates it, so the
//! rest of the domain can rely on its invariants.

mod params;
mod plant_spec;
mod terrain;

#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixtures;

use std::collections::BTreeSet;

use thiserror::Error;

use crate::ids::{CellCoord, CityId, RegionId};

pub use params::{
    CityDef, DemandShape, FrequencyParams, HydroParams, LineParams, RegulatorParams,
    SatisfactionParams, SolarParams, StartDef, StartPlantDef,
};
pub use plant_spec::{PlantKind, PlantSpec, SitingRule};
pub use terrain::{Cell, TerrainGrid};

use crate::units::Ratio;

/// Why a cell cannot host a plant.
#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum SiteError {
    #[error("cell is outside the grid")]
    OutOfBounds,
    #[error("cell is not land")]
    NotLand,
    #[error("cooling water {have:?} below required {need:?}")]
    InsufficientCoolingWater { have: Ratio, need: Ratio },
    #[error("no hydro potential at cell")]
    NoHydroSite,
}

/// Why a region definition was rejected.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum RegionError {
    #[error("grid has {actual} cells, expected {expected}")]
    GridSizeMismatch { expected: usize, actual: usize },
    #[error("no plant spec for {0:?}")]
    MissingPlantSpec(PlantKind),
    #[error("duplicate plant spec for {0:?}")]
    DuplicatePlantSpec(PlantKind),
    #[error("city {0:?} is outside the grid")]
    CityOutOfBounds(CityId),
    #[error("city {0:?} is not on land")]
    CityNotOnLand(CityId),
    #[error("duplicate city {0:?}")]
    DuplicateCity(CityId),
    #[error("start connects unknown city {0:?}")]
    UnknownStartCity(CityId),
    #[error("start plant {kind:?} at {cell:?} cannot be sited: {reason}")]
    InvalidStartSite {
        kind: PlantKind,
        cell: CellCoord,
        reason: SiteError,
    },
    #[error("start substation is not on land")]
    SubstationNotOnLand,
    #[error("invalid shape for {field}")]
    InvalidShape { field: &'static str },
    #[error("invalid band for {field}")]
    InvalidBand { field: &'static str },
}

/// The unvalidated pieces of a region, as authored in data.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionDefinitionParts {
    pub id: RegionId,
    pub name_key: String,
    pub terrain: TerrainGrid,
    pub catalog: Vec<PlantSpec>,
    pub cities: Vec<CityDef>,
    pub demand: DemandShape,
    pub solar: SolarParams,
    pub hydro: HydroParams,
    pub frequency: FrequencyParams,
    pub regulator: RegulatorParams,
    pub satisfaction: SatisfactionParams,
    pub lines: LineParams,
    pub start: StartDef,
}

/// A validated region. Only constructible through `TryFrom<RegionDefinitionParts>`.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionDefinition {
    id: RegionId,
    name_key: String,
    terrain: TerrainGrid,
    /// One spec per kind, in `PlantKind::ALL` order.
    catalog: Vec<PlantSpec>,
    cities: Vec<CityDef>,
    demand: DemandShape,
    solar: SolarParams,
    hydro: HydroParams,
    frequency: FrequencyParams,
    regulator: RegulatorParams,
    satisfaction: SatisfactionParams,
    lines: LineParams,
    start: StartDef,
}

impl RegionDefinition {
    pub fn id(&self) -> &RegionId {
        &self.id
    }

    pub fn name_key(&self) -> &str {
        &self.name_key
    }

    pub fn terrain(&self) -> &TerrainGrid {
        &self.terrain
    }

    /// One spec per plant kind, in `PlantKind::ALL` order.
    pub fn catalog(&self) -> &[PlantSpec] {
        &self.catalog
    }

    pub fn cities(&self) -> &[CityDef] {
        &self.cities
    }

    pub fn demand(&self) -> &DemandShape {
        &self.demand
    }

    pub fn solar(&self) -> &SolarParams {
        &self.solar
    }

    pub fn hydro(&self) -> &HydroParams {
        &self.hydro
    }

    pub fn frequency(&self) -> &FrequencyParams {
        &self.frequency
    }

    pub fn regulator(&self) -> &RegulatorParams {
        &self.regulator
    }

    pub fn satisfaction(&self) -> &SatisfactionParams {
        &self.satisfaction
    }

    pub fn lines(&self) -> &LineParams {
        &self.lines
    }

    pub fn start(&self) -> &StartDef {
        &self.start
    }

    pub fn spec(&self, kind: PlantKind) -> &PlantSpec {
        // Validation guarantees exactly one spec per kind, in `ALL` order.
        self.catalog
            .iter()
            .find(|s| s.kind == kind)
            .expect("validated catalog has every plant kind")
    }

    pub fn city(&self, id: CityId) -> Option<&CityDef> {
        self.cities.iter().find(|c| c.id == id)
    }

    /// Whether `kind` may be built on `cell`.
    pub fn site_check(&self, kind: PlantKind, cell: CellCoord) -> Result<(), SiteError> {
        site_check(&self.terrain, self.spec(kind).siting, cell)
    }
}

fn site_check(terrain: &TerrainGrid, siting: SitingRule, at: CellCoord) -> Result<(), SiteError> {
    let cell = terrain.get(at).ok_or(SiteError::OutOfBounds)?;
    if !cell.land {
        return Err(SiteError::NotLand);
    }
    match siting {
        SitingRule::Anywhere => Ok(()),
        SitingRule::CoolingWater { min } if cell.cooling_water < min => {
            Err(SiteError::InsufficientCoolingWater {
                have: cell.cooling_water,
                need: min,
            })
        }
        SitingRule::HydroSite { min } if cell.hydro_potential < min => Err(SiteError::NoHydroSite),
        SitingRule::CoolingWater { .. } | SitingRule::HydroSite { .. } => Ok(()),
    }
}

fn is_land(terrain: &TerrainGrid, at: CellCoord) -> bool {
    terrain.get(at).is_some_and(|c| c.land)
}

/// Orders the catalog by `PlantKind::ALL`, rejecting gaps and duplicates.
fn order_catalog(mut catalog: Vec<PlantSpec>) -> Result<Vec<PlantSpec>, RegionError> {
    let mut ordered = Vec::with_capacity(PlantKind::ALL.len());
    for kind in PlantKind::ALL {
        let mut matching = catalog.iter().filter(|s| s.kind == kind);
        if matching.next().is_none() {
            return Err(RegionError::MissingPlantSpec(kind));
        }
        if matching.next().is_some() {
            return Err(RegionError::DuplicatePlantSpec(kind));
        }
        let at = catalog
            .iter()
            .position(|s| s.kind == kind)
            .expect("presence checked above");
        ordered.push(catalog.swap_remove(at));
    }
    Ok(ordered)
}

fn validate_cities(terrain: &TerrainGrid, cities: &[CityDef]) -> Result<(), RegionError> {
    let mut seen = BTreeSet::new();
    for city in cities {
        if !seen.insert(city.id) {
            return Err(RegionError::DuplicateCity(city.id));
        }
        if terrain.get(city.cell).is_none() {
            return Err(RegionError::CityOutOfBounds(city.id));
        }
        if !is_land(terrain, city.cell) {
            return Err(RegionError::CityNotOnLand(city.id));
        }
    }
    Ok(())
}

fn validate_start(
    terrain: &TerrainGrid,
    catalog: &[PlantSpec],
    cities: &[CityDef],
    start: &StartDef,
) -> Result<(), RegionError> {
    if !is_land(terrain, start.substation) {
        return Err(RegionError::SubstationNotOnLand);
    }
    for id in &start.connected_cities {
        if !cities.iter().any(|c| c.id == *id) {
            return Err(RegionError::UnknownStartCity(*id));
        }
    }
    for plant in &start.fleet {
        let spec = catalog
            .iter()
            .find(|s| s.kind == plant.kind)
            .expect("catalog validated before start");
        site_check(terrain, spec.siting, plant.cell).map_err(|reason| {
            RegionError::InvalidStartSite {
                kind: plant.kind,
                cell: plant.cell,
                reason,
            }
        })?;
    }
    Ok(())
}

fn all_positive(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite() && *v > 0.0)
}

fn check_shape(ok: bool, field: &'static str) -> Result<(), RegionError> {
    if ok {
        Ok(())
    } else {
        Err(RegionError::InvalidShape { field })
    }
}

fn validate_shapes(parts: &RegionDefinitionParts) -> Result<(), RegionError> {
    let d = &parts.demand;
    check_shape(all_positive(&d.winter), "demand.winter")?;
    check_shape(all_positive(&d.summer), "demand.summer")?;
    check_shape(all_positive(&d.shoulder), "demand.shoulder")?;
    check_shape(all_positive(&d.monthly), "demand.monthly")?;
    let s = &parts.solar;
    // NaN and infinities fail these comparisons, so no separate finiteness check is needed.
    let daylight_ok = s
        .daylight
        .iter()
        .all(|&(rise, set)| 0.0 <= rise && rise < set && set <= 24.0);
    check_shape(daylight_ok, "solar.daylight")?;
    check_shape(all_positive(&s.clearness_mean), "solar.clearness_mean")?;
    check_shape(
        s.clearness_sd.is_finite() && s.clearness_sd >= 0.0,
        "solar.clearness_sd",
    )?;
    check_shape(
        all_positive(&parts.hydro.monthly_flow),
        "hydro.monthly_flow",
    )?;
    // |phi| >= 1 would make the AR(1) innovation scale sqrt(1 - phi^2) NaN or zero.
    check_shape(
        (0.0..1.0).contains(&d.error_persistence),
        "demand.error_persistence",
    )
}

fn check_band(ok: bool, field: &'static str) -> Result<(), RegionError> {
    if ok {
        Ok(())
    } else {
        Err(RegionError::InvalidBand { field })
    }
}

fn finite_positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn validate_bands(regulator: &RegulatorParams) -> Result<(), RegionError> {
    let (min, base, max) = (
        regulator.min_return,
        regulator.base_return,
        regulator.max_return,
    );
    check_band(min <= max && min <= base && base <= max, "regulator.return")?;
    check_band(
        regulator.strikes_to_revoke >= 1,
        "regulator.strikes_to_revoke",
    )?;
    check_band(
        regulator.clean_months_to_expire >= 1,
        "regulator.clean_months_to_expire",
    )
}

fn validate_catalog_values(catalog: &[PlantSpec]) -> Result<(), RegionError> {
    for spec in catalog {
        check_band(finite_positive(spec.capacity.value()), "catalog.capacity")?;
        // Solar output follows the sun, so only dispatchable kinds need a ramp.
        check_band(
            !spec.kind.is_dispatchable() || finite_positive(spec.ramp.value()),
            "catalog.ramp",
        )?;
        check_band(spec.build_months >= 1, "catalog.build_months")?;
        check_band(spec.book_life_months >= 1, "catalog.book_life_months")?;
        check_band(!spec.capital_cost.is_negative(), "catalog.capital_cost")?;
        check_band(!spec.startup_cost.is_negative(), "catalog.startup_cost")?;
        check_band(
            !spec.fixed_om_per_month.is_negative(),
            "catalog.fixed_om_per_month",
        )?;
    }
    Ok(())
}

/// Thresholds are deviations from nominal: `blackout_below < shed_below < 0 <
/// normal_band < curtail_above`.
fn validate_frequency(f: &FrequencyParams) -> Result<(), RegionError> {
    let (normal, curtail) = (f.normal_band.value(), f.curtail_above.value());
    let (shed, blackout) = (f.shed_below.value(), f.blackout_below.value());
    check_band(finite_positive(normal), "frequency.normal_band")?;
    check_band(
        curtail.is_finite() && curtail > normal,
        "frequency.curtail_above",
    )?;
    check_band(shed < 0.0, "frequency.shed_below")?;
    check_band(
        blackout.is_finite() && blackout < shed,
        "frequency.blackout_below",
    )?;
    check_band(finite_positive(f.k_inertia), "frequency.k_inertia")?;
    check_band(
        f.k_damping > 0.0 && f.k_damping <= 1.0,
        "frequency.k_damping",
    )?;
    check_band(f.shed_step > Ratio::ZERO, "frequency.shed_step")?;
    check_band(
        f.blackout_after_shed_ticks >= 1,
        "frequency.blackout_after_shed_ticks",
    )
}

fn validate_lines(lines: &LineParams) -> Result<(), RegionError> {
    check_band(lines.cells_per_month >= 1, "lines.cells_per_month")?;
    check_band(lines.book_life_months >= 1, "lines.book_life_months")?;
    check_band(!lines.cost_per_cell.is_negative(), "lines.cost_per_cell")
}

impl TryFrom<RegionDefinitionParts> for RegionDefinition {
    type Error = RegionError;

    /// Validates in order: grid (checked when the terrain was built), catalog,
    /// cities, start, shapes, regulator bands, catalog values, frequency,
    /// lines. Reports the first error found.
    fn try_from(mut parts: RegionDefinitionParts) -> Result<Self, RegionError> {
        let catalog = order_catalog(std::mem::take(&mut parts.catalog))?;
        validate_cities(&parts.terrain, &parts.cities)?;
        validate_start(&parts.terrain, &catalog, &parts.cities, &parts.start)?;
        validate_shapes(&parts)?;
        validate_bands(&parts.regulator)?;
        validate_catalog_values(&catalog)?;
        validate_frequency(&parts.frequency)?;
        validate_lines(&parts.lines)?;
        Ok(Self {
            id: parts.id,
            name_key: parts.name_key,
            terrain: parts.terrain,
            catalog,
            cities: parts.cities,
            demand: parts.demand,
            solar: parts.solar,
            hydro: parts.hydro,
            frequency: parts.frequency,
            regulator: parts.regulator,
            satisfaction: parts.satisfaction,
            lines: parts.lines,
            start: parts.start,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{CellCoord, CityId};
    use crate::units::{Hertz, Megawatts, Money, RampRate, Ratio};

    #[test]
    fn tiny_region_is_valid() {
        assert!(RegionDefinition::try_from(fixtures::tiny_region_parts()).is_ok());
    }

    #[test]
    fn rejects_each_invalid_part() {
        type Case = (fn(&mut RegionDefinitionParts), RegionError);
        let cases: Vec<Case> = vec![
            (
                |p| {
                    p.catalog.pop();
                },
                RegionError::MissingPlantSpec(PlantKind::Hydro),
            ),
            (
                |p| {
                    let c = p.catalog[0].clone();
                    p.catalog.push(c);
                },
                RegionError::DuplicatePlantSpec(PlantKind::Nuclear),
            ),
            (
                |p| p.cities[0].cell = CellCoord { x: 0, y: 4 },
                RegionError::CityNotOnLand(CityId(1)),
            ),
            (
                |p| p.cities[0].cell = CellCoord { x: 50, y: 4 },
                RegionError::CityOutOfBounds(CityId(1)),
            ),
            (
                |p| p.start.connected_cities.push(CityId(99)),
                RegionError::UnknownStartCity(CityId(99)),
            ),
            (
                |p| p.start.substation = CellCoord { x: 0, y: 0 },
                RegionError::SubstationNotOnLand,
            ),
            (
                |p| p.demand.monthly[3] = 0.0,
                RegionError::InvalidShape {
                    field: "demand.monthly",
                },
            ),
            (
                |p| p.regulator.min_return = Ratio::new(0.2).unwrap(),
                RegionError::InvalidBand {
                    field: "regulator.return",
                },
            ),
        ];
        for (mutate, expected) in cases {
            let mut parts = fixtures::tiny_region_parts();
            mutate(&mut parts);
            assert_eq!(RegionDefinition::try_from(parts).unwrap_err(), expected);
        }
    }

    #[test]
    fn site_check_reasons() {
        let r = fixtures::tiny_region();
        assert_eq!(
            r.site_check(PlantKind::Peaker, CellCoord { x: 0, y: 1 }),
            Err(SiteError::NotLand)
        );
        assert_eq!(
            r.site_check(PlantKind::Peaker, CellCoord { x: 40, y: 1 }),
            Err(SiteError::OutOfBounds)
        );
        assert!(matches!(
            r.site_check(PlantKind::Nuclear, CellCoord { x: 2, y: 1 }),
            Err(SiteError::InsufficientCoolingWater { .. })
        ));
        assert_eq!(
            r.site_check(PlantKind::Hydro, CellCoord { x: 9, y: 1 }),
            Err(SiteError::NoHydroSite)
        );
        assert_eq!(
            r.site_check(PlantKind::Hydro, CellCoord { x: 10, y: 1 }),
            Ok(())
        );
    }

    #[test]
    fn invalid_start_fleet_site_is_rejected() {
        let mut p = fixtures::tiny_region_parts();
        p.start.fleet.push(StartPlantDef {
            kind: PlantKind::Hydro,
            cell: CellCoord { x: 2, y: 2 },
        });
        assert!(matches!(
            RegionDefinition::try_from(p),
            Err(RegionError::InvalidStartSite {
                reason: SiteError::NoHydroSite,
                ..
            })
        ));
    }

    #[test]
    fn extra_validation_edges() {
        let mut p = fixtures::tiny_region_parts();
        p.cities.push(p.cities[0].clone());
        assert_eq!(
            RegionDefinition::try_from(p).unwrap_err(),
            RegionError::DuplicateCity(CityId(1))
        );

        let mut p = fixtures::tiny_region_parts();
        p.regulator.base_return = Ratio::new(0.2).unwrap();
        assert_eq!(
            RegionDefinition::try_from(p).unwrap_err(),
            RegionError::InvalidBand {
                field: "regulator.return"
            }
        );

        let mut p = fixtures::tiny_region_parts();
        p.solar.clearness_mean[0] = f64::NAN;
        assert_eq!(
            RegionDefinition::try_from(p).unwrap_err(),
            RegionError::InvalidShape {
                field: "solar.clearness_mean"
            }
        );

        let mut p = fixtures::tiny_region_parts();
        p.hydro.monthly_flow[5] = -1.0;
        assert_eq!(
            RegionDefinition::try_from(p).unwrap_err(),
            RegionError::InvalidShape {
                field: "hydro.monthly_flow"
            }
        );
    }

    #[test]
    fn grid_size_mismatch_is_reported() {
        assert_eq!(
            TerrainGrid::new(2, 2, Vec::new()).unwrap_err(),
            RegionError::GridSizeMismatch {
                expected: 4,
                actual: 0
            }
        );
    }

    #[test]
    fn catalog_is_reordered_and_lookup_works() {
        let mut p = fixtures::tiny_region_parts();
        p.catalog.reverse();
        let r = RegionDefinition::try_from(p).unwrap();
        let kinds: Vec<_> = r.catalog().iter().map(|s| s.kind).collect();
        assert_eq!(kinds, PlantKind::ALL);
        assert_eq!(r.spec(PlantKind::Coal).capacity.value(), 600.0);
        assert_eq!(r.spec(PlantKind::Coal).min_output().value(), 240.0);
        assert!(r.city(CityId(3)).is_some() && r.city(CityId(9)).is_none());
        assert!(PlantKind::Coal.is_dispatchable() && !PlantKind::Solar.is_dispatchable());
    }

    #[test]
    fn terrain_is_row_major() {
        let r = fixtures::tiny_region();
        let t = r.terrain();
        assert_eq!((t.width(), t.height()), (12, 8));
        assert!(t.get(CellCoord { x: 12, y: 0 }).is_none());
        assert!(!t.get(CellCoord { x: 0, y: 7 }).unwrap().land);
        assert_eq!(
            t.get(CellCoord { x: 10, y: 1 })
                .unwrap()
                .hydro_potential
                .value(),
            0.8
        );
    }

    fn parts_with(edit: impl FnOnce(&mut RegionDefinitionParts)) -> RegionDefinitionParts {
        let mut p = fixtures::tiny_region_parts();
        edit(&mut p);
        p
    }

    fn band(min: f64, base: f64, max: f64) -> Result<RegionDefinition, RegionError> {
        RegionDefinition::try_from(parts_with(|p| {
            p.regulator.min_return = Ratio::new(min).unwrap();
            p.regulator.base_return = Ratio::new(base).unwrap();
            p.regulator.max_return = Ratio::new(max).unwrap();
        }))
    }

    #[test]
    fn name_key_is_the_authored_key() {
        assert_eq!(fixtures::tiny_region().name_key(), "region-tiny");
    }

    #[test]
    fn site_rules_accept_exactly_the_minimum() {
        let mut p = fixtures::tiny_region_parts();
        // Cell (2, 1) has cooling water 0.2 and (10, 1) hydro potential 0.8.
        p.catalog[0].siting = SitingRule::CoolingWater {
            min: Ratio::new(0.2).unwrap(),
        };
        p.catalog[5].siting = SitingRule::HydroSite {
            min: Ratio::new(0.8).unwrap(),
        };
        let r = RegionDefinition::try_from(p).unwrap();
        assert_eq!(
            r.site_check(PlantKind::Nuclear, CellCoord { x: 2, y: 1 }),
            Ok(())
        );
        assert_eq!(
            r.site_check(PlantKind::Hydro, CellCoord { x: 10, y: 1 }),
            Ok(())
        );
    }

    #[test]
    fn demand_shapes_must_be_positive_and_finite() {
        for (field, edit) in [
            (
                "demand.winter",
                (|d: &mut DemandShape| d.winter[5] = 0.0) as fn(&mut DemandShape),
            ),
            ("demand.summer", |d| d.summer[23] = -0.1),
            ("demand.shoulder", |d| d.shoulder[0] = f64::NAN),
            ("demand.monthly", |d| d.monthly[11] = f64::INFINITY),
        ] {
            let r = RegionDefinition::try_from(parts_with(|p| edit(&mut p.demand)));
            assert_eq!(
                r.unwrap_err(),
                RegionError::InvalidShape { field },
                "{field}"
            );
        }
    }

    #[test]
    fn daylight_must_be_an_ordered_window_within_a_day() {
        let bad: [(f64, f64); 7] = [
            (-1.0, 18.0),
            (f64::NAN, 18.0),
            (6.0, f64::NAN),
            (12.0, 12.0),
            (13.0, 12.0),
            (6.0, 24.5),
            (f64::NEG_INFINITY, 12.0),
        ];
        for window in bad {
            let r = RegionDefinition::try_from(parts_with(|p| p.solar.daylight[3] = window));
            assert_eq!(
                r.unwrap_err(),
                RegionError::InvalidShape {
                    field: "solar.daylight"
                }
            );
        }
        for window in [(0.0, 24.0), (5.0, 5.5)] {
            let r = RegionDefinition::try_from(parts_with(|p| p.solar.daylight[3] = window));
            assert!(r.is_ok(), "{window:?}");
        }
    }

    #[test]
    fn clearness_sd_must_be_finite_and_non_negative() {
        for sd in [-0.1, f64::NAN, f64::INFINITY] {
            let r = RegionDefinition::try_from(parts_with(|p| p.solar.clearness_sd = sd));
            assert_eq!(
                r.unwrap_err(),
                RegionError::InvalidShape {
                    field: "solar.clearness_sd"
                }
            );
        }
        assert!(RegionDefinition::try_from(parts_with(|p| p.solar.clearness_sd = 0.0)).is_ok());
    }

    #[test]
    fn return_band_is_inclusive_and_ordered() {
        let invalid = || {
            Err(RegionError::InvalidBand {
                field: "regulator.return",
            })
        };
        assert!(band(0.07, 0.07, 0.11).is_ok());
        assert!(band(0.07, 0.11, 0.11).is_ok());
        assert!(band(0.09, 0.09, 0.09).is_ok());
        assert_eq!(band(0.07, 0.06, 0.11), invalid());
        assert_eq!(band(0.07, 0.12, 0.11), invalid());
        assert_eq!(band(0.12, 0.09, 0.11), invalid());
    }

    #[test]
    fn start_plant_on_water_is_rejected_as_not_land() {
        let p = parts_with(|p| {
            p.start.fleet.push(StartPlantDef {
                kind: PlantKind::Peaker,
                cell: CellCoord { x: 0, y: 2 },
            });
        });
        assert_eq!(
            RegionDefinition::try_from(p).unwrap_err(),
            RegionError::InvalidStartSite {
                kind: PlantKind::Peaker,
                cell: CellCoord { x: 0, y: 2 },
                reason: SiteError::NotLand,
            }
        );
    }

    type Edit = fn(&mut RegionDefinitionParts);

    fn shape(field: &'static str) -> RegionError {
        RegionError::InvalidShape { field }
    }

    fn bad_band(field: &'static str) -> RegionError {
        RegionError::InvalidBand { field }
    }

    fn mw(value: f64) -> Megawatts {
        Megawatts::unchecked(value)
    }

    fn hz(value: f64) -> Hertz {
        Hertz::new(value)
    }

    /// Index of the peaker (dispatchable) and solar specs in the fixture catalog.
    const PEAKER: usize = 3;
    const SOLAR: usize = 4;

    #[test]
    fn out_of_range_numbers_are_rejected() {
        let cases: Vec<(Edit, RegionError)> = vec![
            // demand.error_persistence in [0, 1)
            (
                |p| p.demand.error_persistence = 1.0,
                shape("demand.error_persistence"),
            ),
            (
                |p| p.demand.error_persistence = 1.5,
                shape("demand.error_persistence"),
            ),
            (
                |p| p.demand.error_persistence = -0.01,
                shape("demand.error_persistence"),
            ),
            (
                |p| p.demand.error_persistence = f64::NAN,
                shape("demand.error_persistence"),
            ),
            // regulator counts
            (
                |p| p.regulator.strikes_to_revoke = 0,
                bad_band("regulator.strikes_to_revoke"),
            ),
            (
                |p| p.regulator.clean_months_to_expire = 0,
                bad_band("regulator.clean_months_to_expire"),
            ),
            // catalog
            (
                |p| p.catalog[PEAKER].capacity = mw(0.0),
                bad_band("catalog.capacity"),
            ),
            (
                |p| p.catalog[PEAKER].capacity = mw(-5.0),
                bad_band("catalog.capacity"),
            ),
            (
                |p| p.catalog[SOLAR].capacity = mw(f64::NAN),
                bad_band("catalog.capacity"),
            ),
            (
                |p| p.catalog[PEAKER].capacity = mw(f64::INFINITY),
                bad_band("catalog.capacity"),
            ),
            (
                |p| p.catalog[PEAKER].ramp = RampRate::mw_per_minute(0.0),
                bad_band("catalog.ramp"),
            ),
            (
                |p| p.catalog[PEAKER].ramp = RampRate::mw_per_minute(-1.0),
                bad_band("catalog.ramp"),
            ),
            (
                |p| p.catalog[PEAKER].ramp = RampRate::mw_per_minute(f64::NAN),
                bad_band("catalog.ramp"),
            ),
            (
                |p| p.catalog[PEAKER].ramp = RampRate::mw_per_minute(f64::INFINITY),
                bad_band("catalog.ramp"),
            ),
            (
                |p| p.catalog[PEAKER].build_months = 0,
                bad_band("catalog.build_months"),
            ),
            (
                |p| p.catalog[PEAKER].book_life_months = 0,
                bad_band("catalog.book_life_months"),
            ),
            (
                |p| p.catalog[PEAKER].capital_cost = Money::from_cents(-1),
                bad_band("catalog.capital_cost"),
            ),
            (
                |p| p.catalog[PEAKER].startup_cost = Money::from_cents(-1),
                bad_band("catalog.startup_cost"),
            ),
            (
                |p| p.catalog[PEAKER].fixed_om_per_month = Money::from_cents(-1),
                bad_band("catalog.fixed_om_per_month"),
            ),
            // frequency
            (
                |p| p.frequency.normal_band = hz(0.0),
                bad_band("frequency.normal_band"),
            ),
            (
                |p| p.frequency.normal_band = hz(-0.05),
                bad_band("frequency.normal_band"),
            ),
            (
                |p| p.frequency.normal_band = hz(f64::NAN),
                bad_band("frequency.normal_band"),
            ),
            (
                |p| p.frequency.normal_band = hz(f64::INFINITY),
                bad_band("frequency.normal_band"),
            ),
            (
                |p| p.frequency.curtail_above = hz(0.05),
                bad_band("frequency.curtail_above"),
            ),
            (
                |p| p.frequency.curtail_above = hz(0.04),
                bad_band("frequency.curtail_above"),
            ),
            (
                |p| p.frequency.curtail_above = hz(f64::NAN),
                bad_band("frequency.curtail_above"),
            ),
            (
                |p| p.frequency.curtail_above = hz(f64::INFINITY),
                bad_band("frequency.curtail_above"),
            ),
            (
                |p| p.frequency.shed_below = hz(0.0),
                bad_band("frequency.shed_below"),
            ),
            (
                |p| p.frequency.shed_below = hz(0.1),
                bad_band("frequency.shed_below"),
            ),
            (
                |p| p.frequency.shed_below = hz(f64::NAN),
                bad_band("frequency.shed_below"),
            ),
            (
                |p| p.frequency.blackout_below = hz(-0.5),
                bad_band("frequency.blackout_below"),
            ),
            (
                |p| p.frequency.blackout_below = hz(-0.4),
                bad_band("frequency.blackout_below"),
            ),
            (
                |p| p.frequency.blackout_below = hz(f64::NEG_INFINITY),
                bad_band("frequency.blackout_below"),
            ),
            (
                |p| p.frequency.blackout_below = hz(f64::NAN),
                bad_band("frequency.blackout_below"),
            ),
            (
                |p| p.frequency.k_inertia = 0.0,
                bad_band("frequency.k_inertia"),
            ),
            (
                |p| p.frequency.k_inertia = -1.0,
                bad_band("frequency.k_inertia"),
            ),
            (
                |p| p.frequency.k_inertia = f64::NAN,
                bad_band("frequency.k_inertia"),
            ),
            (
                |p| p.frequency.k_inertia = f64::INFINITY,
                bad_band("frequency.k_inertia"),
            ),
            (
                |p| p.frequency.k_damping = 0.0,
                bad_band("frequency.k_damping"),
            ),
            (
                |p| p.frequency.k_damping = 1.01,
                bad_band("frequency.k_damping"),
            ),
            (
                |p| p.frequency.k_damping = f64::NAN,
                bad_band("frequency.k_damping"),
            ),
            (
                |p| p.frequency.shed_step = Ratio::ZERO,
                bad_band("frequency.shed_step"),
            ),
            (
                |p| p.frequency.blackout_after_shed_ticks = 0,
                bad_band("frequency.blackout_after_shed_ticks"),
            ),
            // lines
            (
                |p| p.lines.cells_per_month = 0,
                bad_band("lines.cells_per_month"),
            ),
            (
                |p| p.lines.book_life_months = 0,
                bad_band("lines.book_life_months"),
            ),
            (
                |p| p.lines.cost_per_cell = Money::from_cents(-1),
                bad_band("lines.cost_per_cell"),
            ),
        ];
        for (i, (edit, expected)) in cases.into_iter().enumerate() {
            let r = RegionDefinition::try_from(parts_with(edit));
            assert_eq!(r.err(), Some(expected), "case {i}");
        }
    }

    #[test]
    fn range_boundaries_are_accepted() {
        let cases: Vec<Edit> = vec![
            |p| p.demand.error_persistence = 0.0,
            |p| p.demand.error_persistence = 0.999_999,
            |p| p.regulator.strikes_to_revoke = 1,
            |p| p.regulator.clean_months_to_expire = 1,
            |p| p.catalog[PEAKER].capacity = mw(0.001),
            |p| p.catalog[PEAKER].ramp = RampRate::mw_per_minute(0.001),
            // Solar output follows the sun, so its ramp is not checked.
            |p| p.catalog[SOLAR].ramp = RampRate::mw_per_minute(0.0),
            |p| p.catalog[PEAKER].build_months = 1,
            |p| p.catalog[PEAKER].book_life_months = 1,
            |p| p.catalog[PEAKER].capital_cost = Money::ZERO,
            |p| p.catalog[PEAKER].startup_cost = Money::ZERO,
            |p| p.catalog[PEAKER].fixed_om_per_month = Money::ZERO,
            |p| p.frequency.normal_band = hz(0.001),
            |p| p.frequency.curtail_above = hz(0.051),
            |p| p.frequency.shed_below = hz(-0.001),
            |p| p.frequency.blackout_below = hz(-0.501),
            |p| p.frequency.k_inertia = 0.001,
            |p| p.frequency.k_damping = 1.0,
            |p| p.frequency.k_damping = 0.001,
            |p| p.frequency.shed_step = Ratio::ONE,
            |p| p.frequency.blackout_after_shed_ticks = 1,
            |p| p.lines.cells_per_month = 1,
            |p| p.lines.book_life_months = 1,
            |p| p.lines.cost_per_cell = Money::ZERO,
        ];
        for (i, edit) in cases.into_iter().enumerate() {
            let r = RegionDefinition::try_from(parts_with(edit));
            assert_eq!(r.err(), None, "case {i}");
        }
    }

    #[test]
    fn with_two_faults_the_earlier_validation_stage_reports() {
        // catalog, then cities, then start, then shapes, then bands.
        let bad_band = |p: &mut RegionDefinitionParts| p.regulator.min_return = Ratio::ONE;
        let bad_shape = |p: &mut RegionDefinitionParts| p.demand.winter[0] = 0.0;
        let bad_start =
            |p: &mut RegionDefinitionParts| p.start.substation = CellCoord { x: 0, y: 0 };
        let bad_city = |p: &mut RegionDefinitionParts| p.cities[0].cell = CellCoord { x: 50, y: 4 };
        let bad_catalog = |p: &mut RegionDefinitionParts| {
            p.catalog.pop();
        };

        let err = |a: &dyn Fn(&mut RegionDefinitionParts),
                   b: &dyn Fn(&mut RegionDefinitionParts)| {
            RegionDefinition::try_from(parts_with(|p| {
                a(p);
                b(p);
            }))
            .unwrap_err()
        };
        assert_eq!(
            err(&bad_band, &bad_shape),
            RegionError::InvalidShape {
                field: "demand.winter"
            }
        );
        assert_eq!(
            err(&bad_shape, &bad_start),
            RegionError::SubstationNotOnLand
        );
        assert_eq!(
            err(&bad_start, &bad_city),
            RegionError::CityOutOfBounds(CityId(1))
        );
        assert_eq!(
            err(&bad_city, &bad_catalog),
            RegionError::MissingPlantSpec(PlantKind::Hydro)
        );
    }
}
