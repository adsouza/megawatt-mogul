//! Optional construction markets, independent of physical/electrical/civil zones.
use super::{PlantKind, PlantSpec, RegionDefinition, RegionDefinitionParts, RegionError};
use crate::{error::DomainError, ids::CellCoord, units::Money};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct EconomicZoneId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlantConstructionOverride {
    pub kind: PlantKind,
    pub capital_cost: Option<Money>,
    pub build_months: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LineConstructionOverride {
    pub cost_per_cell: Option<Money>,
    pub cells_per_month: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EconomicZone {
    pub id: EconomicZoneId,
    pub name_key: String,
    pub plants: Vec<PlantConstructionOverride>,
    pub ac_lines: LineConstructionOverride,
    pub hvdc: super::HvdcConstructionOverride,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EconomicZones {
    pub zones: Vec<EconomicZone>,
    pub cells: Vec<EconomicZoneId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneRouteWork {
    pub zone: EconomicZoneId,
    pub cells: u32,
    pub capital: Money,
    /// Exact work in game months; only presentation may round this fraction.
    pub work_numerator: u128,
    pub work_denominator: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineConstructionTerms {
    pub cells: u32,
    pub capital: Money,
    pub months: u32,
    pub zones: Vec<ZoneRouteWork>,
}

pub(super) fn invalid(zone: Option<EconomicZoneId>, field: &'static str) -> RegionError {
    RegionError::InvalidEconomicZone { zone, field }
}
fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}
/// Validate once, including the exact-work denominator used by every later quote.
pub(super) fn validate(parts: &RegionDefinitionParts) -> Result<(u64, u64), RegionError> {
    let mut denominator = u64::from(parts.lines.cells_per_month);
    let mut dc_denominator = denominator;
    let Some(economics) = &parts.economic_zones else {
        return Ok((denominator, dc_denominator));
    };
    let width = usize::from(parts.terrain.width());
    if economics.cells.len() != width * usize::from(parts.terrain.height()) {
        return Err(invalid(None, "economic_zones.cells"));
    }
    let mut ids = BTreeSet::from([EconomicZoneId(0)]);
    for zone in &economics.zones {
        let bad = |field| invalid(Some(zone.id), field);
        if !ids.insert(zone.id) {
            return Err(bad("economic_zones.id"));
        }
        if zone.name_key.trim().is_empty() {
            return Err(bad("economic_zones.name_key"));
        }
        let mut kinds = BTreeSet::new();
        for p in &zone.plants {
            if !kinds.insert(p.kind) {
                return Err(bad("economic_zones.plants.kind"));
            }
            if p.kind == PlantKind::Geothermal
                && let Some(params) = &parts.geothermal
            {
                let mut spec = parts
                    .catalog
                    .iter()
                    .find(|s| s.kind == PlantKind::Geothermal)
                    .expect("validated kind")
                    .clone();
                if let Some(months) = p.build_months {
                    spec.build_months = months;
                }
                params
                    .validate(&spec)
                    .map_err(|_| bad("economic_zones.geothermal.build_months"))?;
            }
            if p.capital_cost.is_some_and(|m| m.is_negative()) {
                return Err(bad("economic_zones.plants.capital_cost"));
            }
            // The existing persisted instalment vector has a bounded reader. Do not admit a
            // project that cannot be saved/restored, or allocate an unbounded payment vector.
            if p.build_months
                .is_some_and(|m| m == 0 || u64::from(m) > crate::state::MAX_COLLECTION_LEN as u64)
            {
                return Err(bad("economic_zones.plants.build_months"));
            }
        }
        if zone.ac_lines.cost_per_cell.is_some_and(|m| m.is_negative()) {
            return Err(bad("economic_zones.ac_lines.cost_per_cell"));
        }
        let h = &zone.hvdc;
        if h.converter_cost_per_end.is_some_and(|m| m.is_negative())
            || h.cost_per_cell.is_some_and(|m| m.is_negative())
            || h.converter_build_months
                .is_some_and(|m| m == 0 || m as usize > crate::state::MAX_COLLECTION_LEN)
        {
            return Err(bad("economic_zones.hvdc"));
        }
        let dc_speed = h.cells_per_month.unwrap_or(parts.lines.cells_per_month);
        if dc_speed == 0 {
            return Err(bad("economic_zones.hvdc.cells_per_month"));
        }
        dc_denominator = (dc_denominator / gcd(dc_denominator, u64::from(dc_speed)))
            .checked_mul(u64::from(dc_speed))
            .ok_or_else(|| bad("economic_zones.hvdc.cells_per_month"))?;
        let speed = zone
            .ac_lines
            .cells_per_month
            .unwrap_or(parts.lines.cells_per_month);
        if speed == 0 {
            return Err(bad("economic_zones.ac_lines.cells_per_month"));
        }
        denominator = (denominator / gcd(denominator, u64::from(speed)))
            .checked_mul(u64::from(speed))
            .ok_or_else(|| bad("economic_zones.ac_lines.cells_per_month"))?;
    }
    for (index, id) in economics.cells.iter().enumerate() {
        let cell = CellCoord {
            x: (index % width) as u16,
            y: (index / width) as u16,
        };
        if !ids.contains(id) || (id.0 != 0 && !parts.terrain.get(cell).is_some_and(|c| c.land)) {
            return Err(invalid(Some(*id), "economic_zones.cells"));
        }
    }
    Ok((denominator, dc_denominator))
}
fn overflow() -> DomainError {
    DomainError::ConstructionOverflow
}

impl RegionDefinition {
    pub fn has_economic_zones(&self) -> bool {
        self.economic_zones.is_some()
    }
    pub fn economic_zone_at(&self, cell: CellCoord) -> Result<EconomicZoneId, DomainError> {
        if self.terrain.get(cell).is_none() {
            return Err(DomainError::InvalidConstructionCell { cell });
        }
        Ok(self.economic_zones.as_ref().map_or(EconomicZoneId(0), |e| {
            e.cells[usize::from(cell.y) * usize::from(self.terrain.width()) + usize::from(cell.x)]
        }))
    }
    pub fn economic_zone(&self, id: EconomicZoneId) -> Option<&EconomicZone> {
        self.economic_zones
            .as_ref()?
            .zones
            .iter()
            .find(|z| z.id == id)
    }
    pub fn plant_spec_at(
        &self,
        kind: PlantKind,
        cell: CellCoord,
    ) -> Result<PlantSpec, DomainError> {
        let id = self.economic_zone_at(cell)?;
        let mut spec = self.spec(kind).clone();
        if let Some(p) = self
            .economic_zone(id)
            .and_then(|z| z.plants.iter().find(|p| p.kind == kind))
        {
            if let Some(cost) = p.capital_cost {
                spec.capital_cost = cost;
            }
            if let Some(months) = p.build_months {
                spec.build_months = months;
            }
        }
        Ok(spec)
    }
    /// Fixed endpoints and the order's recorded length supply the original route economics.
    /// No topology/eligibility checks: existing and grandfathered lines need valuation too.
    pub fn line_terms(
        &self,
        a: CellCoord,
        b: CellCoord,
        length_cells: f64,
    ) -> Result<LineConstructionTerms, DomainError> {
        self.route_terms(a, b, length_cells, false)
    }
    pub(super) fn route_terms(
        &self,
        a: CellCoord,
        b: CellCoord,
        length_cells: f64,
        dc: bool,
    ) -> Result<LineConstructionTerms, DomainError> {
        self.economic_zone_at(a)?;
        self.economic_zone_at(b)?;
        let max_length = f64::from(self.terrain.width()).hypot(f64::from(self.terrain.height()));
        if !length_cells.is_finite() || length_cells < 0.0 || length_cells > max_length {
            return Err(overflow());
        }
        let n = whole_cells(length_cells);
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let mut counts = BTreeMap::<EconomicZoneId, u32>::new();
        for k in 0..n {
            // Cell centre at a+1/2, sampled at (2k+1)/(2N). Euclidean division
            // supplies floor even on descending y; exact boundary chooses the larger index.
            let d = i128::from(n) * 2;
            let coordinate = |a: u16, b: u16| -> Result<u16, DomainError> {
                let numerator = (i128::from(a) * 2 + 1) * i128::from(n)
                    + (i128::from(b) - i128::from(a)) * (i128::from(k) * 2 + 1);
                u16::try_from(numerator.div_euclid(d)).map_err(|_| overflow())
            };
            let cell = CellCoord {
                x: coordinate(a.x, b.x)?,
                y: coordinate(a.y, b.y)?,
            };
            *counts.entry(self.economic_zone_at(cell)?).or_default() += 1;
        }
        let denominator = if dc {
            self.hvdc_work_denominator
        } else {
            self.ac_work_denominator
        };
        let mut capital = 0i128;
        let mut work = 0u128;
        let mut zones = Vec::new();
        for (zone, cells) in counts {
            let terms = self.economic_zone(zone).map(|z| &z.ac_lines);
            let cost = if dc {
                self.economic_zone(zone)
                    .and_then(|z| z.hvdc.cost_per_cell)
                    .unwrap_or(
                        self.hvdc()
                            .ok_or(DomainError::HvdcUnavailable)?
                            .cost_per_cell,
                    )
            } else {
                terms
                    .and_then(|p| p.cost_per_cell)
                    .unwrap_or(self.lines.cost_per_cell)
            };
            let speed = if dc {
                self.economic_zone(zone)
                    .and_then(|z| z.hvdc.cells_per_month)
            } else {
                terms.and_then(|p| p.cells_per_month)
            }
            .unwrap_or(self.lines.cells_per_month);
            let amount = i128::from(cost.cents())
                .checked_mul(i128::from(cells))
                .ok_or_else(overflow)?;
            let scaled = u128::from(cells)
                .checked_mul(u128::from(denominator / u64::from(speed)))
                .ok_or_else(overflow)?;
            capital = capital.checked_add(amount).ok_or_else(overflow)?;
            work = work.checked_add(scaled).ok_or_else(overflow)?;
            zones.push(ZoneRouteWork {
                zone,
                cells,
                capital: Money::from_cents(i64::try_from(amount).map_err(|_| overflow())?),
                work_numerator: scaled,
                work_denominator: denominator,
            });
        }
        Ok(LineConstructionTerms {
            cells: n,
            capital: Money::from_cents(i64::try_from(capital).map_err(|_| overflow())?),
            months: u32::try_from(work.div_ceil(u128::from(denominator)))
                .map_err(|_| overflow())?
                .max(1),
            zones,
        })
    }
    pub(super) fn validate_start_construction(&self) -> Result<(), RegionError> {
        let mut total = 0i128;
        let mut add = |cost: Money| -> Result<(), RegionError> {
            total += i128::from(cost.cents());
            if total > i128::from(i64::MAX) {
                Err(invalid(None, "start.construction"))
            } else {
                Ok(())
            }
        };
        for plant in &self.start.fleet {
            add(self
                .plant_spec_at(plant.kind, plant.cell)
                .map_err(|_| invalid(None, "start.fleet"))?
                .capital_cost)?;
        }
        for cell in self.start.fleet.iter().map(|p| p.cell).chain(
            self.cities
                .iter()
                .filter(|c| {
                    c.id != self.start_home_at(c.cell).id
                        && self.start.connected_cities.contains(&c.id)
                })
                .map(|c| c.cell),
        ) {
            let home = self.start_home_at(cell).cell;
            add(self
                .line_terms(home, cell, crate::grid::distance_cells(home, cell))
                .map_err(|_| invalid(None, "start.lines"))?
                .capital)?;
        }
        Ok(())
    }
}
/// Preserve the legacy integer-nearness rule exactly for quotes and stored lengths.
pub(crate) fn whole_cells(length: f64) -> u32 {
    let nearest = length.round();
    (if (length - nearest).abs() <= 1e-9 {
        nearest
    } else {
        length.ceil()
    }) as u32
}
