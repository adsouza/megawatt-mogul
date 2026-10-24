//! Resolution-independent title artwork. Decorative only; never opens or advances a session.
use bevy_egui::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

pub(super) const BACKGROUND: Color32 = Color32::from_rgb(9, 23, 32);
pub(super) const ACCENT: Color32 = Color32::from_rgb(111, 227, 218);

const WINDOW_ON: Color32 = Color32::from_rgb(250, 194, 105);
const WINDOW_OFF: Color32 = Color32::from_rgb(16, 43, 55);
// Explicit floor counts keep the window identities stable at every artwork size.
const CITY: [(f32, f32, f32, usize); 6] = [
    (1.6, 1.1, 1.7, 7),
    (2.4, 1.3, 2.4, 11),
    (3.2, 1.1, 1.2, 5),
    (1.7, 2.0, 1.1, 4),
    (2.6, 2.2, 1.8, 8),
    (3.5, 2.0, 0.8, 3),
];
const WINDOW_COUNT: usize = {
    let mut count = 0;
    let mut i = 0;
    while i < CITY.len() {
        count += CITY[i].3 * 4;
        i += 1;
    }
    count
};
const LIGHT_CYCLE: f64 = 10.0;
const LIGHTS_ON_START: f64 = 1.0;
const LIGHT_RAMP: f64 = 3.0;
const LIGHTS_OFF_START: f64 = LIGHTS_ON_START + LIGHT_RAMP + 2.0;
const LIGHTS_OFF_END: f64 = LIGHTS_OFF_START + LIGHT_RAMP;
const JUNCTION: (f32, f32) = (3.4, 3.2);
const SOURCE_ROUTES: [&[(f32, f32)]; 2] = [
    &[(1.2, 4.0), (2.1, 4.0), (2.1, 3.2), JUNCTION],
    &[(4.65, 3.15), (4.2, 3.5), (3.4, 3.5), JUNCTION],
];
const CITY_ROUTE: [(f32, f32); 2] = [JUNCTION, (3.5, 2.6)];

#[derive(Clone, Copy)]
struct MenuClock {
    start: f64,
    frame: u64,
}

/// Presentation time starts afresh when this screen becomes visible, independent of the game.
pub(super) fn menu_elapsed(ctx: &egui::Context) -> f64 {
    let now = ctx.input(|i| i.time);
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|data| {
        let clock = data.get_temp_mut_or_insert_with(egui::Id::new("title_scene_clock"), || {
            MenuClock { start: now, frame }
        });
        if frame > clock.frame.saturating_add(1) {
            clock.start = now;
        }
        clock.frame = frame;
        now - clock.start
    })
}

/// A separate, stable shuffle for each cycle and direction; never consumes simulation RNG.
fn window_order(cycle: u64, powering_on: bool) -> Vec<usize> {
    let mut order: Vec<_> = (0..WINDOW_COUNT).collect();
    order.sort_unstable_by_key(|id| {
        let mut n = (*id as u64)
            .wrapping_add(cycle.wrapping_mul(0x9e3779b97f4a7c15))
            .wrapping_add(if powering_on {
                0x243f6a8885a308d3
            } else {
                0x13198a2e03707344
            });
        n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
        n ^ (n >> 31)
    });
    order
}

fn window_lights(elapsed: f64) -> Vec<bool> {
    let phase = elapsed.rem_euclid(LIGHT_CYCLE);
    let cycle = (elapsed / LIGHT_CYCLE).floor() as u64;
    let powering_on = phase < LIGHTS_OFF_START;
    let switched = if powering_on {
        (phase - LIGHTS_ON_START) / LIGHT_RAMP
    } else {
        (phase - LIGHTS_OFF_START) / LIGHT_RAMP
    };
    let count = (switched.clamp(0.0, 1.0) * WINDOW_COUNT as f64).floor() as usize;
    let mut lights = vec![!powering_on; WINDOW_COUNT];
    for id in window_order(cycle, powering_on).into_iter().take(count) {
        lights[id] = powering_on;
    }
    lights
}

/// Thermal generation time; pauses preserve smoke and power-flow phases.
fn powered_elapsed(elapsed: f64) -> f64 {
    let first_light = LIGHTS_ON_START + LIGHT_RAMP / WINDOW_COUNT as f64;
    let running_per_cycle = LIGHTS_OFF_END - first_light;
    let cycles = (elapsed / LIGHT_CYCLE).floor();
    let phase = elapsed.rem_euclid(LIGHT_CYCLE);
    cycles * running_per_cycle + (phase - first_light).clamp(0.0, running_per_cycle)
}

fn wheel_rotation(elapsed: f64) -> f32 {
    (elapsed * 0.9).rem_euclid(f64::from(std::f32::consts::TAU)) as f32
}

const WHEEL_HEIGHT: f32 = 0.23;
const WHEEL_RADIUS: f32 = 0.52;
type ScenePoint = (f32, f32, f32);

// The wheel turns in a vertical plane along the bank. Its lower paddles dip into the water.
fn wheel_point(angle: f32, depth: f32) -> (f32, f32, f32) {
    (
        5.12 + depth,
        2.75 + angle.cos() * WHEEL_RADIUS,
        WHEEL_HEIGHT + angle.sin() * WHEEL_RADIUS,
    )
}

/// Clip the wheel in world space, before the isometric projection: water hides its lower part.
fn surface_crossing(a: ScenePoint, b: ScenePoint) -> ScenePoint {
    let t = a.2 / (a.2 - b.2);
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, 0.0)
}
fn dry_segment(a: ScenePoint, b: ScenePoint) -> Option<[ScenePoint; 2]> {
    match (a.2 >= 0.0, b.2 >= 0.0) {
        (false, false) => None,
        (true, true) => Some([a, b]),
        (true, false) => Some([a, surface_crossing(a, b)]),
        (false, true) => Some([surface_crossing(a, b), b]),
    }
}
fn dry_polygon(points: &[ScenePoint]) -> Vec<ScenePoint> {
    let mut visible = Vec::new();
    for (&a, &b) in points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
    {
        if a.2 >= 0.0 {
            visible.push(a);
        }
        if (a.2 >= 0.0) != (b.2 >= 0.0) {
            visible.push(surface_crossing(a, b));
        }
    }
    visible
}

pub(super) fn backdrop(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(rect, 0.0, BACKGROUND);
    let step = 40.0;
    for x in 0..=(rect.width() / step) as usize {
        let x = rect.left() + x as f32 * step;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(0.5, Color32::from_rgb(14, 32, 42)),
        );
    }
    for y in 0..=(rect.height() / step) as usize {
        let y = rect.top() + y as f32 * step;
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(0.5, Color32::from_rgb(14, 32, 42)),
        );
    }
    painter.line_segment(
        [rect.left_top(), rect.right_top()],
        Stroke::new(3.0, ACCENT),
    );
}

fn polygon(painter: &egui::Painter, points: Vec<Pos2>, fill: Color32) {
    painter.add(egui::Shape::convex_polygon(points, fill, Stroke::NONE));
}

/// A miniature illuminated grid: raised terrain, a city, a river and two power sources.
pub(super) fn draw(painter: &egui::Painter, rect: Rect, time: f64) {
    let painter = painter.with_clip_rect(rect);
    let scale = (rect.width() / 11.4).min(rect.height() / 9.5);
    let origin = Pos2::new(rect.center().x, rect.center().y - scale * 1.7);
    let project = |x: f32, z: f32, height: f32| {
        origin + Vec2::new((x - z) * scale * 0.86, (x + z) * scale * 0.42 - height)
    };
    let teal = Color32::from_rgb(30, 100, 103);
    let gold = WINDOW_ON;
    let lights = window_lights(time);
    let thermal_running = lights.iter().any(|on| *on);
    let thermal_time = powered_elapsed(time);
    let mut window = 0;
    // Broad halo and long shadows give the island separation from the blueprint background.
    for layer in (1..=10).rev() {
        painter.circle_filled(
            rect.center() + Vec2::new(12.0, 8.0),
            scale * (1.8 + layer as f32 * 0.19),
            Color32::from_rgba_unmultiplied(21, 102, 106, 4),
        );
    }
    let top = [
        project(0.0, 0.0, 0.0),
        project(6.0, 0.0, 0.0),
        project(6.0, 6.0, 0.0),
        project(0.0, 6.0, 0.0),
    ];
    let depth = Vec2::new(0.0, scale * 0.5);
    polygon(
        &painter,
        vec![top[1], top[2], top[2] + depth, top[1] + depth],
        Color32::from_rgb(13, 38, 47),
    );
    polygon(
        &painter,
        vec![top[2], top[3], top[3] + depth, top[2] + depth],
        Color32::from_rgb(18, 51, 58),
    );
    for row in 0..6 {
        for col in 0..6 {
            let (x, z) = (col as f32, row as f32);
            let water = col == 5 || (col == 4 && row < 2);
            if water {
                continue;
            }
            let fill = Color32::from_rgb(
                25 + ((col + row) % 3) * 3,
                62 + ((col * 2 + row) % 3) * 5,
                65,
            );
            polygon(
                &painter,
                vec![
                    project(x, z, 0.0),
                    project(x + 0.97, z, 0.0),
                    project(x + 0.97, z + 0.97, 0.0),
                    project(x, z + 0.97, 0.0),
                ],
                fill,
            );
        }
    }
    // One uninterrupted water surface along the bank, widening at the far end.
    for (x, z, w, d) in [(5.0, 0.0, 1.0, 6.0), (4.0, 0.0, 1.0, 2.0)] {
        polygon(
            &painter,
            vec![
                project(x, z, 0.0),
                project(x + w, z, 0.0),
                project(x + w, z + d, 0.0),
                project(x, z + d, 0.0),
            ],
            Color32::from_rgb(15, 52, 75),
        );
    }
    // Low-poly boxes, with independently switched windows on both visible faces. A raised
    // base lets rooftop structures sit on the roof rather than intersecting the hall's walls.
    let mut building =
        |x: f32, z: f32, w: f32, d: f32, h: f32, base: f32, roof: Color32, floors: usize| {
            let a = project(x, z, base + h);
            let b = project(x + w, z, base + h);
            let c = project(x + w, z + d, base + h);
            let e = project(x, z + d, base + h);
            let down = Vec2::new(0.0, h);
            polygon(
                &painter,
                vec![b, c, c + down, b + down],
                Color32::from_rgb(22, 61, 75),
            );
            polygon(
                &painter,
                vec![c, e, e + down, c + down],
                Color32::from_rgb(29, 83, 91),
            );
            polygon(&painter, vec![a, b, c, e], roof);
            for floor in 1..=floors {
                let level = base + floor as f32 * scale * 0.2;
                for col in 1..=2 {
                    let f = col as f32 / 3.0;
                    for at in [
                        project(x + w, z + d * f, level),
                        project(x + w * f, z + d, level),
                    ] {
                        painter.line_segment(
                            [at - Vec2::new(1.0, 0.0), at + Vec2::new(1.0, 0.0)],
                            Stroke::new(
                                (scale * 0.045).max(1.0),
                                if lights[window] {
                                    WINDOW_ON
                                } else {
                                    WINDOW_OFF
                                },
                            ),
                        );
                        window += 1;
                    }
                }
            }
        };
    // Back-to-front ordering makes the cluster read like the in-game diorama.
    for (x, z, h, floors) in CITY {
        building(x, z, 0.55, 0.55, h * scale, 0.0, teal, floors);
    }
    // A warm turbine hall and tall stacks.
    building(
        0.6,
        4.1,
        1.0,
        0.7,
        scale * 0.45,
        0.0,
        Color32::from_rgb(183, 112, 61),
        0,
    );
    let roof_height = scale * 0.45;
    let stacks = [(0.78, 4.4, 1.2), (1.20, 4.4, 1.5)];
    for (x, z, height) in stacks {
        building(
            x,
            z,
            0.16,
            0.18,
            scale * height - roof_height,
            roof_height,
            gold,
            0,
        );
    }
    // The thermal station only emits smoke while the city needs power. Small rising puffs
    // expand, drift and fade; presentation time never consumes the simulation RNG.
    if thermal_running {
        for (x, z, height) in stacks {
            let stack = project(x + 0.08, z + 0.09, scale * height);
            for puff in 0..5 {
                let age = (thermal_time * 0.45 + f64::from(puff) / 5.0).fract() as f32;
                let drift = Vec2::new(scale * 0.32 * age, -scale * 0.95 * age);
                painter.circle_filled(
                    stack + drift,
                    scale * (0.07 + 0.19 * age),
                    Color32::from_rgba_unmultiplied(160, 181, 190, (120.0 * (1.0 - age)) as u8),
                );
            }
        }
    }
    // A small hydro mill on the bank, with a two-sided wooden wheel over the water.
    building(
        4.35,
        2.45,
        0.6,
        0.7,
        scale * 0.5,
        0.0,
        Color32::from_rgb(183, 112, 61),
        0,
    );
    let projected = |point: ScenePoint| project(point.0, point.1, point.2 * scale);
    let axle = projected((5.12, 2.75, WHEEL_HEIGHT));
    painter.line_segment(
        [projected((4.8, 2.75, WHEEL_HEIGHT)), axle],
        Stroke::new(scale * 0.08, Color32::from_rgb(173, 141, 102)),
    );
    let wheel_line = |a, b, stroke| {
        if let Some(points) = dry_segment(a, b) {
            painter.line_segment(points.map(projected), stroke);
        }
    };
    let rotation = wheel_rotation(time);
    for depth in [0.0, 0.18] {
        let center = (5.12 + depth, 2.75, WHEEL_HEIGHT);
        for n in 0..48 {
            wheel_line(
                wheel_point(n as f32 * std::f32::consts::TAU / 48.0, depth),
                wheel_point((n + 1) as f32 * std::f32::consts::TAU / 48.0, depth),
                Stroke::new(scale * 0.06, Color32::from_rgb(202, 153, 94)),
            );
        }
        for spoke in 0..8 {
            let angle = rotation + spoke as f32 * std::f32::consts::TAU / 8.0;
            wheel_line(
                center,
                wheel_point(angle, depth),
                Stroke::new(scale * 0.035, Color32::from_rgb(163, 111, 65)),
            );
        }
    }
    for paddle in 0..8 {
        let angle = rotation + paddle as f32 * std::f32::consts::TAU / 8.0;
        let visible = dry_polygon(&[
            wheel_point(angle - 0.11, 0.0),
            wheel_point(angle + 0.11, 0.0),
            wheel_point(angle + 0.11, 0.18),
            wheel_point(angle - 0.11, 0.18),
        ]);
        if visible.len() >= 3 {
            polygon(
                &painter,
                visible.into_iter().map(projected).collect(),
                Color32::from_rgb(221, 173, 109),
            );
        }
    }
    painter.circle_filled(axle, scale * 0.08, Color32::from_rgb(227, 188, 135));
    // Height increases with cos(angle), so the +z crossing rises out of the water and
    // the -z crossing dips in. Three staggered rings travel in the matching direction.
    let reach = (WHEEL_RADIUS * WHEEL_RADIUS - WHEEL_HEIGHT * WHEEL_HEIGHT).sqrt();
    for (z, outward) in [(2.75 - reach, false), (2.75 + reach, true)] {
        for ring in 0..3 {
            let phase = (time * 0.6 + f64::from(ring) / 3.0).rem_euclid(1.0) as f32;
            let progress = if outward { phase } else { 1.0 - phase };
            let radius = 0.025 + progress * 0.20;
            // Fade at both ends so a ring can wrap from one radius to the other smoothly.
            let opacity = (235.0 * (progress * std::f32::consts::PI).sin()).max(0.0) as u8;
            let ripple = (0..=32)
                .map(|n| {
                    let angle = n as f32 * std::f32::consts::TAU / 32.0;
                    project(5.21 + angle.cos() * radius, z + angle.sin() * radius, 0.0)
                })
                .collect();
            painter.add(egui::Shape::line(
                ripple,
                Stroke::new(
                    (scale * 0.025).max(1.0),
                    Color32::from_rgba_unmultiplied(140, 215, 225, opacity),
                ),
            ));
        }
    }
    // Both sources feed one junction and a shared final route into the city.
    for (route, flow_time) in [
        (SOURCE_ROUTES[0], thermal_time),
        (SOURCE_ROUTES[1], time),
        (CITY_ROUTE.as_slice(), time),
    ] {
        let points: Vec<_> = route.iter().map(|(x, z)| project(*x, *z, 4.0)).collect();
        for pair in points.windows(2) {
            painter.line_segment(
                [pair[0], pair[1]],
                Stroke::new(8.0, Color32::from_rgba_unmultiplied(60, 225, 215, 15)),
            );
            painter.line_segment([pair[0], pair[1]], Stroke::new(2.0, ACCENT));
            let t = (flow_time * 0.35).fract() as f32;
            painter.circle_filled(
                pair[0].lerp(pair[1], t),
                2.5,
                Color32::from_rgb(245, 255, 215),
            );
        }
        for point in &points {
            painter.circle_filled(*point, 3.0, BACKGROUND);
            painter.circle_stroke(*point, 3.0, Stroke::new(1.0, ACCENT));
        }
    }
    painter
        .ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn lights_begin_dark_hold_for_two_seconds_and_repeat() {
        for time in [0.0, 0.5, 1.0, 9.0, 9.9, 10.0, 10.5] {
            assert!(window_lights(time).iter().all(|on| !on), "{time}");
        }
        for time in [4.0, 4.5, 5.0, 5.9, 6.0] {
            assert!(window_lights(time).iter().all(|on| *on), "{time}");
        }
        assert_ne!(window_order(0, true), window_order(0, false));
        assert_ne!(window_order(0, true), window_order(1, true));
    }

    proptest! {
        #[test]
        fn each_window_switches_once_per_ramp(cycle in 0_u32..1000, step in 0_u32..299) {
            let base = f64::from(cycle) * LIGHT_CYCLE;
            let t = f64::from(step) / 100.0;
            let before = window_lights(base + 1.0 + t);
            let after = window_lights(base + 1.0 + t + 0.01);
            prop_assert!(before.iter().zip(&after).all(|(a, b)| !a || *b));
            let before = window_lights(base + 6.0 + t);
            let after = window_lights(base + 6.0 + t + 0.01);
            prop_assert!(before.iter().zip(&after).all(|(a, b)| *a || !b));
            let mut order = window_order(u64::from(cycle), true);
            order.sort_unstable();
            prop_assert_eq!(order, (0..WINDOW_COUNT).collect::<Vec<_>>());
        }
    }

    #[test]
    fn source_routes_merge_before_the_city() {
        assert_eq!(SOURCE_ROUTES[0].last(), Some(&JUNCTION));
        assert_eq!(SOURCE_ROUTES[1].last(), Some(&JUNCTION));
        assert_eq!(CITY_ROUTE[0], JUNCTION);
        assert_eq!(CITY_ROUTE[1], (3.5, 2.6));
    }

    #[test]
    fn wheel_keeps_turning_while_the_city_is_dark() {
        assert_ne!(wheel_rotation(0.0), wheel_rotation(1.0));
        assert_ne!(wheel_rotation(9.0), wheel_rotation(10.5));
        assert_ne!(wheel_rotation(9.0), wheel_rotation(11.0));
        assert_ne!(wheel_rotation(2.0), wheel_rotation(3.0));
        assert_ne!(wheel_rotation(11.1), wheel_rotation(12.0));
    }

    proptest! {
        #[test]
        fn wheel_keeps_constant_speed_across_dark_cycle_boundary(cycle in 0_u32..1000, millis in 1_u32..2001) {
            let dark_start = f64::from(cycle) * LIGHT_CYCLE + 9.0;
            let time = dark_start + f64::from(millis) / 1000.0;
            prop_assert!(window_lights(time).iter().all(|on| !on));
            let angle = f64::from(wheel_rotation(time) - wheel_rotation(dark_start));
            let expected = (time - dark_start) * 0.9;
            let error = (angle - expected + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
            prop_assert!(error.abs() < 0.00001);
        }

        #[test]
        fn wheel_angle_has_no_jump_at_light_or_cycle_boundary(cycle in 0_u32..1000) {
            for boundary in [1.0 + 3.0 / WINDOW_COUNT as f64, 9.0, 10.0] {
                let time = f64::from(cycle) * LIGHT_CYCLE + boundary;
                let before = wheel_point(wheel_rotation(time - 0.0001), 0.0);
                let after = wheel_point(wheel_rotation(time + 0.0001), 0.0);
                prop_assert!((before.1 - after.1).abs() < 0.001);
                prop_assert!((before.2 - after.2).abs() < 0.001);
            }
        }
    }

    #[test]
    fn waterwheel_straddles_the_water_and_returns_after_one_turn() {
        let bottom = wheel_point(-std::f32::consts::FRAC_PI_2, 0.0);
        let top = wheel_point(std::f32::consts::FRAC_PI_2, 0.0);
        assert!(
            bottom.2 < -0.2 && top.2 > 0.6,
            "lower paddles are submerged"
        );
        let start = wheel_point(0.0, 0.0);
        assert_ne!(start, wheel_point(0.5, 0.0));
        let full_turn = wheel_point(std::f32::consts::TAU, 0.0);
        assert!((start.0 - full_turn.0).abs() < 0.001);
        assert!((start.1 - full_turn.1).abs() < 0.001);
        assert!((start.2 - full_turn.2).abs() < 0.001);
        assert!(start.0 >= 5.0, "wheel sits over the water column");
    }

    proptest! {
        #[test]
        fn submerged_wheel_geometry_is_hidden(angle in -100.0_f32..100.0) {
            let a = wheel_point(angle, 0.0);
            let b = wheel_point(angle + 0.2, 0.18);
            if let Some(points) = dry_segment(a, b) {
                prop_assert!(points.iter().all(|p| p.2 >= 0.0));
                prop_assert!(a.2 >= 0.0 || b.2 >= 0.0);
                if (a.2 >= 0.0) != (b.2 >= 0.0) {
                    prop_assert!(points.iter().any(|p| p.2 == 0.0));
                }
            } else {
                prop_assert!(a.2 < 0.0 && b.2 < 0.0);
            }
            let paddle = [a, wheel_point(angle + 0.2, 0.0), b, wheel_point(angle, 0.18)];
            let visible = dry_polygon(&paddle);
            prop_assert!(visible.iter().all(|p| p.2 >= 0.0));
            for point in paddle.iter().filter(|p| p.2 >= 0.0) {
                prop_assert!(visible.contains(point), "above-water paddles remain visible");
            }
        }
    }

    fn painted_scene(time: f64) -> Vec<egui::Shape> {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.ctx().layer_painter(egui::LayerId::background());
            draw(
                &painter,
                Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 500.0)),
                time,
            );
        });
        output.textures_delta.clear();
        output.shapes.into_iter().map(|shape| shape.shape).collect()
    }

    fn painted_windows(time: f64) -> usize {
        painted_scene(time)
            .iter()
            .filter(|shape| {
                matches!(
                    shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == WINDOW_ON
                )
            })
            .count()
    }

    fn painted_pulses(time: f64) -> Vec<Pos2> {
        painted_scene(time)
            .into_iter()
            .filter_map(|shape| match shape {
                egui::Shape::Circle(circle) if circle.fill == Color32::from_rgb(245, 255, 215) => {
                    Some(circle.center)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn thermal_line_pauses_with_smoke_while_hydro_and_shared_feed_keep_flowing() {
        let start = painted_pulses(0.0);
        assert_eq!(start.len(), 7);
        assert_eq!(start[..3], painted_pulses(1.0)[..3]);
        assert_ne!(start[3..6], painted_pulses(1.0)[3..6]);
        let stopped = painted_pulses(9.0);
        for time in [9.5, 10.0, 10.5, 11.0] {
            let later = painted_pulses(time);
            assert_eq!(stopped[..3], later[..3], "thermal branch at {time}");
            assert_ne!(stopped[3..6], later[3..6], "hydro keeps flowing");
            assert_ne!(stopped[6], later[6], "shared city feed keeps flowing");
        }
        assert_ne!(stopped[..3], painted_pulses(12.0)[..3]);
    }

    #[test]
    fn smoke_is_only_painted_while_at_least_one_window_is_on() {
        let circles = |time| {
            painted_scene(time)
                .iter()
                .filter(|s| matches!(s, egui::Shape::Circle(_)))
                .count()
        };
        let unlit = circles(0.0);
        for time in [0.0, 1.0, 9.0, 9.9, 10.0, 11.0] {
            assert_eq!(circles(time), unlit, "no smoke at {time}");
        }
        for time in [1.03, 2.5, 5.0, 8.99, 11.03] {
            assert!(window_lights(time).iter().any(|on| *on));
            assert!(circles(time) > unlit, "smoke at {time}");
        }
        assert_ne!(painted_scene(4.1), painted_scene(4.2));
    }

    #[test]
    fn painted_windows_follow_the_light_cycle() {
        assert_eq!(painted_windows(0.0), 0);
        assert_eq!(painted_windows(4.5), WINDOW_COUNT);
        let rising = painted_windows(2.5);
        assert!(rising > 0 && rising < WINDOW_COUNT);
        assert_eq!(painted_windows(9.5), 0);
    }

    #[test]
    fn opening_animation_starts_at_zero_even_after_other_screens() {
        let ctx = egui::Context::default();
        let mut elapsed = -1.0;
        let mut frame = |time, draw_menu| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    if draw_menu {
                        elapsed = menu_elapsed(ui.ctx());
                    }
                },
            );
            output.textures_delta.clear();
            elapsed
        };
        assert_eq!(frame(40.0, true), 0.0);
        assert_eq!(frame(42.0, true), 2.0);
        frame(43.0, false);
        assert_eq!(frame(44.0, true), 0.0);
    }
}
