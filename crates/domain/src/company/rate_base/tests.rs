use super::*;
use crate::company::Company;
use crate::events::DomainEvent;
use crate::ids::{LineId, NodeId, PlantId};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

fn month(year: i32, month: u8) -> SimMonth {
    SimMonth::new(year, month).unwrap()
}

fn plant(id: u32) -> AssetRef {
    AssetRef::Plant(PlantId(id))
}

fn line(id: u32) -> AssetRef {
    AssetRef::Line(LineId(id))
}

fn plant_at(firm: f64, node: u32) -> AssetProfile {
    AssetProfile::Plant {
        firm: Megawatts::new(firm),
        node: NodeId(node),
    }
}

fn line_between(from: u32, to: u32) -> AssetProfile {
    AssetProfile::Line {
        from: NodeId(from),
        to: NodeId(to),
    }
}

fn review(year: i32, excess: f64) -> ReserveReviewInputs {
    ReserveReviewInputs {
        year,
        excess: Megawatts::new(excess),
    }
}

fn cents(c: i64) -> Money {
    Money::from_cents(c)
}

fn held(asset: AssetRef, amount: i64) -> DomainEvent {
    DomainEvent::CapitalHeld {
        asset,
        amount: cents(amount),
    }
}

fn released(asset: AssetRef, amount: i64) -> DomainEvent {
    DomainEvent::CapitalReleased {
        asset,
        amount: cents(amount),
    }
}

fn disallowed(asset: AssetRef, amount: i64) -> DomainEvent {
    DomainEvent::CapitalDisallowed {
        asset,
        amount: cents(amount),
    }
}

#[test]
fn depreciation_puts_remainder_in_final_month() {
    let mut rb = RateBase::default();
    rb.add(
        plant(1),
        cents(1_000),
        3,
        month(2028, 1),
        plant_at(100.0, 1),
    );
    let months: Vec<Money> = (0..4).map(|_| rb.depreciate_month()).collect();
    assert_eq!(
        months,
        vec![cents(333), cents(333), cents(334), Money::ZERO]
    );
    assert_eq!(rb.total(), Money::ZERO);
}

proptest! {
    #[test]
    fn depreciation_over_life_equals_original(cents_in in 1i64..10_000_000_000, life in 1u32..600) {
        let mut rb = RateBase::default();
        rb.add(plant(1), cents(cents_in), life, month(2028, 1), plant_at(100.0, 1));
        let total: Money = (0..life + 5).map(|_| rb.depreciate_month()).sum();
        prop_assert_eq!(total, cents(cents_in));
    }
}

#[test]
fn first_excess_review_holds_without_writeoff() {
    let mut company = Company::new(Money::from_dollars(1_000), 70.0);
    let rb = company.rate_base_mut();
    rb.add_legacy(
        plant(0),
        Money::from_dollars(500_000),
        360,
        month(2027, 1),
        plant_at(400.0, 0),
    );
    rb.add(
        plant(1),
        Money::from_dollars(200_000),
        360,
        month(2028, 3),
        plant_at(200.0, 1),
    );
    // Plant-to-home-city line: associated with plant 1.
    rb.add(
        line(2),
        Money::from_dollars(10_000),
        600,
        month(2028, 3),
        line_between(1, 0),
    );
    // City-to-city line: no plant endpoint, never held.
    rb.add(
        line(3),
        Money::from_dollars(4_000),
        600,
        month(2028, 3),
        line_between(0, 9),
    );

    let events = rb.apply_reserve_review(&review(2028, 100.0));

    assert_eq!(
        events,
        vec![
            held(plant(1), 10_000_000), // half of $200,000
            held(line(2), 500_000),     // the same half of $10,000
        ]
    );
    assert_eq!(rb.held_total(), Money::from_dollars(105_000));
    // No allowed return on the held half: it is excluded from the eligible base.
    assert_eq!(rb.eligible_total(), Money::from_dollars(609_000));
    assert_eq!(rb.total(), Money::from_dollars(714_000));
    let slice = rb.asset(plant(1)).unwrap().held_slice().unwrap();
    assert_eq!(
        (
            slice.consecutive_excess_reviews(),
            slice.last_excess_review()
        ),
        (1, 2028)
    );
    assert_eq!(
        rb.asset(plant(1)).unwrap().held_firm(),
        Some(Megawatts::new(100.0))
    );
    assert!(rb.asset(plant(0)).unwrap().held_slice().is_none());
    assert!(rb.asset(line(3)).unwrap().held_slice().is_none());
    assert_eq!(rb.written_off_total(), Money::ZERO);
    // Holding is a non-cash adjustment.
    assert_eq!(company.cash(), Money::from_dollars(1_000));
    assert!(company.ledger().is_empty());
}

#[test]
fn second_consecutive_excess_review_writes_off_held_remainder() {
    let mut rb = RateBase::default();
    rb.add(
        plant(1),
        cents(120_000),
        120,
        month(2028, 6),
        plant_at(100.0, 1),
    );
    assert_eq!(
        rb.apply_reserve_review(&review(2028, 150.0)),
        vec![held(plant(1), 120_000)]
    );
    for _ in 0..12 {
        assert_eq!(rb.depreciate_month(), cents(1_000));
    }
    rb.add(
        plant(2),
        cents(60_000),
        60,
        month(2029, 2),
        plant_at(100.0, 2),
    );

    // Plant 1 was commissioned last year and is still entirely excess: its
    // remaining book value is written off. The other 50 MW of excess holds
    // half of this year's plant 2.
    assert_eq!(
        rb.apply_reserve_review(&review(2029, 150.0)),
        vec![disallowed(plant(1), 108_000), held(plant(2), 30_000)]
    );
    let p1 = rb.asset(plant(1)).unwrap();
    assert_eq!(
        (p1.remaining, p1.written_off(), p1.depreciated()),
        (Money::ZERO, cents(108_000), cents(12_000))
    );
    assert!(p1.held_slice().is_none());
    assert_eq!(rb.written_off_total(), cents(108_000));
    // Written-off capital earns no future depreciation recovery.
    assert_eq!(rb.depreciate_month(), cents(1_000));
    assert_eq!(rb.asset(plant(1)).unwrap().depreciated(), cents(12_000));
    assert_eq!(rb.total(), cents(59_000));
    assert_eq!(rb.held_total(), cents(29_500));
}

#[test]
fn useful_held_capacity_is_released_and_resets_counter() {
    let mut rb = RateBase::default();
    rb.add(
        plant(1),
        cents(100_000),
        100,
        month(2028, 1),
        plant_at(100.0, 1),
    );
    assert_eq!(
        rb.apply_reserve_review(&review(2028, 100.0)),
        vec![held(plant(1), 100_000)]
    );
    assert_eq!(
        rb.apply_reserve_review(&review(2029, 0.0)),
        vec![released(plant(1), 100_000)]
    );
    assert!(rb.asset(plant(1)).unwrap().held_slice().is_none());
    assert_eq!(rb.held_total(), Money::ZERO);
    assert_eq!(rb.eligible_total(), cents(100_000));

    // Excess returns. Plant 1 is no longer new capital and has no held
    // history, so only this year's plant 2 is held, starting a fresh count.
    rb.add(
        plant(2),
        cents(50_000),
        100,
        month(2030, 1),
        plant_at(50.0, 2),
    );
    assert_eq!(
        rb.apply_reserve_review(&review(2030, 500.0)),
        vec![held(plant(2), 50_000)]
    );
    assert_eq!(
        rb.asset(plant(2))
            .unwrap()
            .held_slice()
            .unwrap()
            .consecutive_excess_reviews(),
        1
    );
    assert_eq!(
        rb.apply_reserve_review(&review(2031, 500.0)),
        vec![disallowed(plant(2), 50_000)]
    );
    let p1 = rb.asset(plant(1)).unwrap();
    assert_eq!(
        (p1.written_off(), p1.eligible()),
        (Money::ZERO, cents(100_000))
    );
}

#[test]
fn partial_release_ages_only_the_still_excess_slice() {
    let mut rb = RateBase::default();
    rb.add(
        plant(1),
        cents(100_000),
        100,
        month(2028, 1),
        plant_at(100.0, 1),
    );
    rb.add(
        line(5),
        cents(10_000),
        100,
        month(2028, 1),
        line_between(1, 0),
    );
    assert_eq!(
        rb.apply_reserve_review(&review(2028, 100.0)),
        vec![held(plant(1), 100_000), held(line(5), 10_000)]
    );

    // 40 of plant 1's 100 held MW are still excess: that slice ages to its
    // second review and is written off; the useful 60% is released. The
    // associated line follows the same split.
    rb.add(
        plant(2),
        cents(80_000),
        100,
        month(2029, 1),
        plant_at(100.0, 2),
    );
    assert_eq!(
        rb.apply_reserve_review(&review(2029, 40.0)),
        vec![
            released(plant(1), 60_000),
            disallowed(plant(1), 40_000),
            released(line(5), 6_000),
            disallowed(line(5), 4_000),
        ]
    );
    let p1 = rb.asset(plant(1)).unwrap();
    assert_eq!(
        (p1.remaining, p1.eligible(), p1.written_off()),
        (cents(60_000), cents(60_000), cents(40_000))
    );
    assert!(p1.held_slice().is_none());
    // All of the excess was absorbed by held capital, so new plant 2 is not held.
    assert!(rb.asset(plant(2)).unwrap().held_slice().is_none());

    // New capital never inherits that history: plant 3 starts at count 1 and
    // is released, not disallowed, when the excess goes away.
    rb.add(
        plant(3),
        cents(50_000),
        100,
        month(2030, 1),
        plant_at(50.0, 3),
    );
    assert_eq!(
        rb.apply_reserve_review(&review(2030, 30.0)),
        vec![held(plant(3), 30_000)]
    );
    assert_eq!(
        rb.asset(plant(3))
            .unwrap()
            .held_slice()
            .unwrap()
            .consecutive_excess_reviews(),
        1
    );
    assert_eq!(
        rb.apply_reserve_review(&review(2031, 0.0)),
        vec![released(plant(3), 30_000)]
    );
    assert_eq!(rb.written_off_total(), cents(44_000));
}

#[test]
fn held_capital_keeps_book_depreciation() {
    let mut rb = RateBase::default();
    rb.add(
        plant(1),
        cents(1_200),
        12,
        month(2028, 1),
        plant_at(100.0, 1),
    );
    assert_eq!(
        rb.apply_reserve_review(&review(2028, 50.0)),
        vec![held(plant(1), 600)]
    );
    for _ in 0..6 {
        // The held half keeps depreciating: the full 100 enters the month.
        assert_eq!(rb.depreciate_month(), cents(100));
        assert_eq!(rb.held_total() + rb.eligible_total(), rb.total());
    }
    assert_eq!(
        (rb.total(), rb.held_total(), rb.eligible_total()),
        (cents(600), cents(300), cents(300))
    );
    // A write-off now removes only the remaining held book value.
    assert_eq!(
        rb.apply_reserve_review(&review(2029, 50.0)),
        vec![disallowed(plant(1), 300)]
    );
    let total: Money = (0..10).map(|_| rb.depreciate_month()).sum();
    assert_eq!(total, cents(300));
    let p1 = rb.asset(plant(1)).unwrap();
    assert_eq!(
        (p1.depreciated(), p1.written_off(), p1.remaining),
        (cents(900), cents(300), Money::ZERO)
    );
}

#[test]
fn estimates_and_duplicate_review_do_not_age_holdings() {
    let mut rb = RateBase::default();
    rb.add(
        plant(1),
        cents(100_000),
        100,
        month(2028, 1),
        plant_at(100.0, 1),
    );

    let before = rb.clone();
    let estimate = rb.preview_reserve_review(&review(2028, 100.0));
    assert_eq!(rb, before);
    assert_eq!(
        estimate.changes,
        vec![CapitalChange::Held {
            asset: plant(1),
            amount: cents(100_000)
        }]
    );
    assert_eq!(
        (estimate.held_total, estimate.eligible_total),
        (cents(100_000), Money::ZERO)
    );
    assert_eq!(
        rb.apply_reserve_review(&review(2028, 100.0)),
        vec![held(plant(1), 100_000)]
    );

    // A duplicate review for the same year is a no-op.
    let after_first = rb.clone();
    assert_eq!(rb.apply_reserve_review(&review(2028, 100.0)), vec![]);
    assert_eq!(rb, after_first);
    assert!(
        rb.preview_reserve_review(&review(2028, 100.0))
            .changes
            .is_empty()
    );

    // Repeated estimates of next year's review do not age the holding.
    for _ in 0..3 {
        let estimate = rb.preview_reserve_review(&review(2029, 100.0));
        assert_eq!(estimate.disallowed(), cents(100_000));
        assert_eq!(rb, after_first);
    }
    assert_eq!(
        rb.asset(plant(1))
            .unwrap()
            .held_slice()
            .unwrap()
            .consecutive_excess_reviews(),
        1
    );
    assert_eq!(
        rb.apply_reserve_review(&review(2029, 0.0)),
        vec![released(plant(1), 100_000)]
    );
}

#[test]
fn excess_is_allocated_proportionally_to_new_firm_capacity() {
    let mut rb = RateBase::default();
    rb.add(
        plant(1),
        cents(30_000),
        100,
        month(2028, 1),
        plant_at(100.0, 1),
    );
    rb.add(
        plant(2),
        cents(10_001),
        100,
        month(2028, 1),
        plant_at(300.0, 2),
    );
    // 200 MW over 400 MW of new firm capacity: each new plant is half held.
    assert_eq!(
        rb.apply_reserve_review(&review(2028, 200.0)),
        vec![held(plant(1), 15_000), held(plant(2), 5_000)]
    );
    // 10_001 × ½ = 5_000.5 rounds half-even to 5_000, and the cent stays eligible.
    assert_eq!(rb.asset(plant(2)).unwrap().eligible(), cents(5_001));
}

#[test]
fn line_follows_lowest_id_new_plant_at_its_endpoints() {
    let mut rb = RateBase::default();
    rb.add(
        plant(1),
        cents(10_000),
        100,
        month(2028, 1),
        plant_at(100.0, 1),
    );
    rb.add(
        plant(2),
        cents(10_000),
        100,
        month(2028, 1),
        plant_at(0.0, 2),
    );
    // Joins two new plants: associated with plant 1, the lowest id.
    rb.add(
        line(3),
        cents(1_000),
        100,
        month(2028, 1),
        line_between(2, 1),
    );
    // Built last year: not associated capital for this review.
    rb.add(
        line(4),
        cents(1_000),
        100,
        month(2027, 12),
        line_between(1, 0),
    );
    // Legacy lines are never held.
    rb.add_legacy(
        line(5),
        cents(1_000),
        100,
        month(2028, 1),
        line_between(1, 0),
    );
    assert_eq!(
        rb.apply_reserve_review(&review(2028, 25.0)),
        vec![held(plant(1), 2_500), held(line(3), 250)]
    );
    assert_eq!(rb.held_total(), cents(2_750));
}

#[test]
fn negative_or_zero_excess_holds_nothing() {
    let mut rb = RateBase::default();
    rb.add(
        plant(1),
        cents(10_000),
        100,
        month(2028, 1),
        plant_at(100.0, 1),
    );
    assert_eq!(rb.apply_reserve_review(&review(2028, -40.0)), vec![]);
    assert_eq!(rb.held_total(), Money::ZERO);
    // An earlier year after a later one is also a no-op.
    rb.add(
        plant(2),
        cents(10_000),
        100,
        month(2027, 1),
        plant_at(100.0, 2),
    );
    assert_eq!(rb.apply_reserve_review(&review(2027, 400.0)), vec![]);
}

// Region validation keeps book lives, costs and asset refs valid, so these
// guards are invariants that panic in every build profile.

#[test]
#[should_panic(expected = "book life")]
fn zero_book_life_is_an_invariant_violation() {
    let mut rb = RateBase::default();
    rb.add(plant(1), cents(999), 0, month(2028, 1), plant_at(100.0, 1));
}

#[test]
#[should_panic(expected = "negative capital cost")]
fn negative_cost_is_an_invariant_violation() {
    let mut rb = RateBase::default();
    rb.add(plant(1), cents(-1), 12, month(2028, 1), plant_at(100.0, 1));
}

#[test]
#[should_panic(expected = "already in the rate base")]
fn duplicate_asset_is_an_invariant_violation() {
    let mut rb = RateBase::default();
    rb.add(plant(1), cents(100), 12, month(2028, 1), plant_at(100.0, 1));
    rb.add_legacy(plant(1), cents(200), 12, month(2028, 1), plant_at(100.0, 1));
}

#[test]
fn one_month_book_life_depreciates_fully_next_month() {
    let mut rb = RateBase::default();
    rb.add(plant(1), cents(999), 1, month(2028, 1), plant_at(100.0, 1));
    assert_eq!(rb.depreciate_month(), cents(999));
    assert_eq!(rb.depreciate_month(), Money::ZERO);
}

// ---- Property: arbitrary capital-review sequences ----

#[derive(Debug, Clone)]
enum CapitalOp {
    AddPlant {
        legacy: bool,
        cost: i64,
        life: u32,
        firm: f64,
        node: u32,
    },
    AddLine {
        legacy: bool,
        cost: i64,
        life: u32,
        from: u32,
        to: u32,
    },
    Depreciate(u32),
    Review(f64),
    Preview(f64),
    NextYear,
}

fn arb_capital_op() -> impl Strategy<Value = CapitalOp> {
    prop_oneof![
        3 => (any::<bool>(), 0i64..1_000_000_000_000, 1u32..480, 0.0f64..500.0, 0u32..4).prop_map(
            |(legacy, cost, life, firm, node)| CapitalOp::AddPlant { legacy, cost, life, firm, node }
        ),
        2 => (any::<bool>(), 0i64..100_000_000_000, 1u32..600, 0u32..4, 0u32..6).prop_map(
            |(legacy, cost, life, from, to)| CapitalOp::AddLine { legacy, cost, life, from, to }
        ),
        2 => (1u32..30).prop_map(CapitalOp::Depreciate),
        3 => (-50.0f64..900.0).prop_map(CapitalOp::Review),
        1 => (-50.0f64..900.0).prop_map(CapitalOp::Preview),
        1 => Just(CapitalOp::NextYear),
    ]
}

fn arb_capital_review_sequence() -> impl Strategy<Value = Vec<CapitalOp>> {
    prop::collection::vec(arb_capital_op(), 1..60)
}

fn check_rate_base(rb: &RateBase) -> Result<(), TestCaseError> {
    let mut remaining = Money::ZERO;
    for a in rb.assets() {
        prop_assert!(a.remaining >= Money::ZERO);
        prop_assert!(a.held() >= Money::ZERO && a.eligible() >= Money::ZERO);
        prop_assert_eq!(a.eligible() + a.held(), a.remaining);
        prop_assert_eq!(a.depreciated() + a.written_off() + a.remaining, a.original);
        if a.legacy {
            prop_assert!(a.held_slice().is_none());
            prop_assert_eq!(a.written_off(), Money::ZERO);
        }
        remaining += a.remaining;
    }
    prop_assert_eq!(rb.total(), remaining);
    prop_assert_eq!(rb.eligible_total() + rb.held_total(), rb.total());
    Ok(())
}

fn change_as_event(change: &CapitalChange) -> DomainEvent {
    change.into_event()
}

proptest! {
    #[test]
    fn capital_adjustments_are_bounded_and_non_cash(actions in arb_capital_review_sequence()) {
        let starting = Money::from_dollars(1_000_000);
        let mut company = Company::new(starting, 70.0);
        let mut now = month(2027, 1);
        let mut next_id = 0u32;
        let mut original = Money::ZERO;
        let mut depreciated = Money::ZERO;
        let mut disallowed_total = Money::ZERO;

        for action in actions {
            let before = company.rate_base().clone();
            let rb = company.rate_base_mut();
            match action {
                CapitalOp::AddPlant { legacy, cost, life, firm, node } => {
                    let (asset, profile) = (plant(next_id), plant_at(firm, node));
                    if legacy {
                        rb.add_legacy(asset, cents(cost), life, now, profile);
                    } else {
                        rb.add(asset, cents(cost), life, now, profile);
                    }
                    next_id += 1;
                    original += cents(cost);
                }
                CapitalOp::AddLine { legacy, cost, life, from, to } => {
                    let (asset, profile) = (line(next_id), line_between(from, to));
                    if legacy {
                        rb.add_legacy(asset, cents(cost), life, now, profile);
                    } else {
                        rb.add(asset, cents(cost), life, now, profile);
                    }
                    next_id += 1;
                    original += cents(cost);
                }
                CapitalOp::Depreciate(n) => {
                    for _ in 0..n {
                        depreciated += rb.depreciate_month();
                        now = now.next();
                    }
                }
                CapitalOp::Review(excess) => {
                    let inputs = review(now.year(), excess);
                    let estimate = rb.preview_reserve_review(&inputs);
                    prop_assert_eq!(&*rb, &before);
                    let events = rb.apply_reserve_review(&inputs);
                    let expected: Vec<DomainEvent> = estimate.changes.iter().map(change_as_event).collect();
                    prop_assert_eq!(&events, &expected);
                    prop_assert_eq!(estimate.held_total, rb.held_total());
                    prop_assert_eq!(estimate.eligible_total, rb.eligible_total());
                    for event in &events {
                        match event {
                            DomainEvent::CapitalHeld { asset, amount }
                            | DomainEvent::CapitalReleased { asset, amount } => {
                                prop_assert!(*amount >= Money::ZERO);
                                prop_assert!(!before.asset(*asset).unwrap().legacy);
                            }
                            DomainEvent::CapitalDisallowed { asset, amount } => {
                                prop_assert!(*amount >= Money::ZERO);
                                prop_assert!(!before.asset(*asset).unwrap().legacy);
                                disallowed_total += *amount;
                            }
                            other => prop_assert!(false, "unexpected event {:?}", other),
                        }
                    }
                    let review_year = now.year();
                    for event in &events {
                        match event {
                            // New holds target only this year's non-legacy capital that was not already held.
                            DomainEvent::CapitalHeld { asset, .. } => {
                                let b = before.asset(*asset).unwrap();
                                prop_assert!(b.held_slice().is_none());
                                prop_assert!(b.is_new_in(review_year));
                            }
                            // Releases and write-offs come only from slices held before the review.
                            DomainEvent::CapitalReleased { asset, .. }
                            | DomainEvent::CapitalDisallowed { asset, .. } => {
                                prop_assert!(before.asset(*asset).unwrap().held_slice().is_some());
                            }
                            _ => {}
                        }
                    }
                    // Held firm capacity never exceeds the excess it was held against.
                    let held_firm: f64 = rb
                        .assets()
                        .filter_map(|a| a.held_firm())
                        .map(Megawatts::value)
                        .sum();
                    // (A review for a year already reviewed is a no-op and holds nothing new.)
                    if rb.last_review_year() != before.last_review_year() {
                        prop_assert!(held_firm <= excess.max(0.0) + 1e-6, "held {held_firm} > excess {excess}");
                    }
                    // A held line slice always follows a plant that is itself still held.
                    for a in rb.assets() {
                        if let Some(followed) = a.held_slice().and_then(HeldSlice::follows) {
                            prop_assert!(matches!(a.asset, AssetRef::Line(_)));
                            prop_assert!(rb.asset(AssetRef::Plant(followed)).unwrap().held_slice().is_some());
                        }
                    }
                    // Write-offs never exceed the book value there was to write off.
                    for (a, b) in rb.assets().zip(before.assets()) {
                        prop_assert!(a.written_off() - b.written_off() <= b.remaining);
                        prop_assert_eq!(a.remaining + (a.written_off() - b.written_off()), b.remaining);
                    }
                }
                CapitalOp::Preview(excess) => {
                    let _ = rb.preview_reserve_review(&review(now.year(), excess));
                    prop_assert_eq!(&*rb, &before);
                }
                CapitalOp::NextYear => {
                    for _ in 0..12 {
                        now = now.next();
                    }
                }
            }
            check_rate_base(company.rate_base())?;
            prop_assert_eq!(company.rate_base().written_off_total(), disallowed_total);
        }

        // Run every asset to the end of its book life: no double recovery.
        for _ in 0..700 {
            depreciated += company.rate_base_mut().depreciate_month();
        }
        check_rate_base(company.rate_base())?;
        prop_assert_eq!(company.rate_base().total(), Money::ZERO);
        prop_assert_eq!(depreciated + disallowed_total, original);
        // Rate-base adjustments never touch cash.
        prop_assert_eq!(company.cash(), starting);
        prop_assert!(company.ledger().is_empty());
    }
}

#[test]
fn accessors_report_the_slice_and_the_review_history() {
    let mut rb = RateBase::default();
    assert_eq!(rb.last_review_year(), None);
    rb.add(
        plant(1),
        cents(100_000),
        120,
        month(2028, 1),
        plant_at(100.0, 1),
    );
    rb.add(
        line(5),
        cents(10_000),
        100,
        month(2028, 1),
        line_between(1, 0),
    );
    assert_eq!(rb.asset(plant(1)).unwrap().months_left(), 120);
    assert_eq!(rb.asset(line(5)).unwrap().months_left(), 100);
    rb.depreciate_month();
    assert_eq!(rb.asset(plant(1)).unwrap().months_left(), 119);

    rb.apply_reserve_review(&review(2028, 25.0));
    assert_eq!(rb.last_review_year(), Some(2028));
    let plant_slice = rb.asset(plant(1)).unwrap().held_slice().unwrap();
    assert_eq!(plant_slice.follows(), None);
    assert_eq!(plant_slice.fraction(), 0.25);
    let line_slice = rb.asset(line(5)).unwrap().held_slice().unwrap();
    assert_eq!(line_slice.follows(), Some(PlantId(1)));
    assert_eq!(line_slice.fraction(), 0.25);
}

#[test]
fn write_off_shrinks_the_live_capacity_and_respreads_depreciation() {
    let mut rb = RateBase::default();
    rb.add(
        plant(1),
        cents(100_000),
        100,
        month(2028, 1),
        plant_at(100.0, 1),
    );
    rb.apply_reserve_review(&review(2028, 100.0));
    // 40 of the 100 held MW are still excess a year on: 40% is written off, 60% released.
    rb.apply_reserve_review(&review(2029, 40.0));
    let a = rb.asset(plant(1)).unwrap();
    assert_eq!(a.written_off(), cents(40_000));
    assert_eq!(a.remaining, cents(60_000));
    assert_eq!(a.live_share, SHARE_UNITS / 10 * 6);
    assert_eq!(a.live_firm(), 100.0 * 0.6);
    // The remaining book value is spread over the remaining months.
    assert_eq!(a.monthly_depreciation, cents(600));
}

#[test]
fn write_off_after_the_book_life_does_not_divide_by_zero() {
    let mut rb = RateBase::default();
    rb.add(
        plant(1),
        cents(1_000),
        1,
        month(2028, 1),
        plant_at(100.0, 1),
    );
    rb.apply_reserve_review(&review(2028, 100.0));
    rb.depreciate_month();
    assert_eq!(rb.asset(plant(1)).unwrap().months_left(), 0);
    // Nothing is left to write off, but the second review must still cope.
    rb.apply_reserve_review(&review(2029, 100.0));
    assert_eq!(rb.total(), Money::ZERO);
}

#[test]
fn share_for_firm_rejects_unusable_requests() {
    let asset =
        |profile| RateBaseAsset::new(plant(1), cents(1_000), 10, month(2028, 1), profile, false);
    let plant_asset = asset(plant_at(100.0, 1));
    assert_eq!(plant_asset.share_for_firm(25.0), SHARE_UNITS / 4);
    assert_eq!(
        plant_asset.share_for_firm(500.0),
        SHARE_UNITS,
        "capped by the live capacity"
    );
    assert_eq!(plant_asset.share_for_firm(0.0), 0);
    assert_eq!(plant_asset.share_for_firm(-5.0), 0);
    assert_eq!(plant_asset.share_for_firm(f64::NAN), 0);
    assert_eq!(asset(plant_at(0.0, 1)).share_for_firm(5.0), 0);
    assert_eq!(asset(plant_at(-3.0, 1)).share_for_firm(5.0), 0);
    assert_eq!(asset(line_between(1, 0)).share_for_firm(5.0), 0);
}

#[test]
fn division_rounds_half_to_even_for_both_signs() {
    let table: [(i128, i128, i128); 13] = [
        (0, 5, 0),
        (1, 3, 0),
        (2, 3, 1),
        (3, 2, 2),
        (5, 2, 2),
        (7, 2, 4),
        (9, 2, 4),
        (10, 4, 2),
        (-1, 3, 0),
        (-2, 3, -1),
        (-5, 2, -2),
        (-7, 2, -4),
        (-9, 2, -4),
    ];
    for (n, d, expected) in table {
        assert_eq!(div_round_half_even(n, d), expected, "{n} / {d}");
    }
    assert_eq!(prorate(cents(1_000), 1, 3), cents(333));
    assert_eq!(prorate(cents(1_000), 2, 3), cents(667));
    assert_eq!(prorate(cents(5), 1, 2), cents(2));
    assert_eq!(scale_share(10, 1, 4), 2);
    assert_eq!(scale_share(10, 3, 4), 8);
}

#[test]
fn allocate_splits_in_proportion_and_degrades_safely() {
    assert_eq!(allocate(f64::NAN, &[1.0, 2.0]), vec![0.0, 0.0]);
    assert_eq!(allocate(-5.0, &[1.0, 2.0]), vec![0.0, 0.0]);
    assert_eq!(allocate(0.0, &[1.0, 2.0]), vec![0.0, 0.0]);
    assert_eq!(allocate(10.0, &[1.0, 2.0]), vec![1.0, 2.0]);
    assert_eq!(allocate(3.0, &[]), Vec::<f64>::new());
    let parts = allocate(1.5, &[1.0, 2.0]);
    assert!((parts[0] - 0.5).abs() < 1e-12 && (parts[1] - 1.0).abs() < 1e-12);
    // The last entry takes the rounding remainder but never more than its weight.
    assert!((parts.iter().sum::<f64>() - 1.5).abs() < 1e-12);
}

#[test]
fn zero_firm_storage_does_not_shield_a_shared_generator_line_in_either_id_order() {
    for (battery, generator) in [(1, 2), (2, 1)] {
        let mut rb = RateBase::default();
        rb.add(
            plant(battery),
            cents(1000),
            180,
            month(2027, 1),
            plant_at(0.0, 10),
        );
        rb.add(
            plant(generator),
            cents(2000),
            180,
            month(2027, 1),
            plant_at(100.0, 20),
        );
        rb.add(
            line(1),
            cents(1000),
            180,
            month(2027, 1),
            line_between(10, 20),
        );
        rb.add(
            line(2),
            cents(1000),
            180,
            month(2027, 1),
            line_between(10, 30),
        );
        let changes = rb.apply_reserve_review(&review(2027, 50.0));
        assert!(changes.contains(&held(plant(generator), 1000)));
        assert!(changes.contains(&held(line(1), 500)));
        assert_eq!(rb.asset(plant(battery)).unwrap().held(), Money::ZERO);
        assert_eq!(rb.asset(line(2)).unwrap().held(), Money::ZERO);
        assert_eq!(
            rb.asset(line(1)).unwrap().held_slice().unwrap().follows(),
            Some(PlantId(generator))
        );
    }
}
