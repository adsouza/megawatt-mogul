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
    pub gauge_color: Vec3,
}
impl Default for TuningConfig {
    fn default() -> Self {
        Self {
            model_scale: 1.0,
            sunlight: 14000.0,
            bloom_intensity: 0.12,
            gauge_color: Vec3::new(0.18, 0.65, 0.75),
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

pub const GAUGE_SEGMENTS: u8 = 24;

/// A quiet, segmented square around the fitted plant base, inside the selection outline.
pub fn gauge_track_parts() -> [super::Part; GAUGE_SEGMENTS as usize] {
    std::array::from_fn(|segment| gauge_segment(segment as u8, 1.0, 0.035))
}

fn gauge_segment(segment: u8, fill: f32, height: f32) -> super::Part {
    let step = 0.82 / 6.0;
    let length = step - 0.018;
    let rotation = Quat::from_rotation_y(f32::from(segment / 6) * std::f32::consts::FRAC_PI_2);
    let center = Vec3::new(
        -0.41 + (f32::from(segment % 6) + 0.5) * step - length * (1.0 - fill) * 0.5,
        height,
        -0.41,
    );
    super::Part {
        center: rotation * center,
        size: Vec3::new(length * fill, 0.014, 0.016),
        rotation,
        color: [1.0; 4],
    }
}

/// Exact visible segment bounds. A fractional last segment keeps the gauge proportional.
pub fn output_gauge_part(
    plant: &super::WorldPlant,
    tuning: &TuningConfig,
    segment: u8,
) -> Option<super::Part> {
    if !plant.connected
        || plant.building.is_some()
        || !plant.output.is_finite()
        || segment >= GAUGE_SEGMENTS
    {
        return None;
    }
    let fill = (plant.output.clamp(0.0, 1.0) * f32::from(GAUGE_SEGMENTS) - f32::from(segment))
        .clamp(0.0, 1.0);
    (fill > 0.0).then(|| {
        let mut part = gauge_segment(segment, fill, 0.052);
        part.center = plant.at + part.center * tuning.scale();
        part.size *= tuning.scale();
        part
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{
        ids::{CellCoord, PlantId},
        region::PlantKind,
    };
    use proptest::prelude::*;

    fn plant(output: f32) -> super::super::WorldPlant {
        super::super::WorldPlant {
            id: PlantId(1),
            kind: PlantKind::Solar,
            cell: CellCoord { x: 0, y: 0 },
            at: Vec3::ZERO,
            connected: true,
            building: None,
            online: true,
            output,
        }
    }

    proptest! {
        #[test]
        fn gauge_fill_is_proportional_and_stays_at_the_base(output in 0.0f32..=1.0, scale in 0.4f32..=1.0) {
            let tuning = TuningConfig { model_scale: scale, ..Default::default() };
            let parts: Vec<_> = (0..GAUGE_SEGMENTS)
                .filter_map(|segment| output_gauge_part(&plant(output), &tuning, segment))
                .collect();
            let length: f32 = parts.iter().map(|p| p.size.x).sum();
            prop_assert!((length - (4.0 * 0.82 - 24.0 * 0.018) * scale * output).abs() < 1e-5);
            for part in parts {
                let (min, max) = part.bounds();
                prop_assert!(min.y > 0.0 && max.y < 0.1);
                prop_assert!(min.x >= -0.42 && min.z >= -0.42);
                prop_assert!(max.x <= 0.42 && max.z <= 0.42);
            }
        }
    }

    #[test]
    fn gauges_never_fill_for_zero_output_disconnected_or_unfinished_plants() {
        let tuning = TuningConfig::default();
        let mut disconnected = plant(1.0);
        disconnected.connected = false;
        let mut building = plant(1.0);
        building.building = Some(1);
        for plant in [plant(0.0), disconnected, building] {
            assert!((0..GAUGE_SEGMENTS).all(|s| output_gauge_part(&plant, &tuning, s).is_none()));
        }
    }
}
