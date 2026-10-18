//! What one tick of the power system did, for accounting and display.

use std::collections::BTreeMap;

use super::protection::GridMode;
use crate::ids::PlantId;
use crate::region::PlantKind;
use crate::units::{Hertz, Megawatts, Ratio};

/// The balance of one tick over the connected grid.
///
/// **Right-endpoint convention:** the summary for tick `t` reports the state
/// at instant `t + 1`: after tick `t`'s plant step and the protection actions
/// that follow it (see the timing convention in `generation::commitment`). The
/// energy of tick `t` is that power held for one minute, so a plant reaching
/// `Online` at instant `b` contributes its minimum output to tick `b − 1`.
/// Forcing AGC plants to minimum and blackout trips show in tick `t` itself;
/// the solar curtailment set in tick `t` is withheld from tick `t + 1`.
/// Frequency and protection in tick `t` acted on the supply measured just
/// after the step, so on a tick where protection changed outputs, `imbalance`
/// is not the residual that moved the frequency.
///
/// Only connected plants and cities count. Curtailment is physical: curtailed
/// solar availability is never produced, so it is outside `generation`. The
/// fields always satisfy `generation = Σ by_plant = served + imbalance` and
/// `unserved = demand × shed`.
#[derive(Debug, Clone, PartialEq)]
pub struct TickSummary {
    pub tick: u32,
    /// Actual (noisy) demand of connected cities, before shedding.
    pub demand: Megawatts,
    /// Load supplied: `demand × (1 − shed)`.
    pub served: Megawatts,
    /// Load shed by protection: `demand × shed`.
    pub unserved: Megawatts,
    /// Total output of connected plants.
    pub generation: Megawatts,
    /// Solar availability withheld this tick by over-frequency curtailment.
    pub curtailed: Megawatts,
    /// Signed residual `generation − served`: positive is surplus, negative a
    /// deficit, both absorbed by system inertia.
    pub imbalance: Megawatts,
    /// Generation per plant kind, over the kinds present on the grid.
    pub by_kind: BTreeMap<PlantKind, Megawatts>,
    /// Output of every connected, commissioned plant.
    pub by_plant: BTreeMap<PlantId, Megawatts>,
    /// Absolute frequency: nominal plus deviation.
    pub frequency: Hertz,
    pub mode: GridMode,
    pub shed: Ratio,
}
