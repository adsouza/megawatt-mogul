use super::*;
use crate::error::DomainError;
use crate::generation::PlantStatus;
use crate::ids::CellCoord;
use crate::region::{PlantKind, RegionDefinition, StartPlantDef, fixtures};
use crate::units::{MegawattHours, Megawatts};

fn storage_session() -> (GameSession, PlantId) {
    let mut parts = fixtures::tiny_region_parts();
    parts.demand.forecast_error = Ratio::ZERO;
    parts.start.fleet.push(StartPlantDef {
        kind: PlantKind::Battery,
        cell: CellCoord { x: 5, y: 1 },
    });
    let s = GameSession::new(
        Arc::new(RegionDefinition::try_from(parts).unwrap()),
        7,
        GameSettings::default(),
    );
    let id = s
        .plant_views()
        .iter()
        .find(|p| p.kind == PlantKind::Battery)
        .unwrap()
        .id;
    (s, id)
}

proptest::proptest! {
    #[test]
    fn battery_supply_forecast_preserves_state_energy_and_warning_agreement(
        end_hour in 17usize..22,
        enabled in proptest::bool::ANY,
    ) {
        let (mut s, id) = storage_session();
        let mut hours = [false; 24];
        hours[..end_hour].fill(true);
        for plant in s.plant_views() {
            if matches!(plant.kind, PlantKind::Ccgt | PlantKind::Peaker) {
                s.commit_plant(plant.id, hours).unwrap();
            }
        }
        s.set_battery_enabled(id, enabled).unwrap();
        let before = s.state_hash();
        let points = s.battery_supply_forecast();
        proptest::prop_assert_eq!(s.state_hash(), before);
        proptest::prop_assert_eq!(&points, &s.battery_supply_forecast());
        if !enabled {
            proptest::prop_assert!(points.is_empty());
            return Ok(());
        }
        proptest::prop_assert_eq!(points.len(), crate::time::TICKS_PER_DAY as usize);
        let spec = s.region().spec(PlantKind::Battery).battery.unwrap();
        let power = s.region().spec(PlantKind::Battery).capacity.value();
        let mut stored = 0.0;
        let mut worst = (0.0, 0u8);
        for (tick, point) in points.iter().enumerate() {
            let charge = point.battery_charge.value();
            let discharge = point.battery_discharge.value();
            stored += (charge * spec.charge_efficiency.value()
                - discharge / spec.discharge_efficiency.value()) / 60.0;
            proptest::prop_assert!(stored >= -1e-6 && stored <= spec.energy_capacity.value() + 1e-6);
            proptest::prop_assert!(charge >= 0.0 && charge <= spec.charge_power.value());
            proptest::prop_assert!(discharge >= 0.0 && discharge <= power);
            proptest::prop_assert!(charge == 0.0 || discharge == 0.0);
            let generation: f64 = point.generation.values().map(|mw| mw.value()).sum();
            proptest::prop_assert!((point.net_supply().value() - (generation + discharge - charge)).abs() < 1e-6);
            if charge > 0.0 {
                proptest::prop_assert!(point.net_supply().value() + 1e-6 >= point.demand.value());
            }
            let shortfall = (point.demand - point.net_supply()).value();
            if shortfall > worst.0 { worst = (shortfall, (tick / 60) as u8); }
        }
        let warning = s.capacity_shortfall_check();
        if worst.0 > 0.0 {
            let warning = warning.unwrap();
            proptest::prop_assert!((warning.shortfall.value() - worst.0).abs() < 1e-6);
            proptest::prop_assert_eq!(warning.hour, worst.1);
        } else { proptest::prop_assert!(warning.is_none()); }
    }
}

#[test]
fn battery_commands_are_typed_and_atomic() {
    let (mut s, id) = storage_session();
    for action in 0..5 {
        let before = s.state_hash();
        let result = match action {
            0 => s.commit_plant(id, [true; 24]),
            1 => s.start_plant(id),
            2 => s.stop_plant(id),
            3 => s.set_dispatch_target(id, Megawatts::new(1.0)),
            _ => s.set_auto_follow(id, true),
        };
        assert_eq!(result, Err(DomainError::PlantNotDispatchable { plant: id }));
        assert_eq!(s.state_hash(), before);
    }
    let generator = s
        .plant_views()
        .iter()
        .find(|p| p.kind == PlantKind::Coal)
        .unwrap()
        .id;
    let before = s.state_hash();
    assert_eq!(
        s.set_battery_enabled(generator, false),
        Err(DomainError::PlantNotBattery { plant: generator })
    );
    assert_eq!(s.state_hash(), before);
    s.set_battery_enabled(id, false).unwrap();
    s.begin_operating().unwrap();
    assert!(!s.power.plant(id).unwrap().battery().unwrap().enabled());
}

#[test]
fn battery_cycle_charges_delivers_and_reports_then_clears_at_midnight() {
    let (mut s, id) = storage_session();
    s.begin_operating().unwrap();
    let sunrise = s.region.solar().daylight[s.clock.month().index0()].0 * 60.0;
    let mut closed = None;
    let mut daily_close = 0;
    for tick in 0..1440 {
        for event in s.tick().unwrap() {
            match event {
                DomainEvent::TickCompleted(summary) => {
                    assert!(
                        (summary.generation
                            - summary.served
                            - summary.battery_charge
                            - summary.imbalance)
                            .value()
                            .abs()
                            < 1e-7
                    );
                    if f64::from(tick) < sunrise {
                        assert_eq!(summary.battery_charge, Megawatts::ZERO);
                    }
                    if tick < 1020 {
                        assert_eq!(summary.battery_discharge, Megawatts::ZERO);
                    }
                    if tick >= 1020 {
                        assert_eq!(summary.battery_charge, Megawatts::ZERO);
                    }
                }
                DomainEvent::BatteryDayClosed { plant, .. } if plant == id => daily_close += 1,
                DomainEvent::MonthClosed(report) => closed = Some(report),
                _ => {}
            }
        }
        s.check_invariants().unwrap();
    }
    assert_eq!(daily_close, 1);
    let report = closed.unwrap();
    let storage = &report.storage[&id];
    assert!(storage.input.value() > 0.0);
    assert!(storage.output.value() > 0.0);
    assert!(
        (storage.input.value()
            - storage.output.value()
            - storage.charge_loss.value()
            - storage.discharge_loss.value()
            - storage.unused.value())
        .abs()
            < 1e-6
    );
    assert!(!report.merit_order.iter().any(|p| p.plant == id));
    assert_eq!(
        s.power.plant(id).unwrap().battery().unwrap().stored(),
        MegawattHours::ZERO
    );
    assert!(matches!(
        s.power.plant(id).unwrap().status(),
        PlantStatus::Online { .. }
    ));
    s.finish_review().unwrap();
    s.begin_operating().unwrap();
    assert_eq!(
        s.power.plant(id).unwrap().battery().unwrap().stored(),
        MegawattHours::ZERO
    );
}

#[test]
fn disabling_a_charged_battery_preserves_and_weights_the_midnight_remainder() {
    let (mut s, id) = storage_session();
    s.begin_operating().unwrap();
    for _ in 0..900 {
        s.tick().unwrap();
    }
    let stored = s.power.plant(id).unwrap().battery().unwrap().stored();
    assert!(stored.value() > 0.0);
    s.set_battery_enabled(id, false).unwrap();
    s.check_invariants().unwrap();
    // Several controls at the same accepted-command boundary must not invalidate the last tick.
    s.set_battery_enabled(id, true).unwrap();
    s.check_invariants().unwrap();
    s.set_battery_enabled(id, false).unwrap();
    s.check_invariants().unwrap();
    let mut report = None;
    while s.phase == DayPhase::Operating {
        for e in s.tick().unwrap() {
            if let DomainEvent::MonthClosed(r) = e {
                report = Some(r);
            }
        }
    }
    let r = report.unwrap();
    assert_eq!(r.storage[&id].unused, stored * f64::from(r.days));
    assert_eq!(
        s.power.plant(id).unwrap().battery().unwrap().stored(),
        MegawattHours::ZERO
    );
    s.finish_review().unwrap();
    s.begin_operating().unwrap();
    assert!(!s.power.plant(id).unwrap().battery().unwrap().enabled());
}

#[test]
fn battery_order_commissions_empty_in_two_months_and_uses_ordinary_instalments() {
    let (mut s, _) = storage_session();
    let capital = s.region.spec(PlantKind::Battery).capital_cost;
    let (id, _) = s
        .build_plant(PlantKind::Battery, CellCoord { x: 4, y: 1 })
        .unwrap();
    let before = s.state_hash();
    assert_eq!(
        s.set_battery_enabled(id, false),
        Err(DomainError::PlantNotCommissioned { plant: id })
    );
    assert_eq!(s.state_hash(), before);
    for month in 0..2 {
        s.begin_operating().unwrap();
        while s.phase == DayPhase::Operating {
            s.tick().unwrap();
        }
        if month == 0 {
            assert!(matches!(
                s.power.plant(id).unwrap().status(),
                PlantStatus::UnderConstruction { months_left: 1 }
            ));
        }
        s.finish_review().unwrap();
    }
    let b = s.power.plant(id).unwrap();
    assert_eq!(b.battery().unwrap().stored(), MegawattHours::ZERO);
    assert!(b.battery().unwrap().enabled());
    assert!(!matches!(b.status(), PlantStatus::UnderConstruction { .. }));
    let paid: Money = s
        .company
        .ledger()
        .iter()
        .filter(|e| {
            e.subject == Some(crate::company::AssetRef::Plant(id))
                && e.category == crate::company::LedgerCategory::Construction
        })
        .map(|e| e.amount)
        .sum();
    assert_eq!(paid, -capital);
    assert!(!s.power.network().is_connected(b.node()));
    assert_eq!(b.spec().capacity_credit, Ratio::ZERO);
}

#[test]
fn battery_monthly_weighting_includes_leap_february() {
    for (year, month, days) in [(2027, 2, 28), (2028, 2, 29), (2027, 4, 30), (2027, 1, 31)] {
        let mut parts = fixtures::tiny_region_parts();
        parts.start.month = crate::time::SimMonth::new(year, month).unwrap();
        parts.demand.forecast_error = Ratio::ZERO;
        parts.start.fleet.push(StartPlantDef {
            kind: PlantKind::Battery,
            cell: CellCoord { x: 5, y: 1 },
        });
        let mut s = GameSession::new(
            Arc::new(RegionDefinition::try_from(parts).unwrap()),
            1,
            GameSettings::default(),
        );
        let id = s
            .plant_views()
            .iter()
            .find(|p| p.kind == PlantKind::Battery)
            .unwrap()
            .id;
        s.begin_operating().unwrap();
        for _ in 0..900 {
            s.tick().unwrap();
        }
        let input = s.month_to_date.storage()[&id].input;
        s.set_battery_enabled(id, false).unwrap();
        let mut report = None;
        while s.phase == DayPhase::Operating {
            for e in s.tick().unwrap() {
                if let DomainEvent::MonthClosed(r) = e {
                    report = Some(r);
                }
            }
        }
        let r = report.unwrap();
        assert_eq!(r.days, days);
        assert_eq!(r.storage[&id].input, input * f64::from(days));
    }
}

#[test]
fn battery_peak_shaving_replaces_peaker_energy_without_extra_customer_revenue() {
    fn play(storage: bool) -> Box<MonthlyReport> {
        let mut p = fixtures::tiny_region_parts();
        p.demand.forecast_error = Ratio::ZERO;
        let mut shape = [0.5; 24];
        shape[17..24].fill(1.0);
        p.demand.winter = shape;
        p.demand.summer = shape;
        p.demand.shoulder = shape;
        p.demand.monthly = [1.0; 12];
        p.catalog
            .iter_mut()
            .find(|s| s.kind == PlantKind::Ccgt)
            .unwrap()
            .capacity = Megawatts::new(900.0);
        p.start.fleet = vec![
            StartPlantDef {
                kind: PlantKind::Ccgt,
                cell: CellCoord { x: 5, y: 1 },
            },
            StartPlantDef {
                kind: PlantKind::Peaker,
                cell: CellCoord { x: 5, y: 2 },
            },
        ];
        if storage {
            p.start.fleet.push(StartPlantDef {
                kind: PlantKind::Battery,
                cell: CellCoord { x: 5, y: 3 },
            });
        }
        let mut s = GameSession::new(
            Arc::new(RegionDefinition::try_from(p).unwrap()),
            7,
            GameSettings::default(),
        );
        s.begin_operating().unwrap();
        while s.phase == DayPhase::Operating {
            for e in s.tick().unwrap() {
                if let DomainEvent::MonthClosed(r) = e {
                    return r;
                }
                assert!(
                    !matches!(
                        e,
                        DomainEvent::Blackout | DomainEvent::OverFrequencyTrip { .. }
                    ),
                    "{e:?}"
                );
            }
        }
        unreachable!()
    }
    let without = play(false);
    let with = play(true);
    assert!(
        with.energy_by_kind[&PlantKind::Peaker] < without.energy_by_kind[&PlantKind::Peaker],
        "with {:?} without {:?} storage {:?}",
        with.energy_by_kind,
        without.energy_by_kind,
        with.storage
    );
    assert!((with.served.value() - without.served.value()).abs() < 1e-6);
    assert_eq!(with.revenue, without.revenue);
    assert!(
        with.storage
            .values()
            .any(|b| b.input.value() > 0.0 && b.output.value() > 0.0)
    );
    assert_eq!(with.unserved, MegawattHours::ZERO);
}
