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
    pub islands: BTreeMap<crate::ids::NodeId, super::power_system::IslandTickSummary>,
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
impl crate::state::State for TickSummary {
    fn write(&self, out: &mut Vec<u8>) {
        self.write_schema(out, crate::state::CURRENT_STATE_SCHEMA);
    }
    fn write_schema(&self, out: &mut Vec<u8>, schema: u32) {
        self.tick.write_schema(out, schema);
        self.demand.write_schema(out, schema);
        self.served.write_schema(out, schema);
        self.unserved.write_schema(out, schema);
        self.generation.write_schema(out, schema);
        self.curtailed.write_schema(out, schema);
        self.imbalance.write_schema(out, schema);
        self.by_kind.write_schema(out, schema);
        self.by_plant.write_schema(out, schema);
        self.frequency.write_schema(out, schema);
        self.mode.write_schema(out, schema);
        self.shed.write_schema(out, schema);
        self.outage.write_schema(out, schema);
        self.battery_charge.write_schema(out, schema);
        self.battery_discharge.write_schema(out, schema);
        self.batteries.write_schema(out, schema);
        if schema >= 2 {
            self.curtailed_by_kind.write_schema(out, schema);
        }
        if schema >= 4 {
            self.islands.write_schema(out, schema);
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(Self {
            tick: crate::state::State::read(input)?,
            demand: crate::state::State::read(input)?,
            served: crate::state::State::read(input)?,
            unserved: crate::state::State::read(input)?,
            generation: crate::state::State::read(input)?,
            curtailed: crate::state::State::read(input)?,
            imbalance: crate::state::State::read(input)?,
            by_kind: crate::state::State::read(input)?,
            by_plant: crate::state::State::read(input)?,
            frequency: crate::state::State::read(input)?,
            mode: crate::state::State::read(input)?,
            shed: crate::state::State::read(input)?,
            outage: crate::state::State::read(input)?,
            battery_charge: crate::state::State::read(input)?,
            battery_discharge: crate::state::State::read(input)?,
            batteries: crate::state::State::read(input)?,
            curtailed_by_kind: if input.schema >= 2 {
                crate::state::State::read(input)?
            } else {
                Default::default()
            },
            islands: if input.schema >= 4 {
                crate::state::State::read(input)?
            } else {
                Default::default()
            },
        })
    }
}
