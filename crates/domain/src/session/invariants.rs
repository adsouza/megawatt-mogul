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
