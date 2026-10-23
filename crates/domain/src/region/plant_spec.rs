//! Plant kinds and their per-region specifications.

use crate::time::SimDuration;
use crate::units::{MegawattHours, Megawatts, Money, Price, RampRate, Ratio};

/// The generation and storage plant kinds a player can build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlantKind {
    Nuclear,
    Coal,
    Ccgt,
    Peaker,
    Solar,
    Hydro,
    Battery,
}

impl PlantKind {
    pub const ALL: [PlantKind; 7] = [
        PlantKind::Nuclear,
        PlantKind::Coal,
        PlantKind::Ccgt,
        PlantKind::Peaker,
        PlantKind::Solar,
        PlantKind::Hydro,
        PlantKind::Battery,
    ];

    /// Whether generator commitment and output controls apply.
    /// Solar follows the sun; batteries use the separate daily storage controller.
    pub fn is_dispatchable(self) -> bool {
        !matches!(self, PlantKind::Solar | PlantKind::Battery)
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
    pub battery: Option<BatterySpec>,
}

impl PlantSpec {
    /// Lowest stable output while online.
    pub fn min_output(&self) -> Megawatts {
        self.capacity * self.min_stable.value()
    }
}

/// Storage-specific ratings; discharge power is `PlantSpec::capacity`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatterySpec {
    pub charge_power: Megawatts,
    pub energy_capacity: MegawattHours,
    pub charge_efficiency: Ratio,
    pub discharge_efficiency: Ratio,
    pub discharge_start_hour: u8,
}

// Portable state schema 1: explicit field order and enum tags.
crate::state_record!(PlantSpec {
    kind,
    capacity,
    min_stable,
    ramp,
    startup,
    startup_cost,
    marginal_cost,
    fixed_om_per_month,
    capital_cost,
    build_months,
    capacity_credit,
    book_life_months,
    siting,
    battery
});
impl crate::state::State for SitingRule {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Anywhere => {
                crate::state::State::write(&0u8, out);
            }
            Self::CoolingWater { min } => {
                crate::state::State::write(&1u8, out);
                crate::state::State::write(min, out);
            }
            Self::HydroSite { min } => {
                crate::state::State::write(&2u8, out);
                crate::state::State::write(min, out);
            }
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(match <u8 as crate::state::State>::read(input)? {
            0 => Self::Anywhere,
            1 => Self::CoolingWater {
                min: crate::state::State::read(input)?,
            },
            2 => Self::HydroSite {
                min: crate::state::State::read(input)?,
            },
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}
impl crate::state::State for PlantKind {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Nuclear => {
                crate::state::State::write(&0u8, out);
            }
            Self::Coal => {
                crate::state::State::write(&1u8, out);
            }
            Self::Ccgt => {
                crate::state::State::write(&2u8, out);
            }
            Self::Peaker => {
                crate::state::State::write(&3u8, out);
            }
            Self::Solar => {
                crate::state::State::write(&4u8, out);
            }
            Self::Hydro => {
                crate::state::State::write(&5u8, out);
            }
            Self::Battery => {
                crate::state::State::write(&6u8, out);
            }
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(match <u8 as crate::state::State>::read(input)? {
            0 => Self::Nuclear,
            1 => Self::Coal,
            2 => Self::Ccgt,
            3 => Self::Peaker,
            4 => Self::Solar,
            5 => Self::Hydro,
            6 => Self::Battery,
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}
crate::state_record!(BatterySpec {
    charge_power,
    energy_capacity,
    charge_efficiency,
    discharge_efficiency,
    discharge_start_hour
});
