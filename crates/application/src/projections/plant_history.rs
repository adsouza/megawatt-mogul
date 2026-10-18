//! Per-plant lifetime counters: starts, trips and energy produced.

use std::collections::BTreeMap;

use domain::events::DomainEvent;
use domain::ids::PlantId;
use domain::session::GameSession;
use domain::units::MegawattHours;

use super::Projection;

/// One plant's record so far.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlantHistory {
    pub starts: u32,
    pub trips: u32,
    /// Weighted energy from the merit order of every closed month.
    pub energy: MegawattHours,
}

impl PlantHistory {
    fn new() -> Self {
        Self {
            starts: 0,
            trips: 0,
            energy: MegawattHours::new(0.0),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PlantHistoryProjection {
    plants: BTreeMap<PlantId, PlantHistory>,
}

impl PlantHistoryProjection {
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
            DomainEvent::MonthClosed(report) => {
                for entry in &report.merit_order {
                    self.entry(entry.plant).energy += entry.energy;
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
    use domain::session::MeritOrderEntry;
    use domain::units::Price;

    #[test]
    fn plant_history_counts_starts_and_trips() {
        let session = session();
        let plant = PlantId(1);
        let mut history = PlantHistoryProjection::default();
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
        let mut history = PlantHistoryProjection::default();
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
}
