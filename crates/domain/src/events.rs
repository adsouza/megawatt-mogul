//! Domain events: facts the simulation reports about what just happened.
//!
//! Events carry ids and typed values only, never text. Later tasks add variants.

use crate::grid::{FrequencyBand, TickSummary};
use crate::ids::PlantId;
use crate::region::PlantKind;
use crate::units::Megawatts;

/// Something noteworthy that happened during a simulation step.
#[derive(Debug, Clone, PartialEq)]
pub enum DomainEvent {
    /// A plant began its start-up sequence.
    PlantStarting { plant: PlantId },
    /// A plant finished starting and is now online.
    PlantOnline { plant: PlantId },
    /// A plant finished ramping down (or cancelled its start) and is now offline.
    PlantStopped { plant: PlantId },
    /// A plant went offline unplanned.
    PlantTripped { plant: PlantId },
    /// A plant finished construction.
    PlantCommissioned { plant: PlantId, kind: PlantKind },
    /// A hydro plant's energy budget just ran out.
    HydroBudgetExhausted { plant: PlantId },
    /// The power system finished a tick.
    TickCompleted(TickSummary),
    /// The frequency deviation moved into a different band.
    FrequencyDeviated { band: FrequencyBand },
    /// Under-frequency protection shed more load; `mw` is the total now shed.
    LoadShed { mw: Megawatts },
    /// Over-frequency protection curtailed this much solar output.
    SolarCurtailed { mw: Megawatts },
    /// Protection blacked out the grid: every plant tripped.
    Blackout,
    /// Black-start restoration reconnected the last of the load.
    BlackoutRestored,
}
