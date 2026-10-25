//! Wind and storage contracts use hand-computed physical expectations.
use super::*;

fn wind_region() -> RegionDefinition {
    region_with(|p| {
        noise_free(p);
        p.wind.monthly = [0.5; 12];
        p.wind.initial_sd = 0.0;
        p.wind.innovation_sd = 0.0;
        p.climates[0].shapes[0] = [1.0; 24];
        p.climates[0].shapes[1] = [1.0; 24];
        p.climates[0].shapes[2] = [1.0; 24];
        p.climates[0].monthly = [1.0; 12];
        for city in &mut p.cities {
            city.base_load = Megawatts::new(300.0);
        }
    })
}

#[test]
fn wind_is_real_generation_at_midnight_noon_and_night() {
    let region = wind_region();
    let mut sys = committed_auto_follow(
        &region,
        &[
            connected(PlantKind::Ccgt, cell(5, 1)),
            connected(PlantKind::Wind, cell(5, 2)),
        ],
    );
    let mut rng = SimRng::from_seed(23);
    sys.begin_day(&region, april(), &mut rng);
    for tick in 0..1440 {
        let (summary, events) = sys.tick(&region, april(), tick, &mut rng);
        assert_eq!(
            summary.by_kind[&PlantKind::Wind],
            Megawatts::new(120.0),
            "minute {tick}"
        );
        assert_eq!(
            summary.unserved,
            Megawatts::ZERO,
            "minute {tick}: {events:?}"
        );
        assert!((summary.generation.value() - 600.0).abs() < 1e-7);
    }
}

#[test]
fn shared_wind_does_not_draw_per_farm_or_during_queries() {
    let region = wind_region();
    let mut one = committed_auto_follow(
        &region,
        &[
            connected(PlantKind::Ccgt, cell(5, 1)),
            connected(PlantKind::Wind, cell(5, 2)),
        ],
    );
    let mut two = committed_auto_follow(
        &region,
        &[
            connected(PlantKind::Ccgt, cell(5, 1)),
            connected(PlantKind::Wind, cell(5, 2)),
            connected(PlantKind::Wind, cell(5, 3)),
        ],
    );
    let (mut a, mut b) = (SimRng::from_seed(2), SimRng::from_seed(2));
    one.begin_day(&region, april(), &mut a);
    two.begin_day(&region, april(), &mut b);
    let mut expected = SimRng::from_seed(2);
    draw_clearness(region.solar(), april(), &mut expected);
    expected.standard_normal();
    assert_eq!(
        a, expected,
        "solar then one initial wind draw, even at zero variance"
    );
    for tick in 0..120 {
        one.tick(&region, april(), tick, &mut a);
        two.tick(&region, april(), tick, &mut b);
        assert_eq!(a, b);
        assert_eq!(one.wind_factor(), two.wind_factor());
        let before = one.clone();
        let forecast = one.supply_forecast(&region, april());
        assert!(forecast.assess_every_minute && !forecast.has_storage);
        assert_eq!(
            forecast.minutes[0].generation[&PlantKind::Wind],
            Megawatts::new(120.0)
        );
        assert_eq!(one, before);
        let band = two.wind_forecast(&region, april(), tick);
        assert_eq!(band.0, Megawatts::new(240.0));
        assert_eq!(band.1, Megawatts::new(120.0));
        assert_eq!(band.2, Megawatts::new(360.0));
    }
}

#[test]
fn curtailment_is_proportional_across_solar_and_wind_and_never_counts_calm_wind() {
    let region = wind_region();
    let mut sys = system_with(
        &region,
        &[
            connected(PlantKind::Solar, cell(5, 1)),
            connected(PlantKind::Wind, cell(5, 2)),
        ],
    );
    let mut rng = SimRng::from_seed(4);
    sys.begin_day(&region, april(), &mut rng);
    let tick = 762;
    sys.curtailment = Megawatts::ZERO;
    let (raw, total, _) = sys.availability(&region, april(), tick);
    sys.curtailment = total * 0.25;
    let (limited, _, withheld) = sys.availability(&region, april(), tick);
    for id in [PlantId(1), PlantId(2)] {
        assert!((limited[&id] - raw[&id] * 0.75).value().abs() < 1e-7);
    }
    let split = sys.withheld_by_kind(&region, april(), tick, withheld);
    assert!(
        (split.values().copied().sum::<Megawatts>() - withheld)
            .value()
            .abs()
            < 1e-7
    );
    sys.wind_factor = 0.0;
    let (available, _, _) = sys.availability(&region, april(), tick);
    assert_eq!(available[&PlantId(2)], Megawatts::ZERO);
    assert_eq!(
        sys.withheld_by_kind(&region, april(), tick, withheld)[&PlantKind::Wind],
        Megawatts::ZERO
    );
}

#[test]
fn overlap_can_discharge_then_recharge_but_never_after_eighteen() {
    let (mut sys, region, id) = battery_fixture();
    sys.begin_day(&region, april(), &mut SimRng::from_seed(7));
    charge_inventory(&mut sys, id, 20);
    let p = sys.primary_supply();
    let deficit = p + Megawatts::new(100.0);
    let flow = sys.propose_battery_flows(&region, april(), 960, deficit, deficit);
    assert_eq!(flow[&id].discharge, Megawatts::new(100.0));
    sys.settle_batteries(&flow);
    let surplus_demand = (p - Megawatts::new(50.0)).max(Megawatts::ZERO);
    for tick in [0, 961, 1020, 1079] {
        let flow =
            sys.propose_battery_flows(&region, april(), tick, surplus_demand, surplus_demand);
        assert!(flow[&id].charge > Megawatts::ZERO);
        assert_eq!(flow[&id].discharge, Megawatts::ZERO);
    }
    let flow = sys.propose_battery_flows(&region, april(), 1080, surplus_demand, surplus_demand);
    assert_eq!(flow[&id].charge, Megawatts::ZERO);
}

#[test]
fn unexpected_wind_lull_has_only_finite_battery_cover_and_real_outages() {
    let region = wind_region();
    let mut sys = committed_auto_follow(
        &region,
        &[
            connected(PlantKind::Ccgt, cell(5, 1)),
            connected(PlantKind::Wind, cell(5, 2)),
            connected(PlantKind::Battery, cell(5, 3)),
        ],
    );
    let mut rng = SimRng::from_seed(7);
    sys.begin_day(&region, april(), &mut rng);
    charge_inventory(&mut sys, PlantId(3), 3); // 9 internal MWh.
    sys.wind_factor = 0.0;
    let mut delivered = 0.0;
    let mut unserved = 0.0;
    for tick in 1020..1080 {
        let (s, _) = sys.tick(&region, april(), tick, &mut rng);
        assert_eq!(
            s.battery_charge,
            Megawatts::ZERO,
            "charging must stop before any shedding"
        );
        delivered += s.battery_discharge.value() / 60.0;
        unserved += s.unserved.value() / 60.0;
        assert!(delivered <= 9.0 + 1e-7);
        assert!(
            (s.generation - s.served - s.battery_charge - s.imbalance)
                .value()
                .abs()
                < 1e-7
        );
    }
    assert!(delivered > 0.0 && unserved > 0.0);
}

/// A fleet that ramps 1,000 MW per tick, so every over-frequency trip fits the
/// rest of the fleet's headroom, with a steady regional wind factor.
fn fast_wind_region() -> RegionDefinition {
    region_with(|p| {
        for spec in &mut p.catalog {
            if spec.kind.is_dispatchable() {
                spec.ramp = crate::units::RampRate::mw_per_minute(1_000.0);
            }
        }
        p.wind.initial_sd = 0.0;
        p.wind.innovation_sd = 0.0;
    })
}

/// The overbuild fleet's deviations over a day, optionally with one wind farm
/// whose factor starts `gap` above its monthly mean and decays towards it.
fn overbuild_day(region: &RegionDefinition, gap: Option<f64>) -> Vec<Hertz> {
    let mut defs = start_fleet(region);
    defs.push(connected(PlantKind::Ccgt, cell(4, 6)));
    defs.push(connected(PlantKind::Ccgt, cell(5, 6)));
    if gap.is_some() {
        defs.push(connected(PlantKind::Wind, cell(6, 6)));
    }
    let mut sys = committed_auto_follow(region, &defs);
    let mut rng = SimRng::from_seed(1);
    sys.begin_day(region, april(), &mut rng);
    if let Some(gap) = gap {
        sys.wind_factor = region.wind().monthly[april().index0()] + gap;
    }
    (0..TICKS_PER_DAY)
        .map(|t| sys.tick(region, april(), t, &mut rng).0.frequency - region.frequency().nominal)
        .collect()
}

#[test]
fn wind_above_its_mean_does_not_block_safe_over_frequency_trips() {
    // Above its mean the wind forecast always drifts down. That drift is a few
    // MW an hour, well within a fleet that ramps 1,000 MW a tick, so the
    // surplus still clears within two hours and stays clear, with or without wind.
    let region = fast_wind_region();
    let p = region.frequency();
    for (label, gap) in [("no wind", None), ("wind above its mean", Some(0.3))] {
        let devs = overbuild_day(&region, gap);
        for (t, dev) in devs.iter().enumerate().skip(2 * TICKS_PER_HOUR as usize) {
            assert!(*dev <= p.curtail_above, "{label}, tick {t}: {dev:?}");
        }
    }
}

#[test]
fn the_foreseen_wind_fall_is_the_central_forecast_drop_to_the_hour_end() {
    let region = fast_wind_region();
    let mut sys = committed_auto_follow(
        &region,
        &[
            connected(PlantKind::Ccgt, cell(5, 1)),
            connected(PlantKind::Wind, cell(5, 2)),
        ],
    );
    let mut rng = SimRng::from_seed(3);
    sys.begin_day(&region, april(), &mut rng);
    let w = region.wind();
    let mean = w.monthly[april().index0()];
    let rating = region.spec(PlantKind::Wind).capacity.value()
        * cell_ratio(&region, cell(5, 2), |c| c.wind_potential).value();
    sys.wind_tick = Some(600);
    sys.wind_factor = mean + 0.3;
    let (fall, horizon) = sys.wind_fall(&region, april(), 600);
    assert_eq!(horizon, 60);
    let expected = rating * 0.3 * (1.0 - w.persistence.powi(60));
    assert!(
        (fall.value() - expected).abs() < 1e-9,
        "{fall:?} vs {expected}"
    );
    // Below its mean the forecast rises: nothing is foreseen to fall.
    sys.wind_factor = mean - 0.3;
    assert_eq!(sys.wind_fall(&region, april(), 600).0, Megawatts::ZERO);
}

#[test]
fn a_wind_fall_withholds_a_trip_only_beyond_the_fleets_reach() {
    let mw = Megawatts::new;
    assert!(
        !wind_fall_uncovered(mw(100.0), Megawatts::ZERO, mw(50.0)),
        "without a fall the one-tick headroom gate decides alone"
    );
    assert!(!wind_fall_uncovered(mw(100.0), mw(20.0), mw(120.0)));
    assert!(wind_fall_uncovered(mw(100.0), mw(20.5), mw(120.0)));
    assert!(
        !wind_fall_uncovered(mw(-50.0), mw(60.0), mw(10.0)),
        "a surplus beyond the tripped output counts towards the fall"
    );
}
