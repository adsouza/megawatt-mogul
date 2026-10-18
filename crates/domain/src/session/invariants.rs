//! Whole-session invariants, checked by tests and debug tooling. A violation
//! is a bug in the simulation, never a consequence of player input.

use thiserror::Error;

use super::GameSession;
use crate::generation::PlantStatus;
use crate::grid::TickSummary;
use crate::ids::PlantId;
use crate::region::PlantKind;
use crate::units::{Megawatts, Money};

/// Tolerance for float comparisons, in MW. Tick quantities are sums of a few
/// dozen terms of order 1e3 MW, so honest rounding stays far below this.
const TOLERANCE: f64 = 1e-6;

/// The first invariant found broken.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum InvariantViolation {
    #[error("cash moved by {cash_delta:?} but the ledger sums to {ledger_sum:?}")]
    CashLedgerMismatch {
        cash_delta: Money,
        ledger_sum: Money,
    },
    #[error("plant {plant:?} output is outside its bounds")]
    PlantOutOfBounds { plant: PlantId },
    #[error("tick {tick} does not conserve energy")]
    ConservationBroken { tick: u32 },
}

impl GameSession {
    /// Checks, in order: that cash less starting cash equals the ledger sum;
    /// that every online dispatchable plant's output lies within
    /// `[min_output, capacity]` (hydro may fall to zero, since its reservoir
    /// caps output below minimum); and that the last tick summary satisfies
    /// `generation = Σ by_plant = served + imbalance` and
    /// `served + unserved = demand`.
    pub fn check_invariants(&self) -> Result<(), InvariantViolation> {
        let cash_delta = self.company.cash() - self.company.starting_cash();
        let ledger_sum = self.company.ledger_sum();
        if cash_delta != ledger_sum {
            return Err(InvariantViolation::CashLedgerMismatch {
                cash_delta,
                ledger_sum,
            });
        }
        for plant in self.power.plants().values() {
            let PlantStatus::Online { output, .. } = plant.status() else {
                continue;
            };
            let spec = plant.spec();
            if !spec.kind.is_dispatchable() {
                continue;
            }
            let min = if spec.kind == PlantKind::Hydro {
                Megawatts::ZERO
            } else {
                spec.min_output()
            };
            let below = min.value() - output.value() > TOLERANCE;
            let above = output.value() - spec.capacity.value() > TOLERANCE;
            if below || above {
                return Err(InvariantViolation::PlantOutOfBounds { plant: plant.id() });
            }
        }
        match self.day_ticks.last() {
            Some(summary) if !conserves(summary) => {
                Err(InvariantViolation::ConservationBroken { tick: summary.tick })
            }
            _ => Ok(()),
        }
    }
}

fn conserves(s: &TickSummary) -> bool {
    let near = |a: Megawatts, b: Megawatts| (a - b).value().abs() <= TOLERANCE;
    let by_plant: Megawatts = s.by_plant.values().copied().sum();
    near(s.generation, by_plant)
        && near(s.generation, s.served + s.imbalance)
        && near(s.served + s.unserved, s.demand)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use super::*;
    use crate::grid::GridMode;
    use crate::ids::CellCoord;
    use crate::region::RegionDefinition;
    use crate::region::fixtures::tiny_region_parts;
    use crate::session::GameSettings;
    use crate::units::{Hertz, Ratio};

    fn session_with(edit: impl FnOnce(&mut crate::region::RegionDefinitionParts)) -> GameSession {
        let mut parts = tiny_region_parts();
        edit(&mut parts);
        let region = RegionDefinition::try_from(parts).unwrap();
        GameSession::new(Arc::new(region), 1, GameSettings::default())
    }

    fn plant_id(s: &GameSession, kind: PlantKind) -> PlantId {
        *s.power
            .plants()
            .iter()
            .find(|(_, p)| p.kind() == kind)
            .map(|(id, _)| id)
            .expect("plant of kind")
    }

    fn set_output(s: &mut GameSession, id: PlantId, mw: f64) {
        s.power
            .plant_mut(id)
            .unwrap()
            .set_output_unchecked(Megawatts::new(mw));
    }

    #[test]
    fn a_fresh_session_satisfies_every_invariant() {
        assert_eq!(session_with(|_| {}).check_invariants(), Ok(()));
    }

    #[test]
    fn online_dispatchable_output_must_lie_within_bounds() {
        let mut s = session_with(|_| {});
        let coal = plant_id(&s, PlantKind::Coal);
        let violation = Err(InvariantViolation::PlantOutOfBounds { plant: coal });
        // Coal: minimum 240 MW, capacity 600 MW.
        for (mw, ok) in [
            (240.0, true),
            (600.0, true),
            (239.9, false),
            (0.0, false),
            (600.1, false),
        ] {
            set_output(&mut s, coal, mw);
            let expected = if ok { Ok(()) } else { violation.clone() };
            assert_eq!(s.check_invariants(), expected, "{mw} MW");
        }
        // Half a micro-megawatt of float noise is tolerated either side.
        set_output(&mut s, coal, 240.0 - 5e-7);
        assert_eq!(s.check_invariants(), Ok(()));
        set_output(&mut s, coal, 600.0 + 5e-7);
        assert_eq!(s.check_invariants(), Ok(()));
    }

    #[test]
    fn the_tolerance_is_inclusive_at_exactly_one_micro_megawatt() {
        // A minimum of exactly TOLERANCE with zero output sits exactly TOLERANCE below it;
        // zero capacity with TOLERANCE output sits exactly TOLERANCE above it. Both pass.
        let mut s = session_with(|parts| {
            for spec in &mut parts.catalog {
                if spec.kind == PlantKind::Coal {
                    spec.capacity = Megawatts::new(1.0);
                    spec.min_stable = Ratio::new(TOLERANCE).unwrap();
                }
            }
        });
        let coal = plant_id(&s, PlantKind::Coal);
        set_output(&mut s, coal, 0.0);
        assert_eq!(s.check_invariants(), Ok(()));

        let mut s = session_with(|parts| {
            for spec in &mut parts.catalog {
                if spec.kind == PlantKind::Coal {
                    spec.capacity = Megawatts::ZERO;
                    spec.min_stable = Ratio::ZERO;
                }
            }
        });
        let coal = plant_id(&s, PlantKind::Coal);
        set_output(&mut s, coal, TOLERANCE);
        assert_eq!(s.check_invariants(), Ok(()));
    }

    #[test]
    fn a_dry_hydro_plant_may_sit_below_its_minimum_but_not_other_kinds() {
        let mut s = session_with(|parts| {
            for spec in &mut parts.catalog {
                if spec.kind == PlantKind::Hydro {
                    spec.min_stable = Ratio::new(0.3).unwrap();
                }
            }
        });
        let hydro = s.insert_commissioned_plant(PlantKind::Hydro, CellCoord { x: 10, y: 1 });
        set_output(&mut s, hydro, 0.0);
        assert_eq!(s.check_invariants(), Ok(()));
        // Above capacity is wrong for hydro too.
        set_output(&mut s, hydro, 800.1);
        assert_eq!(
            s.check_invariants(),
            Err(InvariantViolation::PlantOutOfBounds { plant: hydro })
        );
    }

    #[test]
    fn solar_output_is_not_bounded_below_by_a_minimum() {
        let mut s = session_with(|_| {});
        let solar = s.insert_commissioned_plant(PlantKind::Solar, CellCoord { x: 4, y: 4 });
        set_output(&mut s, solar, 9_999.0);
        assert_eq!(s.check_invariants(), Ok(()));
    }

    fn summary(
        demand: f64,
        served: f64,
        generation: f64,
        imbalance: f64,
        by_plant: &[f64],
    ) -> TickSummary {
        TickSummary {
            tick: 7,
            demand: Megawatts::new(demand),
            served: Megawatts::new(served),
            unserved: Megawatts::new(demand - served),
            generation: Megawatts::new(generation),
            curtailed: Megawatts::ZERO,
            imbalance: Megawatts::new(imbalance),
            by_kind: BTreeMap::new(),
            by_plant: by_plant
                .iter()
                .enumerate()
                .map(|(i, mw)| (PlantId(i as u32), Megawatts::new(*mw)))
                .collect(),
            frequency: Hertz::new(60.0),
            mode: GridMode::Normal,
            shed: Ratio::ZERO,
        }
    }

    #[test]
    fn conservation_needs_all_three_identities() {
        // demand 100, served 90, 95 generated by two plants: imbalance +5, unserved 10.
        assert!(conserves(&summary(100.0, 90.0, 95.0, 5.0, &[60.0, 35.0])));
        // generation differs from the sum over plants
        assert!(!conserves(&summary(100.0, 90.0, 95.0, 5.0, &[60.0, 30.0])));
        // generation differs from served + imbalance
        assert!(!conserves(&summary(100.0, 90.0, 95.0, 4.0, &[60.0, 35.0])));
        assert!(!conserves(&summary(100.0, 90.0, 95.0, -5.0, &[60.0, 35.0])));
        // served + unserved differs from demand
        let mut broken = summary(100.0, 90.0, 95.0, 5.0, &[60.0, 35.0]);
        broken.unserved = Megawatts::new(11.0);
        assert!(!conserves(&broken));
        // Differences inside the tolerance are float noise.
        let mut noisy = summary(100.0, 90.0, 95.0, 5.0, &[60.0, 35.0]);
        noisy.generation = Megawatts::new(95.0 + 5e-7);
        assert!(conserves(&noisy));
    }

    #[test]
    fn a_broken_last_tick_is_reported_with_its_tick() {
        let mut s = session_with(|_| {});
        s.day_ticks
            .push(summary(100.0, 90.0, 95.0, 5.0, &[60.0, 35.0]));
        assert_eq!(s.check_invariants(), Ok(()));
        s.day_ticks
            .push(summary(100.0, 90.0, 95.0, 4.0, &[60.0, 35.0]));
        assert_eq!(
            s.check_invariants(),
            Err(InvariantViolation::ConservationBroken { tick: 7 })
        );
    }
}
