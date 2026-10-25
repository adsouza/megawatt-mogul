//! Cached planning inputs must retain the original per-minute arithmetic and ID order.
use super::super::battery::{BatteryPlanningInputs, charging_window};
use super::*;
use crate::generation::fill_minutes;
use crate::state::State;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]
    #[test]
    fn cached_battery_inputs_match_uncached_forecasts_bit_for_bit(
        seed in any::<u64>(), month in 1u8..=12, from in 0u32..1440,
        preview in any::<bool>(), manual in any::<bool>(), enabled in any::<bool>(), mode in 0u8..5, inventory_override in any::<bool>(),
    ) {
        let region = tiny_region();
        let mut sys = committed_auto_follow(&region, &[
            connected(PlantKind::Ccgt, cell(5, 1)),
            connected(PlantKind::Hydro, cell(5, 2)),
            connected(PlantKind::Solar, cell(5, 3)),
            connected(PlantKind::Wind, cell(5, 4)),
            connected(PlantKind::Battery, cell(5, 5)),
            PlantDef { kind: PlantKind::Solar, cell: cell(6, 1), connected: false },
            PlantDef { kind: PlantKind::Wind, cell: cell(6, 2), connected: false },
            connected(PlantKind::Battery, cell(6, 3)),
            connected(PlantKind::Solar, cell(6, 4)),
            connected(PlantKind::Wind, cell(6, 5)),
            connected(PlantKind::Coal, cell(7, 1)),
            connected(PlantKind::Solar, cell(7, 2)),
            connected(PlantKind::Wind, cell(7, 3)),
        ]);
        let month = SimMonth::new(2027, month).unwrap();
        let mut rng = SimRng::from_seed(seed);
        sys.begin_day(&region, month, &mut rng);
        if manual { place(&mut sys, 1, 170.0, Some(230.0)); }
        let p = sys.plant_mut(PlantId(11)).unwrap();
        match mode {
            0 => { p.trip(); }
            1 => { p.trip(); p.start().unwrap(); }
            2 => { p.stop().unwrap(); }
            3 => { p.suppress_commitment_for_day(); }
            _ => { p.set_commitment(CommitmentSchedule::none()); }
        }
        charge_inventory(&mut sys, PlantId(8), 30);
        sys.plant_mut(PlantId(5)).unwrap().battery_mut().unwrap().set_enabled(enabled);
        let inventory: BTreeMap<_, _> = sys.connected_plants().filter_map(|p| p.battery().map(|b| (p.id(), b.clone()))).collect();
        let inventory = inventory_override.then_some(&inventory);
        let before = sys.state_bytes();
        let inputs = BatteryPlanningInputs::new(&sys, &region, month);
        for tick in 0..TICKS_PER_DAY {
            let point = inputs.minute(tick);
            prop_assert_eq!(point.demand.value().to_bits(), sys.connected_demand_forecast(&region, month, tick).value().to_bits());
            prop_assert_eq!(point.solar.value().to_bits(), sys.expected_solar(&region, month, tick).value().to_bits());
            prop_assert_eq!(point.wind.value().to_bits(), sys.wind_forecast(&region, month, tick).0.value().to_bits());
            prop_assert_eq!(inputs.thermal_for_charge(tick, from, preview).value().to_bits(), legacy_thermal_for_charge(&sys, tick, from, preview).value().to_bits());
            prop_assert!(std::ptr::eq(point, inputs.minute(tick)), "a minute is computed only once per plan");
        }
        let actual = sys.charge_plan_with_inventory(&inputs, from, preview, inventory);
        let expected = legacy_charge_plan(&sys, &region, month, from, preview, inventory);
        prop_assert_eq!(actual.state_bytes(), expected.state_bytes(), "full charge plans retain exact float bits");
        let mut collected = actual.clone();
        let points = sys.project_storage_supply_with_inputs(&inputs, from, TICKS_PER_DAY, preview, &mut collected, true);
        let mut schedule_only = actual;
        let discarded = sys.project_storage_supply_with_inputs(&inputs, from, TICKS_PER_DAY, preview, &mut schedule_only, false);
        prop_assert!(discarded.is_empty());
        prop_assert_eq!(points.len(), (TICKS_PER_DAY - from) as usize);
        prop_assert_eq!(collected.state_bytes(), schedule_only.state_bytes(), "omitting forecast points changes no schedule bits");
        prop_assert_eq!(before, sys.state_bytes());
    }
}

// Frozen pre-optimization thermal calculation, including the live ramp and commitment rules.
fn legacy_thermal_for_charge(sys: &PowerSystem, tick: u32, from: u32, preview: bool) -> Megawatts {
    sys.connected_plants()
        .filter(|p| p.kind().is_dispatchable() && p.kind() != PlantKind::Hydro)
        .map(|p| {
            let committed = p.commitment().is_committed((tick / TICKS_PER_HOUR) as u8);
            if preview {
                return if committed {
                    if p.auto_follow() {
                        p.spec().capacity
                    } else {
                        p.initial_target()
                    }
                } else {
                    Megawatts::ZERO
                };
            }
            if !committed && !p.is_commitment_suppressed() {
                return Megawatts::ZERO;
            }
            match p.status() {
                PlantStatus::Online { output, target } => {
                    if p.auto_follow() {
                        p.spec()
                            .capacity
                            .min(output + p.spec().ramp.over_ticks(tick.saturating_sub(from)))
                    } else {
                        target.min(output + p.spec().ramp.over_ticks(tick.saturating_sub(from)))
                    }
                }
                PlantStatus::Starting { ticks_left } if tick.saturating_sub(from) >= ticks_left => {
                    p.spec().capacity.min(
                        p.spec().min_output() + p.spec().ramp.over_ticks(tick - from - ticks_left),
                    )
                }
                _ => Megawatts::ZERO,
            }
        })
        .sum()
}

// Frozen charge planner: compare allocations, including inventory overrides and zero-cap minutes.
fn legacy_charge_plan(
    sys: &PowerSystem,
    region: &RegionDefinition,
    month: SimMonth,
    from: u32,
    preview: bool,
    inventory: Option<&BTreeMap<PlantId, crate::generation::BatteryState>>,
) -> BatteryScheduleView {
    if !sys.connected_plants().any(|p| p.battery().is_some()) {
        return BatteryScheduleView::default();
    }
    let has_room = sys.connected_plants().any(|p| {
        p.battery()
            .zip(p.spec().battery)
            .is_some_and(|(state, spec)| {
                let stored = inventory.and_then(|v| v.get(&p.id())).map_or_else(
                    || if preview { 0.0 } else { state.stored().value() },
                    |s| s.stored().value(),
                );
                state.enabled()
                    && from < u32::from(spec.charge_end_hour) * TICKS_PER_HOUR
                    && stored < spec.energy_capacity.value()
            })
    });
    if !has_room {
        return BatteryScheduleView {
            expected_charging: preview,
            plants: sys
                .connected_plants()
                .filter(|p| p.battery().is_some())
                .map(|p| {
                    (
                        p.id(),
                        PlantBatteryScheduleView {
                            charge: vec![Megawatts::ZERO; TICKS_PER_DAY as usize],
                            discharge: vec![Megawatts::ZERO; TICKS_PER_DAY as usize],
                        },
                    )
                })
                .collect(),
        };
    }
    let n = TICKS_PER_DAY as usize;
    let mut charge_residual: Vec<_> = (0..TICKS_PER_DAY)
        .map(|t| {
            sys.connected_demand_forecast(region, month, t)
                - sys.expected_variable(region, month, t)
        })
        .collect();
    let mut headroom: Vec<_> = (0..TICKS_PER_DAY)
        .map(|t| {
            (legacy_thermal_for_charge(sys, t, from, preview) - charge_residual[t as usize])
                .max(Megawatts::ZERO)
        })
        .collect();
    let mut result = BatteryScheduleView {
        expected_charging: preview,
        ..Default::default()
    };
    for p in sys.connected_plants() {
        let (Some(spec), Some(state)) = (p.spec().battery, p.battery()) else {
            continue;
        };
        let enabled = state.enabled();
        let stored = inventory
            .and_then(|values| values.get(&p.id()))
            .map_or_else(
                || if preview { 0.0 } else { state.stored().value() },
                |s| s.stored().value(),
            );
        let charge_caps: Vec<_> = (0..TICKS_PER_DAY)
            .map(|t| {
                if enabled && t >= from && charging_window(region, month, spec, t) {
                    headroom[t as usize].min(spec.charge_power)
                } else {
                    Megawatts::ZERO
                }
            })
            .collect();
        let charge = fill_minutes(
            &charge_residual,
            &charge_caps,
            MegawattHours::new(
                (spec.energy_capacity.value() - stored).max(0.0) / spec.charge_efficiency.value(),
            ),
            true,
        );
        for t in 0..n {
            charge_residual[t] += charge[t];
            headroom[t] = (headroom[t] - charge[t]).max(Megawatts::ZERO);
        }
        result.plants.insert(
            p.id(),
            PlantBatteryScheduleView {
                charge,
                discharge: vec![Megawatts::ZERO; n],
            },
        );
    }
    result
}
