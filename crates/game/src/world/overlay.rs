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
    Reset,
    ToggleOverlay,
    ToggleEffects,
    ControlRoom,
}
impl WorldControls {
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
    pub pillar_height: f32,
    pub sunlight: f32,
    pub bloom_intensity: f32,
    pub beam_color: Vec3,
}
impl Default for TuningConfig {
    fn default() -> Self {
        Self {
            model_scale: 1.0,
            pillar_height: 6.0,
            sunlight: 14000.0,
            bloom_intensity: 0.12,
            beam_color: Vec3::new(0.1, 0.7, 1.0),
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
    pub fn pillar_height(&self) -> f32 {
        finite(self.pillar_height, 6.0, 0.5, 15.0)
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

/// Exact bounds of the visible unit-cube output beam, shared by rendering and picking.
pub fn pillar_part(plant: &super::WorldPlant, tuning: &TuningConfig) -> Option<super::Part> {
    let height = plant.output * tuning.pillar_height();
    (plant.connected && height > 0.01).then(|| {
        super::Part::block(
            plant.at
                + Vec3::Y
                    * (super::model::parts_height(&super::plant_parts(plant.kind))
                        * tuning.scale()
                        + 0.1
                        + height / 2.0),
            Vec3::new(0.4, height, 0.4),
            [1.0; 4],
        )
    })
}
