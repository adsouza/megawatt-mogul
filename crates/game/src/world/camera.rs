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
    /// Fit a local grid's complete bounds in the unobscured map, preserving the viewing angle.
    pub fn frame_bounds(
        &mut self,
        min: Vec3,
        max: Vec3,
        size: Vec2,
        viewport: egui::Rect,
        legend: Option<egui::Rect>,
    ) {
        if !min.is_finite()
            || !max.is_finite()
            || min.cmpgt(max).any()
            || !size.is_finite()
            || size.min_element() <= 0.0
            || !viewport.is_finite()
            || !viewport.is_positive()
        {
            return;
        }
        let aspect = viewport.aspect_ratio().clamp(0.05, 20.0);
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
        let mut safe = framing_rect(viewport, legend);
        let ndc = |rect: egui::Rect| {
            let left = 2.0 * (rect.left() - viewport.left()) / viewport.width() - 1.0;
            let right = 2.0 * (rect.right() - viewport.left()) / viewport.width() - 1.0;
            let bottom = 1.0 - 2.0 * (rect.bottom() - viewport.top()) / viewport.height();
            let top = 1.0 - 2.0 * (rect.top() - viewport.top()) / viewport.height();
            (left, right, bottom, top)
        };
        let up = rotation * Vec3::Y;
        // An almost entirely covered map can leave only sky above the ground-plane horizon.
        // In that degenerate layout use the centered viewport rather than placing the eye below ground.
        let (_, _, bottom, top) = ndc(safe);
        if 1.0 - up.y / direction.y * (bottom + top) * 0.5 * tan <= 0.1 {
            safe = framing_rect(viewport, None);
        }
        let (left, right, bottom, top) = ndc(safe);
        let (cx, cy) = ((left + right) * 0.5, (bottom + top) * 0.5);
        let center = (min + max) * 0.5;
        // Solve the four frustum inequalities for every corner, with the box center aimed at
        // the free rectangle's center rather than the center of the entire viewport.
        let mut distance = 1.0_f32;
        for x in [min.x, max.x] {
            for y in [min.y, max.y] {
                for z in [min.z, max.z] {
                    let p = inverse * (Vec3::new(x, y, z) - center);
                    distance = distance
                        .max(p.z + 0.2)
                        .max((-p.x / (tan * aspect) - left * p.z) / (cx - left))
                        .max((p.x / (tan * aspect) + right * p.z) / (right - cx))
                        .max((-p.y / tan - bottom * p.z) / (cy - bottom))
                        .max((p.y / tan + top * p.z) / (top - cy));
                }
            }
        }
        // The orbit target stays on y=0. Convert depth from the box center to orbit distance,
        // then move the ground target to account for the off-center screen rectangle.
        let factor = 1.0 - up.y / direction.y * cy * tan;
        let wanted = center.y / direction.y + distance * 1.02 * factor;
        let base = fitted_distance(size, aspect, rotation);
        let zoom = (wanted / base).clamp(0.08, 3.0);
        let actual = (base * zoom).max(8.0 / self.elevation.sin());
        let depth = (actual - center.y / direction.y) / factor;
        let target =
            center - (rotation * Vec3::X) * (cx * depth * tan * aspect) - up * (cy * depth * tan)
                + direction * (depth - actual);
        if target.is_finite() && zoom.is_finite() {
            self.target = target.xz();
            self.zoom = zoom;
        }
    }

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
        let distance = fitted_distance(size, aspect, rotation);
        let target = Vec3::new(self.target.x, 0.0, self.target.y);
        let eye = target + direction * (distance * self.zoom).max(8.0 / self.elevation.sin());
        CameraPose {
            eye,
            target,
            aspect,
        }
    }
}

fn fitted_distance(size: Vec2, aspect: f32, rotation: Quat) -> f32 {
    let inverse = rotation.inverse();
    let tan = (FOV / 2.0).tan();
    let mut distance = 1.0_f32;
    for x in [-size.x / 2.0, size.x / 2.0] {
        for z in [-size.y / 2.0, size.y / 2.0] {
            for y in [-0.25, 4.0] {
                let p = inverse * Vec3::new(x, y, z);
                distance = distance.max(p.z + (p.x.abs() / (tan * aspect)).max(p.y.abs() / tan));
            }
        }
    }
    distance * 1.12
}

fn framing_rect(viewport: egui::Rect, legend: Option<egui::Rect>) -> egui::Rect {
    let padding = 24.0_f32.min(viewport.width().min(viewport.height()) * 0.1);
    let inner = viewport.shrink(padding);
    let Some(legend) = legend.filter(|r| r.is_finite() && r.intersects(inner)) else {
        return inner;
    };
    let cover = legend.expand(8.0).intersect(inner);
    [
        egui::Rect::from_min_max(inner.min, egui::pos2(cover.left(), inner.bottom())),
        egui::Rect::from_min_max(egui::pos2(cover.right(), inner.top()), inner.max),
        egui::Rect::from_min_max(inner.min, egui::pos2(inner.right(), cover.top())),
        egui::Rect::from_min_max(egui::pos2(inner.left(), cover.bottom()), inner.max),
    ]
    .into_iter()
    .filter(|r| r.is_positive())
    .max_by(|a, b| a.area().total_cmp(&b.area()))
    .unwrap_or(inner)
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
        fn framing_a_zone_contains_every_bounds_corner_away_from_the_legend(
            width in 20.0f32..256.0, depth in 20.0f32..160.0,
            x in -0.4f32..0.4, z in -0.4f32..0.4,
            extent_x in 0.0f32..0.5, extent_z in 0.0f32..0.5,
            yaw in 0.0f32..std::f32::consts::TAU, elevation in 0.3f32..1.42,
            pixels_x in 320.0f32..1200.0, pixels_y in 180.0f32..700.0,
            with_legend in any::<bool>(),
        ) {
            let viewport = egui::Rect::from_min_size(egui::pos2(280.0, 55.0), egui::vec2(pixels_x, pixels_y));
            let legend = with_legend.then(|| egui::Rect::from_min_size(viewport.min, egui::vec2(pixels_x * 0.25, pixels_y * 0.3)));
            let size = Vec2::new(width, depth);
            let min = Vec3::new(x * width - extent_x * width, 0.0, z * depth - extent_z * depth);
            let max = Vec3::new(x * width + extent_x * width, 5.0, z * depth + extent_z * depth);
            let mut camera = OrbitCamera { yaw, elevation, zoom: 2.0, target: -size * 0.5 };
            camera.frame_bounds(min, max, size, viewport, legend);
            prop_assert_eq!((camera.yaw, camera.elevation), (yaw, elevation));
            prop_assert!((0.08..=3.0).contains(&camera.zoom));
            let pose = camera.pose(size, viewport.aspect_ratio());
            for x in [min.x, max.x] {
                for y in [min.y, max.y] {
                    for z in [min.z, max.z] {
                        let ndc = pose.project(Vec3::new(x, y, z)).unwrap();
                        let pixel = egui::pos2(viewport.left() + (ndc.x + 1.0) * pixels_x * 0.5,
                            viewport.top() + (1.0 - ndc.y) * pixels_y * 0.5);
                        prop_assert!(viewport.shrink(4.0).contains(pixel), "{pixel:?} outside {viewport:?}");
                        prop_assert!(!legend.is_some_and(|r| r.expand(4.0).contains(pixel)), "{pixel:?} hidden by {legend:?}");
                    }
                }
            }
            let once = camera;
            camera.frame_bounds(min, max, size, viewport, legend);
            prop_assert_eq!(camera, once);
        }

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

    #[test]
    fn framing_ignores_invalid_bounds_and_handles_a_single_bus() {
        let mut camera = OrbitCamera::default();
        let size = Vec2::new(100.0, 60.0);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 300.0));
        for (min, max, viewport) in [
            (Vec3::splat(f32::NAN), Vec3::ONE, rect),
            (Vec3::ONE, Vec3::ZERO, rect),
            (Vec3::ZERO, Vec3::ONE, egui::Rect::NOTHING),
        ] {
            camera.frame_bounds(min, max, size, viewport, None);
            assert_eq!(camera, OrbitCamera::default());
        }
        let point = Vec3::new(30.0, 2.0, -15.0);
        camera.frame_bounds(point, point, size, rect, None);
        assert!(camera.zoom < 1.0);
        assert!(
            camera
                .pose(size, rect.aspect_ratio())
                .project(point)
                .unwrap()
                .truncate()
                .length()
                < 1e-4
        );
    }
}
