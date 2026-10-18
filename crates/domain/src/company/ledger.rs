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
}

/// One movement of cash. Revenue is positive and costs are negative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    pub month: SimMonth,
    pub category: LedgerCategory,
    pub amount: Money,
    pub subject: Option<AssetRef>,
}
