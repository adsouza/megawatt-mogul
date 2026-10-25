//! Geographic bounds and civil clocks for a region.

use std::collections::BTreeSet;

use super::{RegionDefinition, RegionDefinitionParts, RegionError};
use crate::ids::{CellCoord, CityId};
use crate::time::SimMonth;

/// Equirectangular bounds in degrees, with row zero at the northern edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoBounds {
    pub west: f64,
    pub east: f64,
    pub south: f64,
    pub north: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ZoneId(pub u16);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeZone {
    pub id: ZoneId,
    pub name_key: Option<String>,
    pub standard_offset_minutes: i32,
    pub dst_months: [bool; 12],
}

impl TimeZone {
    /// Civil UTC offset, constant throughout the representative month.
    /// Region validation bounds the standard offset before a clock is used.
    pub fn offset_minutes(&self, month: SimMonth) -> i32 {
        self.standard_offset_minutes
            + if self.dst_months[usize::from(month.month() - 1)] {
                60
            } else {
                0
            }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionClock {
    pub system_zone: ZoneId,
    pub zones: Vec<TimeZone>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClimateId(pub u16);

/// Region metadata is separate from the persisted city definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CityLocale {
    pub zone: ZoneId,
    pub climate: ClimateId,
}

impl RegionDefinition {
    pub fn geo(&self) -> &GeoBounds {
        &self.geo
    }

    /// Cell centre as (latitude, longitude); the caller supplies a cell in this grid.
    pub fn cell_lat_lon(&self, cell: CellCoord) -> (f64, f64) {
        let lon = self.geo.west
            + (f64::from(cell.x) + 0.5) * (self.geo.east - self.geo.west)
                / f64::from(self.terrain.width());
        let lat = self.geo.north
            - (f64::from(cell.y) + 0.5) * (self.geo.north - self.geo.south)
                / f64::from(self.terrain.height());
        (lat, lon)
    }

    pub fn system_zone(&self) -> &TimeZone {
        self.zone(self.clock.system_zone)
    }

    /// Civil zone for a city belonging to this validated region.
    pub fn city_zone(&self, city: CityId) -> &TimeZone {
        let locale = self
            .city_locales
            .get(&city)
            .expect("city belongs to the region");
        self.zone(locale.zone)
    }

    /// Converts system-clock ticks to civil minutes for a city in this region.
    pub fn local_minute(&self, city: CityId, month: SimMonth, tick: u32) -> u32 {
        let delta =
            self.city_zone(city).offset_minutes(month) - self.system_zone().offset_minutes(month);
        // Reduce before the signed conversion so even u32::MAX cannot overflow.
        ((tick % 1440) as i32 + delta).rem_euclid(1440) as u32
    }

    fn zone(&self, id: ZoneId) -> &TimeZone {
        self.clock
            .zones
            .iter()
            .find(|zone| zone.id == id)
            .expect("validated clock has the referenced zone")
    }
}

pub(super) fn validate(parts: &RegionDefinitionParts) -> Result<(), RegionError> {
    let invalid = |field| RegionError::InvalidShape { field };
    let b = &parts.geo;
    if !(-180.0..=180.0).contains(&b.west)
        || !(-180.0..=180.0).contains(&b.east)
        || !(-90.0..=90.0).contains(&b.south)
        || !(-90.0..=90.0).contains(&b.north)
        || b.west >= b.east
        || b.south >= b.north
    {
        return Err(invalid("bounds"));
    }
    if parts.terrain.width() == 0 || parts.terrain.height() == 0 {
        return Err(invalid("grid"));
    }
    let mut zones = BTreeSet::new();
    for zone in &parts.clock.zones {
        if !zones.insert(zone.id) {
            return Err(invalid("time_zones.id"));
        }
        if !(-720..=840).contains(&zone.standard_offset_minutes)
            || zone.standard_offset_minutes % 15 != 0
        {
            return Err(invalid("time_zones.offset"));
        }
    }
    if !zones.contains(&parts.clock.system_zone) {
        return Err(invalid("system_zone"));
    }
    let cities: BTreeSet<_> = parts.cities.iter().map(|city| city.id).collect();
    if parts.city_locales.len() != cities.len()
        || cities
            .iter()
            .any(|city| !parts.city_locales.contains_key(city))
        || parts
            .city_locales
            .values()
            .any(|locale| !zones.contains(&locale.zone))
    {
        return Err(invalid("cities.zone"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{CellCoord, CityId};
    use crate::region::{
        RegionDefinition, RegionDefinitionParts, RegionError, TerrainGrid, fixtures,
    };
    use crate::time::SimMonth;
    use proptest::prelude::*;

    fn month(month: u8) -> SimMonth {
        SimMonth::new(2027, month).unwrap()
    }

    fn zone(id: u16, offset: i32, dst: [bool; 12]) -> TimeZone {
        TimeZone {
            id: ZoneId(id),
            name_key: None,
            standard_offset_minutes: offset,
            dst_months: dst,
        }
    }

    fn parts(zones: Vec<TimeZone>) -> RegionDefinitionParts {
        let mut parts = fixtures::tiny_region_parts();
        parts.clock = RegionClock {
            system_zone: zones[0].id,
            zones,
        };
        parts.city_locales = parts
            .cities
            .iter()
            .map(|c| {
                (
                    c.id,
                    CityLocale {
                        zone: parts.clock.system_zone,
                        climate: ClimateId(0),
                    },
                )
            })
            .collect();
        parts
    }

    #[test]
    fn offsets_follow_dst_months() {
        let cet = zone(
            0,
            60,
            [
                false, false, false, true, true, true, true, true, true, true, false, false,
            ],
        );
        assert_eq!(cet.offset_minutes(month(3)), 60);
        assert_eq!(cet.offset_minutes(month(4)), 120);
        assert_eq!(cet.offset_minutes(month(10)), 120);
        assert_eq!(cet.offset_minutes(month(11)), 60);
        let december = zone(0, -180, [true; 12]);
        assert_eq!(december.offset_minutes(month(12)), -120);
    }

    #[test]
    fn local_minute_wraps_both_ways() {
        let mut p = parts(vec![
            zone(0, 0, [false; 12]),
            zone(1, 330, [false; 12]),
            zone(2, -180, [false; 12]),
        ]);
        p.city_locales.get_mut(&CityId(1)).unwrap().zone = ZoneId(1);
        p.city_locales.get_mut(&CityId(2)).unwrap().zone = ZoneId(2);
        let r = RegionDefinition::try_from(p).unwrap();
        assert_eq!(r.local_minute(CityId(1), month(1), 0), 330);
        assert_eq!(r.local_minute(CityId(1), month(1), 1439), 329);
        assert_eq!(r.local_minute(CityId(2), month(1), 60), 1320);
        assert_eq!(r.system_zone().id, ZoneId(0));
        assert_eq!(r.city_zone(CityId(1)).standard_offset_minutes, 330);
        let mut p = parts(vec![
            zone(
                0,
                60,
                [
                    false, false, false, true, true, true, true, true, true, true, false, false,
                ],
            ),
            zone(1, 60, [false; 12]),
        ]);
        p.city_locales.get_mut(&CityId(1)).unwrap().zone = ZoneId(1);
        let r = RegionDefinition::try_from(p).unwrap();
        assert_eq!(r.local_minute(CityId(1), month(1), 30), 30);
        assert_eq!(r.local_minute(CityId(1), month(7), 30), 1410);
    }

    #[test]
    fn cell_centres_span_the_bounds() {
        let mut p = fixtures::tiny_region_parts();
        p.geo = GeoBounds {
            west: -10.0,
            east: 10.0,
            south: 0.0,
            north: 5.0,
        };
        let cell = p.terrain.get(CellCoord { x: 1, y: 0 }).unwrap().clone();
        p.terrain = TerrainGrid::new(10, 5, vec![cell; 50]).unwrap();
        for (i, city) in p.cities.iter_mut().enumerate() {
            city.cell = CellCoord { x: i as u16, y: 0 };
        }
        p.start.fleet.clear();
        let r = RegionDefinition::try_from(p).unwrap();
        assert_eq!(r.geo().west, -10.0);
        assert_eq!(r.cell_lat_lon(CellCoord { x: 0, y: 0 }), (4.5, -9.0));
        assert_eq!(r.cell_lat_lon(CellCoord { x: 9, y: 4 }), (0.5, 9.0));
    }

    fn rejects(edit: impl FnOnce(&mut RegionDefinitionParts), field: &'static str) {
        let mut p = fixtures::tiny_region_parts();
        edit(&mut p);
        assert_eq!(
            RegionDefinition::try_from(p).unwrap_err(),
            RegionError::InvalidShape { field }
        );
    }

    #[test]
    fn clock_rejects_invalid_offsets() {
        for offset in [1, -721, 841, -735, 855, i32::MIN, i32::MAX] {
            rejects(
                |p| p.clock.zones[0].standard_offset_minutes = offset,
                "time_zones.offset",
            );
        }
        for offset in [-720, 840] {
            let p = parts(vec![zone(0, offset, [true; 12])]);
            assert!(RegionDefinition::try_from(p).is_ok());
        }
    }

    #[test]
    fn clock_rejects_duplicate_zone_ids() {
        rejects(
            |p| p.clock.zones.push(p.clock.zones[0].clone()),
            "time_zones.id",
        );
    }

    #[test]
    fn clock_requires_a_known_system_zone() {
        rejects(|p| p.clock.system_zone = ZoneId(99), "system_zone");
        rejects(|p| p.clock.zones.clear(), "system_zone");
    }

    #[test]
    fn city_locales_require_every_city_and_known_zones() {
        rejects(
            |p| {
                p.city_locales.remove(&CityId(1));
            },
            "cities.zone",
        );
        rejects(
            |p| p.city_locales.get_mut(&CityId(1)).unwrap().zone = ZoneId(99),
            "cities.zone",
        );
        rejects(
            |p| {
                p.city_locales.insert(
                    CityId(99),
                    CityLocale {
                        zone: ZoneId(0),
                        climate: ClimateId(0),
                    },
                );
            },
            "cities.zone",
        );
    }

    #[test]
    fn geography_requires_finite_ordered_earth_coordinates() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -181.0, 181.0] {
            rejects(|p| p.geo.west = value, "bounds");
            rejects(|p| p.geo.east = value, "bounds");
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -91.0, 91.0] {
            rejects(|p| p.geo.south = value, "bounds");
            rejects(|p| p.geo.north = value, "bounds");
        }
        rejects(|p| p.geo.east = p.geo.west, "bounds");
        rejects(|p| p.geo.east = p.geo.west - 1.0, "bounds");
        rejects(|p| p.geo.north = p.geo.south, "bounds");
        rejects(|p| p.geo.north = p.geo.south - 1.0, "bounds");
        rejects(
            |p| p.terrain = TerrainGrid::new(0, 0, vec![]).unwrap(),
            "grid",
        );
        let mut p = fixtures::tiny_region_parts();
        p.geo = GeoBounds {
            west: -180.0,
            east: 180.0,
            south: -90.0,
            north: 90.0,
        };
        assert!(RegionDefinition::try_from(p).is_ok());
    }

    proptest! {
        #[test]
        fn local_minute_is_normalized_with_the_monthly_offset_difference(
            system_quarters in -48i32..=56, city_quarters in -48i32..=56,
            system_dst in any::<[bool; 12]>(), city_dst in any::<[bool; 12]>(),
            m in 1u8..=12, tick in any::<u32>(),
        ) {
            let mut p = parts(vec![zone(0, system_quarters * 15, system_dst), zone(1, city_quarters * 15, city_dst)]);
            p.city_locales.get_mut(&CityId(1)).unwrap().zone = ZoneId(1);
            let r = RegionDefinition::try_from(p).unwrap();
            let local = r.local_minute(CityId(1), month(m), tick);
            let delta = i64::from((city_quarters - system_quarters) * 15)
                + if city_dst[usize::from(m - 1)] { 60 } else { 0 }
                - if system_dst[usize::from(m - 1)] { 60 } else { 0 };
            prop_assert!(local < 1440);
            prop_assert_eq!((i64::from(local) - i64::from(tick)).rem_euclid(1440), delta.rem_euclid(1440));
        }
    }
}
