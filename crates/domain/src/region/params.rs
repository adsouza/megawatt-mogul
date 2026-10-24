//! Region tuning parameters and the starting scenario.

use super::{RegionError, plant_spec::PlantKind};
use crate::ids::{CellCoord, CityId};
use crate::time::SimMonth;
use crate::units::{Hertz, Megawatts, Money, Price, Ratio};

/// A demand centre.
#[derive(Debug, Clone, PartialEq)]
pub struct CityDef {
    pub id: CityId,
    pub name_key: String,
    pub cell: CellCoord,
    pub base_load: Megawatts,
}

/// Demand multipliers. Spring and autumn use `shoulder`.
#[derive(Debug, Clone, PartialEq)]
pub struct DemandShape {
    pub winter: [f64; 24],
    pub summer: [f64; 24],
    pub shoulder: [f64; 24],
    pub monthly: [f64; 12],
    pub forecast_error: Ratio,
    pub error_persistence: f64,
}

/// Solar resource parameters. `daylight` holds (sunrise, sunset) hours per month.
#[derive(Debug, Clone, PartialEq)]
pub struct SolarParams {
    pub daylight: [(f64, f64); 12],
    pub clearness_mean: [f64; 12],
    pub clearness_sd: f64,
}

/// Correlated regional wind resource. All parameters are authored region data.
#[derive(Debug, Clone, PartialEq)]
pub struct WindParams {
    pub monthly: [f64; 12],
    pub persistence: f64,
    pub innovation_sd: f64,
    pub initial_sd: f64,
    pub band: Ratio,
}

/// Hydro resource parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct HydroParams {
    pub monthly_flow: [f64; 12],
}

/// Frequency dynamics and protection thresholds. Thresholds are deviations from nominal.
#[derive(Debug, Clone, PartialEq)]
pub struct FrequencyParams {
    pub nominal: Hertz,
    pub k_inertia: f64,
    pub k_damping: f64,
    pub normal_band: Hertz,
    pub shed_below: Hertz,
    pub blackout_below: Hertz,
    pub blackout_after_shed_ticks: u32,
    pub curtail_above: Hertz,
    /// Deviation above which generation can trip on over-frequency protection.
    pub trip_above: Hertz,
    /// Consecutive ticks above `trip_above` before the trip fires.
    pub trip_after_ticks: u32,
    /// Frequency step between successive under-frequency load-shed stages.
    pub shed_stage_spacing: Hertz,
    pub shed_step: Ratio,
}

/// A validated, monotone storage-capacity accreditation ladder.
#[derive(Debug, Clone, PartialEq)]
pub struct StorageAccreditation {
    points: Vec<(f64, Ratio)>,
}

impl StorageAccreditation {
    pub fn new(points: Vec<(f64, Ratio)>) -> Result<Self, RegionError> {
        let valid = !points.is_empty()
            && points.iter().all(|(h, _)| h.is_finite() && *h > 0.0)
            && points
                .windows(2)
                .all(|p| p[0].0 < p[1].0 && p[0].1 <= p[1].1);
        if !valid {
            return Err(RegionError::InvalidShape {
                field: "regulator.storage_accreditation",
            });
        }
        Ok(Self { points })
    }

    pub fn points(&self) -> &[(f64, Ratio)] {
        &self.points
    }

    pub fn credit(&self, hours: f64) -> Ratio {
        if hours.is_nan() || hours <= 0.0 {
            return Ratio::ZERO;
        }
        let (mut previous_hours, mut previous_credit) = (0.0, Ratio::ZERO);
        for &(end, credit) in &self.points {
            if hours == end {
                return credit;
            }
            if hours < end {
                let fraction = (hours - previous_hours) / (end - previous_hours);
                return Ratio::new(
                    previous_credit.value() + fraction * (credit.value() - previous_credit.value()),
                )
                .expect("interpolation between validated credits remains a ratio");
            }
            (previous_hours, previous_credit) = (end, credit);
        }
        previous_credit
    }
}

/// Region configuration is reconstructed from archived data, never persisted in a snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct RegulatorParams {
    pub storage_accreditation: StorageAccreditation,
    pub satisfaction_target: f64,
    pub return_per_satisfaction_point: f64,
    pub starting_tariff: Price,
    pub reference_tariff: Price,
    pub base_return: Ratio,
    pub min_return: Ratio,
    pub max_return: Ratio,
    pub reliability_target: Ratio,
    pub return_per_reliability: f64,
    pub return_per_cost_improvement: f64,
    pub reserve_margin: Ratio,
    pub strike_threshold: Ratio,
    pub strikes_to_revoke: u8,
    pub clean_months_to_expire: u8,
    /// Deviation from nominal beyond which a tick counts as a frequency violation.
    pub frequency_band: Hertz,
    /// Share of a month's ticks in violation that earns a frequency strike.
    pub frequency_violation_share: Ratio,
}

/// Customer satisfaction model parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct SatisfactionParams {
    pub starting: f64,
    pub reliability_floor: Ratio,
    pub price_sensitivity: f64,
    pub smoothing: Ratio,
}

/// Transmission line construction parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct LineParams {
    pub cost_per_cell: Money,
    pub cells_per_month: u32,
    pub book_life_months: u32,
}

/// A plant the player owns at the start of the game.
#[derive(Debug, Clone, PartialEq)]
pub struct StartPlantDef {
    pub kind: PlantKind,
    pub cell: CellCoord,
}

/// The starting scenario.
#[derive(Debug, Clone, PartialEq)]
pub struct StartDef {
    pub month: SimMonth,
    pub cash: Money,
    /// An existing city in `connected_cities`, whose node anchors the network.
    pub home_city: CityId,
    pub connected_cities: Vec<CityId>,
    pub fleet: Vec<StartPlantDef>,
}

// Portable state schema 1: explicit field order and enum tags.
crate::state_record!(CityDef {
    id,
    name_key,
    cell,
    base_load
});

#[cfg(test)]
mod storage_accreditation_tests {
    use super::*;
    use proptest::prelude::*;

    fn shipped() -> StorageAccreditation {
        StorageAccreditation::new(
            vec![(2.0, 0.65), (4.0, 0.85), (6.0, 0.93), (8.0, 0.96)]
                .into_iter()
                .map(|(h, c)| (h, Ratio::new(c).unwrap()))
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn credit_is_exact_at_points_linear_between_and_flat_beyond() {
        let ladder = shipped();
        for &(hours, credit) in ladder.points() {
            assert_eq!(ladder.credit(hours), credit);
        }
        for (hours, expected) in [(1.0, 0.325), (3.0, 0.75), (5.0, 0.89), (7.0, 0.945)] {
            assert!((ladder.credit(hours).value() - expected).abs() < 1e-12);
        }
        for hours in [0.0, -1.0, f64::NAN, f64::NEG_INFINITY] {
            assert_eq!(ladder.credit(hours), Ratio::ZERO);
        }
        for hours in [10.0, f64::INFINITY] {
            assert_eq!(ladder.credit(hours), Ratio::new(0.96).unwrap());
        }
    }

    #[test]
    fn malformed_ladders_are_rejected() {
        for points in [
            vec![],
            vec![(0.0, 0.5)],
            vec![(-1.0, 0.5)],
            vec![(4.0, 0.8), (2.0, 0.9)],
            vec![(2.0, 0.8), (2.0, 0.9)],
            vec![(2.0, 0.9), (4.0, 0.8)],
            vec![(f64::NAN, 0.5)],
            vec![(f64::INFINITY, 0.5)],
        ] {
            assert!(
                StorageAccreditation::new(
                    points
                        .into_iter()
                        .map(|(h, c)| (h, Ratio::new(c).unwrap()))
                        .collect()
                )
                .is_err()
            );
        }
    }

    proptest! {
        #[test]
        fn credit_is_monotone_in_duration(a in 0.0f64..20.0, b in 0.0f64..20.0) {
            let ladder = shipped();
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            prop_assert!(ladder.credit(lo) <= ladder.credit(hi));
        }
    }
}
