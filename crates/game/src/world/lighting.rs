//! Home-city astronomical lighting, retaining the legacy illustrative sun arc.
use bevy::prelude::*;
use domain::{
    generation::sun_factor,
    region::{RegionDefinition, SolarModel},
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

pub fn sun_position(region: &RegionDefinition, month: SimMonth, tick: u32) -> SunPosition {
    if let Some(angles) = region.sun_at_home(month, tick) {
        let elevation = angles.elevation_deg.to_radians();
        let azimuth = angles.azimuth_deg.to_radians();
        return SunPosition {
            toward_sun: Vec3::new(
                (elevation.cos() * azimuth.sin()) as f32,
                elevation.sin() as f32,
                (-elevation.cos() * azimuth.cos()) as f32,
            ),
            strength: elevation.sin().max(0.0) as f32,
        };
    }
    let SolarModel::DaylightTable(daylight) = &region.solar().model else {
        unreachable!("computed sun handled above")
    };
    let (rise, set) = daylight[month.index0()];
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
        strength: sun_factor(daylight, month, tick) as f32,
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
    let region = &model.region;
    let tick = if model.phase == DayPhase::Planning {
        match &region.solar().model {
            SolarModel::DaylightTable(daylight) => {
                let (rise, set) = daylight[model.month.index0()];
                ((rise + set) * 0.5 * f64::from(TICKS_PER_HOUR)).round() as u32
            }
            SolarModel::Astronomical => {
                let home = region
                    .city(region.start().home_city)
                    .expect("validated home city");
                let (_, lon) = region.cell_lat_lon(home.cell);
                (720.0 - lon * 4.0 + f64::from(region.system_zone().offset_minutes(model.month)))
                    .round()
                    .rem_euclid(1440.0) as u32
            }
        }
    } else {
        model.tick
    };
    let sun = sun_position(region, model.month, tick);
    for (mut light, mut transform) in &mut lights {
        // At the zenith/nadir the usual up vector is parallel to the light direction.
        let up = if sun.toward_sun.y.abs() > 0.9999 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        *transform = Transform::from_translation(sun.toward_sun * 100.0).looking_at(Vec3::ZERO, up);
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
