//! Under-frequency load shedding, blackout and black-start restoration, and
//! over-frequency curtailment and generator trips.

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
    /// Over-frequency trip counter (spec §14.2): ticks above `trip_above`
    /// while not armed, ticks above `curtail_above` since arming or the last
    /// trip while armed.
    pub over_ticks: u32,
    /// Whether over-frequency protection is armed (see [`evaluate`]).
    pub over_armed: bool,
}

impl Default for ProtectionState {
    fn default() -> Self {
        Self {
            mode: GridMode::Normal,
            shed: Ratio::ZERO,
            shed_ticks: 0,
            over_ticks: 0,
            over_armed: false,
        }
    }
}

impl ProtectionState {
    /// Keeps an over-frequency trip that fell due, but was not carried out,
    /// due: the window stays full, so the trip is due again on the next tick
    /// that counts (see [`evaluate`]). Protection stays armed. The caller
    /// withholds a trip that would open a deficit the rest of the fleet cannot
    /// ramp to cover (see `PowerSystem`).
    pub fn withhold_trip(&mut self, p: &FrequencyParams) {
        self.over_ticks = p.trip_after_ticks;
    }
}

/// What the protection scheme did this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtectionOutcome {
    None,
    Shed,
    Restored,
    Curtail,
    /// Over-frequency protection trips one generator this tick (spec §14.2).
    /// Only ever returned above `curtail_above`, so curtailment also runs.
    OverFrequencyTrip,
    Blackout,
    BlackoutRestored,
}

impl ProtectionOutcome {
    /// Whether over-frequency curtailment runs this tick: on `Curtail`, and on
    /// `OverFrequencyTrip`, since the existing response comes before any trip.
    pub fn curtails(self) -> bool {
        matches!(self, Self::Curtail | Self::OverFrequencyTrip)
    }
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

/// Advances the over-frequency trip counter by one Normal-mode tick (spec
/// §14.2) and returns whether a trip is due. `surplus` is whether supply
/// exceeds the load being served this tick.
///
/// - At or below `curtail_above`, protection disarms and both fields reset.
/// - Not armed, a tick above `trip_above` counts and any other tick resets
///   the count.
/// - Armed, every tick above `curtail_above` counts.
/// - **Surplus gate (builder's call).** A tick counts only while there is a
///   physical surplus; a tick without one resets the count, armed or not, but
///   does not disarm. Deviation relaxes with a time constant of 1 / k_damping
///   (5 ticks by default, the same as `trip_after_ticks`), so after a large
///   surplus it stays above `curtail_above` for windows after supply already
///   matches load. Counting those ticks would keep tripping plants into a
///   deficit, and so into load shedding or blackout, contradicting §14.2's
///   gradual shedding and §14.6's "the surplus clears".
/// - Protection arms once the count reaches `trip_after_ticks`, and a trip is
///   due whenever it has. The caller resets the count when it trips.
fn track_over_frequency(
    state: &mut ProtectionState,
    dev: Hertz,
    surplus: bool,
    p: &FrequencyParams,
) -> bool {
    if dev <= p.curtail_above {
        state.over_armed = false;
        state.over_ticks = 0;
        return false;
    }
    if surplus && (state.over_armed || dev > p.trip_above) {
        state.over_ticks = state.over_ticks.saturating_add(1);
    } else {
        state.over_ticks = 0;
    }
    if state.over_ticks >= p.trip_after_ticks {
        state.over_armed = true;
    }
    state.over_armed && state.over_ticks >= p.trip_after_ticks
}

/// Advance protection by one tick given the current deviation, `supply`
/// available and full (unshed) `demand`.
///
/// In Blackout the deviation is held at zero by the caller, so it is ignored:
/// load comes back one step at a time while supply covers the next step, and
/// the over-frequency counter is left alone.
///
/// In Normal mode the over-frequency counter advances first (see
/// [`track_over_frequency`]), with a physical surplus meaning `supply`
/// above the served load `demand × (1 − shed)`. A due trip replaces `Curtail`
/// with `OverFrequencyTrip` and resets the count, so at most one trip happens
/// per `trip_after_ticks` window. Restoring shed load still takes priority
/// over both (builder's call): a trip that falls due on a restoring tick
/// waits, still armed and with its count held, for the next over-frequency
/// tick that does not restore.
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

    let surplus = supply.value() > demand.value() * (1.0 - shed);
    let trip_due = track_over_frequency(state, dev, surplus, p);

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
        if trip_due {
            state.over_ticks = 0;
            ProtectionOutcome::OverFrequencyTrip
        } else {
            ProtectionOutcome::Curtail
        }
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
            ..ProtectionState::default()
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

    #[test]
    fn thresholds_are_strict_at_exactly_the_limit() {
        let p = params();
        let (shed_below, blackout_below, curtail_above, band) = (
            p.shed_below.value(),
            p.blackout_below.value(),
            p.curtail_above.value(),
            p.normal_band.value(),
        );
        // Exactly at the shed threshold: not yet shedding.
        let mut s = ProtectionState::default();
        assert_eq!(
            eval(&mut s, shed_below, 100.0, 100.0),
            ProtectionOutcome::None
        );
        assert_eq!(s.shed, Ratio::ZERO);
        // Exactly at the blackout threshold: shedding, not yet blackout.
        let mut s = ProtectionState::default();
        assert_eq!(
            eval(&mut s, blackout_below, 100.0, 100.0),
            ProtectionOutcome::Shed
        );
        assert_eq!(s.mode, GridMode::Normal);
        // Exactly at the curtail threshold: not curtailing.
        let mut s = ProtectionState::default();
        assert_eq!(
            eval(&mut s, curtail_above, 100.0, 100.0),
            ProtectionOutcome::None
        );
        // The bottom of the normal band restores shed load; just below it does not.
        let shedding = || ProtectionState {
            shed: Ratio::new(0.10).unwrap(),
            shed_ticks: 4,
            ..ProtectionState::default()
        };
        let mut s = shedding();
        assert_eq!(
            eval(&mut s, -band, 100.0, 100.0),
            ProtectionOutcome::Restored
        );
        assert_eq!(s.shed_ticks, 0);
        let mut s = shedding();
        assert_eq!(
            eval(&mut s, -band - 1e-6, 100.0, 100.0),
            ProtectionOutcome::None
        );
        assert_eq!(s.shed_ticks, 4);
    }

    // ---- Over-frequency trips (spec §14.2, §14.6 "Trips are sustained and gradual") ----

    /// Supply 10% above the 100 MW demand: a physical surplus.
    const SURPLUS: f64 = 110.0;

    /// The 1-based ticks on which `devs` (applied in order from `s`) trip.
    fn trip_ticks(s: &mut ProtectionState, devs: impl IntoIterator<Item = f64>) -> Vec<usize> {
        devs.into_iter()
            .enumerate()
            .filter_map(|(i, dev)| {
                (eval(s, dev, SURPLUS, 100.0) == ProtectionOutcome::OverFrequencyTrip)
                    .then_some(i + 1)
            })
            .collect()
    }

    #[test]
    fn never_arms_at_or_below_trip_above() {
        let trip_above = params().trip_above.value();
        let mut s = ProtectionState::default();
        for _ in 0..1_000 {
            assert_eq!(
                eval(&mut s, trip_above, SURPLUS, 100.0),
                ProtectionOutcome::Curtail
            );
            assert!(!s.over_armed);
            assert_eq!(s.over_ticks, 0);
        }
    }

    #[test]
    fn arms_after_exactly_trip_after_ticks_then_trips_once_per_window() {
        let n = params().trip_after_ticks;
        assert_eq!(n, 5, "the fixture's default");
        let mut s = ProtectionState::default();
        for tick in 1..n {
            assert_eq!(
                eval(&mut s, 2.0, SURPLUS, 100.0),
                ProtectionOutcome::Curtail
            );
            assert!(!s.over_armed, "tick {tick}");
            assert_eq!(s.over_ticks, tick);
        }
        assert_eq!(
            eval(&mut s, 2.0, SURPLUS, 100.0),
            ProtectionOutcome::OverFrequencyTrip
        );
        assert!(s.over_armed);
        assert_eq!(s.over_ticks, 0, "a trip resets the window");
        // Sustained: one trip per window, at ticks 10 and 15 of the run.
        assert_eq!(trip_ticks(&mut s, [2.0; 10]), vec![5, 10]);
        // Armed, a deviation between curtail_above and trip_above still counts.
        assert_eq!(trip_ticks(&mut s, [1.0; 10]), vec![5, 10]);
        assert!(s.over_armed);
    }

    #[test]
    fn unarmed_count_resets_at_or_below_trip_above() {
        let trip_above = params().trip_above.value();
        let mut s = ProtectionState::default();
        let mut devs = vec![2.0; 4];
        devs.push(trip_above);
        devs.extend([2.0; 4]);
        assert!(trip_ticks(&mut s, devs).is_empty());
        assert!(!s.over_armed);
        assert_eq!(s.over_ticks, 4);
        assert_eq!(
            eval(&mut s, 2.0, SURPLUS, 100.0),
            ProtectionOutcome::OverFrequencyTrip
        );
    }

    #[test]
    fn disarms_at_curtail_above() {
        let curtail_above = params().curtail_above.value();
        let mut s = ProtectionState::default();
        assert_eq!(trip_ticks(&mut s, [2.0; 7]), vec![5]);
        assert!(s.over_armed);
        assert_eq!(s.over_ticks, 2);
        assert_eq!(
            eval(&mut s, curtail_above, SURPLUS, 100.0),
            ProtectionOutcome::None
        );
        assert!(!s.over_armed);
        assert_eq!(s.over_ticks, 0);
        // Disarmed, the band between curtail_above and trip_above no longer trips.
        assert!(trip_ticks(&mut s, [1.0; 100]).is_empty());
        assert!(!s.over_armed);
        assert_eq!(s.over_ticks, 0);
    }

    #[test]
    fn no_trip_counts_while_supply_is_at_or_below_demand() {
        let mut s = ProtectionState::default();
        // Far above trip_above, but supply only matches (then falls short of) load.
        for supply in [100.0, 90.0] {
            for _ in 0..100 {
                assert_eq!(eval(&mut s, 3.0, supply, 100.0), ProtectionOutcome::Curtail);
                assert_eq!((s.over_ticks, s.over_armed), (0, false));
            }
        }
        // Shed load counts: 100 MW of supply against 95 MW served is a surplus.
        let mut shedding = ProtectionState {
            shed: Ratio::new(0.05).unwrap(),
            ..ProtectionState::default()
        };
        assert_eq!(
            eval(&mut shedding, 3.0, 100.0, 100.0),
            ProtectionOutcome::Restored
        );
        assert_eq!(shedding.over_ticks, 1);
        // A tick without surplus resets the window, armed or not, without disarming.
        assert_eq!(trip_ticks(&mut s, [3.0; 4]), Vec::<usize>::new());
        assert_eq!(eval(&mut s, 3.0, 100.0, 100.0), ProtectionOutcome::Curtail);
        assert_eq!((s.over_ticks, s.over_armed), (0, false));
        assert_eq!(trip_ticks(&mut s, [3.0; 7]), vec![5]);
        assert_eq!(eval(&mut s, 3.0, 100.0, 100.0), ProtectionOutcome::Curtail);
        assert_eq!((s.over_ticks, s.over_armed), (0, true));
        assert_eq!(trip_ticks(&mut s, [3.0; 5]), vec![5]);
    }

    #[test]
    fn under_frequency_disarms_over_frequency_protection() {
        let mut s = ProtectionState::default();
        trip_ticks(&mut s, [2.0; 7]);
        assert!(s.over_armed);
        assert_eq!(eval(&mut s, -0.6, 100.0, 100.0), ProtectionOutcome::Shed);
        assert!(!s.over_armed);
        assert_eq!(s.over_ticks, 0);
    }

    #[test]
    fn a_trip_due_while_restoring_waits_for_the_first_tick_that_does_not_restore() {
        let mut s = ProtectionState {
            shed: Ratio::new(0.25).unwrap(),
            ..ProtectionState::default()
        };
        for _ in 0..5 {
            assert_eq!(
                eval(&mut s, 2.0, SURPLUS, 100.0),
                ProtectionOutcome::Restored
            );
        }
        assert_eq!(s.shed, Ratio::ZERO);
        assert_eq!((s.over_ticks, s.over_armed), (5, true));
        assert_eq!(
            eval(&mut s, 2.0, SURPLUS, 100.0),
            ProtectionOutcome::OverFrequencyTrip
        );
        assert_eq!(s.over_ticks, 0);
    }

    #[test]
    fn blackout_leaves_over_frequency_counters_alone() {
        let mut s = ProtectionState {
            over_ticks: 3,
            ..blackout_state()
        };
        assert_eq!(eval(&mut s, 3.0, 0.0, 100.0), ProtectionOutcome::None);
        assert_eq!((s.over_ticks, s.over_armed), (3, false));
    }

    proptest! {
        /// Trips are sustained and gradual: each trip is preceded by a full
        /// window of over-frequency ticks with a physical surplus (the first
        /// by `trip_after_ticks` such ticks above `trip_above`), so no trip
        /// happens at or below `curtail_above`, without a surplus, or while
        /// every deviation so far is at or below `trip_above`.
        #[test]
        fn trips_need_a_full_window_of_over_frequency(
            ticks in prop::collection::vec((-0.2f64..3.0, 96.0f64..116.0), 1..400),
        ) {
            let p = params();
            let n = p.trip_after_ticks as usize;
            let (curtail, trip) = (p.curtail_above.value(), p.trip_above.value());
            let mut s = ProtectionState::default();
            let mut armed = false;
            let mut last_trip: Option<usize> = None;
            let mut trips = 0;
            for (i, &(dev, supply)) in ticks.iter().enumerate() {
                let outcome = eval(&mut s, dev, supply, 100.0);
                if outcome != ProtectionOutcome::OverFrequencyTrip {
                    if dev <= curtail {
                        armed = false;
                    }
                    continue;
                }
                trips += 1;
                prop_assert!(i + 1 >= n);
                let window = &ticks[i + 1 - n..=i];
                prop_assert!(window.iter().all(|&(_, supply)| supply > 100.0));
                if armed {
                    prop_assert!(window.iter().all(|&(d, _)| d > curtail));
                    if let Some(last) = last_trip {
                        prop_assert!(i - last >= n, "two trips in one window");
                    }
                } else {
                    prop_assert!(window.iter().all(|&(d, _)| d > trip));
                }
                armed = true;
                last_trip = Some(i);
            }
            prop_assert!(trips <= ticks.len() / n);
        }
    }

    #[test]
    fn snapped_lands_on_the_boundaries_only_strictly_inside_the_snap_distance() {
        assert_eq!(snapped(0.0), Ratio::ZERO);
        assert_eq!(snapped(SNAP / 2.0), Ratio::ZERO);
        assert_eq!(snapped(-SNAP / 2.0), Ratio::ZERO);
        assert_eq!(snapped(SNAP).value(), SNAP);
        assert_eq!(snapped(1.0 - SNAP / 2.0), Ratio::ONE);
        assert_eq!(snapped(1.0 + SNAP / 2.0), Ratio::ONE);
        assert_eq!(snapped(1.0 - 1e-6).value(), 1.0 - 1e-6);
        // Out-of-range values clamp rather than snap to the far end.
        assert_eq!(snapped(-1.0), Ratio::ZERO);
        assert_eq!(snapped(2.0), Ratio::ONE);
        assert_eq!(snapped(0.5).value(), 0.5);
    }
}
