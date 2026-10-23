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
use crate::units::{MegawattHours, Megawatts, Money, Price, Ratio};

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
    /// Daily inventory measured before the midnight reset (unweighted).
    BatteryDayClosed {
        plant: PlantId,
        unused: MegawattHours,
    },
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
    /// A rotating outage of `share` of demand was ordered for `hour` (spec
    /// §15.3), including a change to zero ("outage lifted").
    RotatingOutageOrdered { hour: u8, share: Ratio },
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
    /// A city became energised from the home city and is now a customer.
    CityConnected { city: CityId },
    /// The played month closed; boxed because the report is large.
    MonthClosed(Box<MonthlyReport>),
    /// Cash was negative at too many consecutive month closes: game over.
    Bankrupt,
}

// Portable state schema 1: explicit field order and enum tags.
impl crate::state::State for DomainEvent {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::PlantStarting { plant } => {
                crate::state::State::write(&0u8, out);
                crate::state::State::write(plant, out);
            }
            Self::PlantOnline { plant } => {
                crate::state::State::write(&1u8, out);
                crate::state::State::write(plant, out);
            }
            Self::PlantStopped { plant } => {
                crate::state::State::write(&2u8, out);
                crate::state::State::write(plant, out);
            }
            Self::PlantTripped { plant } => {
                crate::state::State::write(&3u8, out);
                crate::state::State::write(plant, out);
            }
            Self::PlantCommissioned { plant, kind } => {
                crate::state::State::write(&4u8, out);
                crate::state::State::write(plant, out);
                crate::state::State::write(kind, out);
            }
            Self::HydroBudgetExhausted { plant } => {
                crate::state::State::write(&5u8, out);
                crate::state::State::write(plant, out);
            }
            Self::BatteryDayClosed { plant, unused } => {
                crate::state::State::write(&6u8, out);
                crate::state::State::write(plant, out);
                crate::state::State::write(unused, out);
            }
            Self::TickCompleted(v0) => {
                crate::state::State::write(&7u8, out);
                crate::state::State::write(v0, out);
            }
            Self::FrequencyDeviated { band } => {
                crate::state::State::write(&8u8, out);
                crate::state::State::write(band, out);
            }
            Self::LoadShed { mw } => {
                crate::state::State::write(&9u8, out);
                crate::state::State::write(mw, out);
            }
            Self::SolarCurtailed { mw } => {
                crate::state::State::write(&10u8, out);
                crate::state::State::write(mw, out);
            }
            Self::OverFrequencyTrip { plant } => {
                crate::state::State::write(&11u8, out);
                crate::state::State::write(plant, out);
            }
            Self::Blackout => {
                crate::state::State::write(&12u8, out);
            }
            Self::BlackoutRestored => {
                crate::state::State::write(&13u8, out);
            }
            Self::RotatingOutageOrdered { hour, share } => {
                crate::state::State::write(&14u8, out);
                crate::state::State::write(hour, out);
                crate::state::State::write(share, out);
            }
            Self::LedgerEntryRecorded(v0) => {
                crate::state::State::write(&15u8, out);
                crate::state::State::write(v0, out);
            }
            Self::CapitalHeld { asset, amount } => {
                crate::state::State::write(&16u8, out);
                crate::state::State::write(asset, out);
                crate::state::State::write(amount, out);
            }
            Self::CapitalReleased { asset, amount } => {
                crate::state::State::write(&17u8, out);
                crate::state::State::write(asset, out);
                crate::state::State::write(amount, out);
            }
            Self::CapitalDisallowed { asset, amount } => {
                crate::state::State::write(&18u8, out);
                crate::state::State::write(asset, out);
                crate::state::State::write(amount, out);
            }
            Self::StrikeIssued { strikes, reason } => {
                crate::state::State::write(&19u8, out);
                crate::state::State::write(strikes, out);
                crate::state::State::write(reason, out);
            }
            Self::FrequencyNoticeIssued { month } => {
                crate::state::State::write(&20u8, out);
                crate::state::State::write(month, out);
            }
            Self::StrikeExpired { strikes } => {
                crate::state::State::write(&21u8, out);
                crate::state::State::write(strikes, out);
            }
            Self::LicenseRevoked => {
                crate::state::State::write(&22u8, out);
            }
            Self::RateReviewCompleted {
                tariff,
                allowed_return,
            } => {
                crate::state::State::write(&23u8, out);
                crate::state::State::write(tariff, out);
                crate::state::State::write(allowed_return, out);
            }
            Self::PhaseChanged { from, to } => {
                crate::state::State::write(&24u8, out);
                crate::state::State::write(from, out);
                crate::state::State::write(to, out);
            }
            Self::PlantConstructionStarted { plant, kind, cell } => {
                crate::state::State::write(&25u8, out);
                crate::state::State::write(plant, out);
                crate::state::State::write(kind, out);
                crate::state::State::write(cell, out);
            }
            Self::LineConstructionStarted { line } => {
                crate::state::State::write(&26u8, out);
                crate::state::State::write(line, out);
            }
            Self::LineCommissioned { line } => {
                crate::state::State::write(&27u8, out);
                crate::state::State::write(line, out);
            }
            Self::CityConnected { city } => {
                crate::state::State::write(&28u8, out);
                crate::state::State::write(city, out);
            }
            Self::MonthClosed(v0) => {
                crate::state::State::write(&29u8, out);
                crate::state::State::write(v0, out);
            }
            Self::Bankrupt => {
                crate::state::State::write(&30u8, out);
            }
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(match <u8 as crate::state::State>::read(input)? {
            0 => Self::PlantStarting {
                plant: crate::state::State::read(input)?,
            },
            1 => Self::PlantOnline {
                plant: crate::state::State::read(input)?,
            },
            2 => Self::PlantStopped {
                plant: crate::state::State::read(input)?,
            },
            3 => Self::PlantTripped {
                plant: crate::state::State::read(input)?,
            },
            4 => Self::PlantCommissioned {
                plant: crate::state::State::read(input)?,
                kind: crate::state::State::read(input)?,
            },
            5 => Self::HydroBudgetExhausted {
                plant: crate::state::State::read(input)?,
            },
            6 => Self::BatteryDayClosed {
                plant: crate::state::State::read(input)?,
                unused: crate::state::State::read(input)?,
            },
            7 => Self::TickCompleted(crate::state::State::read(input)?),
            8 => Self::FrequencyDeviated {
                band: crate::state::State::read(input)?,
            },
            9 => Self::LoadShed {
                mw: crate::state::State::read(input)?,
            },
            10 => Self::SolarCurtailed {
                mw: crate::state::State::read(input)?,
            },
            11 => Self::OverFrequencyTrip {
                plant: crate::state::State::read(input)?,
            },
            12 => Self::Blackout,
            13 => Self::BlackoutRestored,
            14 => Self::RotatingOutageOrdered {
                hour: crate::state::State::read(input)?,
                share: crate::state::State::read(input)?,
            },
            15 => Self::LedgerEntryRecorded(crate::state::State::read(input)?),
            16 => Self::CapitalHeld {
                asset: crate::state::State::read(input)?,
                amount: crate::state::State::read(input)?,
            },
            17 => Self::CapitalReleased {
                asset: crate::state::State::read(input)?,
                amount: crate::state::State::read(input)?,
            },
            18 => Self::CapitalDisallowed {
                asset: crate::state::State::read(input)?,
                amount: crate::state::State::read(input)?,
            },
            19 => Self::StrikeIssued {
                strikes: crate::state::State::read(input)?,
                reason: crate::state::State::read(input)?,
            },
            20 => Self::FrequencyNoticeIssued {
                month: crate::state::State::read(input)?,
            },
            21 => Self::StrikeExpired {
                strikes: crate::state::State::read(input)?,
            },
            22 => Self::LicenseRevoked,
            23 => Self::RateReviewCompleted {
                tariff: crate::state::State::read(input)?,
                allowed_return: crate::state::State::read(input)?,
            },
            24 => Self::PhaseChanged {
                from: crate::state::State::read(input)?,
                to: crate::state::State::read(input)?,
            },
            25 => Self::PlantConstructionStarted {
                plant: crate::state::State::read(input)?,
                kind: crate::state::State::read(input)?,
                cell: crate::state::State::read(input)?,
            },
            26 => Self::LineConstructionStarted {
                line: crate::state::State::read(input)?,
            },
            27 => Self::LineCommissioned {
                line: crate::state::State::read(input)?,
            },
            28 => Self::CityConnected {
                city: crate::state::State::read(input)?,
            },
            29 => Self::MonthClosed(crate::state::State::read(input)?),
            30 => Self::Bankrupt,
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}
