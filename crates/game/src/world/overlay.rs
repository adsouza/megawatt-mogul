use super::OrbitCamera;
use bevy::prelude::*;
use domain::session::DayPhase;

/// Presentation controls. Never serialised into the simulation or exposed as domain tuning.
#[derive(Resource, Debug, Clone)]
pub struct WorldControls {
    pub orbit: OrbitCamera,
    pub overlay: bool,
    pub effects: bool,
    pub resource_mode: bool,
    pub phase: Option<DayPhase>,
}
impl Default for WorldControls {
    fn default() -> Self {
        Self {
            orbit: default(),
            overlay: true,
            effects: true,
            resource_mode: false,
            phase: None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WorldAction {
    Orbit(Vec2),
    Pan(Vec2, Vec2, f32),
    Zoom(f32),
    ZoomAt {
        delta: f32,
        anchor: Vec3,
        size: Vec2,
        aspect: f32,
    },
    Reset,
    ToggleOverlay,
    ToggleEffects,
    ControlRoom,
}
impl WorldControls {
    /// Camera/overlay state belongs to a session; effects are an application preference.
    pub(super) fn reset_session(&mut self) {
        *self = Self {
            effects: self.effects,
            ..default()
        };
    }
    pub fn enter_phase(&mut self, phase: DayPhase) {
        if self.phase != Some(phase) {
            self.phase = Some(phase);
            self.overlay = true;
            self.resource_mode = false;
        }
    }
    pub fn apply(&mut self, action: WorldAction) {
        match action {
            WorldAction::Orbit(d) => self.orbit.orbit(d),
            WorldAction::Pan(d, size, pixels) => self.orbit.pan(d, size, pixels),
            WorldAction::Zoom(d) => self.orbit.zoom(d),
            WorldAction::ZoomAt {
                delta,
                anchor,
                size,
                aspect,
            } => self.orbit.zoom_at(delta, anchor, size, aspect),
            WorldAction::Reset => self.orbit = default(),
            WorldAction::ToggleOverlay => self.overlay = !self.overlay,
            WorldAction::ToggleEffects => self.effects = !self.effects,
            WorldAction::ControlRoom => {
                self.resource_mode = false;
                self.overlay = true;
            }
        }
    }
    pub fn hologram(&self) -> bool {
        self.overlay && !self.resource_mode && self.phase == Some(DayPhase::Operating)
    }
}
/// Only visual values are editable. Simulation tuning lives in region RON and requires a new run.
#[derive(Resource, Reflect, Debug, Clone)]
#[reflect(Resource)]
pub struct TuningConfig {
    pub model_scale: f32,
    pub sunlight: f32,
    pub bloom_intensity: f32,
}
impl Default for TuningConfig {
    fn default() -> Self {
        Self {
            model_scale: 1.0,
            sunlight: 14000.0,
            bloom_intensity: 0.12,
        }
    }
}
fn finite(value: f32, fallback: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}
impl TuningConfig {
    pub fn scale(&self) -> f32 {
        // Models are already fitted to their cell; tuning may shrink, but never overflow it.
        finite(self.model_scale, 1.0, 0.4, 1.0)
    }
    pub fn sunlight(&self) -> f32 {
        finite(self.sunlight, 14000.0, 1000.0, 50000.0)
    }
    pub fn bloom_intensity(&self) -> f32 {
        finite(self.bloom_intensity, 0.12, 0.0, 0.4)
    }
}

/// All connected cities receive the same fraction, including at blackout and partial shedding.
pub fn supply_fraction(mode: domain::grid::GridMode, outage: f64, shed: f64) -> f32 {
    if mode == domain::grid::GridMode::Blackout {
        0.0
    } else {
        ((1.0 - outage) * (1.0 - shed)).clamp(0.0, 1.0) as f32
    }
}

pub(super) fn city_window_fill(served: f32, group: usize) -> f32 {
    (served.clamp(0.0, 1.0) * super::model::CITY_WINDOW_GROUPS as f32 - group as f32)
        .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn lit_window_fraction_tracks_supply_and_never_increases_during_a_shortage(
            a in 0.0f32..=1.0, b in 0.0f32..=1.0,
        ) {
            let groups = super::super::model::CITY_WINDOW_GROUPS;
            let mut total = 0.0;
            for group in 0..groups {
                let fill = city_window_fill(a, group);
                prop_assert!((0.0..=1.0).contains(&fill));
                total += fill;
                prop_assert!(city_window_fill(a.min(b), group) <= city_window_fill(a.max(b), group));
                prop_assert_eq!(city_window_fill(0.0, group), 0.0);
                prop_assert_eq!(city_window_fill(1.0, group), 1.0);
            }
            prop_assert!((total / groups as f32 - a).abs() < 1e-6);
        }
    }
}
