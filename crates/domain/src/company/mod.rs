//! The player's utility company: its cash ledger, rate base and customer
//! satisfaction.

pub mod ledger;
pub mod rate_base;
pub mod satisfaction;

pub use ledger::{AssetRef, LedgerCategory, LedgerEntry};
pub use rate_base::{
    AssetProfile, CapitalAssessment, CapitalChange, HeldSlice, RateBase, RateBaseAsset,
    ReserveReviewInputs,
};
pub use satisfaction::satisfaction_target;

use std::collections::VecDeque;

use crate::events::DomainEvent;
use crate::region::{RegulatorParams, SatisfactionParams};
use crate::units::{MegawattHours, Money, Price, Ratio};

/// Months of served/demand history behind [`Company::reliability`].
const RELIABILITY_WINDOW: usize = 3;
/// Consecutive negative-cash month closes that make the company insolvent.
const INSOLVENT_AFTER_CLOSES: u8 = 3;

/// The player's utility: cash and its ledger, the rate base, and customer
/// satisfaction.
#[derive(Debug, Clone, PartialEq)]
pub struct Company {
    starting_cash: Money,
    cash: Money,
    ledger: Vec<LedgerEntry>,
    rate_base: RateBase,
    satisfaction: f64,
    /// `(served, demand)` for the most recent months, oldest first.
    reliability_window: VecDeque<(MegawattHours, MegawattHours)>,
    negative_closes: u8,
}

impl Company {
    pub fn new(starting_cash: Money, satisfaction: f64) -> Self {
        Self {
            starting_cash,
            cash: starting_cash,
            ledger: Vec::new(),
            rate_base: RateBase::default(),
            satisfaction,
            reliability_window: VecDeque::with_capacity(RELIABILITY_WINDOW),
            negative_closes: 0,
        }
    }

    /// Books a cash movement and reports it.
    pub fn record(&mut self, entry: LedgerEntry) -> DomainEvent {
        self.cash += entry.amount;
        self.ledger.push(entry.clone());
        DomainEvent::LedgerEntryRecorded(entry)
    }

    pub fn cash(&self) -> Money {
        self.cash
    }

    pub fn starting_cash(&self) -> Money {
        self.starting_cash
    }

    pub fn ledger(&self) -> &[LedgerEntry] {
        &self.ledger
    }

    /// Sum of every ledger entry: always `cash − starting_cash`.
    pub fn ledger_sum(&self) -> Money {
        self.ledger.iter().map(|e| e.amount).sum()
    }

    pub fn rate_base(&self) -> &RateBase {
        &self.rate_base
    }

    pub fn rate_base_mut(&mut self) -> &mut RateBase {
        &mut self.rate_base
    }

    /// Customer satisfaction, `0.0..=100.0`.
    pub fn satisfaction(&self) -> f64 {
        self.satisfaction
    }

    /// Records one month's served and demanded energy, keeping the last three.
    pub fn push_reliability(&mut self, served: MegawattHours, demand: MegawattHours) {
        if self.reliability_window.len() == RELIABILITY_WINDOW {
            self.reliability_window.pop_front();
        }
        self.reliability_window.push_back((served, demand));
    }

    /// Served over demanded energy across the window; one when nothing was demanded.
    pub fn reliability(&self) -> Ratio {
        let served: MegawattHours = self.reliability_window.iter().map(|(s, _)| *s).sum();
        let demand: MegawattHours = self.reliability_window.iter().map(|(_, d)| *d).sum();
        if demand.value() <= 0.0 {
            return Ratio::ONE;
        }
        Ratio::new((served.value() / demand.value()).clamp(0.0, 1.0)).unwrap_or(Ratio::ONE)
    }

    /// Moves satisfaction a smoothing step toward [`satisfaction_target`]:
    /// `s += α × (target − s)`.
    pub fn update_satisfaction(
        &mut self,
        reliability: Ratio,
        tariff: Price,
        regulator: &RegulatorParams,
        params: &SatisfactionParams,
    ) {
        let target = satisfaction_target(
            reliability,
            tariff,
            regulator.reference_tariff,
            regulator.reliability_target,
            params,
        );
        self.satisfaction += params.smoothing.value() * (target - self.satisfaction);
    }

    /// Closes the month for solvency: true once cash has been negative at
    /// three consecutive closes. A non-negative close resets the count.
    pub fn close_solvency_month(&mut self) -> bool {
        if self.cash.is_negative() {
            self.negative_closes = self.negative_closes.saturating_add(1);
        } else {
            self.negative_closes = 0;
        }
        self.negative_closes >= INSOLVENT_AFTER_CLOSES
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::fixtures::tiny_region;
    use crate::time::SimMonth;
    use crate::units::{MegawattHours, Money, Price, Ratio};
    use proptest::prelude::*;

    fn entry(amount: Money) -> LedgerEntry {
        LedgerEntry {
            month: SimMonth::new(2028, 1).unwrap(),
            category: LedgerCategory::Revenue,
            amount,
            subject: None,
        }
    }

    proptest! {
        #[test]
        fn cash_change_equals_ledger_sum(amounts in prop::collection::vec(-1_000_000_000i64..1_000_000_000, 0..100)) {
            let mut c = Company::new(Money::from_dollars(5), 70.0);
            for a in amounts { c.record(entry(Money::from_cents(a))); }
            prop_assert_eq!(c.cash() - c.starting_cash(), c.ledger_sum());
        }
    }

    #[test]
    fn record_returns_ledger_entry_recorded() {
        let mut c = Company::new(Money::ZERO, 70.0);
        let e = LedgerEntry {
            subject: Some(AssetRef::Plant(crate::ids::PlantId(3))),
            category: LedgerCategory::Fuel,
            ..entry(Money::from_cents(-250))
        };
        assert_eq!(
            c.record(e.clone()),
            DomainEvent::LedgerEntryRecorded(e.clone())
        );
        assert_eq!(c.ledger(), &[e]);
        assert_eq!(c.cash(), Money::from_cents(-250));
    }

    #[test]
    fn bankrupt_after_three_negative_closes() {
        let mut c = Company::new(Money::from_dollars(5), 70.0);
        c.record(entry(Money::from_dollars(-10)));
        assert!(!c.close_solvency_month());
        assert!(!c.close_solvency_month());
        c.record(entry(Money::from_dollars(10)));
        assert!(
            !c.close_solvency_month(),
            "non-negative cash resets the count"
        );
        c.record(entry(Money::from_dollars(-10)));
        assert!(!c.close_solvency_month());
        assert!(!c.close_solvency_month());
        assert!(c.close_solvency_month());
    }

    #[test]
    fn reliability_window_keeps_three_months() {
        let mut c = Company::new(Money::ZERO, 70.0);
        assert_eq!(c.reliability(), Ratio::ONE);
        for served in [0.0, 90.0, 80.0, 100.0] {
            c.push_reliability(MegawattHours::new(served), MegawattHours::new(100.0));
        }
        assert_eq!(c.reliability(), Ratio::new(0.9).unwrap());
    }

    #[test]
    fn reliability_is_one_without_demand() {
        let mut c = Company::new(Money::ZERO, 70.0);
        c.push_reliability(MegawattHours::ZERO, MegawattHours::ZERO);
        assert_eq!(c.reliability(), Ratio::ONE);
    }

    #[test]
    fn satisfaction_moves_by_smoothing() {
        let region = tiny_region();
        let mut c = Company::new(Money::ZERO, 50.0);
        c.update_satisfaction(
            Ratio::new(0.999).unwrap(),
            Price::per_mwh(100.0),
            region.regulator(),
            region.satisfaction(),
        );
        assert_eq!(c.satisfaction(), 60.0);
    }
}
