//! Electrical storage: one inventory, with losses settled once from actual grid flows.

use crate::region::BatterySpec;
use crate::units::{MegawattHours, Megawatts};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatteryFlow {
    pub opening: MegawattHours,
    pub charge: Megawatts,
    pub discharge: Megawatts,
    pub stored: MegawattHours,
    pub charge_loss: MegawattHours,
    pub discharge_loss: MegawattHours,
}
impl Default for BatteryFlow {
    fn default() -> Self {
        Self {
            opening: MegawattHours::ZERO,
            charge: Megawatts::ZERO,
            discharge: Megawatts::ZERO,
            stored: MegawattHours::ZERO,
            charge_loss: MegawattHours::ZERO,
            discharge_loss: MegawattHours::ZERO,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatteryState {
    enabled: bool,
    stored: MegawattHours,
    flow: BatteryFlow,
    control_override: bool,
}
impl Default for BatteryState {
    fn default() -> Self {
        Self::new()
    }
}
impl BatteryState {
    pub fn new() -> Self {
        Self {
            enabled: true,
            stored: MegawattHours::ZERO,
            flow: BatteryFlow::default(),
            control_override: false,
        }
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn stored(&self) -> MegawattHours {
        self.stored
    }
    pub fn flow(&self) -> BatteryFlow {
        self.flow
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.clear_flow();
        self.control_override = true;
    }
    pub(crate) fn flow_overridden(&self) -> bool {
        self.control_override
    }
    pub(crate) fn clear_flow(&mut self) {
        self.flow = BatteryFlow {
            opening: self.stored,
            stored: self.stored,
            ..BatteryFlow::default()
        };
    }
    /// A pure cap. Positive charging takes precedence; callers choose one direction.
    pub(crate) fn feasible(
        &self,
        spec: BatterySpec,
        discharge_max: Megawatts,
        charge: Megawatts,
        discharge: Megawatts,
    ) -> BatteryFlow {
        let mut flow = BatteryFlow {
            opening: self.stored,
            stored: self.stored,
            ..BatteryFlow::default()
        };
        if !self.enabled {
            return flow;
        }
        let c = charge.value().max(0.0).min(spec.charge_power.value()).min(
            ((spec.energy_capacity.value() - self.stored.value()) * 60.0
                / spec.charge_efficiency.value())
            .max(0.0),
        );
        let d = if c > 0.0 {
            0.0
        } else {
            discharge
                .value()
                .max(0.0)
                .min(discharge_max.value())
                .min(self.stored.value() * spec.discharge_efficiency.value() * 60.0)
        };
        flow.charge = Megawatts::new(c);
        flow.discharge = Megawatts::new(d);
        flow.charge_loss = MegawattHours::new((1.0 - spec.charge_efficiency.value()) * c / 60.0);
        flow.discharge_loss =
            MegawattHours::new((1.0 / spec.discharge_efficiency.value() - 1.0) * d / 60.0);
        flow.stored = MegawattHours::new(
            (self.stored.value() + spec.charge_efficiency.value() * c / 60.0
                - d / (60.0 * spec.discharge_efficiency.value()))
            .max(0.0)
            .min(spec.energy_capacity.value()),
        );
        flow
    }
    pub(crate) fn stage(&mut self, flow: BatteryFlow) {
        self.flow = flow;
    }
    /// Commit once, after protection. Provisional dispatch uses `feasible` without mutating inventory.
    pub(crate) fn settle(
        &mut self,
        spec: BatterySpec,
        discharge_max: Megawatts,
        charge: Megawatts,
        discharge: Megawatts,
    ) -> BatteryFlow {
        self.flow = self.feasible(spec, discharge_max, charge, discharge);
        self.stored = self.flow.stored;
        self.control_override = false;
        self.flow
    }
    pub(crate) fn close_day(&mut self) -> MegawattHours {
        let unused = self.stored;
        self.stored = MegawattHours::ZERO;
        self.clear_flow();
        unused
    }
}

/// Physical energy totals; unweighted in the accumulator, month-weighted in reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatteryTotals {
    pub input: MegawattHours,
    pub output: MegawattHours,
    pub charge_loss: MegawattHours,
    pub discharge_loss: MegawattHours,
    pub unused: MegawattHours,
}
impl Default for BatteryTotals {
    fn default() -> Self {
        Self {
            input: MegawattHours::ZERO,
            output: MegawattHours::ZERO,
            charge_loss: MegawattHours::ZERO,
            discharge_loss: MegawattHours::ZERO,
            unused: MegawattHours::ZERO,
        }
    }
}
impl BatteryTotals {
    pub fn record(&mut self, flow: BatteryFlow) {
        self.input += flow.charge.energy_over_ticks(1);
        self.output += flow.discharge.energy_over_ticks(1);
        self.charge_loss += flow.charge_loss;
        self.discharge_loss += flow.discharge_loss;
    }
    pub fn weighted(self, weight: f64) -> Self {
        Self {
            input: self.input * weight,
            output: self.output * weight,
            charge_loss: self.charge_loss * weight,
            discharge_loss: self.discharge_loss * weight,
            unused: self.unused * weight,
        }
    }
}

/// Read-side operating mode; construction is shown by the plant construction card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryMode {
    Disabled,
    Disconnected,
    Holding,
    Charging,
    Discharging,
    Empty,
    Full,
    GridSuspended,
}

// Portable state schema 1: explicit field order and enum tags.
crate::state_record!(BatteryState {
    enabled,
    stored,
    flow,
    control_override
});
crate::state_record!(BatteryFlow {
    opening,
    charge,
    discharge,
    stored,
    charge_loss,
    discharge_loss
});
crate::state_record!(BatteryTotals {
    input,
    output,
    charge_loss,
    discharge_loss,
    unused
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::{PlantKind, fixtures};
    use crate::units::{MegawattHours, Megawatts, Ratio};
    use proptest::prelude::*;

    fn spec() -> crate::region::BatterySpec {
        fixtures::tiny_region()
            .spec(PlantKind::Battery)
            .battery
            .unwrap()
    }

    #[test]
    fn fractional_final_charge_and_discharge_preserve_energy() {
        let spec = spec();
        let mut state = BatteryState::new();
        let flow = state.settle(
            spec,
            Megawatts::new(200.0),
            Megawatts::new(200.0),
            Megawatts::ZERO,
        );
        assert_eq!(flow.stored, MegawattHours::new(3.0));
        assert!((flow.charge_loss.value() - 1.0 / 3.0).abs() < 1e-12);
        let flow = state.settle(
            spec,
            Megawatts::new(200.0),
            Megawatts::ZERO,
            Megawatts::new(200.0),
        );
        assert_eq!(flow.discharge, Megawatts::new(180.0));
        assert_eq!(flow.stored, MegawattHours::ZERO);
    }

    #[test]
    fn disabling_retains_inventory_and_midnight_clears_it() {
        let mut state = BatteryState::new();
        state.settle(
            spec(),
            Megawatts::new(200.0),
            Megawatts::new(100.0),
            Megawatts::ZERO,
        );
        state.set_enabled(false);
        assert!(state.stored().value() > 0.0);
        assert_eq!(
            state
                .settle(
                    spec(),
                    Megawatts::new(200.0),
                    Megawatts::ZERO,
                    Megawatts::new(200.0)
                )
                .discharge,
            Megawatts::ZERO
        );
        let unused = state.close_day();
        assert!(unused.value() > 0.0);
        assert_eq!(state.stored(), MegawattHours::ZERO);
        assert!(!state.enabled());
        assert_eq!(state.close_day(), MegawattHours::ZERO);
    }

    proptest! {
        #[test]
        fn arbitrary_flows_conserve_energy(
            requests in prop::collection::vec(-400.0f64..400.0, 0..300),
            charge_eff in 0.1f64..=1.0,
            discharge_eff in 0.1f64..=1.0,
        ) {
            let mut spec = spec();
            spec.charge_efficiency = Ratio::new(charge_eff).unwrap();
            spec.discharge_efficiency = Ratio::new(discharge_eff).unwrap();
            let mut state = BatteryState::new();
            let (mut input, mut output, mut losses) = (0.0, 0.0, 0.0);
            for request in requests {
                let flow = state.settle(spec, Megawatts::new(200.0), Megawatts::new(request.max(0.0)), Megawatts::new((-request).max(0.0)));
                input += flow.charge.value() / 60.0;
                output += flow.discharge.value() / 60.0;
                losses += flow.charge_loss.value() + flow.discharge_loss.value();
                prop_assert!(flow.charge == Megawatts::ZERO || flow.discharge == Megawatts::ZERO);
                prop_assert!(flow.stored.value() >= 0.0 && flow.stored <= spec.energy_capacity);
                prop_assert!((input - output - losses - flow.stored.value()).abs() < 1e-8);
            }
        }
    }
}
