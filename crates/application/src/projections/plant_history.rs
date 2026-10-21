//! Per-plant lifetime counters: starts, trips and energy produced.

use std::collections::BTreeMap;

use domain::events::DomainEvent;
use domain::ids::PlantId;
use domain::session::GameSession;
use domain::units::{MegawattHours, Money, Price};

use super::Projection;

/// One plant's record so far.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlantHistory {
    pub starts: u32,
    pub trips: u32,
    /// Weighted energy from the merit order of every closed month.
    pub energy: MegawattHours,
    /// What that energy earned at the tariff in force each month, rounded to
    /// cents once per plant per month. A read-side figure for display: the
    /// ledger books revenue for the company as a whole, not per plant.
    pub revenue: Money,
}

impl PlantHistory {
    fn new() -> Self {
        Self {
            starts: 0,
            trips: 0,
            energy: MegawattHours::new(0.0),
            revenue: Money::ZERO,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlantHistoryProjection {
    plants: BTreeMap<PlantId, PlantHistory>,
    /// The tariff in force for the month about to close.
    tariff: Price,
    /// A tariff a rate review set, in force from the next month.
    next_tariff: Option<Price>,
}

impl PlantHistoryProjection {
    /// Empty history. Months are priced from the region's starting tariff, not
    /// the session's current one, so a projection rebuilt by replaying events
    /// into a later session still prices every month as it was.
    pub fn new(session: &GameSession) -> Self {
        Self {
            plants: BTreeMap::new(),
            tariff: session.region().regulator().starting_tariff,
            next_tariff: None,
        }
    }

    /// `None` until the plant has started, tripped or produced energy.
    pub fn get(&self, plant: PlantId) -> Option<&PlantHistory> {
        self.plants.get(&plant)
    }

    fn entry(&mut self, plant: PlantId) -> &mut PlantHistory {
        self.plants.entry(plant).or_insert_with(PlantHistory::new)
    }
}

impl Projection for PlantHistoryProjection {
    fn apply(&mut self, event: &DomainEvent, _session: &GameSession) {
        match event {
            DomainEvent::PlantStarting { plant } => self.entry(*plant).starts += 1,
            DomainEvent::PlantTripped { plant } => self.entry(*plant).trips += 1,
            // December's review emits its event just before that month's
            // `MonthClosed`, but the month was sold at the old tariff.
            DomainEvent::RateReviewCompleted { tariff, .. } => self.next_tariff = Some(*tariff),
            DomainEvent::MonthClosed(report) => {
                let tariff = self.tariff;
                for entry in &report.merit_order {
                    let record = self.entry(entry.plant);
                    record.energy += entry.energy;
                    // The entry's energy is already weighted by the month's
                    // days: one rounding, to cents, ties to even.
                    record.revenue += tariff.cost_of(entry.energy, 1);
                }
                if let Some(next) = self.next_tariff.take() {
                    self.tariff = next;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projections::test_support::{report, session};
    use domain::region::PlantKind;
    use domain::region::RegionDefinition;
    use domain::region::fixtures::tiny_region_parts;
    use domain::session::GameSettings;
    use domain::session::MeritOrderEntry;
    use domain::units::{Money, Price, Ratio};
    use std::sync::Arc;

    #[test]
    fn plant_history_counts_starts_and_trips() {
        let session = session();
        let plant = PlantId(1);
        let mut history = PlantHistoryProjection::new(&session);
        assert_eq!(history.get(plant), None);
        for event in [
            DomainEvent::PlantStarting { plant },
            DomainEvent::PlantTripped { plant },
            DomainEvent::PlantStarting { plant },
        ] {
            history.apply(&event, &session);
        }
        let record = history.get(plant).unwrap();
        assert_eq!((record.starts, record.trips), (2, 1));
        assert_eq!(history.get(PlantId(2)), None);
    }

    #[test]
    fn plant_history_accumulates_weighted_energy_over_months() {
        let session = session();
        let mut history = PlantHistoryProjection::new(&session);
        for (month, mwh) in [(3, 1_000.0), (4, 500.0)] {
            let mut r = report(2027, month, 100.0);
            r.merit_order = vec![MeritOrderEntry {
                plant: PlantId(1),
                kind: PlantKind::Coal,
                marginal_cost: Price::per_mwh(20.0),
                energy: MegawattHours::new(mwh),
            }];
            history.apply(&DomainEvent::MonthClosed(Box::new(r)), &session);
        }
        assert_eq!(
            history.get(PlantId(1)).unwrap().energy,
            MegawattHours::new(1_500.0)
        );
    }

    /// A session whose region starts at a $1/MWh tariff, so the cents of a
    /// month's revenue are exactly the energy in eighths of a MWh.
    fn dollar_tariff_session() -> GameSession {
        let mut parts = tiny_region_parts();
        parts.regulator.starting_tariff = Price::per_mwh(1.0);
        GameSession::new(
            Arc::new(RegionDefinition::try_from(parts).unwrap()),
            1,
            GameSettings::default(),
        )
    }

    fn closed_month(month: u8, plant: PlantId, mwh: f64) -> DomainEvent {
        let mut r = report(2027, month, 100.0);
        r.merit_order = vec![MeritOrderEntry {
            plant,
            kind: PlantKind::Coal,
            marginal_cost: Price::per_mwh(20.0),
            energy: MegawattHours::new(mwh),
        }];
        DomainEvent::MonthClosed(Box::new(r))
    }

    #[test]
    fn plant_revenue_is_energy_times_tariff_rounded_once() {
        let session = dollar_tariff_session();
        let plant = PlantId(1);
        let mut history = PlantHistoryProjection::new(&session);
        assert_eq!(history.get(plant), None);

        // 0.125 MWh at $1 is 12.5 cents: ties go to even, 12. Rounding once
        // per month gives 24 cents for two such months; summing the exact
        // 25 cents and rounding at the end would not.
        history.apply(&closed_month(3, plant, 0.125), &session);
        assert_eq!(history.get(plant).unwrap().revenue, Money::from_cents(12));
        history.apply(&closed_month(4, plant, 0.125), &session);
        assert_eq!(history.get(plant).unwrap().revenue, Money::from_cents(24));

        // The cents between 13 and 14 round to the even 14.
        history.apply(&closed_month(5, plant, 0.135), &session);
        let expected = Money::from_cents(24 + (13.5_f64).round_ties_even() as i64);
        assert_eq!(history.get(plant).unwrap().revenue, expected);
        assert_eq!(expected, Money::from_cents(38));
    }

    #[test]
    fn plant_revenue_uses_the_tariff_in_force_that_month() {
        let session = dollar_tariff_session();
        let plant = PlantId(1);
        let mut history = PlantHistoryProjection::new(&session);
        // December's review sets a new tariff, and its event arrives before
        // December's own `MonthClosed`; December was still sold at $1.
        history.apply(
            &DomainEvent::RateReviewCompleted {
                tariff: Price::per_mwh(2.0),
                allowed_return: Ratio::ZERO,
            },
            &session,
        );
        history.apply(&closed_month(12, plant, 1.0), &session);
        assert_eq!(history.get(plant).unwrap().revenue, Money::from_cents(100));
        // January is the first month at $2.
        history.apply(&closed_month(1, plant, 1.0), &session);
        assert_eq!(history.get(plant).unwrap().revenue, Money::from_cents(300));
    }

    #[test]
    fn plant_revenue_starts_from_the_regions_tariff_not_the_sessions_current_one() {
        // Rebuilt projections replay events against the region's starting
        // tariff whatever the session's tariff has since become.
        let session = dollar_tariff_session();
        let history = PlantHistoryProjection::new(&session);
        assert_eq!(history.tariff, session.region().regulator().starting_tariff);
    }
}
