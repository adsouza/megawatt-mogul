//! What one tick of the power system did, for accounting and display.

use std::collections::BTreeMap;

use super::protection::GridMode;
use crate::ids::PlantId;
use crate::region::PlantKind;
use crate::units::{Hertz, Megawatts, Ratio};

/// The balance of one tick over the connected grid.
///
/// **Right-endpoint convention:** the summary for tick `t` reports the state
/// after tick `t`'s plant step, i.e. at instant `t + 1` (see the timing
/// convention in `generation::commitment`). The energy of tick `t` is that
/// power held for one minute, so a plant reaching `Online` at instant `b`
/// contributes its minimum output to tick `b − 1`. Protection actions taken at
/// the end of tick `t` (forcing AGC plants to minimum, tripping every plant in
/// a blackout) first show in tick `t + 1`; solar curtailment is accounted in
/// tick `t` itself.
///
/// Only connected plants and cities count. The fields always satisfy
/// `generation = served + curtailed + imbalance` and `unserved = demand × shed`.
#[derive(Debug, Clone, PartialEq)]
pub struct TickSummary {
    pub tick: u32,
    /// Actual (noisy) demand of connected cities, before shedding.
    pub demand: Megawatts,
    /// Load supplied: `demand × (1 − shed)`.
    pub served: Megawatts,
    /// Load shed by protection: `demand × shed`.
    pub unserved: Megawatts,
    /// Total output of connected plants, including solar that was then curtailed.
    pub generation: Megawatts,
    /// Solar output curtailed by over-frequency protection.
    pub curtailed: Megawatts,
    /// Signed residual `generation − curtailed − served`: positive is surplus,
    /// negative a deficit, both absorbed by system inertia.
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
