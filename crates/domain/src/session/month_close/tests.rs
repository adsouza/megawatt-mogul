use std::sync::Arc;

use proptest::prelude::*;

use super::super::*;
use super::{LedgerTotals, is_zero_capital, share};
use crate::company::{LedgerCategory, ReserveReviewInputs};
use crate::generation::PlantStatus;
use crate::ids::CityId;
use crate::region::fixtures::{tiny_region, tiny_region_parts};
use crate::region::{PlantKind, RegionDefinitionParts};
use crate::regulation::{Licence, RateReviewOutcome, YearTally, reserve_excess};
use crate::time::{SimMonth, TICKS_PER_DAY};
use crate::units::{MegawattHours, Megawatts, Ratio};

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
    let (peaker, _) = s.build_plant(PlantKind::Peaker, cell(5, 5)).unwrap();
    let node = s.power().plant(peaker).unwrap().node();
    let (line, _) = s.build_line(s.substation(), node).unwrap();
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
/// connected, city 2 (1,000 MW) sits one line-month from the substation, and
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
    parts.cities[1].cell = cell(8, 5);
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
    parts.start.fleet = vec![coal.clone(), coal.clone(), coal];
    parts
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
        s.build_line(s.substation(), city2).unwrap();
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
fn december_capacity_and_city_connection_are_assessed_together() {
    let (s, peaker, events) = december_with_new_peaker(true);
    let report = report_in(&events);
    assert_eq!(report.peak_demand, Megawatts::new(1000.0));
    assert!(events.contains(&DomainEvent::CityConnected { city: CityId(2) }));
    assert!(events.contains(&DomainEvent::PlantCommissioned {
        plant: peaker,
        kind: PlantKind::Peaker
    }));
    let firm = s.power().firm_capacity();
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
        assert!(events.contains(&DomainEvent::StrikeIssued { strikes: month }));
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
    assert!(report.unserved > MegawattHours::ZERO);
    assert!(
        report
            .incidents
            .iter()
            .any(|i| i.kind == IncidentKind::Blackout)
    );
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
        p.start.connected_cities.clear()
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
