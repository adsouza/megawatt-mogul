//! Camera maths shared by rendering, projection and picking. No renderer or window required.
use bevy::{camera::Viewport, prelude::*};
use bevy_egui::egui;

pub const FOV: f32 = std::f32::consts::FRAC_PI_4;

/// Fully contained physical viewport, rounded inward so it never paints over a HUD panel.
pub fn physical_viewport(rect: egui::Rect, scale: f32, window: UVec2) -> Option<Viewport> {
    if !rect.is_finite() || !scale.is_finite() || scale <= 0.0 || !rect.is_positive() {
        return None;
    }
    let min = Vec2::new(rect.min.x, rect.min.y) * scale;
    let max = Vec2::new(rect.max.x, rect.max.y) * scale;
    let start = min.ceil().max(Vec2::ZERO).min(window.as_vec2()).as_uvec2();
    let end = max.floor().max(Vec2::ZERO).min(window.as_vec2()).as_uvec2();
    let size = end.saturating_sub(start);
    (size.x > 0 && size.y > 0).then_some(Viewport {
        physical_position: start,
        physical_size: size,
        ..default()
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrbitCamera {
    pub yaw: f32,
    pub elevation: f32,
    pub zoom: f32,
    pub target: Vec2,
}
impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            elevation: 0.95,
            zoom: 1.0,
            target: Vec2::ZERO,
        }
    }
}
impl OrbitCamera {
    pub fn orbit(&mut self, delta: Vec2) {
        if !delta.is_finite() {
            return;
        }
        self.yaw = (self.yaw - delta.x * 0.008).rem_euclid(std::f32::consts::TAU);
        self.elevation = (self.elevation + delta.y * 0.008).clamp(0.3, 1.42);
    }
    pub fn zoom(&mut self, delta: f32) {
        if delta.is_finite() {
            self.zoom =
                (self.zoom * (-delta.clamp(-2000.0, 2000.0) * 0.002).exp()).clamp(0.08, 3.0);
        }
    }
    pub fn pan(&mut self, delta: Vec2, size: Vec2, pixels: f32) {
        if !delta.is_finite() || !pixels.is_finite() || pixels <= 0.0 {
            return;
        }
        let right = Vec2::new(self.yaw.cos(), -self.yaw.sin());
        let forward = Vec2::new(self.yaw.sin(), self.yaw.cos());
        self.target +=
            (-right * delta.x - forward * delta.y) * size.max_element() * self.zoom / pixels;
        self.target = self.target.clamp(-size * 0.55, size * 0.55);
    }
    pub fn pose(&self, size: Vec2, aspect: f32) -> CameraPose {
        let aspect = if aspect.is_finite() {
            aspect.clamp(0.05, 20.0)
        } else {
            1.0
        };
        let direction = Vec3::new(
            self.yaw.sin() * self.elevation.cos(),
            self.elevation.sin(),
            self.yaw.cos() * self.elevation.cos(),
        );
        let rotation = Transform::from_translation(direction)
            .looking_at(Vec3::ZERO, Vec3::Y)
            .rotation;
        let inverse = rotation.inverse();
        let tan = (FOV / 2.0).tan();
        let mut distance = 1.0f32;
        for x in [-size.x / 2.0, size.x / 2.0] {
            for z in [-size.y / 2.0, size.y / 2.0] {
                for y in [-0.25, 4.0] {
                    let p = inverse * Vec3::new(x, y, z);
                    distance =
                        distance.max(p.z + (p.x.abs() / (tan * aspect)).max(p.y.abs() / tan));
                }
            }
        }
        let target = Vec3::new(self.target.x, 0.0, self.target.y);
        let eye =
            target + direction * (distance * 1.12 * self.zoom).max(8.0 / self.elevation.sin());
        CameraPose {
            eye,
            target,
            aspect,
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct CameraPose {
    pub eye: Vec3,
    pub target: Vec3,
    pub aspect: f32,
}
impl CameraPose {
    pub fn transform(self) -> Transform {
        Transform::from_translation(self.eye).looking_at(self.target, Vec3::Y)
    }
    pub fn view_projection(self) -> Mat4 {
        bevy::math::proj::perspective(FOV, self.aspect, 0.1, 2000.0)
            * self.transform().to_matrix().inverse()
    }
    /// NDC, x/y in [-1,1], z in [0,1]. Reject behind/near/far points.
    pub fn project(self, point: Vec3) -> Option<Vec3> {
        let clip = self.view_projection() * point.extend(1.0);
        if !clip.is_finite() || clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        (ndc.z >= 0.0 && ndc.z <= 1.0).then_some(ndc)
    }
    pub fn ray(self, ndc: Vec2) -> Option<Ray3d> {
        if !ndc.is_finite() {
            return None;
        }
        let tan = (FOV / 2.0).tan();
        let direction =
            self.transform().rotation * Vec3::new(ndc.x * self.aspect * tan, ndc.y * tan, -1.0);
        Dir3::new(direction).ok().map(|direction| Ray3d {
            origin: self.eye,
            direction,
        })
    }
}
