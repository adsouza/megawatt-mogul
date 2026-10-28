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
    ExplorationSucceeded {
        plant: PlantId,
    },
    ExplorationDry {
        plant: PlantId,
        capital_lost: Money,
    },
    FieldDeclineThreshold {
        plant: PlantId,
        threshold: u8,
        first: bool,
    },
    MakeupDrillingOrdered {
        plant: PlantId,
    },
    MakeupDrillingCompleted {
        plant: PlantId,
        restored: Megawatts,
    },

    /// A plant began its start-up sequence.
    PlantStarting {
        plant: PlantId,
    },
    /// A plant finished starting and is now online.
    PlantOnline {
        plant: PlantId,
    },
    /// A plant finished ramping down (or cancelled its start) and is now offline.
    PlantStopped {
        plant: PlantId,
    },
    /// A plant went offline unplanned.
    PlantTripped {
        plant: PlantId,
    },
    /// A plant finished construction.
    PlantCommissioned {
        plant: PlantId,
        kind: PlantKind,
    },
    /// A hydro plant's energy budget just ran out.
    HydroBudgetExhausted {
        plant: PlantId,
    },
    /// Daily inventory measured before the midnight reset (unweighted).
    BatteryDayClosed {
        plant: PlantId,
        unused: MegawattHours,
    },
    /// The power system finished a tick.
    TickCompleted(TickSummary),
    /// The frequency deviation moved into a different band.
    FrequencyDeviated {
        band: FrequencyBand,
    },
    /// Under-frequency protection shed more load; `mw` is the total now shed.
    LoadShed {
        mw: Megawatts,
    },
    /// Over-frequency protection set solar curtailment to `mw`, withheld from
    /// solar availability from the next tick.
    SolarCurtailed {
        mw: Megawatts,
    },
    RenewableCurtailed {
        kind: PlantKind,
        mw: Megawatts,
    },
    /// Over-frequency protection tripped `plant` (emitted alongside its
    /// `PlantTripped`).
    OverFrequencyTrip {
        plant: PlantId,
    },
    /// Protection blacked out the grid: every plant tripped.
    Blackout,
    /// Black-start restoration reconnected the last of the load.
    BlackoutRestored,
    /// A rotating outage of `share` of demand was ordered for `hour` (spec
    /// §15.3), including a change to zero ("outage lifted").
    RotatingOutageOrdered {
        hour: u8,
        share: Ratio,
    },
    /// A cash movement was booked to the company ledger.
    LedgerEntryRecorded(LedgerEntry),
    /// A reserve review held `amount` of an asset's book value out of the
    /// eligible rate base as plant held for future use (non-cash).
    CapitalHeld {
        asset: AssetRef,
        amount: Money,
    },
    /// A reserve review returned `amount` of held book value to the eligible
    /// rate base because the capacity became useful (non-cash).
    CapitalReleased {
        asset: AssetRef,
        amount: Money,
    },
    /// A reserve review permanently wrote off `amount` of held book value
    /// still excess at its second consecutive review (non-cash).
    CapitalDisallowed {
        asset: AssetRef,
        amount: Money,
    },
    /// The company took a strike for `reason`; `strikes` is the new total.
    StrikeIssued {
        strikes: u8,
        reason: StrikeReason,
    },
    /// A month failed only frequency performance and drew a formal notice
    /// instead of a strike; `month` is the failing month.
    FrequencyNoticeIssued {
        month: SimMonth,
    },
    /// A strike lapsed after clean months; `strikes` is the new total.
    StrikeExpired {
        strikes: u8,
    },
    /// Too many strikes: the company lost its licence.
    LicenseRevoked,
    /// A rate review set a new tariff and allowed return.
    RateReviewCompleted {
        tariff: Price,
        allowed_return: Ratio,
    },
    /// The session moved from one day phase to the next.
    PhaseChanged {
        from: DayPhase,
        to: DayPhase,
    },
    /// The player ordered a plant; it is now under construction.
    PlantConstructionStarted {
        plant: PlantId,
        kind: PlantKind,
        cell: CellCoord,
    },
    /// The player ordered a transmission line; it is now under construction.
    LineConstructionStarted {
        line: LineId,
    },
    /// A transmission line finished construction and is in service.
    LineCommissioned {
        line: LineId,
    },
    /// A city became energised from the home city and is now a customer.
    CityConnected {
        city: CityId,
    },
    /// The played month closed; boxed because the report is large.
    MonthClosed(Box<MonthlyReport>),
    /// Cash was negative at too many consecutive month closes: game over.
    Bankrupt,
    /// An unfinished transmission line was cancelled; future instalments stop.
    LineConstructionCancelled {
        line: LineId,
    },
}

// Portable state schema 1: explicit field order and enum tags.
impl crate::state::State for DomainEvent {
    fn write(&self, out: &mut Vec<u8>) {
        self.write_schema(out, crate::state::CURRENT_STATE_SCHEMA);
    }
    fn write_schema(&self, out: &mut Vec<u8>, schema: u32) {
        match self {
            Self::ExplorationSucceeded { plant } => {
                crate::state::State::write(&33u8, out);
                crate::state::State::write_schema(plant, out, schema);
            }
            Self::ExplorationDry {
                plant,
                capital_lost,
            } => {
                crate::state::State::write(&34u8, out);
                crate::state::State::write_schema(plant, out, schema);
                crate::state::State::write_schema(capital_lost, out, schema);
            }
            Self::FieldDeclineThreshold {
                plant,
                threshold,
                first,
            } => {
                crate::state::State::write(&35u8, out);
                crate::state::State::write_schema(plant, out, schema);
                crate::state::State::write_schema(threshold, out, schema);
                crate::state::State::write_schema(first, out, schema);
            }
            Self::MakeupDrillingOrdered { plant } => {
                crate::state::State::write(&36u8, out);
                crate::state::State::write_schema(plant, out, schema);
            }
            Self::MakeupDrillingCompleted { plant, restored } => {
                crate::state::State::write(&37u8, out);
                crate::state::State::write_schema(plant, out, schema);
                crate::state::State::write_schema(restored, out, schema);
            }
            Self::PlantStarting { plant } => {
                crate::state::State::write_schema(&0u8, out, schema);
                crate::state::State::write_schema(plant, out, schema);
            }
            Self::PlantOnline { plant } => {
                crate::state::State::write_schema(&1u8, out, schema);
                crate::state::State::write_schema(plant, out, schema);
            }
            Self::PlantStopped { plant } => {
                crate::state::State::write_schema(&2u8, out, schema);
                crate::state::State::write_schema(plant, out, schema);
            }
            Self::PlantTripped { plant } => {
                crate::state::State::write_schema(&3u8, out, schema);
                crate::state::State::write_schema(plant, out, schema);
            }
            Self::PlantCommissioned { plant, kind } => {
                crate::state::State::write_schema(&4u8, out, schema);
                crate::state::State::write_schema(plant, out, schema);
                crate::state::State::write_schema(kind, out, schema);
            }
            Self::HydroBudgetExhausted { plant } => {
                crate::state::State::write_schema(&5u8, out, schema);
                crate::state::State::write_schema(plant, out, schema);
            }
            Self::BatteryDayClosed { plant, unused } => {
                crate::state::State::write_schema(&6u8, out, schema);
                crate::state::State::write_schema(plant, out, schema);
                crate::state::State::write_schema(unused, out, schema);
            }
            Self::TickCompleted(v0) => {
                crate::state::State::write_schema(&7u8, out, schema);
                crate::state::State::write_schema(v0, out, schema);
            }
            Self::FrequencyDeviated { band } => {
                crate::state::State::write_schema(&8u8, out, schema);
                crate::state::State::write_schema(band, out, schema);
            }
            Self::LoadShed { mw } => {
                crate::state::State::write_schema(&9u8, out, schema);
                crate::state::State::write_schema(mw, out, schema);
            }
            Self::SolarCurtailed { mw } => {
                crate::state::State::write_schema(&10u8, out, schema);
                crate::state::State::write_schema(mw, out, schema);
            }
            Self::OverFrequencyTrip { plant } => {
                crate::state::State::write_schema(&11u8, out, schema);
                crate::state::State::write_schema(plant, out, schema);
            }
            Self::Blackout => {
                crate::state::State::write_schema(&12u8, out, schema);
            }
            Self::BlackoutRestored => {
                crate::state::State::write_schema(&13u8, out, schema);
            }
            Self::RotatingOutageOrdered { hour, share } => {
                crate::state::State::write_schema(&14u8, out, schema);
                crate::state::State::write_schema(hour, out, schema);
                crate::state::State::write_schema(share, out, schema);
            }
            Self::LedgerEntryRecorded(v0) => {
                crate::state::State::write_schema(&15u8, out, schema);
                crate::state::State::write_schema(v0, out, schema);
            }
            Self::CapitalHeld { asset, amount } => {
                crate::state::State::write_schema(&16u8, out, schema);
                crate::state::State::write_schema(asset, out, schema);
                crate::state::State::write_schema(amount, out, schema);
            }
            Self::CapitalReleased { asset, amount } => {
                crate::state::State::write_schema(&17u8, out, schema);
                crate::state::State::write_schema(asset, out, schema);
                crate::state::State::write_schema(amount, out, schema);
            }
            Self::CapitalDisallowed { asset, amount } => {
                crate::state::State::write_schema(&18u8, out, schema);
                crate::state::State::write_schema(asset, out, schema);
                crate::state::State::write_schema(amount, out, schema);
            }
            Self::StrikeIssued { strikes, reason } => {
                crate::state::State::write_schema(&19u8, out, schema);
                crate::state::State::write_schema(strikes, out, schema);
                crate::state::State::write_schema(reason, out, schema);
            }
            Self::FrequencyNoticeIssued { month } => {
                crate::state::State::write_schema(&20u8, out, schema);
                crate::state::State::write_schema(month, out, schema);
            }
            Self::StrikeExpired { strikes } => {
                crate::state::State::write_schema(&21u8, out, schema);
                crate::state::State::write_schema(strikes, out, schema);
            }
            Self::LicenseRevoked => {
                crate::state::State::write_schema(&22u8, out, schema);
            }
            Self::RateReviewCompleted {
                tariff,
                allowed_return,
            } => {
                crate::state::State::write_schema(&23u8, out, schema);
                crate::state::State::write_schema(tariff, out, schema);
                crate::state::State::write_schema(allowed_return, out, schema);
            }
            Self::PhaseChanged { from, to } => {
                crate::state::State::write_schema(&24u8, out, schema);
                crate::state::State::write_schema(from, out, schema);
                crate::state::State::write_schema(to, out, schema);
            }
            Self::PlantConstructionStarted { plant, kind, cell } => {
                crate::state::State::write_schema(&25u8, out, schema);
                crate::state::State::write_schema(plant, out, schema);
                crate::state::State::write_schema(kind, out, schema);
                crate::state::State::write_schema(cell, out, schema);
            }
            Self::LineConstructionStarted { line } => {
                crate::state::State::write_schema(&26u8, out, schema);
                crate::state::State::write_schema(line, out, schema);
            }
            Self::LineCommissioned { line } => {
                crate::state::State::write_schema(&27u8, out, schema);
                crate::state::State::write_schema(line, out, schema);
            }
            Self::CityConnected { city } => {
                crate::state::State::write_schema(&28u8, out, schema);
                crate::state::State::write_schema(city, out, schema);
            }
            Self::MonthClosed(v0) => {
                crate::state::State::write_schema(&29u8, out, schema);
                crate::state::State::write_schema(v0, out, schema);
            }
            Self::Bankrupt => {
                crate::state::State::write_schema(&30u8, out, schema);
            }
            Self::LineConstructionCancelled { line } => {
                crate::state::State::write_schema(&31u8, out, schema);
                crate::state::State::write_schema(line, out, schema);
            }
            Self::RenewableCurtailed { kind, mw } => {
                crate::state::State::write_schema(&32u8, out, schema);
                crate::state::State::write_schema(kind, out, schema);
                crate::state::State::write_schema(mw, out, schema);
            }
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(match <u8 as crate::state::State>::read(input)? {
            33 if input.schema >= 6 => Self::ExplorationSucceeded {
                plant: crate::state::State::read(input)?,
            },
            34 if input.schema >= 6 => Self::ExplorationDry {
                plant: crate::state::State::read(input)?,
                capital_lost: crate::state::State::read(input)?,
            },
            35 if input.schema >= 6 => Self::FieldDeclineThreshold {
                plant: crate::state::State::read(input)?,
                threshold: crate::state::State::read(input)?,
                first: crate::state::State::read(input)?,
            },
            36 if input.schema >= 6 => Self::MakeupDrillingOrdered {
                plant: crate::state::State::read(input)?,
            },
            37 if input.schema >= 6 => Self::MakeupDrillingCompleted {
                plant: crate::state::State::read(input)?,
                restored: crate::state::State::read(input)?,
            },
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
            31 => Self::LineConstructionCancelled {
                line: crate::state::State::read(input)?,
            },
            32 if input.schema >= 2 => Self::RenewableCurtailed {
                kind: crate::state::State::read(input)?,
                mw: crate::state::State::read(input)?,
            },
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}
