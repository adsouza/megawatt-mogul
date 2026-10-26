//! Electrical areas are independent of civil clocks and construction markets.
use super::{RegionDefinition, RegionDefinitionParts, RegionError};
use crate::{ids::CellCoord, units::Money};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct AreaId(pub u32);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElectricalArea {
    pub id: AreaId,
    pub name_key: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElectricalAreas {
    pub areas: Vec<ElectricalArea>,
    pub cells: Vec<AreaId>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct HvdcSpec {
    pub rating_mw: f64,
    /// Combined converter cost, including both ends.
    pub converter_cost: Money,
    pub cost_per_cell: Money,
    pub converter_build_months: u32,
    pub ramp_mw_per_min: f64,
    pub converter_loss: f64,
    pub loss_per_cell: f64,
    pub book_life_months: u32,
    pub designed_hours_per_year: f64,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HvdcConstructionOverride {
    pub converter_cost_per_end: Option<Money>,
    pub converter_build_months: Option<u32>,
    pub cost_per_cell: Option<Money>,
    pub cells_per_month: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkConstructionTerms {
    pub capital: Money,
    pub months: u32,
    pub route: super::LineConstructionTerms,
    /// Canonical endpoint order, for pricing only; never changes flow orientation.
    pub converter_costs: [Money; 2],
}
impl RegionDefinition {
    pub fn areas(&self) -> Option<&ElectricalAreas> {
        self.areas.as_ref()
    }
    pub fn area_at(&self, cell: CellCoord) -> AreaId {
        if self.terrain.get(cell).is_none() {
            return AreaId(0);
        }
        self.areas.as_ref().map_or(AreaId(0), |a| {
            a.cells[usize::from(cell.y) * usize::from(self.terrain.width()) + usize::from(cell.x)]
        })
    }
    pub fn hvdc(&self) -> Option<&HvdcSpec> {
        self.hvdc.as_ref()
    }
    pub fn link_terms(
        &self,
        a: CellCoord,
        b: CellCoord,
        length: f64,
    ) -> Result<LinkConstructionTerms, crate::error::DomainError> {
        use crate::error::DomainError;
        let spec = self.hvdc().ok_or(DomainError::HvdcUnavailable)?;
        let route = self.route_terms(a, b, length, true)?;
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let defaults = spec.converter_cost.split_even(2);
        let mut costs = [Money::ZERO; 2];
        let mut months = route.months;
        for (index, cell) in [a, b].into_iter().enumerate() {
            let terms = self
                .economic_zone(self.economic_zone_at(cell)?)
                .map(|z| &z.hvdc);
            costs[index] = terms
                .and_then(|t| t.converter_cost_per_end)
                .unwrap_or(defaults[index]);
            months = months.max(
                terms
                    .and_then(|t| t.converter_build_months)
                    .unwrap_or(spec.converter_build_months),
            );
        }
        let cents = i128::from(route.capital.cents())
            + i128::from(costs[0].cents())
            + i128::from(costs[1].cents());
        let capital =
            Money::from_cents(i64::try_from(cents).map_err(|_| DomainError::ConstructionOverflow)?);
        Ok(LinkConstructionTerms {
            capital,
            months,
            route,
            converter_costs: costs,
        })
    }
}
pub(super) fn validate(p: &RegionDefinitionParts) -> Result<(), RegionError> {
    let invalid = |field| RegionError::InvalidShape { field };
    if let Some(areas) = &p.areas {
        let width = usize::from(p.terrain.width());
        if areas.cells.len() != width * usize::from(p.terrain.height()) || areas.areas.is_empty() {
            return Err(invalid("areas.cells"));
        }
        let mut ids = BTreeSet::new();
        for area in &areas.areas {
            if area.id.0 == 0 || !ids.insert(area.id) || area.name_key.trim().is_empty() {
                return Err(invalid("areas.id"));
            }
        }
        for (index, id) in areas.cells.iter().enumerate() {
            let land = p
                .terrain
                .get(CellCoord {
                    x: (index % width) as u16,
                    y: (index / width) as u16,
                })
                .is_some_and(|c| c.land);
            if (land && !ids.contains(id)) || (!land && id.0 != 0) {
                return Err(invalid("areas.cells"));
            }
        }
        let at = |cell: CellCoord| areas.cells[usize::from(cell.y) * width + usize::from(cell.x)];
        let home = p
            .cities
            .iter()
            .find(|c| c.id == p.start.home_city)
            .expect("validated home");
        if p.cities
            .iter()
            .filter(|c| p.start.connected_cities.contains(&c.id))
            .any(|c| at(c.cell) != at(home.cell))
            || p.start
                .fleet
                .iter()
                .any(|plant| at(plant.cell) != at(home.cell))
        {
            return Err(invalid("start.areas"));
        }
    }
    if let Some(h) = &p.hvdc {
        let longest = f64::from(p.terrain.width())
            .hypot(f64::from(p.terrain.height()))
            .ceil();
        if !h.rating_mw.is_finite()
            || h.rating_mw <= 0.0
            || !h.ramp_mw_per_min.is_finite()
            || h.ramp_mw_per_min <= 0.0
            || h.converter_cost.is_negative()
            || h.cost_per_cell.is_negative()
            || h.converter_build_months == 0
            || h.converter_build_months as usize > crate::state::MAX_COLLECTION_LEN
            || h.book_life_months == 0
            || !h.designed_hours_per_year.is_finite()
            || h.designed_hours_per_year <= 0.0
            || h.designed_hours_per_year > 8784.0
            || !h.converter_loss.is_finite()
            || h.converter_loss < 0.0
            || !h.loss_per_cell.is_finite()
            || h.loss_per_cell < 0.0
            || 2.0 * h.converter_loss + longest * h.loss_per_cell > 0.5
        {
            return Err(invalid("hvdc"));
        }
    }
    Ok(())
}
crate::state_tuple!(AreaId);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::{RegionDefinition, fixtures};
    #[test]
    fn electrical_area_gaps_unknown_ids_and_unsafe_loss_factors_are_rejected() {
        let mut p = fixtures::two_area_parts();
        p.areas.as_mut().unwrap().cells[7] = AreaId(0);
        assert!(RegionDefinition::try_from(p).is_err());
        let mut p = fixtures::two_area_parts();
        p.areas.as_mut().unwrap().cells[7] = AreaId(99);
        assert!(RegionDefinition::try_from(p).is_err());
        for loss in [f64::NAN, -0.1, 0.5] {
            let mut p = fixtures::two_area_parts();
            p.hvdc.as_mut().unwrap().converter_loss = loss;
            assert!(RegionDefinition::try_from(p).is_err());
        }
    }
    proptest::proptest! {
        #[test]
        fn partial_hvdc_converter_overrides_inherit_each_field_independently(
            half in 0i64..10_000,
            west_cost in 0i64..20_000,
            east_cost in 0i64..20_000,
            west_months in 1u32..12,
            east_months in 1u32..12,
        ) {
            let a = CellCoord { x: 3, y: 4 };
            let b = CellCoord { x: 8, y: 4 };
            let mut base = fixtures::two_area_parts();
            base.hvdc.as_mut().unwrap().converter_cost = Money::from_cents(half * 2 + 1);
            let unzoned = RegionDefinition::try_from(base.clone()).unwrap();
            let default = unzoned.link_terms(a, b, 5.0).unwrap();
            // Exercise every field-presence combination, including zero-priced overrides,
            // either missing terminal and authored zones with no HVDC overrides at all.
            for mask in 0u8..16 {
                let mut parts = base.clone();
                parts.economic_zones = fixtures::economic_region_parts().economic_zones;
                let zones = &mut parts.economic_zones.as_mut().unwrap().zones;
                zones[0].hvdc.converter_cost_per_end = (mask & 1 != 0).then_some(Money::from_cents(west_cost));
                zones[1].hvdc.converter_cost_per_end = (mask & 2 != 0).then_some(Money::from_cents(east_cost));
                zones[0].hvdc.converter_build_months = (mask & 4 != 0).then_some(west_months);
                zones[1].hvdc.converter_build_months = (mask & 8 != 0).then_some(east_months);
                let region = RegionDefinition::try_from(parts).unwrap();
                let quote = region.link_terms(a, b, 5.0).unwrap();
                let costs = [
                    if mask & 1 != 0 { west_cost } else { half },
                    if mask & 2 != 0 { east_cost } else { half + 1 },
                ];
                let months = 3.max(if mask & 4 != 0 { west_months } else { 2 })
                    .max(if mask & 8 != 0 { east_months } else { 2 });
                proptest::prop_assert_eq!(quote.converter_costs.map(Money::cents), costs);
                proptest::prop_assert_eq!(quote.route.capital.cents(), 5 * 7);
                proptest::prop_assert_eq!(quote.capital.cents(), costs[0] + costs[1] + 5 * 7);
                proptest::prop_assert_eq!(quote.months, months);
                proptest::prop_assert_eq!(&quote, &region.link_terms(b, a, 5.0).unwrap());
                if mask == 0 {
                    proptest::prop_assert_eq!(quote.capital, default.capital);
                    proptest::prop_assert_eq!(quote.months, default.months);
                    proptest::prop_assert_eq!(quote.converter_costs, default.converter_costs);
                }
            }
        }

        #[test]
        fn partial_hvdc_route_overrides_inherit_regional_rates_and_round_work_once(
            west_cost in 0i64..1_000,
            east_cost in 0i64..1_000,
            west_speed in 1u32..9,
            east_speed in 1u32..9,
        ) {
            let a = CellCoord { x: 3, y: 4 };
            let b = CellCoord { x: 8, y: 4 };
            // Five canonical samples fall in cells 4,5 (west) and 6,7,8 (east).
            for mask in 0u8..16 {
                let mut parts = fixtures::two_area_parts();
                parts.economic_zones = fixtures::economic_region_parts().economic_zones;
                let zones = &mut parts.economic_zones.as_mut().unwrap().zones;
                zones[0].hvdc.cost_per_cell = (mask & 1 != 0).then_some(Money::from_cents(west_cost));
                zones[1].hvdc.cost_per_cell = (mask & 2 != 0).then_some(Money::from_cents(east_cost));
                zones[0].hvdc.cells_per_month = (mask & 4 != 0).then_some(west_speed);
                zones[1].hvdc.cells_per_month = (mask & 8 != 0).then_some(east_speed);
                let region = RegionDefinition::try_from(parts).unwrap();
                let quote = region.link_terms(a, b, 5.0).unwrap();
                let west_cost = if mask & 1 != 0 { west_cost } else { 7 };
                let east_cost = if mask & 2 != 0 { east_cost } else { 7 };
                let west_speed = if mask & 4 != 0 { west_speed } else { 2 };
                let east_speed = if mask & 8 != 0 { east_speed } else { 2 };
                let route_months = (2 * east_speed + 3 * west_speed).div_ceil(west_speed * east_speed);
                proptest::prop_assert_eq!(quote.route.cells, 5);
                proptest::prop_assert_eq!(quote.route.zones.iter().map(|z| z.cells).collect::<Vec<_>>(), vec![2, 3]);
                proptest::prop_assert_eq!(quote.route.capital.cents(), 2 * west_cost + 3 * east_cost);
                proptest::prop_assert_eq!(quote.capital.cents(), 101 + 2 * west_cost + 3 * east_cost);
                proptest::prop_assert_eq!(quote.route.months, route_months);
                proptest::prop_assert_eq!(quote.months, route_months.max(2));
                proptest::prop_assert_eq!(&quote, &region.link_terms(b, a, 5.0).unwrap());
            }
        }

        #[test]
        fn hvdc_route_reversal_and_default_converter_split_are_exact(x in 1u16..12,y in 0u16..8) {
            let r=RegionDefinition::try_from(fixtures::two_area_parts()).unwrap();
            let a=CellCoord{x:6,y:4};let b=CellCoord{x,y};let n=crate::grid::distance_cells(a,b);
            let q=r.link_terms(a,b,n).unwrap();
            proptest::prop_assert_eq!(&q,&r.link_terms(b,a,n).unwrap());
            proptest::prop_assert_eq!(q.converter_costs.iter().map(|c|c.cents()).sum::<i64>(),101);
            proptest::prop_assert_eq!(q.capital.cents(),101+7*i64::from(q.route.cells));
            proptest::prop_assert_eq!(q.months,r.line_terms(a,b,n).unwrap().months.max(2));
        }
    }
    #[test]
    fn terminal_overrides_and_dc_route_rates_are_independent_of_ac() {
        let mut p = fixtures::economic_region_parts();
        let multi = fixtures::two_area_parts();
        p.areas = multi.areas;
        p.hvdc = multi.hvdc;
        let e = p.economic_zones.as_mut().unwrap();
        e.zones[0].hvdc = HvdcConstructionOverride {
            converter_cost_per_end: Some(Money::from_cents(41)),
            converter_build_months: Some(8),
            cost_per_cell: Some(Money::from_cents(11)),
            cells_per_month: Some(2),
        };
        e.zones[1].hvdc = HvdcConstructionOverride {
            converter_cost_per_end: Some(Money::from_cents(59)),
            converter_build_months: Some(3),
            cost_per_cell: Some(Money::from_cents(13)),
            cells_per_month: Some(3),
        };
        let r = RegionDefinition::try_from(p).unwrap();
        let a = CellCoord { x: 3, y: 2 };
        let b = CellCoord { x: 8, y: 6 };
        let len = crate::grid::distance_cells(a, b);
        let dc = r.link_terms(a, b, len).unwrap();
        let ac = r.line_terms(a, b, len).unwrap();
        assert_eq!(
            dc.converter_costs,
            [Money::from_cents(41), Money::from_cents(59)]
        );
        assert_eq!(dc.months, 8);
        assert_eq!(dc.capital.cents(), dc.route.capital.cents() + 100);
        assert_ne!(dc.route.capital, ac.capital);
        assert_eq!(dc, r.link_terms(b, a, len).unwrap());
    }
}
