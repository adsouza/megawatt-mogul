//! Endgame demand growth: a multiplier on city base load that compounds monthly.

use crate::units::Megawatts;

/// Compounded demand growth since the start of play, as a multiplier on base load.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DemandGrowth {
    factor: f64,
}

impl DemandGrowth {
    /// No growth: applying it returns the demand bit for bit.
    pub const NONE: Self = Self { factor: 1.0 };

    pub fn factor(self) -> f64 {
        self.factor
    }

    pub fn apply(self, demand: Megawatts) -> Megawatts {
        demand * self.factor
    }

    /// One month of growth at the monthly factor `monthly`.
    pub fn stepped(self, monthly: f64) -> Self {
        Self {
            factor: self.factor * monthly,
        }
    }

    /// The growth after `steps` more months, by repeated multiplication so a
    /// look-ahead matches the stepping it previews bit for bit.
    pub fn ahead(self, monthly: f64, steps: u32) -> Self {
        (0..steps).fold(self, |growth, _| growth.stepped(monthly))
    }
}

/// The nominal monthly growth factor for an annual rate: `1 + annual / 12`.
pub fn monthly_growth_factor(annual: f64) -> f64 {
    1.0 + annual / 12.0
}

// Portable state: one factor. A corrupt save must not feed a nonsense multiplier into demand.
impl crate::state::State for DemandGrowth {
    fn write(&self, out: &mut Vec<u8>) {
        self.factor.write(out);
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        let factor = f64::read(input)?;
        if factor.is_finite() && factor > 0.0 {
            Ok(Self { factor })
        } else {
            Err(crate::state::StateError::Invalid)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Megawatts;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn none_is_bit_exact(v in -1e7_f64..1e7) {
            let out = DemandGrowth::NONE.apply(Megawatts::new(v));
            prop_assert_eq!(out.value().to_bits(), v.to_bits());
        }
    }

    #[test]
    fn ahead_is_repeated_stepping() {
        let m = monthly_growth_factor(0.033);
        assert_eq!(
            DemandGrowth::NONE.ahead(m, 3),
            DemandGrowth::NONE.stepped(m).stepped(m).stepped(m)
        );
    }

    #[test]
    fn decoding_rejects_a_factor_that_is_not_finite_and_positive() {
        use crate::state::{State, StateError};
        let ok = DemandGrowth::NONE.stepped(1.002);
        assert_eq!(DemandGrowth::from_state_bytes(&ok.state_bytes()), Ok(ok));
        for bad in [0.0, -1.0, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                DemandGrowth::from_state_bytes(&f64::to_bits(bad).to_le_bytes()),
                Err(StateError::Invalid),
                "{bad}"
            );
        }
    }

    #[test]
    fn monthly_factor_is_nominal() {
        assert_eq!(monthly_growth_factor(0.024), 1.002);
    }
}
