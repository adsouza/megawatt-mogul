use super::*;
use crate::{error::DomainError, ids::CellCoord, units::Money};
use proptest::prelude::*;

#[test]
fn local_plant_terms_override_only_construction_fields() {
    let region = RegionDefinition::try_from(fixtures::economic_region_parts()).unwrap();
    for (cell, cents, months) in [
        (CellCoord { x: 4, y: 4 }, 101, 2),
        (CellCoord { x: 8, y: 4 }, 202, 3),
    ] {
        let mut expected = region.spec(PlantKind::Wind).clone();
        expected.capital_cost = Money::from_cents(cents);
        expected.build_months = months;
        assert_eq!(
            region.plant_spec_at(PlantKind::Wind, cell).unwrap(),
            expected
        );
        assert_eq!(
            region.plant_spec_at(PlantKind::Solar, cell).unwrap(),
            *region.spec(PlantKind::Solar)
        );
    }
    assert!(matches!(
        region.plant_spec_at(PlantKind::Wind, CellCoord { x: 99, y: 1 }),
        Err(DomainError::InvalidConstructionCell { .. })
    ));
}

#[test]
fn cross_zone_work_is_rounded_once_and_samples_boundaries_exactly() {
    let region = RegionDefinition::try_from(fixtures::economic_region_parts()).unwrap();
    let terms = region
        .line_terms(CellCoord { x: 4, y: 4 }, CellCoord { x: 8, y: 4 }, 4.0)
        .unwrap();
    assert_eq!(
        (terms.cells, terms.capital.cents(), terms.months),
        (4, 24, 2)
    );
    assert_eq!(
        terms
            .zones
            .iter()
            .map(|z| (z.zone.0, z.cells))
            .collect::<Vec<_>>(),
        vec![(1, 1), (2, 3)]
    );
    // 1/2 + 3/3 = 1.5 -> 2, not ceil(1/2) + ceil(3/3) for all cases.
    let short = region
        .line_terms(CellCoord { x: 4, y: 4 }, CellCoord { x: 6, y: 4 }, 2.0)
        .unwrap();
    assert_eq!(short.months, 1);
}

#[test]
fn repeated_crossings_on_a_descending_diagonal_use_each_sample_once() {
    let mut parts = fixtures::economic_region_parts();
    // Alternating columns create repeated crossings, independent of either endpoint's zone.
    for (index, zone) in parts
        .economic_zones
        .as_mut()
        .unwrap()
        .cells
        .iter_mut()
        .enumerate()
    {
        let x = index % 12;
        *zone = EconomicZoneId(if x == 0 { 0 } else { 1 + (x % 2) as u32 });
    }
    let region = RegionDefinition::try_from(parts).unwrap();
    let a = CellCoord { x: 1, y: 7 };
    let b = CellCoord { x: 7, y: 1 };
    // ceil(sqrt(72)) = 9 samples; their x indices are 1,2,3,3,4,5,5,6,7.
    let quote = region
        .line_terms(a, b, crate::grid::distance_cells(a, b))
        .unwrap();
    assert_eq!(
        (quote.cells, quote.capital.cents(), quote.months),
        (9, 51, 4)
    );
    assert_eq!(
        quote
            .zones
            .iter()
            .map(|z| (z.zone.0, z.cells))
            .collect::<Vec<_>>(),
        vec![(1, 3), (2, 6)]
    );
    assert_eq!(
        quote,
        region
            .line_terms(b, a, crate::grid::distance_cells(b, a))
            .unwrap()
    );
}

#[test]
fn invalid_maps_profiles_rates_and_costs_are_rejected() {
    type EconomicEdit = Box<dyn Fn(&mut EconomicZones)>;
    let edits: Vec<EconomicEdit> = vec![
        Box::new(|e| e.zones[0].id = EconomicZoneId(0)),
        Box::new(|e| e.zones.push(e.zones[0].clone())),
        Box::new(|e| e.zones[0].name_key.clear()),
        Box::new(|e| e.cells.pop().map(|_| ()).unwrap()),
        Box::new(|e| e.cells[1] = EconomicZoneId(99)),
        Box::new(|e| e.cells[0] = EconomicZoneId(1)),
        Box::new(|e| {
            let p = e.zones[0].plants[0].clone();
            e.zones[0].plants.push(p);
        }),
        Box::new(|e| e.zones[0].plants[0].build_months = Some(0)),
        Box::new(|e| e.zones[0].plants[0].capital_cost = Some(Money::from_cents(-1))),
        Box::new(|e| e.zones[0].ac_lines.cells_per_month = Some(0)),
        Box::new(|e| e.zones[0].ac_lines.cost_per_cell = Some(Money::from_cents(-1))),
    ];
    for edit in edits {
        let mut parts = fixtures::economic_region_parts();
        edit(parts.economic_zones.as_mut().unwrap());
        assert!(RegionDefinition::try_from(parts).is_err());
    }
    let mut parts = fixtures::economic_region_parts();
    parts.lines.cells_per_month = 4_294_967_291;
    let zones = &mut parts.economic_zones.as_mut().unwrap().zones;
    zones[0].ac_lines.cells_per_month = Some(4_294_967_279);
    zones[1].ac_lines.cells_per_month = Some(4_294_967_231);
    assert!(RegionDefinition::try_from(parts).is_err());
}

proptest! {
    #[test]
    fn route_reversal_and_uniform_profile_identity(ax in 1u16..12, ay in 0u16..8, bx in 1u16..12, by in 0u16..8, speed in 1u32..50, cost in 0i64..100_000) {
        let a=CellCoord{x:ax,y:ay}; let b=CellCoord{x:bx,y:by};
        let length=crate::grid::distance_cells(a,b);
        let region=RegionDefinition::try_from(fixtures::economic_region_parts()).unwrap();
        prop_assert_eq!(region.line_terms(a,b,length).unwrap(),region.line_terms(b,a,length).unwrap());
        let mut parts=fixtures::economic_region_parts();
        parts.lines.cost_per_cell=Money::from_cents(cost); parts.lines.cells_per_month=speed;
        for z in &mut parts.economic_zones.as_mut().unwrap().zones { z.ac_lines=LineConstructionOverride::default(); }
        let region=RegionDefinition::try_from(parts.clone()).unwrap();
        parts.economic_zones=None;
        let base=RegionDefinition::try_from(parts.clone()).unwrap();
        parts.economic_zones=Some(EconomicZones{zones:vec![],cells:vec![EconomicZoneId(0);96]});
        let default=RegionDefinition::try_from(parts).unwrap();
        let resolved=region.line_terms(a,b,length).unwrap();
        let old=base.line_terms(a,b,length).unwrap();
        prop_assert_eq!((resolved.capital,resolved.months),(old.capital,old.months));
        prop_assert_eq!(default.line_terms(a,b,length).unwrap(),old);
    }
}

#[test]
fn partial_overrides_inherit_and_water_uses_the_regional_default() {
    let mut parts = fixtures::economic_region_parts();
    parts.economic_zones.as_mut().unwrap().zones[0]
        .plants
        .push(PlantConstructionOverride {
            kind: PlantKind::Solar,
            capital_cost: Some(Money::from_cents(1)),
            build_months: None,
        });
    parts.economic_zones.as_mut().unwrap().zones[1]
        .plants
        .push(PlantConstructionOverride {
            kind: PlantKind::Solar,
            capital_cost: None,
            build_months: Some(4),
        });
    let region = RegionDefinition::try_from(parts).unwrap();
    let west = region
        .plant_spec_at(PlantKind::Solar, CellCoord { x: 4, y: 4 })
        .unwrap();
    let east = region
        .plant_spec_at(PlantKind::Solar, CellCoord { x: 8, y: 4 })
        .unwrap();
    assert_eq!(
        (west.capital_cost.cents(), west.build_months),
        (1, region.spec(PlantKind::Solar).build_months)
    );
    assert_eq!(
        (east.capital_cost, east.build_months),
        (region.spec(PlantKind::Solar).capital_cost, 4)
    );
    let water = region
        .line_terms(CellCoord { x: 0, y: 0 }, CellCoord { x: 0, y: 4 }, 4.0)
        .unwrap();
    assert_eq!(
        (water.capital, water.months),
        (region.lines().cost_per_cell * 4, 2)
    );
    assert_eq!(water.zones[0].zone, EconomicZoneId(0));
}

#[test]
fn quote_overflow_and_invalid_orders_leave_state_and_ids_untouched() {
    use crate::session::{GameSession, GameSettings};
    use std::sync::Arc;
    let mut parts = fixtures::economic_region_parts();
    let economics = parts.economic_zones.as_mut().unwrap();
    economics.cells.fill(EconomicZoneId(0));
    for x in 1..12 {
        economics.cells[7 * 12 + x] = EconomicZoneId(1);
    }
    economics.zones[0].ac_lines.cost_per_cell = Some(Money::from_cents(i64::MAX));
    let mut session = GameSession::new(
        Arc::new(RegionDefinition::try_from(parts).unwrap()),
        7,
        GameSettings::default(),
    );
    let (a, _) = session
        .build_plant(PlantKind::Wind, CellCoord { x: 2, y: 7 })
        .unwrap();
    let (b, _) = session
        .build_plant(PlantKind::Wind, CellCoord { x: 10, y: 7 })
        .unwrap();
    let a = session
        .plant_views()
        .into_iter()
        .find(|p| p.id == a)
        .unwrap()
        .node;
    let b = session
        .plant_views()
        .into_iter()
        .find(|p| p.id == b)
        .unwrap()
        .node;
    let before = session.snapshot_state();
    assert_eq!(
        session.line_quote(a, b),
        Err(DomainError::ConstructionOverflow)
    );
    assert_eq!(
        session.build_line(a, b),
        Err(DomainError::ConstructionOverflow)
    );
    assert!(
        session
            .build_plant(PlantKind::Wind, CellCoord { x: 99, y: 0 })
            .is_err()
    );
    assert_eq!(session.snapshot_state(), before);
    let mut parts = fixtures::economic_region_parts();
    parts.start.month = crate::time::SimMonth::new(i32::MAX, 12).unwrap();
    let mut session = GameSession::new(
        Arc::new(RegionDefinition::try_from(parts).unwrap()),
        7,
        GameSettings::default(),
    );
    let before = session.snapshot_state();
    assert_eq!(
        session.build_plant(PlantKind::Wind, CellCoord { x: 4, y: 4 }),
        Err(DomainError::ConstructionOverflow)
    );
    assert_eq!(session.snapshot_state(), before);
}

proptest! {
    #[test]
    fn unknown_zone_assignments_are_rejected_with_the_offending_id(index in 0usize..96, id in 3u32..u32::MAX) {
        let mut parts=fixtures::economic_region_parts();
        parts.economic_zones.as_mut().unwrap().cells[index]=EconomicZoneId(id);
        prop_assert!(matches!(RegionDefinition::try_from(parts),Err(RegionError::InvalidEconomicZone{zone:Some(found),..}) if found==EconomicZoneId(id)), "unknown zone must fail with its ID");
    }
}
