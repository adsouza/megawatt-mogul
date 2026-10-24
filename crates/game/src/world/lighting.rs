//! A representative northern-hemisphere sun arc tied to the simulation's daylight calendar.
use bevy::prelude::*;
use domain::{
    generation::sun_factor,
    region::SolarParams,
    session::DayPhase,
    time::{SimMonth, TICKS_PER_HOUR},
};

use super::{TuningConfig, WorldScene};

#[derive(Debug, Clone, Copy)]
pub struct SunPosition {
    /// East is +X, north is -Z. This points toward the sun, opposite the light rays.
    pub toward_sun: Vec3,
    pub strength: f32,
}

pub fn sun_position(solar: &SolarParams, month: SimMonth, tick: u32) -> SunPosition {
    let (rise, set) = solar.daylight[month.index0()];
    let hour = f64::from(tick) / f64::from(TICKS_PER_HOUR);
    let phase = ((hour - rise) / (set - rise)).clamp(0.0, 1.0) as f32;
    let angle = std::f32::consts::PI * phase;
    // Longer days get a higher arc. This is an illustrative regional sun, not per-cell astronomy.
    let peak = (0.35 + (set - rise - 8.0) as f32 * 0.1).clamp(0.25, 1.25);
    SunPosition {
        toward_sun: Vec3::new(
            angle.cos(),
            angle.sin() * peak.sin(),
            angle.sin() * peak.cos(),
        ),
        strength: sun_factor(solar, month, tick) as f32,
    }
}

#[derive(Component)]
pub(super) struct SunLight;

pub(super) fn update_sunlight(
    scene: Res<WorldScene>,
    tuning: Res<TuningConfig>,
    mut lights: Query<(&mut DirectionalLight, &mut Transform), With<SunLight>>,
    ambient: Option<ResMut<GlobalAmbientLight>>,
) {
    let Some(model) = &scene.model else {
        return;
    };
    let solar = model.region.solar();
    let (rise, set) = solar.daylight[model.month.index0()];
    let tick = if model.phase == DayPhase::Planning {
        ((rise + set) * 0.5 * f64::from(TICKS_PER_HOUR)).round() as u32
    } else {
        model.tick
    };
    let sun = sun_position(solar, model.month, tick);
    for (mut light, mut transform) in &mut lights {
        *transform =
            Transform::from_translation(sun.toward_sun * 100.0).looking_at(Vec3::ZERO, Vec3::Y);
        light.illuminance = tuning.sunlight() * sun.strength;
        light.shadow_maps_enabled = sun.strength > 0.0;
    }
    if let Some(mut ambient) = ambient {
        // Readable nighttime terrain without inventing sunlight or solar shadows after sunset.
        ambient.brightness = 350.0 + (1.0 - sun.strength) * 2150.0;
        ambient.color = Color::srgb(
            0.70 + sun.strength * 0.30,
            0.80 + sun.strength * 0.08,
            1.0 - sun.strength * 0.28,
        );
    }
}
