//! Construction commitments and scenarios extrapolated from completed operating cash flows.
use domain::{
    company::{AssetRef, RateBase},
    error::DomainError,
    regulation::{RateReviewInputs, YearTally, compute_rate_review},
    session::{DayPhase, GameSession, MonthlyReport, PlantQuote},
    time::SimMonth,
    units::{MegawattHours, Megawatts, Money, Price},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomeRange {
    pub samples: usize,
    pub low: Money,
    pub mean: Money,
    pub high: Money,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CashMonth {
    pub month: SimMonth,
    pub committed: Money,
    pub proposed: Money,
    pub capital_only: Money,
    pub baseline: Money,
    pub low: Money,
    pub mean: Money,
    pub high: Money,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CashTariffChange {
    pub month: SimMonth,
    pub prices: [Price; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct CashOutlook {
    pub current: Money,
    pub income: Option<IncomeRange>,
    pub months: Vec<CashMonth>,
    pub tariff_changes: Vec<CashTariffChange>,
}

fn income_range(
    history: &[&MonthlyReport],
    tariffs: [Price; 3],
) -> Result<Option<IncomeRange>, DomainError> {
    if history.is_empty() {
        return Ok(None);
    }
    let mut scenarios = Vec::new();
    for tariff in tariffs {
        scenarios.push(
            history
                .iter()
                .map(|r| {
                    money(
                        i128::from(r.net.cents())
                            - i128::from(r.construction.cents())
                            - i128::from(r.revenue.cents())
                            + i128::from(tariff.cost_of(r.served, 1).cents()),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    Ok(Some(IncomeRange {
        samples: history.len(),
        low: *scenarios[0].iter().min().expect("nonempty history"),
        mean: money(
            scenarios[1]
                .iter()
                .map(|v| i128::from(v.cents()))
                .sum::<i128>()
                / history.len() as i128,
        )?,
        high: *scenarios[2].iter().max().expect("nonempty history"),
    }))
}

/// Translate allowed-return bounds through the actual tariff formula, holding recent operations
/// constant and depreciating today's assets. Future commissions and reserve decisions are unknown.
fn review_tariffs(
    session: &GameSession,
    history: &[&MonthlyReport],
    capital: &RateBase,
) -> Result<Option<[Price; 3]>, DomainError> {
    let served = history.iter().map(|r| r.served.value()).sum::<f64>();
    if served <= 0.0 {
        return Ok(None);
    }
    let cost = history
        .iter()
        .map(|r| {
            i128::from(r.revenue.cents()) - i128::from(r.net.cents())
                + i128::from(r.construction.cents())
        })
        .sum();
    let mut depreciating = capital.clone();
    let depreciation = money(
        (0..history.len())
            .map(|_| i128::from(depreciating.depreciate_month().cents()))
            .sum(),
    )?;
    let inputs = RateReviewInputs {
        satisfaction: session.company().satisfaction(),
        tally: YearTally {
            served: MegawattHours::new(served),
            operating_cost: money(cost)?,
            months: history.len() as u8,
            ..Default::default()
        },
        depreciation,
        eligible_rate_base: capital.eligible_total(),
        firm_capacity: Megawatts::ZERO,
        projected_peak: Megawatts::ZERO,
        previous_tariff: session.regulator().tariff(),
        previous_cost_per_mwh: None,
    };
    let params = session.region().regulator();
    Ok(Some(
        [params.min_return, params.base_return, params.max_return].map(|rate| {
            let mut params = params.clone();
            params.min_return = rate;
            params.max_return = rate;
            compute_rate_review(&inputs, &params).tariff
        }),
    ))
}

fn money(cents: i128) -> Result<Money, DomainError> {
    i64::try_from(cents)
        .map(Money::from_cents)
        .map_err(|_| DomainError::ConstructionOverflow)
}

/// Actual local quote payments, conditional on successful geothermal exploration.
pub fn quote_payments(quote: PlantQuote) -> Vec<Money> {
    if let Some(exploration) = quote.exploration {
        let mut payments = exploration.capital_at_risk.split_even(exploration.months);
        payments.extend(
            (quote.capital - exploration.capital_at_risk)
                .split_even(quote.months - exploration.months),
        );
        payments
    } else {
        quote.capital.split_even(quote.months)
    }
}

fn payment_queues(session: &GameSession) -> Vec<Vec<Money>> {
    let mut queues: Vec<_> = session
        .construction_instalments()
        .iter()
        .map(|(asset, queue)| {
            let mut payments: Vec<_> = queue.iter().copied().collect();
            // Exploration queues only hold the at-risk stage. The rest is installed after success;
            // include that conditional stage here so existing projects and previews agree.
            if let AssetRef::Plant(id) = asset
                && let Some(plant) = session.power().plant(*id)
                && let Some(field) = plant.geothermal()
                && field.exploration_left > 0
            {
                let params = session
                    .region()
                    .geothermal()
                    .expect("geothermal project parameters");
                payments.extend(
                    (plant.spec().capital_cost - field.exploration_capital)
                        .split_even(plant.spec().build_months - params.exploration_months),
                );
            }
            payments
        })
        .collect();
    queues.extend(session.power().plants().values().filter_map(|plant| {
        plant
            .geothermal()?
            .makeup
            .as_ref()
            .map(|project| project.payments.iter().copied().collect())
    }));
    queues
}

/// Read-only cash scenarios, capped at 120 months. Historical construction is excluded from the
/// operating run rate so it is never charged a second time. No confidence level is implied.
pub fn cash_outlook(
    session: &GameSession,
    history: &[MonthlyReport],
    candidate: Option<PlantQuote>,
) -> Result<CashOutlook, DomainError> {
    let history: Vec<_> = history.iter().rev().take(12).collect();
    let income = income_range(&history, [session.regulator().tariff(); 3])?;
    let mut active_income = income.clone();
    let mut capital = session.company().rate_base().clone();
    let mut tariff_changes = Vec::new();
    let proposed = candidate.map(quote_payments).unwrap_or_default();
    let queues = payment_queues(session);
    let horizon = queues
        .iter()
        .map(|q| q.len())
        .chain([proposed.len(), 12])
        .max()
        .unwrap_or(12)
        .min(120);
    let current = session.company_view().cash;
    let first = session
        .clock()
        .month()
        .checked_add_months(u32::from(session.phase() == DayPhase::Review))?;
    let (mut committed_total, mut proposed_total) = (0i128, 0i128);
    let (mut low, mut mean, mut high) = (0i128, 0i128, 0i128);
    let mut months = Vec::with_capacity(horizon);
    for index in 0..horizon {
        let month = first.checked_add_months(index as u32)?;
        if month.month() == 1
            && month.year() > session.clock().month().year()
            // December Review has already set January's tariff. It is known, not a scenario.
            && !(index == 0 && session.phase() == DayPhase::Review && session.clock().month().month() == 12)
            && let Some(prices) = review_tariffs(session, &history, &capital)?
        {
            active_income = income_range(&history, prices)?;
            tariff_changes.push(CashTariffChange { month, prices });
        }
        let committed = queues
            .iter()
            .filter_map(|q| q.get(index))
            .map(|m| i128::from(m.cents()))
            .sum::<i128>();
        let proposed = proposed.get(index).map_or(0, |m| i128::from(m.cents()));
        committed_total += committed;
        proposed_total += proposed;
        let capital_only = i128::from(current.cents()) - committed_total - proposed_total;
        if let Some(income) = &active_income {
            low += i128::from(income.low.cents());
            mean += i128::from(income.mean.cents());
            high += i128::from(income.high.cents());
        }
        months.push(CashMonth {
            month,
            committed: money(committed)?,
            proposed: money(proposed)?,
            capital_only: money(capital_only)?,
            baseline: money(i128::from(current.cents()) - committed_total + mean)?,
            low: money(capital_only + low)?,
            mean: money(capital_only + mean)?,
            high: money(capital_only + high)?,
        });
        capital.depreciate_month();
    }
    Ok(CashOutlook {
        current,
        income,
        months,
        tariff_changes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projections::test_support::{report, session};
    use domain::{ids::CellCoord, region::PlantKind};

    #[test]
    fn december_review_uses_the_tariff_already_awarded_for_january() {
        let mut parts = domain::region::fixtures::tiny_region_parts();
        parts.start.month = SimMonth::new(2027, 12).unwrap();
        let mut session = GameSession::new(
            std::sync::Arc::new(domain::region::RegionDefinition::try_from(parts).unwrap()),
            1,
            Default::default(),
        );
        session.begin_operating().unwrap();
        for _ in 0..domain::time::TICKS_PER_DAY {
            session.tick().unwrap();
        }
        let mut history = report(2027, 12, 0.0);
        history.served = MegawattHours::new(1000.0);
        history.revenue = Money::from_cents(100_000);
        history.net = Money::from_cents(50_000);
        let result = cash_outlook(&session, &[history.clone()], None).unwrap();
        assert_eq!(result.months[0].month, SimMonth::new(2028, 1).unwrap());
        assert!(result.tariff_changes.is_empty());
        assert_eq!(
            result.months[0].mean.cents() - result.current.cents(),
            session
                .regulator()
                .tariff()
                .cost_of(history.served, 1)
                .cents()
                - 50_000
        );
    }

    #[test]
    fn january_tariff_band_changes_future_income_without_repricing_prior_cash() {
        let mut parts = domain::region::fixtures::tiny_region_parts();
        parts.start.month = SimMonth::new(2027, 11).unwrap();
        let session = GameSession::new(
            std::sync::Arc::new(domain::region::RegionDefinition::try_from(parts).unwrap()),
            1,
            Default::default(),
        );
        let mut report = report(2027, 10, 0.0);
        report.served = MegawattHours::new(1000.0);
        report.revenue = session.regulator().tariff().cost_of(report.served, 1);
        report.net = report.revenue - Money::from_cents(50_000);
        let result = cash_outlook(&session, &[report.clone()], None).unwrap();
        let before = result.current.cents();
        assert_eq!(result.months[0].mean.cents(), before + report.net.cents());
        assert_eq!(
            result.months[1].mean.cents(),
            before + 2 * report.net.cents()
        );
        let change = &result.tariff_changes[0];
        assert_eq!(change.month, SimMonth::new(2028, 1).unwrap());
        assert!(change.prices[0].value() < change.prices[2].value());
        let december = &result.months[1];
        let january = &result.months[2];
        assert_eq!(
            january.mean.cents() - december.mean.cents(),
            change.prices[1].cost_of(report.served, 1).cents() - 50_000
        );
        assert_eq!(
            january.low.cents() - december.low.cents(),
            change.prices[0].cost_of(report.served, 1).cents() - 50_000
        );
        assert_eq!(
            january.high.cents() - december.high.cents(),
            change.prices[2].cost_of(report.served, 1).cents() - 50_000
        );
        assert_ne!(
            january.mean.cents() - december.mean.cents(),
            report.net.cents()
        );
    }

    #[test]
    fn review_starts_next_month_and_only_charges_remaining_instalments() {
        let mut session = session();
        let terrain = session.region().terrain();
        let cell = (0..terrain.height())
            .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
            .find(|at| session.plant_quote(PlantKind::Battery, *at).is_ok())
            .unwrap();
        session.build_plant(PlantKind::Battery, cell).unwrap();
        session.begin_operating().unwrap();
        for _ in 0..domain::time::TICKS_PER_DAY {
            session.tick().unwrap();
        }
        assert_eq!(session.phase(), DayPhase::Review);
        let remaining: i64 = session
            .construction_instalments()
            .values()
            .flatten()
            .map(|m| m.cents())
            .sum();
        let result = cash_outlook(&session, &[], None).unwrap();
        assert_eq!(result.months[0].month, session.clock().month().next());
        assert_eq!(
            result
                .months
                .iter()
                .map(|m| m.committed.cents())
                .sum::<i64>(),
            remaining
        );
        assert_eq!(
            result.months.last().unwrap().capital_only.cents(),
            result.current.cents() - remaining
        );
    }

    #[test]
    fn makeup_drilling_payments_are_included_even_though_they_have_a_separate_queue() {
        let mut parts = domain::region::fixtures::tiny_region_parts();
        let starter = session();
        let terrain = starter.region().terrain();
        let cell = (0..terrain.height())
            .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
            .find(|at| starter.plant_quote(PlantKind::Geothermal, *at).is_ok())
            .unwrap();
        parts.start.fleet.push(domain::region::StartPlantDef {
            kind: PlantKind::Geothermal,
            cell,
        });
        // Exercise field decline quickly using the fixture's existing ceiling step.
        let params = parts.geothermal.as_mut().unwrap();
        params.monthly_decline = params.ceiling_step;
        let mut session = GameSession::new(
            std::sync::Arc::new(domain::region::RegionDefinition::try_from(parts).unwrap()),
            1,
            Default::default(),
        );
        for _ in 0..2 {
            session.begin_operating().unwrap();
            for _ in 0..domain::time::TICKS_PER_DAY {
                session.tick().unwrap();
            }
            session.finish_review().unwrap();
        }
        let plant = session
            .plant_views()
            .into_iter()
            .find(|p| p.kind == PlantKind::Geothermal)
            .unwrap()
            .id;
        let quote = session.makeup_quote(plant).unwrap();
        session.drill_makeup_wells(plant).unwrap();
        assert!(session.construction_instalments().is_empty());
        let before = session.state_hash();
        let forecast = cash_outlook(&session, &[], None).unwrap();
        assert_eq!(
            forecast
                .months
                .iter()
                .map(|m| m.committed.cents())
                .sum::<i64>(),
            quote.capital.cents()
        );
        assert_eq!(session.state_hash(), before);
    }

    #[test]
    fn geothermal_preview_uses_the_same_staged_payments_as_an_order() {
        let mut session = session();
        let terrain = session.region().terrain();
        let cell = (0..terrain.height())
            .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
            .find(|at| session.plant_quote(PlantKind::Geothermal, *at).is_ok())
            .unwrap();
        let quote = session.plant_quote(PlantKind::Geothermal, cell).unwrap();
        let preview = cash_outlook(&session, &[], Some(quote)).unwrap();
        session.build_plant(PlantKind::Geothermal, cell).unwrap();
        let ordered = cash_outlook(&session, &[], None).unwrap();
        assert_eq!(
            preview
                .months
                .iter()
                .map(|m| m.proposed)
                .collect::<Vec<_>>(),
            ordered
                .months
                .iter()
                .map(|m| m.committed)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn construction_is_not_double_counted_as_operating_loss() {
        let session = session();
        let mut a = report(2027, 1, 0.0);
        a.net = Money::from_cents(-700);
        a.construction = Money::from_cents(-1000);
        let mut b = a.clone();
        b.net = Money::from_cents(-300);
        let result = cash_outlook(&session, &[a, b], None).unwrap();
        assert_eq!(
            result.income,
            Some(IncomeRange {
                samples: 2,
                low: Money::from_cents(300),
                mean: Money::from_cents(500),
                high: Money::from_cents(700),
            })
        );
        assert_eq!(result.months[1].mean.cents(), result.current.cents() + 1000);
    }

    #[test]
    fn unpaid_queues_and_proposed_plant_reduce_cash_without_changing_session() {
        let mut session = session();
        let terrain = session.region().terrain();
        let mut cells = (0..terrain.height())
            .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
            .filter(|at| session.plant_quote(PlantKind::Battery, *at).is_ok());
        let cell = cells.next().unwrap();
        let candidate = session
            .plant_quote(PlantKind::Battery, cells.next().unwrap())
            .unwrap();
        session.build_plant(PlantKind::Battery, cell).unwrap();
        let before = session.state_hash();
        let outlook = cash_outlook(&session, &[], Some(candidate)).unwrap();
        assert!(outlook.income.is_none());
        let committed: i64 = session
            .construction_instalments()
            .values()
            .flatten()
            .map(|m| m.cents())
            .sum();
        let last = outlook.months.last().unwrap();
        assert_eq!(
            last.mean.cents(),
            outlook.current.cents() - committed - candidate.capital.cents()
        );
        assert_eq!(last.baseline.cents(), outlook.current.cents() - committed);
        assert_eq!(session.state_hash(), before);
    }

    proptest::proptest! {
        #[test]
        fn income_bounds_and_candidate_cost_hold_for_every_month(a in -100_000i64..100_000, b in -100_000i64..100_000, capital in 0i64..1_000_000) {
            let session = session();
            let mut first = report(2027, 1, 0.0);
            first.net = Money::from_cents(a); first.construction = Money::ZERO;
            let mut second = first.clone(); second.net = Money::from_cents(b);
            let candidate = PlantQuote { capital: Money::from_cents(capital), months: 3, exploration: None, economic_zone: Default::default() };
            let result = cash_outlook(&session, &[first, second], Some(candidate)).unwrap();
            let mut paid = 0;
            for month in result.months {
                paid += month.proposed.cents();
                proptest::prop_assert!(month.low <= month.mean && month.mean <= month.high);
                proptest::prop_assert_eq!(month.baseline.cents() - month.mean.cents(), paid);
            }
            proptest::prop_assert_eq!(paid, capital);
        }
    }
}
