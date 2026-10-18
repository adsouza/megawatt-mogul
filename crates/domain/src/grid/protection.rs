//! Under-frequency load shedding, blackout and black-start restoration.

use crate::region::FrequencyParams;
use crate::units::{Hertz, Megawatts, Ratio};

/// Shed values this close to a boundary snap onto it, so stepwise float
/// arithmetic lands exactly on 0 and 1.
const SNAP: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridMode {
    Normal,
    Blackout,
}

/// The protection scheme's memory between ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProtectionState {
    pub mode: GridMode,
    /// Fraction of demand currently shed.
    pub shed: Ratio,
    /// Consecutive ticks spent shedding.
    pub shed_ticks: u32,
}

impl Default for ProtectionState {
    fn default() -> Self {
        Self {
            mode: GridMode::Normal,
            shed: Ratio::ZERO,
            shed_ticks: 0,
        }
    }
}

/// What the protection scheme did this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtectionOutcome {
    None,
    Shed,
    Restored,
    Curtail,
    Blackout,
    BlackoutRestored,
}

/// Clamp into the unit interval, snapping values within `SNAP` of 0 or 1.
fn snapped(value: f64) -> Ratio {
    let v = if value.abs() < SNAP {
        0.0
    } else if (value - 1.0).abs() < SNAP {
        1.0
    } else {
        value.clamp(0.0, 1.0)
    };
    Ratio::new(v).unwrap_or(Ratio::ZERO)
}

/// Advance protection by one tick given the current deviation, `supply`
/// available and full (unshed) `demand`.
///
/// In Blackout the deviation is held at zero by the caller, so it is ignored:
/// load comes back one step at a time while supply covers the next step.
pub fn evaluate(
    state: &mut ProtectionState,
    dev: Hertz,
    p: &FrequencyParams,
    supply: Megawatts,
    demand: Megawatts,
) -> ProtectionOutcome {
    let step = p.shed_step.value();
    let shed = state.shed.value();

    if state.mode == GridMode::Blackout {
        // Tolerance: `1 - shed + step` drifts a few ulps above 1 near the end of restoration.
        let needed = demand.value() * (1.0 - shed + step);
        if supply.value() >= needed - SNAP {
            state.shed = snapped(shed - step);
            if state.shed == Ratio::ZERO {
                state.mode = GridMode::Normal;
                state.shed_ticks = 0;
                return ProtectionOutcome::BlackoutRestored;
            }
        }
        return ProtectionOutcome::None;
    }

    if dev < p.shed_below {
        state.shed = snapped(shed + step);
        state.shed_ticks += 1;
        if dev < p.blackout_below || state.shed_ticks >= p.blackout_after_shed_ticks {
            state.mode = GridMode::Blackout;
            state.shed = Ratio::ONE;
            return ProtectionOutcome::Blackout;
        }
        ProtectionOutcome::Shed
    } else if dev >= -p.normal_band && shed > 0.0 {
        state.shed = snapped(shed - step);
        state.shed_ticks = 0;
        ProtectionOutcome::Restored
    } else if dev > p.curtail_above {
        ProtectionOutcome::Curtail
    } else {
        if dev >= -p.normal_band {
            state.shed_ticks = 0;
        }
        ProtectionOutcome::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::fixtures::tiny_region;
    use proptest::prelude::*;

    fn params() -> FrequencyParams {
        tiny_region().frequency().clone()
    }

    fn eval(s: &mut ProtectionState, dev: f64, supply: f64, demand: f64) -> ProtectionOutcome {
        evaluate(
            s,
            Hertz::new(dev),
            &params(),
            Megawatts::new(supply),
            Megawatts::new(demand),
        )
    }

    fn blackout_state() -> ProtectionState {
        ProtectionState {
            mode: GridMode::Blackout,
            shed: Ratio::ONE,
            shed_ticks: 30,
        }
    }

    #[test]
    fn sheds_in_steps_below_threshold() {
        let mut s = ProtectionState::default();
        assert_eq!(eval(&mut s, -0.6, 100.0, 100.0), ProtectionOutcome::Shed);
        assert_eq!(eval(&mut s, -0.6, 100.0, 100.0), ProtectionOutcome::Shed);
        assert!((s.shed.value() - 0.10).abs() < 1e-12);
        assert_eq!(s.shed_ticks, 2);
    }

    #[test]
    fn blacks_out_after_sustained_shedding() {
        let mut s = ProtectionState::default();
        for _ in 0..29 {
            assert_eq!(eval(&mut s, -0.6, 100.0, 100.0), ProtectionOutcome::Shed);
        }
        assert_eq!(
            eval(&mut s, -0.6, 100.0, 100.0),
            ProtectionOutcome::Blackout
        );
        assert_eq!(s.mode, GridMode::Blackout);
        assert_eq!(s.shed, Ratio::ONE);
    }

    #[test]
    fn blacks_out_immediately_below_blackout_threshold() {
        let mut s = ProtectionState::default();
        assert_eq!(
            eval(&mut s, -1.6, 100.0, 100.0),
            ProtectionOutcome::Blackout
        );
        assert_eq!(s.mode, GridMode::Blackout);
        assert_eq!(s.shed, Ratio::ONE);
    }

    #[test]
    fn restoration_needs_supply() {
        let mut s = blackout_state();
        assert_eq!(eval(&mut s, 0.0, 0.0, 100.0), ProtectionOutcome::None);
        assert_eq!(s.shed, Ratio::ONE);
        assert_eq!(s.mode, GridMode::Blackout);
        for _ in 0..19 {
            assert_eq!(eval(&mut s, 0.0, 100.0, 100.0), ProtectionOutcome::None);
            assert_eq!(s.mode, GridMode::Blackout);
        }
        assert_eq!(
            eval(&mut s, 0.0, 100.0, 100.0),
            ProtectionOutcome::BlackoutRestored
        );
        assert_eq!(s.mode, GridMode::Normal);
        assert_eq!(s.shed, Ratio::ZERO);
        assert_eq!(s.shed_ticks, 0);
    }

    #[test]
    fn blackout_restoration_ignores_deviation() {
        let mut s = blackout_state();
        assert_eq!(eval(&mut s, -3.0, 100.0, 100.0), ProtectionOutcome::None);
        assert!((s.shed.value() - 0.95).abs() < 1e-12);
    }

    #[test]
    fn curtails_above_threshold() {
        let mut s = ProtectionState::default();
        assert_eq!(eval(&mut s, 0.6, 100.0, 100.0), ProtectionOutcome::Curtail);
    }

    #[test]
    fn restore_takes_priority_over_curtail() {
        let mut s = ProtectionState {
            shed: Ratio::new(0.10).unwrap(),
            ..ProtectionState::default()
        };
        assert_eq!(eval(&mut s, 0.6, 100.0, 100.0), ProtectionOutcome::Restored);
        assert!((s.shed.value() - 0.05).abs() < 1e-12);
    }

    #[test]
    fn low_but_not_shedding_holds_shed_and_ticks() {
        let mut s = ProtectionState {
            shed: Ratio::new(0.10).unwrap(),
            shed_ticks: 7,
            ..ProtectionState::default()
        };
        assert_eq!(eval(&mut s, -0.3, 100.0, 100.0), ProtectionOutcome::None);
        assert!((s.shed.value() - 0.10).abs() < 1e-12);
        assert_eq!(s.shed_ticks, 7);
    }

    #[test]
    fn normal_band_with_no_shed_resets_ticks() {
        let mut s = ProtectionState {
            shed_ticks: 5,
            ..ProtectionState::default()
        };
        assert_eq!(eval(&mut s, 0.0, 100.0, 100.0), ProtectionOutcome::None);
        assert_eq!(s.shed_ticks, 0);
    }

    #[test]
    fn restore_steps_land_exactly_on_zero() {
        let mut s = ProtectionState {
            shed: Ratio::ONE,
            ..ProtectionState::default()
        };
        for _ in 0..20 {
            assert_eq!(eval(&mut s, 0.0, 100.0, 100.0), ProtectionOutcome::Restored);
        }
        assert_eq!(s.shed, Ratio::ZERO);
        assert_eq!(eval(&mut s, 0.0, 100.0, 100.0), ProtectionOutcome::None);
    }

    proptest! {
        #[test]
        fn shed_stays_in_unit_interval(
            devs in prop::collection::vec(-3.0f64..3.0, 1..300),
            supply in 0.0f64..2000.0,
        ) {
            let mut s = ProtectionState::default();
            for dev in devs {
                eval(&mut s, dev, supply, 1000.0);
                prop_assert!((0.0..=1.0).contains(&s.shed.value()));
            }
        }
    }
}
