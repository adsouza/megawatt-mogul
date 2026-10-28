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
    Wind,
    Geothermal,
}

impl PlantKind {
    pub const ALL: [PlantKind; 9] = [
        PlantKind::Nuclear,
        PlantKind::Coal,
        PlantKind::Ccgt,
        PlantKind::Peaker,
        PlantKind::Solar,
        PlantKind::Hydro,
        PlantKind::Battery,
        PlantKind::Wind,
        PlantKind::Geothermal,
    ];

    pub fn is_variable(self) -> bool {
        matches!(self, Self::Solar | Self::Wind)
    }

    /// Whether generator commitment and output controls apply.
    /// Solar follows the sun; batteries use the separate daily storage controller.
    pub fn is_dispatchable(self) -> bool {
        !matches!(
            self,
            PlantKind::Solar | PlantKind::Wind | PlantKind::Battery
        )
    }
}

/// Where a plant may be sited, beyond the cell being land.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SitingRule {
    Anywhere,
    CoolingWater { min: Ratio },
    HydroSite { min: Ratio },
    GeothermalSite { min: Ratio },
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
    /// Regulatory firm MW: generators use their credit; storage uses deliverable duration.
    pub fn firm_capacity(&self, accreditation: &super::StorageAccreditation) -> Megawatts {
        match self.battery {
            Some(battery) if self.kind == PlantKind::Battery => {
                let hours = battery.energy_capacity.value() * battery.discharge_efficiency.value()
                    / self.capacity.value();
                accreditation.credit(hours).of(self.capacity)
            }
            _ => self.capacity_credit.of(self.capacity),
        }
    }

    /// Lowest stable output while online.
    pub fn min_output(&self) -> Megawatts {
        self.capacity * self.min_stable.value()
    }
}

#[cfg(test)]
mod accreditation_tests {
    use super::*;
    use crate::region::fixtures::tiny_region;

    #[test]
    fn the_shipped_battery_is_accredited_at_170_mw() {
        let region = tiny_region();
        let ladder = &region.regulator().storage_accreditation;
        assert_eq!(
            region.spec(PlantKind::Battery).firm_capacity(ladder),
            Megawatts::new(170.0)
        );
        for spec in region
            .catalog()
            .iter()
            .filter(|s| s.kind != PlantKind::Battery)
        {
            assert_eq!(
                spec.firm_capacity(ladder),
                spec.capacity_credit.of(spec.capacity)
            );
        }
    }

    #[test]
    fn storage_firm_uses_deliverable_energy_and_discharge_rating() {
        let region = tiny_region();
        for (energy, efficiency, discharge, expected) in [
            (800.0, 0.5, 200.0, 130.0),
            (600.0, 1.0, 200.0, 150.0),
            (800.0, 1.0, 100.0, 96.0),
            (10_000.0, 1.0, 200.0, 192.0),
        ] {
            let mut spec = region.spec(PlantKind::Battery).clone();
            spec.capacity = Megawatts::new(discharge);
            let battery = spec.battery.as_mut().unwrap();
            battery.energy_capacity = MegawattHours::new(energy);
            battery.discharge_efficiency = Ratio::new(efficiency).unwrap();
            assert!(
                (spec
                    .firm_capacity(&region.regulator().storage_accreditation)
                    .value()
                    - expected)
                    .abs()
                    < 1e-9
            );
        }
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
    pub charge_end_hour: u8,
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
            Self::GeothermalSite { min } => {
                crate::state::State::write(&3u8, out);
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
            3 if input.schema >= 6 => Self::GeothermalSite {
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
            Self::Geothermal => {
                crate::state::State::write(&8u8, out);
            }
            Self::Wind => {
                crate::state::State::write(&7u8, out);
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
            7 if input.schema >= 2 => Self::Wind,
            8 if input.schema >= 6 => Self::Geothermal,
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}
crate::state_record!(BatterySpec {
    charge_power,
    energy_capacity,
    charge_efficiency,
    discharge_efficiency,
    discharge_start_hour;
    charge_end_hour: 0
});
