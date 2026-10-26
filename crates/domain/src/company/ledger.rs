//! The cash ledger: every movement of the company's cash, by month and category.
//!
//! The ledger holds cash only. Depreciation and capital write-offs are
//! non-cash rate-base adjustments and never appear here.

use crate::ids::{LineId, PlantId};
use crate::time::SimMonth;
use crate::units::Money;

/// What a ledger entry was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LedgerCategory {
    Revenue,
    Fuel,
    StartUp,
    FixedOm,
    Construction,
}

/// A capital asset the company owns. Plants order before lines, each by id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AssetRef {
    Plant(PlantId),
    Line(LineId),
    Link(crate::ids::LinkId),
}

/// One movement of cash. Revenue is positive and costs are negative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    pub month: SimMonth,
    pub category: LedgerCategory,
    pub amount: Money,
    pub subject: Option<AssetRef>,
}

// Portable state schema 1: explicit field order and enum tags.
impl crate::state::State for AssetRef {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Plant(v0) => {
                crate::state::State::write(&0u8, out);
                crate::state::State::write(v0, out);
            }
            Self::Link(id) => {
                crate::state::State::write(&2u8, out);
                crate::state::State::write(id, out);
            }
            Self::Line(v0) => {
                crate::state::State::write(&1u8, out);
                crate::state::State::write(v0, out);
            }
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(match <u8 as crate::state::State>::read(input)? {
            0 => Self::Plant(crate::state::State::read(input)?),
            2 => Self::Link(crate::state::State::read(input)?),
            1 => Self::Line(crate::state::State::read(input)?),
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}
crate::state_record!(LedgerEntry {
    month,
    category,
    amount,
    subject
});
impl crate::state::State for LedgerCategory {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Revenue => {
                crate::state::State::write(&0u8, out);
            }
            Self::Fuel => {
                crate::state::State::write(&1u8, out);
            }
            Self::StartUp => {
                crate::state::State::write(&2u8, out);
            }
            Self::FixedOm => {
                crate::state::State::write(&3u8, out);
            }
            Self::Construction => {
                crate::state::State::write(&4u8, out);
            }
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(match <u8 as crate::state::State>::read(input)? {
            0 => Self::Revenue,
            1 => Self::Fuel,
            2 => Self::StartUp,
            3 => Self::FixedOm,
            4 => Self::Construction,
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}
