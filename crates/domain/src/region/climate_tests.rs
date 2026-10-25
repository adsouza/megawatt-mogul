use super::*;
use crate::time::{Season, SimMonth};
use proptest::prelude::*;
fn month(m: u8) -> SimMonth {
    SimMonth::new(2027, m).unwrap()
}
#[test]
fn run_lengths_cover_short_and_wrapping_seasons() {
    use Season::*;
    let india = Calendar {
        seasons: [
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
            PostMonsoon,
        ],
    };
    assert_eq!(india.run_length(month(1)), 2);
    assert_eq!(india.run_length(month(7)), 4);
    for m in [12, 1, 2] {
        assert_eq!(Calendar::northern_meteorological().run_length(month(m)), 3);
    }
    assert_eq!(
        Calendar {
            seasons: [Summer; 12]
        }
        .run_length(month(5)),
        12
    );
}
#[test]
fn a_southern_calendar_uses_summer_in_january() {
    use Season::*;
    let mut p = fixtures::tiny_region_parts();
    p.calendar = Calendar {
        seasons: [
            Summer, Summer, Autumn, Autumn, Autumn, Winter, Winter, Winter, Spring, Spring, Spring,
            Summer,
        ],
    };
    p.climates[0].month_shapes = [1, 1, 2, 2, 2, 0, 0, 0, 2, 2, 2, 1];
    p.climates[0].shapes = vec![[2.0; 24], [3.0; 24], [1.0; 24]];
    p.climates[0].monthly = [1.0; 12];
    let r = RegionDefinition::try_from(p).unwrap();
    assert_eq!(r.season(month(1)), Summer);
    assert_eq!(r.load_factor(CityId(1), month(1), 0), 3.0);
    assert_eq!(r.load_factor(CityId(1), month(7), 0), 2.0);
}
#[test]
fn cities_in_other_zones_peak_at_their_local_hour() {
    let mut p = fixtures::tiny_region_parts();
    p.clock.zones.push(TimeZone {
        id: ZoneId(1),
        name_key: None,
        standard_offset_minutes: 180,
        dst_months: [false; 12],
    });
    p.city_locales.get_mut(&CityId(2)).unwrap().zone = ZoneId(1);
    p.climates[0].shapes = vec![[0.5; 24]];
    p.climates[0].shapes[0][18] = 1.0;
    p.climates[0].month_shapes = [0; 12];
    p.climates[0].monthly = [1.0; 12];
    let r = RegionDefinition::try_from(p).unwrap();
    assert_eq!(r.load_factor(CityId(2), month(1), 15 * 60), 1.0);
    assert_eq!(r.load_factor(CityId(1), month(1), 15 * 60), 0.5);
    let cities: Vec<_> = r
        .cities()
        .iter()
        .map(|c| crate::demand::DemandCenter::new(c.clone(), crate::ids::NodeId(0)))
        .collect();
    assert_eq!(
        crate::demand::forecast_demand(&cities, &r, month(1), 15 * 60).value(),
        850.0
    );
    assert_eq!(
        crate::demand::hourly_forecast(&cities, &r, month(1))[15]
            .mean
            .value(),
        750.0
    );
}
#[test]
fn climates_reject_bad_shapes_references_ids_and_noise() {
    type Edit = fn(&mut RegionDefinitionParts);
    let edits: Vec<(Edit, &str)> = vec![
        (|p| p.climates.clear(), "cities.climate"),
        (|p| p.climates.push(p.climates[0].clone()), "climates.id"),
        (|p| p.climates[0].shapes.clear(), "climates.shapes"),
        (
            |p| p.climates[0].shapes = vec![[1.0; 24]; 5],
            "climates.shapes",
        ),
        (
            |p| p.climates[0].month_shapes[11] = 3,
            "climates.month_shapes",
        ),
        (|p| p.climates[0].shapes[0][0] = f64::NAN, "climates.shapes"),
        (|p| p.climates[0].monthly[0] = -0.1, "climates.monthly"),
        (
            |p| p.city_locales.get_mut(&CityId(1)).unwrap().climate = ClimateId(99),
            "cities.climate",
        ),
        (
            |p| p.demand_noise.error_persistence = 1.0,
            "demand.error_persistence",
        ),
    ];
    for (edit, field) in edits {
        let mut p = fixtures::tiny_region_parts();
        edit(&mut p);
        assert_eq!(
            RegionDefinition::try_from(p).unwrap_err(),
            RegionError::InvalidShape { field }
        );
    }
    for value in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut p = fixtures::tiny_region_parts();
        p.climates[0].shapes[1][23] = value;
        assert!(RegionDefinition::try_from(p).is_err());
        let mut p = fixtures::tiny_region_parts();
        p.climates[0].monthly[11] = value;
        assert!(RegionDefinition::try_from(p).is_err());
    }
    let mut p = fixtures::tiny_region_parts();
    p.climates[0].shapes[0] = [0.0; 24];
    p.climates[0].monthly = [0.0; 12];
    assert!(RegionDefinition::try_from(p).is_ok());
}
proptest! {
    #[test]
    fn cyclic_run_matches_connected_equal_months(labels in prop::array::uniform12(0u8..6), m in 0usize..12) {
        let variants=[Season::Winter,Season::Spring,Season::Summer,Season::Autumn,Season::Monsoon,Season::PostMonsoon];
        let c=Calendar { seasons: labels.map(|i| variants[usize::from(i)]) };
        let mut connected=std::collections::BTreeSet::from([m]);
        loop { let before=connected.len(); for i in connected.clone() { for j in [(i+1)%12,(i+11)%12] { if labels[j]==labels[m] { connected.insert(j); } } } if connected.len()==before { break; } }
        prop_assert_eq!(usize::from(c.run_length(month(m as u8+1))),connected.len());
    }
}

// Test-only copy of the pre-climate shape algorithm, independently selecting seasons.
fn old_shape_factor(
    winter: &[f64; 24],
    summer: &[f64; 24],
    shoulder: &[f64; 24],
    monthly: &[f64; 12],
    m: SimMonth,
    tick: u32,
) -> f64 {
    let hourly = match m.month() {
        12 | 1 | 2 => winter,
        6..=8 => summer,
        _ => shoulder,
    };
    let hour = (tick / 60 % 24) as usize;
    let next = (hour + 1) % 24;
    let fraction = f64::from(tick % 60) / 60.0;
    let interpolated = hourly[hour] + (hourly[next] - hourly[hour]) * fraction;
    interpolated * monthly[m.index0()]
}
#[test]
fn legacy_climate_is_bit_identical_to_the_old_shape() {
    let shoulder = [
        0.70, 0.66, 0.64, 0.63, 0.64, 0.68, 0.76, 0.85, 0.90, 0.92, 0.93, 0.94, 0.94, 0.94, 0.95,
        0.96, 0.98, 1.00, 0.99, 0.96, 0.91, 0.85, 0.78, 0.73,
    ];
    let mut winter = shoulder;
    let mut summer = shoulder;
    for h in 0..24 {
        if matches!(h,6..=9|17..=21) {
            winter[h] *= 1.1;
        }
        if matches!(h, 10..=19) {
            summer[h] *= 1.15;
        }
    }
    let monthly = [
        1.10, 1.05, 0.95, 0.90, 0.95, 1.05, 1.15, 1.15, 1.00, 0.92, 0.98, 1.08,
    ];
    let r = fixtures::tiny_region();
    for m in 1..=12 {
        for tick in 0..1440 {
            let expected = old_shape_factor(&winter, &summer, &shoulder, &monthly, month(m), tick);
            assert_eq!(
                r.load_factor(CityId(1), month(m), tick).to_bits(),
                expected.to_bits()
            );
            let cities: Vec<_> = r
                .cities()
                .iter()
                .map(|def| crate::demand::DemandCenter::new(def.clone(), crate::ids::NodeId(0)))
                .collect();
            let sum: crate::units::Megawatts =
                cities.iter().map(|c| c.def().base_load * expected).sum();
            assert_eq!(
                crate::demand::forecast_demand(&cities, &r, month(m), tick)
                    .value()
                    .to_bits(),
                sum.value().to_bits()
            );
        }
    }
}
