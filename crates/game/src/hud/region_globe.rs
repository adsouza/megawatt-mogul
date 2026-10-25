//! Presentation-only globe geometry and clock; independent of the simulation.
use bevy::math::Vec3;
use bevy_egui::egui::{self, Pos2, Rect};

const TILT: f32 = 22.0 * std::f32::consts::PI / 180.0;
const DEGREES_PER_SECOND: f64 = -360.0 / 24.0;
const RESUME_DELAY: f64 = 2.0;
const FOCUS_DURATION: f64 = 0.6;

#[derive(Clone, Debug)]
struct Focus {
    started: f64,
    from: f64,
    arc: f64,
}

#[derive(Clone, Debug)]
pub(super) struct GlobeSpin {
    pub longitude: f64,
    last_frame: Option<f64>,
    paused_until: f64,
    was_held: bool,
    focus: Option<Focus>,
}
impl Default for GlobeSpin {
    fn default() -> Self {
        Self {
            longitude: -100.0,
            last_frame: None,
            paused_until: 0.0,
            was_held: false,
            focus: None,
        }
    }
}
impl GlobeSpin {
    /// Animate the shortest longitude arc, keeping the fixed polar tilt.
    pub fn focus(&mut self, now: f64, longitude: f64) {
        if !now.is_finite() || !longitude.is_finite() {
            return;
        }
        self.advance(now, false, 0.0, 1.0);
        self.focus = Some(Focus {
            started: now,
            from: self.longitude,
            arc: (longitude - self.longitude + 180.0).rem_euclid(360.0) - 180.0,
        });
        self.paused_until = now + FOCUS_DURATION + RESUME_DELAY;
        self.was_held = false;
    }
    pub fn advance(&mut self, now: f64, held: bool, drag: f32, diameter: f32) {
        if !now.is_finite() {
            return;
        }
        let previous = self.last_frame.replace(now).unwrap_or(now);
        if held {
            self.focus = None;
            self.paused_until = now + RESUME_DELAY;
            if drag.is_finite() && diameter.is_finite() && diameter > 0.0 {
                self.longitude -= f64::from(drag / diameter) * 360.0;
            }
        } else {
            if self.was_held {
                self.paused_until = now + RESUME_DELAY;
            }
            if let Some(focus) = &self.focus {
                let t = ((now - focus.started) / FOCUS_DURATION).clamp(0.0, 1.0);
                self.longitude = focus.from + focus.arc * t * t * (3.0 - 2.0 * t);
                if t >= 1.0 {
                    self.focus = None;
                }
            }
            self.longitude += (now - previous.max(self.paused_until)).max(0.0) * DEGREES_PER_SECOND;
        }
        self.was_held = held;
        self.longitude = (self.longitude + 180.0).rem_euclid(360.0) - 180.0;
    }
}

pub(super) fn sphere(lon: f64, lat: f64) -> Vec3 {
    let (s, c) = lon.to_radians().sin_cos();
    let (sy, cy) = lat.to_radians().sin_cos();
    Vec3::new((cy * s) as f32, sy as f32, (cy * c) as f32)
}

/// Orthographic view of a unit sphere, tilted about screen x toward the observer.
/// Rotate longitude around the Earth's axis first so dragging never changes the pole tilt.
pub(super) struct GlobeView {
    sin: f32,
    cos: f32,
    tilt_sin: f32,
    tilt_cos: f32,
}
impl GlobeView {
    pub fn new(longitude: f64) -> Self {
        let (sin, cos) = (longitude.to_radians() as f32).sin_cos();
        let (tilt_sin, tilt_cos) = TILT.sin_cos();
        Self {
            sin,
            cos,
            tilt_sin,
            tilt_cos,
        }
    }
    pub fn rotate(&self, p: Vec3) -> Vec3 {
        let x = self.cos * p.x - self.sin * p.z;
        let z = self.sin * p.x + self.cos * p.z;
        Vec3::new(
            x,
            self.tilt_cos * p.y - self.tilt_sin * z,
            self.tilt_sin * p.y + self.tilt_cos * z,
        )
    }
    pub fn screen(&self, rect: Rect, p: Vec3) -> Pos2 {
        rect.center() + egui::vec2(p.x, -p.y) * (rect.width().min(rect.height()) * 0.5)
    }
    pub fn project(&self, rect: Rect, lon: f64, lat: f64) -> Option<Pos2> {
        let p = self.rotate(sphere(lon, lat));
        (p.z >= 0.0).then(|| self.screen(rect, p))
    }
    pub fn unproject(&self, rect: Rect, at: Pos2) -> Option<(f64, f64)> {
        let radius = rect.width().min(rect.height()) * 0.5;
        if !at.is_finite() || radius <= 0.0 {
            return None;
        }
        let x = (at.x - rect.center().x) / radius;
        let y = (rect.center().y - at.y) / radius;
        let squared = x * x + y * y;
        if squared > 1.0 {
            return None;
        }
        let z = (1.0 - squared).max(0.0).sqrt(); // front hemisphere only
        let native_y = self.tilt_cos * y + self.tilt_sin * z;
        let native_z = -self.tilt_sin * y + self.tilt_cos * z;
        let native_x = self.cos * x + self.sin * native_z;
        let native_z = -self.sin * x + self.cos * native_z;
        Some((
            f64::from(native_x.atan2(native_z)).to_degrees(),
            f64::from(native_y.clamp(-1.0, 1.0).asin()).to_degrees(),
        ))
    }
}

/// Clip a face crossing the visible limb. Normalizing intersections keeps the outline circular.
pub(super) fn clip_limb(face: &[Vec3]) -> Vec<Vec3> {
    let mut clipped = Vec::with_capacity(6);
    let mut previous = *face.last().expect("a mesh face has vertices");
    for &next in face {
        if (previous.z >= 0.0) != (next.z >= 0.0) {
            let t = previous.z / (previous.z - next.z);
            clipped.push(previous.lerp(next, t).normalize_or_zero());
        }
        if next.z >= 0.0 {
            clipped.push(next);
        }
        previous = next;
    }
    clipped
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec3;
    use bevy_egui::egui::{self, Rect};
    use proptest::prelude::*;

    #[test]
    fn one_rotation_takes_24_seconds_without_frame_rate_dependence() {
        let mut spin = GlobeSpin::default();
        spin.advance(0.0, false, 0.0, 400.0);
        spin.advance(6.0, false, 0.0, 400.0);
        assert!((spin.longitude - 170.0).abs() < 1e-9);
        spin.advance(24.0, false, 0.0, 400.0);
        assert!((spin.longitude + 100.0).abs() < 1e-9);
    }

    #[test]
    fn focus_takes_shortest_arc_then_waits_two_seconds_before_resuming() {
        for (start, target, halfway) in [(170.0, -170.0, 180.0), (-170.0, 170.0, -180.0)] {
            let mut spin = GlobeSpin {
                longitude: start,
                ..Default::default()
            };
            spin.focus(1.0, target);
            spin.advance(1.3, false, 0.0, 400.0);
            assert!(((spin.longitude - halfway + 180.0).rem_euclid(360.0) - 180.0).abs() < 1e-9);
            spin.advance(1.6, false, 0.0, 400.0);
            assert!((spin.longitude - target).abs() < 1e-9);
            spin.advance(3.6, false, 0.0, 400.0);
            assert!((spin.longitude - target).abs() < 1e-9);
            spin.advance(4.0, false, 0.0, 400.0);
            assert!((spin.longitude - (target - 6.0)).abs() < 1e-9);
        }
    }

    #[test]
    fn dragging_interrupts_focus_without_jumping_to_its_target() {
        let mut spin = GlobeSpin::default();
        spin.focus(0.0, 80.0);
        spin.advance(0.3, false, 0.0, 400.0);
        let before = spin.longitude;
        spin.advance(0.3, true, -10.0, 400.0);
        assert!((spin.longitude - before - 9.0).abs() < 1e-5);
        spin.advance(0.4, false, 0.0, 400.0);
        spin.advance(1.0, false, 0.0, 400.0);
        assert!((spin.longitude - before - 9.0).abs() < 1e-5);
    }

    #[test]
    fn dragging_reverses_quickly_then_resumes_after_two_idle_seconds() {
        let mut spin = GlobeSpin::default();
        spin.advance(0.0, false, 0.0, 400.0);
        spin.advance(0.1, true, 100.0, 400.0);
        assert!((spin.longitude - 170.0).abs() < 1e-9);
        spin.advance(0.2, true, -200.0, 400.0);
        assert!((spin.longitude + 10.0).abs() < 1e-9);
        spin.advance(1.0, true, 0.0, 400.0);
        spin.advance(1.1, false, 0.0, 400.0);
        spin.advance(3.0, false, 0.0, 400.0);
        assert!((spin.longitude + 10.0).abs() < 1e-9);
        spin.advance(3.6, false, 0.0, 400.0);
        assert!((spin.longitude + 17.5).abs() < 1e-9);
    }

    #[test]
    fn north_pole_tilts_toward_the_viewer_and_back_faces_cannot_be_picked() {
        let view = GlobeView::new(0.0);
        let north = view.rotate(Vec3::Y);
        assert!((north.z - 22.0f32.to_radians().sin()).abs() < 1e-6);
        assert!(view.rotate(-Vec3::Y).z < 0.0);
        assert!(view.rotate(sphere(180.0, 0.0)).z < 0.0);
        let rect = Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(400.0, 400.0));
        assert!(view.unproject(rect, rect.left_top()).is_none());
        assert!(view.unproject(rect, egui::pos2(f32::NAN, 0.0)).is_none());
        let (lon, lat) = view.unproject(rect, rect.center()).unwrap();
        assert!(lon.abs() < 1e-5 && (lat - 22.0).abs() < 1e-5);
    }

    proptest! {
        #[test]
        fn focus_uses_shortest_arc_and_is_independent_of_frame_partition(
            start in -180.0f64..180.0, target in -180.0f64..180.0,
            seconds in 0.0f64..5.0, frames in 1u32..100,
        ) {
            let mut single = GlobeSpin { longitude: start, ..Default::default() };
            let mut partitioned = single.clone();
            single.focus(0.0, target);
            partitioned.focus(0.0, target);
            single.advance(seconds, false, 0.0, 400.0);
            for frame in 1..=frames {
                partitioned.advance(seconds * f64::from(frame) / f64::from(frames), false, 0.0, 400.0);
            }
            let difference = (single.longitude - partitioned.longitude + 180.0).rem_euclid(360.0) - 180.0;
            prop_assert!(difference.abs() < 1e-8);
            if seconds <= FOCUS_DURATION {
                let arc = (target - start + 180.0).rem_euclid(360.0) - 180.0;
                let t = seconds / FOCUS_DURATION;
                let expected = start + arc * t * t * (3.0 - 2.0 * t);
                prop_assert!(((single.longitude - expected + 180.0).rem_euclid(360.0) - 180.0).abs() < 1e-8);
            }
        }

        #[test]
        fn rotation_is_independent_of_frame_partition(seconds in 0.0f64..240.0, frames in 1u32..240) {
            let mut single=GlobeSpin::default();
            let mut partitioned=GlobeSpin::default();
            single.advance(0.0,false,0.0,400.0);
            partitioned.advance(0.0,false,0.0,400.0);
            single.advance(seconds,false,0.0,400.0);
            for frame in 1..=frames {
                partitioned.advance(seconds*f64::from(frame)/f64::from(frames),false,0.0,400.0);
            }
            let difference=(single.longitude-partitioned.longitude+180.0).rem_euclid(360.0)-180.0;
            prop_assert!(difference.abs()<1e-8);
        }

        #[test]
        fn limb_clipping_never_emits_a_hidden_or_outside_vertex(longitude in -180.0f64..180.0, rotation in -180.0f64..180.0, latitude in -85.0f64..85.0) {
            let view=GlobeView::new(rotation);
            let face=[sphere(longitude,latitude),sphere(longitude+2.0,latitude),sphere(longitude+2.0,latitude+1.0),sphere(longitude,latitude+1.0)].map(|p|view.rotate(p));
            for p in clip_limb(&face) {
                prop_assert!(p.is_finite() && p.z>=-1e-6 && p.x*p.x+p.y*p.y<=1.00001);
            }
        }

        #[test]
        fn visible_geography_round_trips_at_any_rotation(lon in -180.0f64..180.0, lat in -89.0f64..89.0, rotation in -180.0f64..180.0) {
            let view=GlobeView::new(rotation);
            let point=view.rotate(sphere(lon,lat));
            if point.z>0.05 {
                let rect=Rect::from_min_size(egui::pos2(20.0,30.0),egui::vec2(700.0,700.0));
                let (x,y)=view.unproject(rect,view.screen(rect,point)).unwrap();
                let difference=(x-lon+180.0).rem_euclid(360.0)-180.0;
                prop_assert!(difference.abs()<0.001 && (y-lat).abs()<0.001);
            }
        }
    }
}
