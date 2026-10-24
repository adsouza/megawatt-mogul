//! What one tick of the power system did, for accounting and display.

use std::collections::BTreeMap;

use super::protection::GridMode;
use crate::generation::BatteryFlow;
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
/// fields always satisfy `generation = Σ by_plant = served + battery_charge + imbalance`,
/// `served = demand × (1 − outage) × (1 − shed)` and `unserved = demand −
/// served`: a rotating outage cuts load ahead of protection, whose `shed` is a
/// share of the load the outage leaves (spec §15.3).
#[derive(Debug, Clone, PartialEq)]
pub struct TickSummary {
    pub tick: u32,
    /// Actual (noisy) demand of connected cities, before any outage or
    /// shedding.
    pub demand: Megawatts,
    /// Load supplied: `demand × (1 − outage) × (1 − shed)`.
    pub served: Megawatts,
    /// Demand not supplied, `demand − served`: the rotating outage and the
    /// load protection shed. Without an outage it is `demand × shed`.
    pub unserved: Megawatts,
    /// Total output of connected plants.
    pub generation: Megawatts,
    pub battery_charge: Megawatts,
    pub battery_discharge: Megawatts,
    pub batteries: BTreeMap<PlantId, BatteryFlow>,
    /// Solar availability withheld this tick by over-frequency curtailment.
    pub curtailed: Megawatts,
    /// Signed residual `generation − served − battery_charge`: positive is surplus, negative a
    /// deficit, both absorbed by system inertia.
    pub imbalance: Megawatts,
    /// Generation per plant kind, over the kinds present on the grid.
    pub by_kind: BTreeMap<PlantKind, Megawatts>,
    /// Output of every connected, commissioned plant.
    pub by_plant: BTreeMap<PlantId, Megawatts>,
    /// Absolute frequency: nominal plus deviation.
    pub frequency: Hertz,
    pub mode: GridMode,
    /// Protection's shed, as a share of the load the outage leaves.
    pub shed: Ratio,
    /// The rotating-outage share of demand cut on purpose (spec §15.3).
    pub outage: Ratio,
    pub curtailed_by_kind: BTreeMap<PlantKind, Megawatts>,
}

// Portable state schema 1: explicit field order and enum tags.
crate::state_record!(TickSummary {
    tick,
    demand,
    served,
    unserved,
    generation,
    curtailed,
    imbalance,
    by_kind,
    by_plant,
    frequency,
    mode,
    shed,
    outage,
    battery_charge,
    battery_discharge,
    batteries;
    curtailed_by_kind: Default::default()
});
