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
    /// Reveal a plant selected from a panel. Preserve a useful close view; a distant overview
    /// moves in to a little under half its fit distance. Recheck visibility after that zoom so
    /// a plant pushed out of the viewport is centered too.
    pub fn reveal(
        &mut self,
        point: Vec3,
        size: Vec2,
        viewport: egui::Rect,
        legend: Option<egui::Rect>,
    ) {
        if !point.is_finite() || !viewport.is_finite() || !viewport.is_positive() {
            return;
        }
        self.zoom = self.zoom.min(0.45);
        let visible = self
            .pose(size, viewport.width() / viewport.height())
            .project(point)
            .is_some_and(|ndc| {
                let pixel = egui::pos2(
                    viewport.left() + (ndc.x + 1.0) * 0.5 * viewport.width(),
                    viewport.top() + (1.0 - ndc.y) * 0.5 * viewport.height(),
                );
                viewport.shrink(16.0).contains(pixel)
                    && !legend.is_some_and(|rect| rect.expand(8.0).contains(pixel))
            });
        if !visible {
            // The orbit looks at y=0. Offset its ground target so the elevated plant itself
            // projects to the center, while preserving the player's viewing angle.
            self.target = Vec2::new(point.x, point.z)
                - Vec2::new(self.yaw.sin(), self.yaw.cos()) * point.y / self.elevation.tan();
        }
    }

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
    /// Keep the visible surface point at the same screen position while changing distance.
    /// At the pan boundary the target is constrained just as it is for a manual pan.
    pub fn zoom_at(&mut self, delta: f32, anchor: Vec3, size: Vec2, aspect: f32) {
        let before = self.pose(size, aspect);
        self.zoom(delta);
        let after = self.pose(size, aspect);
        if before.eye == after.eye {
            return;
        }
        let Some(ndc) = before.project(anchor) else {
            return;
        };
        let Some(ray) = after.ray(ndc.truncate()) else {
            return;
        };
        // Intersect the new pointer ray with the anchor's horizontal plane. Moving the
        // target in X/Z by the difference restores that exact point, including roof height.
        if ray.direction.y.abs() < 1e-4 {
            return;
        }
        let distance = (anchor.y - ray.origin.y) / ray.direction.y;
        if !distance.is_finite() || distance < 0.0 {
            return;
        }
        let correction = anchor - ray.get_point(distance);
        let target = self.target + Vec2::new(correction.x, correction.z);
        if target.is_finite() {
            self.target = target.clamp(-size * 0.55, size * 0.55);
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
#[derive(Debug, Clone, Copy, PartialEq)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn revealing_a_plant_keeps_it_in_view_without_zooming_out_or_changing_the_angle(
            width in 20.0f32..140.0, depth in 20.0f32..100.0,
            x in -0.5f32..0.5, z in -0.5f32..0.5, height in 0.0f32..4.0,
            target_x in -0.55f32..0.55, target_z in -0.55f32..0.55,
            yaw in 0.0f32..std::f32::consts::TAU, elevation in 0.3f32..1.42,
            zoom in 0.08f32..3.0, pixels_x in 320.0f32..1600.0, pixels_y in 200.0f32..900.0,
        ) {
            let viewport = egui::Rect::from_min_size(egui::pos2(260.0, 30.0), egui::vec2(pixels_x, pixels_y));
            let size = Vec2::new(width, depth);
            let point = Vec3::new(x * width, height, z * depth);
            let before = OrbitCamera {
                yaw, elevation, zoom, target: Vec2::new(target_x * width, target_z * depth),
            };
            let mut after = before;
            after.reveal(point, size, viewport, None);
            let ndc = after.pose(size, pixels_x / pixels_y).project(point).unwrap();
            prop_assert!(ndc.x.abs() < 1.0 && ndc.y.abs() < 1.0);
            prop_assert_eq!((after.yaw, after.elevation), (before.yaw, before.elevation));
            prop_assert!(after.zoom <= before.zoom);
            if after.target != before.target {
                prop_assert!(ndc.truncate().length() < 1e-4);
            }
            let once = after;
            after.reveal(point, size, viewport, None);
            prop_assert_eq!(after, once, "repeated selection keeps the camera still");
        }
    }

    #[test]
    fn a_plant_behind_the_legend_is_centered() {
        let mut camera = OrbitCamera {
            zoom: 0.25,
            ..default()
        };
        let size = Vec2::new(100.0, 60.0);
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 300.0));
        let ray = camera
            .pose(size, viewport.aspect_ratio())
            .ray(Vec2::new(-0.6, 0.5))
            .unwrap();
        let point = ray.get_point((2.0 - ray.origin.y) / ray.direction.y);
        let legend = egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(180.0, 130.0));
        camera.reveal(point, size, viewport, Some(legend));
        let ndc = camera
            .pose(size, viewport.aspect_ratio())
            .project(point)
            .unwrap();
        assert!(ndc.truncate().length() < 1e-4);
        assert_eq!(camera.zoom, 0.25);
    }
}
