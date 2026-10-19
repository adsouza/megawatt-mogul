//! Domain events: facts the simulation reports about what just happened.
//!
//! Events carry ids and typed values only, never text. Later tasks add variants.

use crate::company::{AssetRef, LedgerEntry};
use crate::grid::{FrequencyBand, TickSummary};
use crate::ids::{CellCoord, CityId, LineId, PlantId};
use crate::region::PlantKind;
use crate::regulation::StrikeReason;
use crate::session::{DayPhase, MonthlyReport};
use crate::time::SimMonth;
use crate::units::{Megawatts, Money, Price, Ratio};

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
    /// Over-frequency protection set solar curtailment to `mw`, withheld from
    /// solar availability from the next tick.
    SolarCurtailed { mw: Megawatts },
    /// Over-frequency protection tripped `plant` (emitted alongside its
    /// `PlantTripped`).
    OverFrequencyTrip { plant: PlantId },
    /// Protection blacked out the grid: every plant tripped.
    Blackout,
    /// Black-start restoration reconnected the last of the load.
    BlackoutRestored,
    /// A cash movement was booked to the company ledger.
    LedgerEntryRecorded(LedgerEntry),
    /// A reserve review held `amount` of an asset's book value out of the
    /// eligible rate base as plant held for future use (non-cash).
    CapitalHeld { asset: AssetRef, amount: Money },
    /// A reserve review returned `amount` of held book value to the eligible
    /// rate base because the capacity became useful (non-cash).
    CapitalReleased { asset: AssetRef, amount: Money },
    /// A reserve review permanently wrote off `amount` of held book value
    /// still excess at its second consecutive review (non-cash).
    CapitalDisallowed { asset: AssetRef, amount: Money },
    /// The company took a strike for `reason`; `strikes` is the new total.
    StrikeIssued { strikes: u8, reason: StrikeReason },
    /// A month failed only frequency performance and drew a formal notice
    /// instead of a strike; `month` is the failing month.
    FrequencyNoticeIssued { month: SimMonth },
    /// A strike lapsed after clean months; `strikes` is the new total.
    StrikeExpired { strikes: u8 },
    /// Too many strikes: the company lost its licence.
    LicenseRevoked,
    /// A rate review set a new tariff and allowed return.
    RateReviewCompleted {
        tariff: Price,
        allowed_return: Ratio,
    },
    /// The session moved from one day phase to the next.
    PhaseChanged { from: DayPhase, to: DayPhase },
    /// The player ordered a plant; it is now under construction.
    PlantConstructionStarted {
        plant: PlantId,
        kind: PlantKind,
        cell: CellCoord,
    },
    /// The player ordered a transmission line; it is now under construction.
    LineConstructionStarted { line: LineId },
    /// A transmission line finished construction and is in service.
    LineCommissioned { line: LineId },
    /// A city became energised from the substation and is now a customer.
    CityConnected { city: CityId },
    /// The played month closed; boxed because the report is large.
    MonthClosed(Box<MonthlyReport>),
    /// Cash was negative at too many consecutive month closes: game over.
    Bankrupt,
}
