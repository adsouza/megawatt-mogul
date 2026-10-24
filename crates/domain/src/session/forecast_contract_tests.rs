//! Independent physical oracles: chart/warning agreement alone can share a bug.
use super::*;
use crate::ids::LineId;
use crate::region::{PlantKind, RegionDefinitionParts, StartPlantDef, fixtures};
use crate::time::{SimMonth, TICKS_PER_DAY};
use crate::units::Megawatts;
use proptest::prelude::*;

fn make(parts: RegionDefinitionParts) -> GameSession {
    GameSession::new(
        Arc::new(RegionDefinition::try_from(parts).unwrap()),
        7,
        GameSettings::default(),
    )
}

fn disconnect(s: &mut GameSession, node: NodeId) {
    let old = s.power.network();
    let mut network = Network::new(s.anchor, *old.node(s.anchor).unwrap());
    for (id, definition) in old.nodes().filter(|(id, _)| *id != s.anchor) {
        network.add_node(id, *definition).unwrap();
    }
    for line in old
        .lines()
        .filter(|line| line.from != node && line.to != node)
    {
        network
            .add_line(line.id, line.from, line.to, line.status)
            .unwrap();
    }
    *s.power.network_mut() = network;
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn thermal_forecast_matches_independent_capacity_and_target_arithmetic(
        start in 0usize..24, duration in 1usize..=20,
        fraction in 0u16..=100, agc in any::<bool>(), isolated in any::<bool>(),
    ) {
        let mut s = make(fixtures::tiny_region_parts());
        for view in s.plant_views() {
            s.commit_plant(view.id, [false; 24]).unwrap();
        }
        let ccgt = s.plant_views().into_iter().find(|p| p.kind == PlantKind::Ccgt).unwrap();
        // At least four hours off: enough for this CCGT's shutdown plus startup.
        let mut hours = [false; 24];
        for i in 0..duration { hours[(start + i) % 24] = true; }
        s.commit_plant(ccgt.id, hours).unwrap();
        let spec = s.region.spec(PlantKind::Ccgt);
        let capacity = spec.capacity.value();
        let target = spec.min_output() + (spec.capacity - spec.min_output()) * (f64::from(fraction) / 100.0);
        s.set_dispatch_target(ccgt.id, target).unwrap();
        s.set_auto_follow(ccgt.id, agc).unwrap();
        if isolated { disconnect(&mut s, ccgt.node); }

        // An unfinished plant with an already-energised connection must still count as zero.
        let (unfinished, _) = s.build_plant(PlantKind::Peaker, CellCoord { x: 4, y: 1 }).unwrap();
        let node = s.power.plant(unfinished).unwrap().node();
        s.power.network_mut().add_line(LineId(u32::MAX), s.anchor, node, LineStatus::InService).unwrap();
        let before = s.state_hash();
        let forecast = s.supply_forecast();
        prop_assert_eq!(s.state_hash(), before);
        prop_assert_eq!(forecast.minutes.len(), TICKS_PER_DAY as usize);
        let mut worst = (0.0, 0u8);
        for (tick, point) in forecast.minutes.iter().enumerate() {
            let expected = if !isolated && hours[tick / 60] { if agc { capacity } else { target.value() } } else { 0.0 };
            prop_assert!((point.net_supply().value() - expected).abs() < 1e-6);
            prop_assert!(point.generation.get(&PlantKind::Peaker).is_none_or(|mw| *mw == Megawatts::ZERO));
            // The non-storage warning intentionally assesses the two hour endpoints.
            if tick % 60 == 0 || tick % 60 == 59 {
                let shortfall = point.demand.value() - expected;
                if shortfall > worst.0 { worst = (shortfall, (tick / 60) as u8); }
            }
        }
        match s.capacity_shortfall_check() {
            Some(w) => { prop_assert!((w.shortfall.value() - worst.0).abs() < 1e-6); prop_assert_eq!(w.hour, worst.1); }
            None => prop_assert_eq!(worst.0, 0.0),
        }
    }

    #[test]
    fn mixed_resource_forecast_cannot_create_water_or_stored_energy(
        month in 1u8..=12, flow in 1u16..=150, count in 1usize..=3,
        hydro_on in any::<bool>(),
    ) {
        // Every case covers enabled, disabled, disconnected AND unfinished storage.
        for availability in 0u8..4 {
            let mut parts = fixtures::tiny_region_parts();
            parts.start.month = SimMonth::new(2027, month).unwrap();
            parts.hydro.monthly_flow = [f64::from(flow) / 100.0; 12];
            parts.start.fleet.extend([
                StartPlantDef { kind: PlantKind::Hydro, cell: CellCoord { x: 10, y: 1 } },
                StartPlantDef { kind: PlantKind::Solar, cell: CellCoord { x: 6, y: 1 } },
                StartPlantDef { kind: PlantKind::Wind, cell: CellCoord { x: 7, y: 1 } },
            ]);
            for y in 0..count {
                parts.start.fleet.push(StartPlantDef { kind: PlantKind::Battery, cell: CellCoord { x: 4, y: y as u16 } });
            }
            let mut s = make(parts);
            let views = s.plant_views();
            let hydro = views.iter().find(|p| p.kind == PlantKind::Hydro).unwrap();
            s.commit_plant(hydro.id, [hydro_on; 24]).unwrap();
            // Remove evening thermal capacity to exercise feasible battery discharge.
            for p in views.iter().filter(|p| matches!(p.kind, PlantKind::Ccgt | PlantKind::Peaker)) {
                let mut hours = [true; 24]; hours[17..].fill(false);
                s.commit_plant(p.id, hours).unwrap();
            }
            for p in views.iter().filter(|p| p.kind == PlantKind::Battery) {
                match availability {
                    0 => {}, // Enabled, connected and commissioned.
                    1 => { s.set_battery_enabled(p.id, false).unwrap(); },
                    2 => disconnect(&mut s, p.node),
                    _ => {
                        let old = s.power.plant(p.id).unwrap();
                        let plant = Plant::new_under_construction(p.id, p.node, p.cell, old.spec().clone());
                        s.power.insert_plant(plant);
                    },
                }
            }
            let before = s.state_hash();
            let preview = s.supply_forecast();
            prop_assert_eq!(s.state_hash(), before);
            prop_assert_eq!(preview.has_storage, availability == 0);
            let battery = s.region.spec(PlantKind::Battery);
            let limits = battery.battery.unwrap();
            let mut stored = 0.0;
            let mut hydro_used = 0.0;
            let mut charged = 0.0;
            let mut discharged = 0.0;
            let (rise, set) = s.region.solar().daylight[month as usize - 1];
            let solar_capacity = s.region.spec(PlantKind::Solar).capacity.value();
            let irradiance = s.region.terrain().get(CellCoord { x: 6, y: 1 }).unwrap().irradiance.value();
            let clearness = s.region.solar().clearness_mean[month as usize - 1];
            for (tick, point) in preview.minutes.iter().enumerate() {
                let hour = tick as f64 / 60.0;
                let sun = if hour <= rise || hour >= set { 0.0 } else { (std::f64::consts::PI * (hour - rise) / (set - rise)).sin() };
                prop_assert!((point.generation[&PlantKind::Solar].value() - solar_capacity * irradiance * sun * clearness).abs() < 1e-6);
                let expected_wind = s.region.spec(PlantKind::Wind).capacity.value()
                    * s.region.terrain().get(CellCoord { x: 7, y: 1 }).unwrap().wind_potential.value()
                    * s.region.wind().monthly[month as usize - 1];
                prop_assert!((point.generation[&PlantKind::Wind].value() - expected_wind).abs() < 1e-6);
                if tick >= usize::from(limits.charge_end_hour) * 60 { prop_assert_eq!(point.battery_charge, Megawatts::ZERO); }
                let water_mw = point.generation.get(&PlantKind::Hydro).copied().unwrap_or(Megawatts::ZERO).value();
                hydro_used += water_mw / 60.0;
                prop_assert!(water_mw >= 0.0 && water_mw <= s.region.spec(PlantKind::Hydro).capacity.value() + 1e-6);
                prop_assert!(hydro_used <= hydro.hydro_day_budget.unwrap().value() + 1e-6);
                if !hydro_on { prop_assert_eq!(water_mw, 0.0); }
                let charge = point.battery_charge.value();
                let discharge = point.battery_discharge.value();
                charged += charge / 60.0;
                discharged += discharge / 60.0;
                stored += (charge * limits.charge_efficiency.value() - discharge / limits.discharge_efficiency.value()) / 60.0;
                prop_assert!(stored >= -1e-6 && stored <= count as f64 * limits.energy_capacity.value() + 1e-6);
                prop_assert!(charge >= 0.0 && charge <= count as f64 * limits.charge_power.value() + 1e-6);
                prop_assert!(discharge >= 0.0 && discharge <= count as f64 * battery.capacity.value() + 1e-6);
                prop_assert!(charge == 0.0 || discharge == 0.0);
                if availability != 0 { prop_assert_eq!((charge, discharge), (0.0, 0.0)); }
                if tick < usize::from(limits.discharge_start_hour) * 60 { prop_assert_eq!(discharge, 0.0); }
                let generation: f64 = point.generation.values().map(|mw| mw.value()).sum();
                prop_assert!((point.net_supply().value() - (generation + discharge - charge)).abs() < 1e-6);
                if charge > 0.0 { prop_assert!(generation - charge + 1e-6 >= point.demand.value()); }
            }
            prop_assert!(discharged <= charged * limits.charge_efficiency.value() * limits.discharge_efficiency.value() + 1e-6);
        }
    }

    #[test]
    fn balanced_noise_free_manual_forecast_matches_every_operating_minute(fraction in 0u16..=100) {
        let mut parts = fixtures::tiny_region_parts();
        let spec = parts.catalog.iter().find(|p| p.kind == PlantKind::Ccgt).unwrap();
        let target = spec.min_output() + (spec.capacity - spec.min_output()) * (f64::from(fraction) / 100.0);
        parts.start.fleet.retain(|p| p.kind == PlantKind::Ccgt);
        parts.demand.forecast_error = Ratio::ZERO;
        parts.demand.winter = [1.0; 24]; parts.demand.summer = [1.0; 24]; parts.demand.shoulder = [1.0; 24]; parts.demand.monthly = [1.0; 12];
        let customers = parts.start.connected_cities.len() as f64;
        for city in &mut parts.cities { city.base_load = target * (1.0 / customers); }
        let mut s = make(parts);
        let id = s.plant_views()[0].id;
        s.set_dispatch_target(id, target).unwrap(); s.set_auto_follow(id, false).unwrap();
        let forecast = s.supply_forecast();
        prop_assert!(s.capacity_shortfall_check().is_none());
        s.begin_operating().unwrap();
        for point in forecast.minutes {
            let events = s.tick().unwrap();
            let actual = events.iter().find_map(|e| if let DomainEvent::TickCompleted(t) = e { Some(t) } else { None }).unwrap();
            prop_assert!((actual.generation - point.net_supply()).abs().value() < 1e-6);
            prop_assert!((actual.demand - point.demand).abs().value() < 1e-6);
            prop_assert_eq!(actual.unserved, Megawatts::ZERO);
            prop_assert!(!events.contains(&DomainEvent::Blackout));
        }
        prop_assert_eq!(s.phase(), DayPhase::Review);
        prop_assert_eq!(s.check_invariants(), Ok(()));
        prop_assert!(s.power.plant(id).unwrap().is_online());
    }
}
