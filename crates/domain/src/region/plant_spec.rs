//! Plant kinds and their per-region specifications.

use crate::time::SimDuration;
use crate::units::{Megawatts, Money, Price, RampRate, Ratio};

/// The kinds of generating plant a player can build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlantKind {
    Nuclear,
    Coal,
    Ccgt,
    Peaker,
    Solar,
    Hydro,
}

impl PlantKind {
    pub const ALL: [PlantKind; 6] = [
        PlantKind::Nuclear,
        PlantKind::Coal,
        PlantKind::Ccgt,
        PlantKind::Peaker,
        PlantKind::Solar,
        PlantKind::Hydro,
    ];

    /// Whether the operator can choose its output. Solar output follows the sun.
    pub fn is_dispatchable(self) -> bool {
        self != PlantKind::Solar
    }
}

/// Where a plant may be sited, beyond the cell being land.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SitingRule {
    Anywhere,
    CoolingWater { min: Ratio },
    HydroSite { min: Ratio },
}

/// Technical and economic characteristics of one plant kind in a region.
#[derive(Debug, Clone, PartialEq)]
pub struct PlantSpec {
    pub kind: PlantKind,
    pub capacity: Megawatts,
    pub min_stable: Ratio,
    pub ramp: RampRate,
    pub startup: SimDuration,
    pub startup_cost: Money,
    pub marginal_cost: Price,
    pub fixed_om_per_month: Money,
    pub capital_cost: Money,
    pub build_months: u32,
    pub capacity_credit: Ratio,
    pub book_life_months: u32,
    pub siting: SitingRule,
}

impl PlantSpec {
    /// Lowest stable output while online.
    pub fn min_output(&self) -> Megawatts {
        self.capacity * self.min_stable.value()
    }
}
