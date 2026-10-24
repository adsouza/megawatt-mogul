use std::sync::Arc;

use proptest::prelude::*;

use super::super::*;
use super::{LedgerTotals, is_zero_capital, share};
use crate::company::{LedgerCategory, ReserveReviewInputs};
use crate::generation::PlantStatus;
use crate::grid::GridMode;
use crate::ids::CityId;
use crate::region::fixtures::{tiny_region, tiny_region_parts};
use crate::region::{PlantKind, RegionDefinitionParts};
use crate::regulation::{Licence, RateReviewOutcome, StrikeReason, YearTally, reserve_excess};
use crate::time::{SimMonth, TICKS_PER_DAY};
use crate::units::{Hertz, MegawattHours, Megawatts, Ratio};

fn session_from(parts: RegionDefinitionParts, seed: u64) -> GameSession {
    let region = RegionDefinition::try_from(parts).expect("test region is valid");
    GameSession::new(Arc::new(region), seed, GameSettings::default())
}

fn session() -> GameSession {
    GameSession::new(Arc::new(tiny_region()), 1, GameSettings::default())
}

fn starting_in(year: i32, month: u8) -> RegionDefinitionParts {
    let mut parts = tiny_region_parts();
    parts.start.month = SimMonth::new(year, month).unwrap();
    parts
}

fn plant_of(s: &GameSession, kind: PlantKind) -> PlantId {
    s.plant_views()
        .iter()
        .find(|p| p.kind == kind)
        .expect("plant of kind")
        .id
}

fn cell(x: u16, y: u16) -> CellCoord {
    CellCoord { x, y }
}

/// Commits every start-fleet plant all day under automatic generation control.
fn auto_fleet(s: &mut GameSession) {
    let ids: Vec<PlantId> = s.power().plants().keys().copied().collect();
    for plant in ids {
        s.start_plant(plant).unwrap();
        s.set_auto_follow(plant, true).unwrap();
    }
}

/// The start fleet plus one more all-day AGC peaker: enough range to follow
/// the tiny region's load in every month without shedding.
fn robust_fleet(s: &mut GameSession) -> PlantId {
    let extra = s.insert_commissioned_plant(PlantKind::Peaker, cell(6, 5));
    auto_fleet(s);
    extra
}

/// Plays the current month's day to its close; returns every event.
fn play_day(s: &mut GameSession) -> Vec<DomainEvent> {
    let mut events = s.begin_operating().unwrap();
    events.extend(finish_day(s));
    events
}

/// Ticks the rest of the day; returns the events, ending with the close.
fn finish_day(s: &mut GameSession) -> Vec<DomainEvent> {
    let mut events = Vec::new();
    while s.phase() == DayPhase::Operating {
        events.extend(s.tick().unwrap());
    }
    assert_eq!(s.phase(), DayPhase::Review);
    events
}

fn report_in(events: &[DomainEvent]) -> &MonthlyReport {
    let reports: Vec<&MonthlyReport> = events
        .iter()
        .filter_map(|e| match e {
            DomainEvent::MonthClosed(report) => Some(report.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(reports.len(), 1, "exactly one MonthClosed");
    reports[0]
}

/// Sum of the ledger entries in `category` for `subject`, in `month`.
fn booked(
    s: &GameSession,
    month: SimMonth,
    category: LedgerCategory,
    subject: Option<AssetRef>,
) -> Money {
    s.company()
        .ledger()
        .iter()
        .filter(|e| e.month == month && e.category == category && e.subject == subject)
        .map(|e| e.amount)
        .sum()
}

fn starts_of(events: &[DomainEvent], plant: PlantId) -> usize {
    events
        .iter()
        .filter(|e| **e == DomainEvent::PlantStarting { plant })
        .count()
}

fn startup_cost(kind: PlantKind) -> Money {
    tiny_region().spec(kind).startup_cost
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]
    #[test]
    fn revenue_is_weighted_and_rounded_once(seed in any::<u64>()) {
        let mut s = GameSession::new(Arc::new(tiny_region()), seed, GameSettings::default());
        auto_fleet(&mut s);
        let tariff = s.regulator().tariff();
        let events = play_day(&mut s);
        let served: MegawattHours = events
            .iter()
            .filter_map(|e| match e {
                DomainEvent::TickCompleted(t) => Some(t.served.energy_over_ticks(1)),
                _ => None,
            })
            .fold(MegawattHours::ZERO, |acc, e| acc + e);
        let report = report_in(&events);
        prop_assert_eq!(report.days, 30);
        prop_assert_eq!(report.revenue, tariff.cost_of(served, 30));
        prop_assert_eq!(report.served, served * 30.0);
    }
}

#[test]
fn ledger_matches_cash_after_a_month() {
    let mut s = session();
    auto_fleet(&mut s);
    let events = play_day(&mut s);
    assert_eq!(s.check_invariants(), Ok(()));
    let report = report_in(&events);
    assert_eq!(report.net, s.company().cash() - s.company().starting_cash());
    assert_eq!(
        report.net,
        report.revenue + report.fuel + report.start_up + report.fixed_om + report.construction
    );
    assert!(report.revenue > Money::ZERO && report.fuel < Money::ZERO);
    // Fixed O&M for each of the three commissioned plants.
    let region = tiny_region();
    let om: Money = [PlantKind::Coal, PlantKind::Ccgt, PlantKind::Peaker]
        .iter()
        .map(|k| region.spec(*k).fixed_om_per_month)
        .sum();
    assert_eq!(report.fixed_om, -om);
}

#[test]
fn peaker_commissions_after_twelve_months_with_exact_instalments() {
    let mut s = session();
    robust_fleet(&mut s);
    let (peaker, _) = s.build_plant(PlantKind::Peaker, cell(4, 5)).unwrap();
    let node = s.power().plant(peaker).unwrap().node();
    let (line, _) = s.build_line(s.anchor_node(), node).unwrap();
    let mut commissioned_in = Vec::new();
    let mut line_in = Vec::new();
    for month in 1..=12 {
        let events = play_day(&mut s);
        assert_eq!(s.status(), SessionStatus::Active, "month {month}");
        if events.contains(&DomainEvent::PlantCommissioned {
            plant: peaker,
            kind: PlantKind::Peaker,
        }) {
            commissioned_in.push(month);
        }
        if events.contains(&DomainEvent::LineCommissioned { line }) {
            line_in.push(month);
        }
        assert_eq!(s.check_invariants(), Ok(()));
        s.finish_review().unwrap();
    }
    assert_eq!(commissioned_in, vec![12]);
    // sqrt(5) cells round up to 3 at 2 cells a month: two months.
    assert_eq!(line_in, vec![2]);
    let paid: Money = s
        .company()
        .ledger()
        .iter()
        .filter(|e| {
            e.category == LedgerCategory::Construction && e.subject == Some(AssetRef::Plant(peaker))
        })
        .map(|e| e.amount)
        .sum();
    assert_eq!(paid, Money::from_dollars(-200_000_000));
    assert_eq!(s.pending_instalments(AssetRef::Plant(peaker)), None);
    assert_eq!(s.pending_instalments(AssetRef::Line(line)), None);
    // Both assets entered the rate base as new capital in their commissioning month.
    let rate_base = s.company().rate_base();
    let plant_asset = rate_base.asset(AssetRef::Plant(peaker)).unwrap();
    assert!(!plant_asset.legacy);
    assert_eq!(plant_asset.original, Money::from_dollars(200_000_000));
    assert_eq!(plant_asset.added_on, SimMonth::new(2028, 3).unwrap());
    let line_asset = rate_base.asset(AssetRef::Line(line)).unwrap();
    assert!(!line_asset.legacy);
    assert_eq!(line_asset.original, Money::from_dollars(120_000_000));
    assert_eq!(
        s.regulator().tally().firm_added,
        Megawatts::new(200.0 * 0.95)
    );
    // A newly commissioned plant is treated as returning from a month offline.
    assert!(s.power().plant(peaker).unwrap().previous_month_offline());
}

/// Flat, noise-free demand so peaks are exact: city 1 (1,000 MW) is
/// connected, city 2 (1,000 MW) sits one line-month from the home city, and
/// the start fleet is three coal units (1,800 MW firm, credit 1). A peaker
/// (500 MW firm) builds in one month.
fn flat_december() -> RegionDefinitionParts {
    let mut parts = starting_in(2027, 12);
    let demand = &mut parts.demand;
    demand.winter = [1.0; 24];
    demand.summer = [1.0; 24];
    demand.shoulder = [1.0; 24];
    demand.monthly = [1.0; 12];
    demand.forecast_error = Ratio::ZERO;
    parts.cities[0].base_load = Megawatts::new(1000.0);
    parts.cities[1].base_load = Megawatts::new(1000.0);
    parts.cities[1].cell = cell(7, 5);
    parts.start.connected_cities = vec![CityId(1)];
    for spec in &mut parts.catalog {
        match spec.kind {
            PlantKind::Coal => spec.capacity_credit = Ratio::ONE,
            PlantKind::Peaker => {
                spec.capacity = Megawatts::new(500.0);
                spec.capacity_credit = Ratio::ONE;
                spec.build_months = 1;
            }
            _ => {}
        }
    }
    let coal = parts.start.fleet[0].clone();
    // Distinct cooled sites, each still two priced line cells from the home city.
    parts.start.fleet = [(7, 3), (8, 4), (6, 2)]
        .into_iter()
        .map(|(x, y)| crate::region::StartPlantDef {
            cell: cell(x, y),
            ..coal.clone()
        })
        .collect();
    parts
}

fn storage_need_session(batteries: usize, reserve_gap: bool) -> (GameSession, Vec<PlantId>) {
    let mut parts = flat_december();
    parts.start.month = SimMonth::new(2027, 10).unwrap();
    if reserve_gap {
        parts.start.fleet.truncate(2);
        parts
            .catalog
            .iter_mut()
            .find(|spec| spec.kind == PlantKind::Coal)
            .unwrap()
            .capacity = Megawatts::new(500.0);
    }
    let mut session = session_from(parts, 7);
    auto_fleet(&mut session);
    let mut ids = Vec::new();
    for y in 0..batteries {
        let (plant, _) = session
            .build_plant(PlantKind::Battery, cell(5, y as u16))
            .unwrap();
        let node = session.power().plant(plant).unwrap().node();
        session.build_line(session.anchor_node(), node).unwrap();
        ids.push(plant);
    }
    (session, ids)
}

#[test]
fn excess_batteries_are_held_then_written_off() {
    let (mut s, batteries) = storage_need_session(2, false);
    for year in [2027, 2028] {
        while s.clock().month().month() != 12 || s.clock().month().year() != year {
            play_day(&mut s);
            s.finish_review().unwrap();
        }
        let events = play_day(&mut s);
        for id in &batteries {
            assert!(
                events.iter().any(|e| match e {
                    DomainEvent::CapitalHeld { asset, amount } if year == 2027 =>
                        *asset == AssetRef::Plant(*id) && *amount > Money::ZERO,
                    DomainEvent::CapitalDisallowed { asset, amount } if year == 2028 =>
                        *asset == AssetRef::Plant(*id) && *amount > Money::ZERO,
                    _ => false,
                }),
                "battery {id:?}, review {year}"
            );
        }
        s.finish_review().unwrap();
    }
}

#[test]
fn a_battery_that_fills_a_reserve_gap_is_not_held() {
    let (mut s, batteries) = storage_need_session(1, true);
    for _ in 0..2 {
        play_day(&mut s);
        s.finish_review().unwrap();
    }
    let events = play_day(&mut s);
    assert_eq!(s.clock().month().month(), 12);
    assert!(!events.iter().any(|e| matches!(e, DomainEvent::CapitalHeld { asset: AssetRef::Plant(id), .. } if batteries.contains(id))));
    let asset = s
        .company()
        .rate_base()
        .asset(AssetRef::Plant(batteries[0]))
        .unwrap();
    assert_eq!(asset.held(), Money::ZERO);
    assert!(
        matches!(asset.profile, AssetProfile::Plant { firm, .. } if firm == Megawatts::new(170.0))
    );
}

fn city_node(s: &GameSession, city: CityId) -> NodeId {
    s.city_views()
        .into_iter()
        .find(|c| c.id == city)
        .expect("city")
        .node
}

/// A December game on [`flat_december`] that orders a peaker, optionally
/// connects city 2, and plays the month.
fn december_with_new_peaker(connect_city: bool) -> (GameSession, PlantId, Vec<DomainEvent>) {
    let mut s = session_from(flat_december(), 1);
    auto_fleet(&mut s);
    let (peaker, _) = s.build_plant(PlantKind::Peaker, cell(5, 5)).unwrap();
    if connect_city {
        let city2 = city_node(&s, CityId(2));
        s.build_line(s.anchor_node(), city2).unwrap();
    }
    let events = play_day(&mut s);
    (s, peaker, events)
}

#[test]
fn december_runs_rate_review() {
    let mut s = session_from(starting_in(2027, 12), 1);
    auto_fleet(&mut s);
    let events = play_day(&mut s);
    let report = report_in(&events);
    let outcome = report.rate_review.as_ref().expect("December reviews rates");
    assert!(events.contains(&DomainEvent::RateReviewCompleted {
        tariff: outcome.tariff,
        allowed_return: outcome.allowed_return,
    }));
    assert_eq!(s.regulator().tariff(), outcome.tariff);
    assert_eq!(s.regulator().tally(), &YearTally::default());
    assert_eq!(s.year_depreciation, Money::ZERO);
    // Other months do not review.
    s.finish_review().unwrap();
    let january = play_day(&mut s);
    assert_eq!(report_in(&january).rate_review, None);
    assert!(
        !january
            .iter()
            .any(|e| matches!(e, DomainEvent::RateReviewCompleted { .. }))
    );
}

#[test]
fn december_uses_the_updated_satisfaction_without_repricing_the_closed_month() {
    let mut parts = flat_december();
    parts.regulator.return_per_reliability = 0.0;
    parts.regulator.return_per_cost_improvement = 0.0;
    let tariff = parts.regulator.starting_tariff;
    let mut s = session_from(parts, 7);
    auto_fleet(&mut s);
    let before = s.company().satisfaction();
    let events = play_day(&mut s);
    let after = s.company().satisfaction();
    assert_ne!(before, after);
    let p = s.region().regulator();
    let expected = (p.base_return.value()
        + p.return_per_satisfaction_point * (after - p.satisfaction_target))
        .clamp(p.min_return.value(), p.max_return.value());
    let report = report_in(&events);
    assert!((report.rate_review.as_ref().unwrap().allowed_return.value() - expected).abs() < 1e-12);
    assert_eq!(report.revenue, tariff.cost_of(report.served, 1));
}

#[test]
fn december_capacity_and_city_connection_are_assessed_together() {
    let (s, peaker, events) = december_with_new_peaker(true);
    let report = report_in(&events);
    assert_eq!(report.peak_demand, Megawatts::new(1000.0));
    assert!(events.contains(&DomainEvent::CityConnected { city: CityId(2) }));
    assert!(events.contains(&DomainEvent::PlantCommissioned {
        plant: peaker,
        kind: PlantKind::Peaker
    }));
    let firm = s
        .power()
        .firm_capacity(&s.region().regulator().storage_accreditation);
    assert_eq!(firm, Megawatts::new(2300.0));
    let next = SimMonth::new(2028, 1).unwrap();
    let projected = s.power().projected_connected_peak(s.region(), next);
    assert_eq!(projected, Megawatts::new(2000.0));
    // 2.3 GW firm is within 2 GW × 1.2: nothing is held.
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, DomainEvent::CapitalHeld { .. }))
    );
    assert_eq!(s.company().rate_base().held_total(), Money::ZERO);
    // Against the 1 GW actual peak alone the same firm would be excess...
    let margin = s.region().regulator().reserve_margin;
    let actual = report.peak_demand;
    assert!(reserve_excess(firm, actual, actual, margin) > Megawatts::ZERO);
    // ...and without the new connection the new peaker is held.
    let (alone, alone_peaker, alone_events) = december_with_new_peaker(false);
    let held = alone
        .company()
        .rate_base()
        .asset(AssetRef::Plant(alone_peaker))
        .unwrap()
        .held();
    assert!(held > Money::ZERO);
    assert!(alone_events.contains(&DomainEvent::CapitalHeld {
        asset: AssetRef::Plant(alone_peaker),
        amount: held,
    }));
}

#[test]
fn held_capital_earns_no_return_in_the_review() {
    let (s, _, events) = december_with_new_peaker(false);
    let report = report_in(&events);
    let outcome = report.rate_review.as_ref().unwrap();
    let rate_base = s.company().rate_base();
    assert!(rate_base.held_total() > Money::ZERO);
    // The game began this December, so the year's depreciation is all there is.
    let depreciation: Money = rate_base.assets().map(|a| a.depreciated()).sum();
    let operating = -(report.fuel + report.start_up + report.fixed_om);
    let earned = outcome.revenue_requirement - operating - depreciation;
    // One month is tallied, so a twelfth of the year's return is earned.
    let return_on = |base: Money| {
        Money::from_cents(
            (outcome.allowed_return.value() * base.cents() as f64 / 12.0).round_ties_even() as i64,
        )
    };
    assert_eq!(earned, return_on(rate_base.eligible_total()));
    assert_ne!(earned, return_on(rate_base.total()));
}

#[test]
fn first_review_holds_second_review_disallows_if_still_excess() {
    let (mut s, peaker, events) = december_with_new_peaker(false);
    let asset = AssetRef::Plant(peaker);
    let held = s.company().rate_base().asset(asset).unwrap().held();
    assert!(events.contains(&DomainEvent::CapitalHeld {
        asset,
        amount: held
    }));
    assert_eq!(s.regulator().tally(), &YearTally::default());
    let mut disallowed = Vec::new();
    for _ in 0..12 {
        s.finish_review().unwrap();
        let events = play_day(&mut s);
        assert_eq!(s.status(), SessionStatus::Active);
        let slice = s.company().rate_base().asset(asset).unwrap().held_slice();
        if s.clock().month().month() < 12 {
            // The annual tally reset keeps the held slice and its history.
            assert_eq!(slice.map(|h| h.consecutive_excess_reviews()), Some(1));
        }
        disallowed.extend(events.into_iter().filter(
            |e| matches!(e, DomainEvent::CapitalDisallowed { asset: a, .. } if *a == asset),
        ));
    }
    assert_eq!(s.clock().month(), SimMonth::new(2028, 12).unwrap());
    assert_eq!(disallowed.len(), 1);
    let rate_base = s.company().rate_base();
    assert!(rate_base.asset(asset).unwrap().written_off() > Money::ZERO);
    assert_eq!(rate_base.held_total(), Money::ZERO);
    // A written-off plant stays dispatchable.
    s.finish_review().unwrap();
    assert_eq!(s.start_plant(peaker), Ok(Vec::new()));
}

#[test]
fn an_automatic_black_start_is_charged_once_with_the_daily_weight() {
    let mut s = session();
    let peaker = plant_of(&s, PlantKind::Peaker);
    let month = s.clock().month();
    s.begin_operating().unwrap();
    // Abrupt equipment losses leave only the peaker running on AGC.
    for kind in [PlantKind::Coal, PlantKind::Ccgt] {
        let id = plant_of(&s, kind);
        s.set_auto_follow(id, false).unwrap();
        s.power.plant_mut(id).unwrap().trip();
    }
    let events = finish_day(&mut s);
    assert!(events.contains(&DomainEvent::Blackout));
    assert_eq!(starts_of(&events, peaker), 1);
    assert!(events.contains(&DomainEvent::PlantOnline { plant: peaker }));
    let expected = -startup_cost(PlantKind::Peaker) * i64::from(month.days_in_month());
    assert_eq!(
        booked(
            &s,
            month,
            LedgerCategory::StartUp,
            Some(AssetRef::Plant(peaker))
        ),
        expected
    );
    assert_eq!(report_in(&events).start_up, expected);
    assert!(s.check_invariants().is_ok());
}

#[test]
fn all_day_return_from_offline_costs_one_start() {
    let mut s = session();
    let returning = robust_fleet(&mut s);
    let april = s.clock().month();
    let events = play_day(&mut s);
    assert_eq!(starts_of(&events, returning), 0);
    assert_eq!(
        booked(
            &s,
            april,
            LedgerCategory::StartUp,
            Some(AssetRef::Plant(returning))
        ),
        -startup_cost(PlantKind::Peaker)
    );
    // The established start fleet pays nothing.
    assert_eq!(
        report_in(&events).start_up,
        -startup_cost(PlantKind::Peaker)
    );
}

#[test]
fn continuing_all_day_pattern_has_no_restart_charge() {
    let mut s = session();
    let returning = robust_fleet(&mut s);
    play_day(&mut s);
    assert!(!s.power().plant(returning).unwrap().previous_month_offline());
    s.finish_review().unwrap();
    let may = s.clock().month();
    let events = play_day(&mut s);
    assert_eq!(report_in(&events).start_up, Money::ZERO);
    assert_eq!(
        booked(
            &s,
            may,
            LedgerCategory::StartUp,
            Some(AssetRef::Plant(returning))
        ),
        Money::ZERO
    );
    assert!(
        s.power()
            .plants()
            .values()
            .all(|p| !p.previous_month_offline())
    );
}

#[test]
fn daily_restart_costs_days_starts_without_boundary_duplicate() {
    let mut s = session_from(starting_in(2028, 2), 1);
    auto_fleet(&mut s);
    let returning = s.insert_commissioned_plant(PlantKind::Peaker, cell(6, 5));
    let mut hours = [false; 24];
    hours[8..=20].fill(true);
    s.commit_plant(returning, hours).unwrap();
    let february = s.clock().month();
    s.begin_operating().unwrap();
    assert!(s.day_boundary_starts().is_empty());
    let events = finish_day(&mut s);
    assert_eq!(starts_of(&events, returning), 1);
    assert_eq!(report_in(&events).days, 29);
    assert_eq!(
        booked(
            &s,
            february,
            LedgerCategory::StartUp,
            Some(AssetRef::Plant(returning))
        ),
        -(startup_cost(PlantKind::Peaker) * 29)
    );
}

#[test]
fn month_boundary_initialisation_is_not_a_second_start_event() {
    let mut s = session();
    auto_fleet(&mut s);
    let returning = s.insert_commissioned_plant(PlantKind::Peaker, cell(6, 5));
    // Runs 00:00-05:59: initialised online at midnight (synthetic, no event),
    // and its one recurring start is the 23:50 start for the next midnight.
    let mut hours = [false; 24];
    hours[0..=5].fill(true);
    s.commit_plant(returning, hours).unwrap();
    s.begin_operating().unwrap();
    assert!(s.power().plant(returning).unwrap().is_online());
    assert!(s.day_boundary_starts().is_empty());
    let events = finish_day(&mut s);
    assert_eq!(starts_of(&events, returning), 1);
    assert_eq!(
        booked(
            &s,
            SimMonth::new(2027, 4).unwrap(),
            LedgerCategory::StartUp,
            Some(AssetRef::Plant(returning))
        ),
        -(startup_cost(PlantKind::Peaker) * 30)
    );
}

#[test]
fn manual_operation_counts_as_a_month_online() {
    let mut s = session();
    robust_fleet(&mut s);
    let ccgt = plant_of(&s, PlantKind::Ccgt);
    s.stop_plant(ccgt).unwrap(); // no pattern
    s.power
        .plant_mut(ccgt)
        .unwrap()
        .set_previous_month_offline(true);
    s.begin_operating().unwrap();
    for _ in 0..600 {
        s.tick().unwrap();
    }
    let events = s.start_plant(ccgt).unwrap();
    assert_eq!(events, vec![DomainEvent::PlantStarting { plant: ccgt }]);
    for _ in 600..TICKS_PER_DAY {
        s.tick().unwrap();
    }
    // The manual start is a recurring event, weighted by the month's days.
    assert_eq!(
        booked(
            &s,
            SimMonth::new(2027, 4).unwrap(),
            LedgerCategory::StartUp,
            Some(AssetRef::Plant(ccgt))
        ),
        -(startup_cost(PlantKind::Ccgt) * 30)
    );
    assert!(!s.power().plant(ccgt).unwrap().previous_month_offline());
}

#[test]
fn idle_month_marks_a_plant_offline_for_the_next() {
    let mut s = session();
    robust_fleet(&mut s);
    let ccgt = plant_of(&s, PlantKind::Ccgt);
    s.stop_plant(ccgt).unwrap();
    play_day(&mut s);
    assert!(s.power().plant(ccgt).unwrap().previous_month_offline());
    // Back to all day next month: one boundary start.
    s.finish_review().unwrap();
    s.start_plant(ccgt).unwrap();
    let may = s.clock().month();
    let events = play_day(&mut s);
    assert_eq!(starts_of(&events, ccgt), 0);
    assert_eq!(
        booked(
            &s,
            may,
            LedgerCategory::StartUp,
            Some(AssetRef::Plant(ccgt))
        ),
        -startup_cost(PlantKind::Ccgt)
    );
}

#[test]
fn bankruptcy_ends_game_and_freezes_state() {
    let mut parts = tiny_region_parts();
    parts.start.cash = Money::from_dollars(-1_000_000_000_000);
    let mut s = session_from(parts, 1);
    robust_fleet(&mut s);
    let mut events = Vec::new();
    for month in 1..=3 {
        events = play_day(&mut s);
        if month < 3 {
            assert_eq!(s.status(), SessionStatus::Active);
            s.finish_review().unwrap();
        }
    }
    assert_eq!(
        s.status(),
        SessionStatus::GameOver(GameOverReason::Bankrupt)
    );
    assert!(events.contains(&DomainEvent::Bankrupt));
    // The month still closes and is reported.
    report_in(&events);
    assert_eq!(s.phase(), DayPhase::Review);
    let h = s.state_hash();
    assert_eq!(
        s.begin_operating(),
        Err(DomainError::GameOver {
            reason: GameOverReason::Bankrupt
        })
    );
    assert_eq!(
        s.finish_review(),
        Err(DomainError::GameOver {
            reason: GameOverReason::Bankrupt
        })
    );
    assert_eq!(s.state_hash(), h);
}

#[test]
fn sustained_shortfall_revokes_licence() {
    let mut parts = tiny_region_parts();
    parts.start.fleet.clear();
    let mut s = session_from(parts, 1);
    let mut events = Vec::new();
    for month in 1..=3 {
        events = play_day(&mut s);
        assert!(events.contains(&DomainEvent::StrikeIssued {
            strikes: month,
            reason: StrikeReason::UnservedEnergy
        }));
        if month < 3 {
            assert_eq!(s.status(), SessionStatus::Active);
            s.finish_review().unwrap();
        }
    }
    assert!(events.contains(&DomainEvent::LicenseRevoked));
    assert_eq!(s.regulator().licence(), Licence::Revoked);
    assert_eq!(
        s.status(),
        SessionStatus::GameOver(GameOverReason::LicenseRevoked)
    );
    let report = report_in(&events);
    // With no fleet the whole forecast is uncovered: every hour is a full
    // rotating outage (spec §15.3), so nothing is served and nothing blacks
    // out.
    assert!(report.unserved > MegawattHours::ZERO);
    assert!((report.unserved.value() - report.demand.value()).abs() < 1e-6);
    assert!(
        !report
            .incidents
            .iter()
            .any(|i| i.kind == IncidentKind::Blackout)
    );
}

/// A session whose frequency band is so tight that demand noise alone puts
/// almost every Normal-mode tick out of band, on a fleet that serves all load.
fn noisy_frequency_session() -> GameSession {
    let mut parts = tiny_region_parts();
    parts.regulator.frequency_band = Hertz::new(1e-9);
    let mut s = session_from(parts, 1);
    robust_fleet(&mut s);
    s
}

#[test]
fn a_healthy_month_reports_no_frequency_failure() {
    let mut parts = tiny_region_parts();
    parts.regulator.frequency_band = Hertz::new(100.0);
    let mut s = session_from(parts, 1);
    robust_fleet(&mut s);
    let events = play_day(&mut s);
    let report = report_in(&events);
    assert_eq!(report.out_of_band_share, Ratio::ZERO);
    assert!(!report.frequency_failed);
    assert!(!report.frequency_notice);
}

#[test]
fn first_frequency_only_month_draws_a_notice_then_the_next_strikes() {
    let mut s = noisy_frequency_session();
    let month = s.clock().month();
    let events = play_day(&mut s);
    let report = report_in(&events);
    assert_eq!(report.unserved, MegawattHours::ZERO);
    assert!(report.out_of_band_share > s.region().regulator().frequency_violation_share);
    assert!(report.frequency_failed);
    assert!(report.frequency_notice);
    assert!(events.contains(&DomainEvent::FrequencyNoticeIssued { month }));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, DomainEvent::StrikeIssued { .. }))
    );
    assert_eq!(s.regulator().strikes(), 0);
    assert_eq!(s.regulator().last_frequency_notice(), Some(month));

    s.finish_review().unwrap();
    let events = play_day(&mut s);
    let report = report_in(&events);
    assert!(report.frequency_failed);
    assert!(!report.frequency_notice);
    assert!(events.contains(&DomainEvent::StrikeIssued {
        strikes: 1,
        reason: StrikeReason::FrequencyPerformance
    }));
    assert_eq!(s.regulator().last_frequency_notice(), Some(month));
}

#[test]
fn failing_both_tests_gives_one_unserved_strike_and_spares_the_notice() {
    let mut parts = tiny_region_parts();
    parts.regulator.frequency_band = Hertz::new(1e-9);
    let mut s = session_from(parts, 1);
    s.empty_start();
    // One plant alone cannot carry the load: load is shed while the grid is
    // still in Normal mode, and demand noise keeps frequency off nominal.
    let coal = plant_of(&s, PlantKind::Coal);
    s.start_plant(coal).unwrap();
    s.set_auto_follow(coal, true).unwrap();
    let events = play_day(&mut s);
    let report = report_in(&events);
    assert!(report.unserved > MegawattHours::ZERO);
    assert!(report.frequency_failed);
    assert!(!report.frequency_notice);
    let strikes: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, DomainEvent::StrikeIssued { .. }))
        .collect();
    assert_eq!(
        strikes,
        [&DomainEvent::StrikeIssued {
            strikes: 1,
            reason: StrikeReason::UnservedEnergy
        }]
    );
    assert_eq!(s.regulator().last_frequency_notice(), None);
}

#[test]
fn report_merit_order_sorted_by_cost() {
    let mut s = session();
    auto_fleet(&mut s);
    let events = play_day(&mut s);
    let report = report_in(&events);
    assert_eq!(report.merit_order.len(), 3);
    assert!(
        report
            .merit_order
            .windows(2)
            .all(|w| w[0].marginal_cost <= w[1].marginal_cost)
    );
    let kinds: Vec<PlantKind> = report.merit_order.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![PlantKind::Coal, PlantKind::Ccgt, PlantKind::Peaker]
    );
    // Energy is weighted by days and agrees with the per-kind totals.
    for entry in &report.merit_order {
        assert_eq!(report.energy_by_kind[&entry.kind], entry.energy);
    }
    let fuel: Money = report
        .merit_order
        .iter()
        .map(|e| -e.marginal_cost.cost_of(e.energy, 1))
        .sum();
    assert!((fuel - report.fuel).cents().abs() <= 3, "rounding only");
}

#[test]
fn reports_and_histories_are_kept_across_months() {
    let mut s = session();
    robust_fleet(&mut s);
    let april = play_day(&mut s);
    let first = report_in(&april).clone();
    let ledger_after_april = s.company().ledger().len();
    let tally_after_april = s.regulator().tally().clone();
    s.finish_review().unwrap();
    let may = play_day(&mut s);
    let second = report_in(&may).clone();
    assert_eq!(first.month, SimMonth::new(2027, 4).unwrap());
    assert_eq!(second.month, SimMonth::new(2027, 5).unwrap());
    assert_eq!((first.days, second.days), (30, 31));
    assert_ne!(first.served, second.served);
    // The second month starts from a fresh accumulator, not on top of April.
    assert!(second.served < first.served * 1.5);
    // Company and regulator history spans both months.
    assert!(s.company().ledger().len() > ledger_after_april);
    assert!(s.company().ledger().iter().any(|e| e.month == first.month));
    let tally = s.regulator().tally();
    assert_eq!(tally.served, tally_after_april.served + second.served);
    assert_eq!(tally.demand, tally_after_april.demand + second.demand);
    assert_eq!(
        s.company().cash() - s.company().starting_cash(),
        first.net + second.net
    );
}

#[test]
fn close_clears_the_finished_day() {
    let mut s = session();
    auto_fleet(&mut s);
    play_day(&mut s);
    assert_eq!(s.grid_snapshot().last_tick, None);
    assert!(s.day_ticks.is_empty());
    assert_eq!(s.month_to_date, MonthAccumulator::default());
    s.finish_review().unwrap();
    assert_eq!(s.grid_snapshot().last_tick, None);
}

#[test]
fn plant_under_construction_keeps_its_returning_state_through_a_close() {
    let mut s = session();
    robust_fleet(&mut s);
    let (peaker, _) = s.build_plant(PlantKind::Peaker, cell(5, 5)).unwrap();
    play_day(&mut s);
    let plant = s.power().plant(peaker).unwrap();
    assert_eq!(
        plant.status(),
        PlantStatus::UnderConstruction { months_left: 11 }
    );
    assert!(plant.previous_month_offline());
}

#[test]
fn reliability_share_is_served_over_demand_clamped_and_defaults_to_one() {
    let mwh = MegawattHours::new;
    assert_eq!(share(mwh(50.0), mwh(100.0)).value(), 0.5);
    assert_eq!(share(mwh(100.0), mwh(100.0)), Ratio::ONE);
    assert_eq!(share(mwh(150.0), mwh(100.0)), Ratio::ONE);
    assert_eq!(share(mwh(-5.0), mwh(100.0)), Ratio::ZERO);
    assert_eq!(share(mwh(0.0), mwh(0.0)), Ratio::ONE);
    assert_eq!(share(mwh(10.0), mwh(0.0)), Ratio::ONE);
    assert_eq!(share(mwh(10.0), mwh(-1.0)), Ratio::ONE);
}

#[test]
fn only_capital_adjustments_of_no_money_are_noise() {
    let asset = AssetRef::Plant(PlantId(1));
    let zero = Money::ZERO;
    let some = Money::from_cents(1);
    assert!(is_zero_capital(&DomainEvent::CapitalHeld {
        asset,
        amount: zero
    }));
    assert!(is_zero_capital(&DomainEvent::CapitalReleased {
        asset,
        amount: zero
    }));
    assert!(is_zero_capital(&DomainEvent::CapitalDisallowed {
        asset,
        amount: zero
    }));
    assert!(!is_zero_capital(&DomainEvent::CapitalHeld {
        asset,
        amount: some
    }));
    assert!(!is_zero_capital(&DomainEvent::CapitalReleased {
        asset,
        amount: some
    }));
    assert!(!is_zero_capital(&DomainEvent::CapitalDisallowed {
        asset,
        amount: some
    }));
    assert!(!is_zero_capital(&DomainEvent::Blackout));
}

#[test]
fn ledger_totals_split_operating_cost_from_the_net() {
    let totals = LedgerTotals {
        revenue: Money::from_cents(1_000),
        fuel: Money::from_cents(-100),
        start_up: Money::from_cents(-20),
        fixed_om: Money::from_cents(-3),
        construction: Money::from_cents(-50),
    };
    assert_eq!(totals.operating_cost(), Money::from_cents(123));
    assert_eq!(totals.net(), Money::from_cents(827));
}

/// Runs December's real review on a copy of `s` and checks that
/// `rate_review_estimate` on `s` predicted it exactly.
fn assert_estimate_matches_annual_review(s: &GameSession) -> RateReviewEstimate {
    let estimate = s.rate_review_estimate();
    let mut real = s.clone();
    let month = real.clock.month();
    let mut events = Vec::new();
    let outcome = real.annual_review(month, &mut events);
    assert_eq!(estimate.projected_return, outcome.allowed_return);
    let total = |pick: fn(&DomainEvent) -> Option<Money>| events.iter().filter_map(pick).sum();
    let held: Money = total(|e| match e {
        DomainEvent::CapitalHeld { amount, .. } => Some(*amount),
        _ => None,
    });
    let released: Money = total(|e| match e {
        DomainEvent::CapitalReleased { amount, .. } => Some(*amount),
        _ => None,
    });
    let disallowed: Money = total(|e| match e {
        DomainEvent::CapitalDisallowed { amount, .. } => Some(*amount),
        _ => None,
    });
    assert_eq!(
        (
            estimate.capital_to_hold,
            estimate.capital_to_release,
            estimate.capital_at_risk
        ),
        (held, released, disallowed)
    );
    estimate
}

/// A December session whose rate base holds two 800 MW slices from last
/// year's review.
fn december_with_held_capital(edit: impl FnOnce(&mut RegionDefinitionParts)) -> GameSession {
    let mut parts = starting_in(2027, 12);
    edit(&mut parts);
    let mut s = session_from(parts, 1);
    let firm = |mw: f64, node: u32| AssetProfile::Plant {
        firm: Megawatts::new(mw),
        node: NodeId(node),
    };
    let cost = Money::from_cents(1_000_000);
    let last_year = SimMonth::new(2026, 4).unwrap();
    for id in [900, 901] {
        s.company.rate_base_mut().add(
            AssetRef::Plant(PlantId(id)),
            cost,
            100,
            last_year,
            firm(800.0, id),
        );
    }
    s.company
        .rate_base_mut()
        .apply_reserve_review(&ReserveReviewInputs {
            year: 2026,
            excess: Megawatts::new(1_600.0),
        });
    s
}

#[test]
fn the_rate_review_estimate_previews_exactly_what_december_does() {
    // No demand: held capital is partly released and partly written off.
    let e = assert_estimate_matches_annual_review(&december_with_held_capital(|p| {
        for city in &mut p.cities {
            city.base_load = Megawatts::ZERO;
        }
    }));
    assert!(e.capital_to_release > Money::ZERO && e.capital_at_risk > Money::ZERO);
    // Real demand: the projection of next month's peak decides the excess,
    // and there is none, so everything held is released.
    let e = assert_estimate_matches_annual_review(&december_with_held_capital(|_| {}));
    assert!(e.capital_to_release > Money::ZERO && e.capital_at_risk == Money::ZERO);
}

/// The first December review of a game that started in `start_month` of 2027
/// and tallied `months` identical months: the same code path `close_month`
/// takes in December (`annual_review`), fed the same steady monthly figures
/// through the regulator's tally and the year's depreciation total.
///
/// Depreciation is booked as a flat amount per month rather than by running
/// the rate base down, because the real straight-line decay would leave the
/// 12-month game a smaller closing rate base than the 9-month one: a genuine
/// difference unrelated to proration, which would hide the one under test.
fn first_december_review(start_month: u8, months: u8) -> RateReviewOutcome {
    let mut s = session_from(starting_in(2027, start_month), 1);
    for _ in 0..months {
        s.regulator.record_month(
            MegawattHours::new(30_000.0),
            MegawattHours::new(30_000.0),
            Money::from_dollars(1_500_000),
            Megawatts::new(70.0),
        );
        s.year_depreciation += Money::from_dollars(400_000);
    }
    assert_eq!(s.regulator().tally().months, months);
    let mut events = Vec::new();
    s.annual_review(SimMonth::new(2027, 12).unwrap(), &mut events)
}

#[test]
fn april_start_does_not_over_recover() {
    let april = first_december_review(4, 9);
    let january = first_december_review(1, 12);
    assert_eq!(april.allowed_return, january.allowed_return);
    assert!(
        (april.tariff.value() - january.tariff.value()).abs() < 0.01,
        "April {} vs January {}",
        april.tariff.value(),
        january.tariff.value()
    );
}

#[test]
fn the_estimate_and_the_review_read_the_same_months() {
    let mut s = session_from(starting_in(2027, 4), 1);
    for _ in 0..5 {
        s.regulator.record_month(
            MegawattHours::new(1.0),
            MegawattHours::new(1.0),
            Money::ZERO,
            Megawatts::new(1.0),
        );
    }
    let inputs = s.review_inputs(s.clock.month());
    assert_eq!(inputs.rate_review(Money::ZERO).tally.months, 5);
}

/// Commits the two big plants all day on manual targets far above the load
/// trough, so the overnight hours run a surplus with no player action. The
/// inherited schedule is undone first: no plant is on AGC.
fn overbuilt_session() -> GameSession {
    let mut s = session();
    s.empty_start();
    for kind in [PlantKind::Coal, PlantKind::Ccgt] {
        let plant = plant_of(&s, kind);
        s.start_plant(plant).unwrap();
        let capacity = s.region().spec(kind).capacity;
        s.set_dispatch_target(plant, capacity).unwrap();
    }
    s
}

#[test]
fn report_overgeneration_is_the_weighted_positive_imbalance() {
    let mut s = overbuilt_session();
    let weight = f64::from(s.clock().month().days_in_month());
    let mut events = s.begin_operating().unwrap();
    let mut surplus = MegawattHours::ZERO;
    while s.phase() == DayPhase::Operating {
        let tick = s.tick().unwrap();
        // Each tick's own summary: the snapshot's last tick is cleared when
        // the final tick closes the month.
        for event in &tick {
            if let DomainEvent::TickCompleted(summary) = event {
                surplus += summary.imbalance.max(Megawatts::ZERO).energy_over_ticks(1);
            }
        }
        events.extend(tick);
    }
    let report = report_in(&events);
    assert!(report.overgeneration > MegawattHours::ZERO);
    assert!((report.overgeneration.value() - (surplus * weight).value()).abs() < 1e-6);
    // The signed month total nets deficits off, so it cannot exceed the surplus.
    assert!(report.imbalance <= report.overgeneration);
}

#[test]
fn report_counts_over_frequency_trips_from_incidents() {
    let mut s = overbuilt_session();
    let events = play_day(&mut s);
    let report = report_in(&events);
    let tripped = events
        .iter()
        .filter(|e| matches!(e, DomainEvent::OverFrequencyTrip { .. }))
        .count();
    let incidents = report
        .incidents
        .iter()
        .filter(|i| matches!(i.kind, IncidentKind::OverFrequencyTrip { .. }))
        .count();
    assert_eq!(incidents, tripped);
    assert_eq!(report.over_frequency_trips as usize, tripped);
    assert!(tripped > 0, "overbuilt day should trip a plant");
}

#[test]
fn a_balanced_month_reports_no_trips() {
    let mut s = session();
    robust_fleet(&mut s);
    let events = play_day(&mut s);
    assert_eq!(report_in(&events).over_frequency_trips, 0);
}

/// Cities 1–3 connected with 3,000 MW of base load on city 3 and no demand
/// noise, against the 1,300 MW start fleet on automatic generation control:
/// the day opens in a rotating outage that deepens with the load.
fn shortage_session() -> GameSession {
    let mut parts = tiny_region_parts();
    parts.demand.forecast_error = Ratio::ZERO;
    parts.start.connected_cities.push(CityId(3));
    let city = parts.cities.iter_mut().find(|c| c.id == CityId(3)).unwrap();
    city.base_load = Megawatts::new(3_000.0);
    let mut s = session_from(parts, 1);
    auto_fleet(&mut s);
    s
}

fn summaries_in(events: &[DomainEvent]) -> Vec<&TickSummary> {
    events
        .iter()
        .filter_map(|e| match e {
            DomainEvent::TickCompleted(summary) => Some(summary),
            _ => None,
        })
        .collect()
}

/// One minute of `power`, as the month accumulator books it.
fn minute(power: Megawatts) -> MegawattHours {
    power.energy_over_ticks(1)
}

#[test]
fn rotating_outage_is_part_of_unserved() {
    let mut s = shortage_session();
    let weight = f64::from(s.clock().month().days_in_month());
    let events = play_day(&mut s);
    let report = report_in(&events);
    let planned: MegawattHours = summaries_in(&events)
        .iter()
        .map(|t| minute(t.demand * t.outage.value()))
        .sum();
    assert!(report.rotating_outage > MegawattHours::ZERO);
    assert!((report.rotating_outage.value() - (planned * weight).value()).abs() < 1e-6);
    // Part of unserved, never added to it: the invariant, and the sum itself.
    let unserved: MegawattHours = summaries_in(&events)
        .iter()
        .map(|t| minute(t.unserved))
        .sum();
    assert!((report.unserved.value() - (unserved * weight).value()).abs() < 1e-6);
    assert!(
        report.rotating_outage.value() <= report.unserved.value() + 1e-6 * report.demand.value()
    );
}

#[test]
fn peak_outage_share_is_the_largest_ordered() {
    let mut s = shortage_session();
    let events = play_day(&mut s);
    let report = report_in(&events);
    let orders: Vec<Ratio> = events
        .iter()
        .filter_map(|e| match e {
            DomainEvent::RotatingOutageOrdered { share, .. } => Some(*share),
            _ => None,
        })
        .collect();
    assert!(orders.len() > 1, "the day re-sizes its outage");
    let largest = orders
        .into_iter()
        .fold(Ratio::ZERO, |a, b| if b > a { b } else { a });
    assert!(largest > Ratio::ZERO);
    assert_eq!(report.peak_outage_share, largest);
}

#[test]
fn an_ordered_outage_is_an_incident() {
    let mut s = shortage_session();
    let events = play_day(&mut s);
    let report = report_in(&events);
    let ordered: Vec<(u8, Ratio)> = events
        .iter()
        .filter_map(|e| match e {
            DomainEvent::RotatingOutageOrdered { hour, share } => Some((*hour, *share)),
            _ => None,
        })
        .collect();
    let incidents: Vec<Ratio> = report
        .incidents
        .iter()
        .filter_map(|i| match i.kind {
            IncidentKind::RotatingOutage { share } => Some(share),
            _ => None,
        })
        .collect();
    let non_zero: Vec<Ratio> = ordered
        .iter()
        .map(|(_, share)| *share)
        .filter(|share| *share > Ratio::ZERO)
        .collect();
    // Every non-zero order is an incident, in order; lifting is not one.
    assert_eq!(incidents, non_zero);
    // Hour 0's order is emitted by `begin_operating`, after the accumulator
    // reset, and still counts: it is the day's first incident, at tick 0.
    assert_eq!(ordered[0].0, 0);
    let first = report.incidents.first().expect("an incident");
    assert_eq!(first.tick, 0);
    assert_eq!(
        first.kind,
        IncidentKind::RotatingOutage {
            share: ordered[0].1
        }
    );
}

#[test]
fn blackout_dark_load_is_not_counted_as_planned() {
    let mut s = shortage_session();
    let weight = f64::from(s.clock().month().days_in_month());
    let mut events = s.begin_operating().unwrap();
    for _ in 0..50 {
        events.extend(s.tick().unwrap());
    }
    // The whole fleet is lost mid-shortage and stays down: a blackout.
    let fleet: Vec<PlantId> = s.power().plants().keys().copied().collect();
    for plant in fleet {
        let p = s.power.plant_mut(plant).unwrap();
        p.trip();
        p.suppress_commitment_for_day();
    }
    events.extend(finish_day(&mut s));
    let report = report_in(&events);

    let ticks = summaries_in(&events);
    let dark: Vec<&&TickSummary> = ticks
        .iter()
        .filter(|t| t.mode == GridMode::Blackout)
        .collect();
    assert!(dark.len() > 100, "the day ends dark: {} ticks", dark.len());
    // Dark load is protection's, never planned: the outage is 0 in a
    // blackout, so those ticks add to unserved but not to rotating_outage.
    assert!(dark.iter().all(|t| t.outage == Ratio::ZERO));
    let dark_unserved: MegawattHours = dark.iter().map(|t| minute(t.unserved)).sum();
    let planned: MegawattHours = ticks
        .iter()
        .map(|t| minute(t.demand * t.outage.value()))
        .sum();
    assert!(planned > MegawattHours::ZERO, "the day began in an outage");
    assert!((report.rotating_outage.value() - (planned * weight).value()).abs() < 1e-6);
    assert!(
        report.unserved.value() - report.rotating_outage.value()
            >= (dark_unserved * weight).value() - 1e-6
    );
    assert!(report.rotating_outage < report.unserved);
}
