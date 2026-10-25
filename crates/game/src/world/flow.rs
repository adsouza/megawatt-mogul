//! Illustrative source-to-load routes for the copper-plate view, never electrical load flow.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bevy::prelude::*;
use domain::{
    grid::LineStatus,
    ids::{CellCoord, LineId, NodeId},
    session::{DayPhase, LineView},
};

use super::Part;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowDirection {
    Forward,
    Reverse,
}

pub(super) fn route_net_flows(
    lines: &[LineView],
    sources: &BTreeMap<NodeId, f64>,
    sinks: &BTreeMap<NodeId, f64>,
) -> BTreeMap<LineId, f64> {
    let total: f64 = sources.values().sum();
    let demand: f64 = sinks.values().sum();
    if total <= 0.0 || demand <= 0.0 {
        return BTreeMap::new();
    }
    let mut ordered: Vec<_> = lines
        .iter()
        .filter(|l| l.status == LineStatus::InService)
        .collect();
    ordered.sort_by_key(|l| l.id);
    let mut adjacent: BTreeMap<NodeId, Vec<(NodeId, LineId, f64)>> = BTreeMap::new();
    for line in ordered {
        adjacent
            .entry(line.from)
            .or_default()
            .push((line.to, line.id, 1.0));
        adjacent
            .entry(line.to)
            .or_default()
            .push((line.from, line.id, -1.0));
    }
    let mut net: BTreeMap<LineId, f64> = BTreeMap::new();
    for (&source, &output) in sources.iter().filter(|(_, output)| **output > 0.0) {
        let mut reached = BTreeSet::from([source]);
        let mut parents = BTreeMap::new();
        let mut queue = VecDeque::from([source]);
        while let Some(node) = queue.pop_front() {
            for &(next, line, sign) in adjacent.get(&node).into_iter().flatten() {
                if reached.insert(next) {
                    parents.insert(next, (node, line, sign));
                    queue.push_back(next);
                }
            }
        }
        for (&sink, &load) in sinks.iter().filter(|(_, load)| **load > 0.0) {
            if !reached.contains(&sink) {
                continue;
            }
            // Normalised transfers only decide direction; no per-line MW/loading is claimed.
            let transfer = (output / total) * (load / demand);
            let mut node = sink;
            while let Some(&(parent, line, sign)) = parents.get(&node) {
                *net.entry(line).or_default() += sign * transfer;
                node = parent;
            }
        }
    }
    net.retain(|_, value| value.abs() > 1e-9);
    net
}

pub(super) fn direction(net: f64) -> Option<FlowDirection> {
    (net.abs() > 1e-9).then_some(if net > 0.0 {
        FlowDirection::Forward
    } else {
        FlowDirection::Reverse
    })
}

/// Canonical map endpoints of one visible stretch, split wherever collinear lines overlap.
pub type FlowPathKey = (CellCoord, CellCoord);

#[derive(Debug, Clone)]
pub struct FlowPath {
    /// Lowest-ID line covering this stretch owns its shared geometry and pulse entities.
    pub line: LineId,
    pub parts: Vec<Part>,
    pub energized: bool,
    /// Normalised signed transfer in canonical endpoint order, never measured electrical MW.
    pub net_flow: f64,
}

fn on_segment(at: CellCoord, from: CellCoord, to: CellCoord) -> bool {
    let delta = |a: u16, b: u16| i64::from(a) - i64::from(b);
    delta(at.x, from.x) * delta(to.y, from.y) == delta(at.y, from.y) * delta(to.x, from.x)
        && (from.x.min(to.x)..=from.x.max(to.x)).contains(&at.x)
        && (from.y.min(to.y)..=from.y.max(to.y)).contains(&at.y)
}

/// Clip the existing terraced geometry rather than resampling terrain at different intervals.
fn path_parts(line: &super::WorldLine, from: CellCoord, to: CellCoord) -> Vec<Part> {
    let mut parts = Vec::new();
    let Some(first) = line.parts.first() else {
        return parts;
    };
    let origin = first.center - first.rotation * Vec3::Z * (first.size.z - 0.06) * 0.5;
    let axis = Vec2::new(
        f32::from(line.to.x) - f32::from(line.from.x),
        f32::from(line.to.y) - f32::from(line.from.y),
    );
    let length_sq = axis.length_squared();
    if length_sq == 0.0 {
        return parts;
    }
    let at = |cell: CellCoord| {
        Vec2::new(
            f32::from(cell.x) - f32::from(line.from.x),
            f32::from(cell.y) - f32::from(line.from.y),
        )
        .dot(axis)
            / length_sq
    };
    let (a, b) = (at(from), at(to));
    let (lower, upper) = (a.min(b), a.max(b));
    for part in &line.parts {
        let half = part.rotation * Vec3::Z * (part.size.z - 0.06).max(0.0) * 0.5;
        let start = part.center - half;
        let end = part.center + half;
        let t0 = (start.xz() - origin.xz()).dot(axis) / length_sq;
        let t1 = (end.xz() - origin.xz()).dot(axis) / length_sq;
        let (start, end) = if (t1 - t0).abs() < 1e-7 {
            if t0 <= lower || t0 >= upper {
                continue;
            }
            (start, end)
        } else {
            if t1 <= lower || t0 >= upper {
                continue;
            }
            (
                start.lerp(end, ((lower - t0) / (t1 - t0)).clamp(0.0, 1.0)),
                start.lerp(end, ((upper - t0) / (t1 - t0)).clamp(0.0, 1.0)),
            )
        };
        let delta = end - start;
        if let Some(along) = delta.try_normalize() {
            parts.push(Part {
                center: (start + end) * 0.5,
                size: Vec3::new(part.size.x, part.size.y, delta.length() + 0.06),
                rotation: Quat::from_rotation_arc(Vec3::Z, along),
                color: part.color,
            });
        }
    }
    if a > b {
        parts.reverse();
        for part in &mut parts {
            part.rotation *= Quat::from_rotation_y(std::f32::consts::PI);
        }
    }
    parts
}

pub(super) fn net_flow_paths(
    lines: &BTreeMap<LineId, super::WorldLine>,
) -> BTreeMap<FlowPathKey, FlowPath> {
    let endpoints: BTreeSet<_> = lines
        .values()
        .filter(|l| l.building.is_none())
        .flat_map(|l| [l.from, l.to])
        .collect();
    let mut paths: BTreeMap<FlowPathKey, FlowPath> = BTreeMap::new();
    for line in lines
        .values()
        .filter(|l| l.building.is_none() && l.from != l.to)
    {
        let breaks: Vec<_> = endpoints
            .iter()
            .copied()
            .filter(|at| on_segment(*at, line.from, line.to))
            .collect();
        for pair in breaks.windows(2) {
            let key = (pair[0], pair[1]);
            let path = paths.entry(key).or_insert_with(|| FlowPath {
                line: line.id,
                parts: path_parts(line, key.0, key.1),
                energized: false,
                net_flow: 0.0,
            });
            path.energized |= line.energized;
            path.net_flow += if line.from < line.to {
                line.net_flow
            } else {
                -line.net_flow
            };
        }
    }
    paths
}

pub(super) fn pulse_transform(
    parts: &[Part],
    direction: FlowDirection,
    distance: f32,
    slot: u32,
    count: u32,
) -> Option<Transform> {
    let length: f32 = parts.iter().map(|p| (p.size.z - 0.06).max(0.0)).sum();
    if length <= 0.0 || count == 0 || !distance.is_finite() {
        return None;
    }
    let mut fraction = (distance / length + slot as f32 / count as f32).rem_euclid(1.0);
    if direction == FlowDirection::Reverse {
        fraction = 1.0 - fraction;
    }
    let mut remaining = fraction * length;
    for part in parts {
        let segment = (part.size.z - 0.06).max(0.0);
        if segment <= 0.0 {
            continue;
        }
        if remaining <= segment {
            let along = part.rotation * Vec3::Z;
            let center = part.center + along * (remaining - segment * 0.5) + Vec3::Y * 0.07;
            // The shared mesh is a sphere: keep uniform scale so it reads as a dot from any view.
            return Some(Transform::from_translation(center));
        }
        remaining -= segment;
    }
    None
}

#[derive(Component)]
pub struct FlowPulse {
    pub path: FlowPathKey,
    pub(super) slot: u32,
    pub(super) count: u32,
}
#[derive(Resource, Default)]
pub(super) struct FlowAnimation {
    distance: f32,
    generation: Option<u64>,
}

pub(super) fn update_pulses(
    scene: Res<super::WorldScene>,
    controls: Res<super::WorldControls>,
    speed: Option<Res<crate::sim::SimSpeed>>,
    persistence: Option<Res<crate::sim::Persistence>>,
    time: Res<Time>,
    mut animation: ResMut<FlowAnimation>,
    mut pulses: Query<(&FlowPulse, &mut Transform, &mut Visibility)>,
) {
    if animation.generation != scene.generation {
        animation.distance = 0.0;
        animation.generation = scene.generation;
    }
    let running = scene
        .model
        .as_ref()
        .is_some_and(|m| m.phase == DayPhase::Operating)
        && speed.is_some_and(|s| *s != crate::sim::SimSpeed::Paused)
        && !persistence.is_some_and(|p| p.frozen());
    if running {
        animation.distance += time.delta_secs() * 1.2;
    }
    for (pulse, mut transform, mut visibility) in &mut pulses {
        let pose = scene
            .model
            .as_ref()
            .and_then(|m| m.flow_paths.get(&pulse.path))
            .filter(|path| controls.hologram() && path.energized)
            .and_then(|path| {
                pulse_transform(
                    &path.parts,
                    direction(path.net_flow)?,
                    animation.distance,
                    pulse.slot,
                    pulse.count,
                )
            });
        if let Some(pose) = pose {
            *transform = pose;
            *visibility = Visibility::Visible;
        } else {
            *visibility = Visibility::Hidden;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::grid::LineStatus;
    use proptest::prelude::*;

    fn route_flows(
        lines: &[LineView],
        sources: &BTreeMap<NodeId, f64>,
        sinks: &BTreeMap<NodeId, f64>,
    ) -> BTreeMap<LineId, FlowDirection> {
        route_net_flows(lines, sources, sinks)
            .into_iter()
            .filter_map(|(line, net)| direction(net).map(|d| (line, d)))
            .collect()
    }

    fn line(id: u32, from: u32, to: u32) -> LineView {
        LineView {
            id: LineId(id),
            from: NodeId(from),
            to: NodeId(to),
            status: LineStatus::InService,
        }
    }
    fn loads(entries: &[(u32, f64)]) -> BTreeMap<NodeId, f64> {
        entries
            .iter()
            .map(|(id, value)| (NodeId(*id), *value))
            .collect()
    }

    fn wire(
        id: u32,
        from: CellCoord,
        to: CellCoord,
        net_flow: f64,
    ) -> (LineId, super::super::WorldLine) {
        let start = Vec3::new(f32::from(from.x), 1.0, f32::from(from.y));
        let end = Vec3::new(f32::from(to.x), 1.0, f32::from(to.y));
        let delta = end - start;
        let id = LineId(id);
        (
            id,
            super::super::WorldLine {
                id,
                from,
                to,
                net_flow,
                flow: direction(net_flow),
                selection: crate::hud::Selection::Line(id),
                parts: vec![Part {
                    center: (start + end) * 0.5,
                    size: Vec3::new(0.1, 0.1, delta.length() + 0.06),
                    rotation: Quat::from_rotation_arc(Vec3::Z, delta.normalize()),
                    color: [1.0; 4],
                }],
                building: None,
                energized: true,
                midpoint: (start + end) * 0.5,
            },
        )
    }

    #[test]
    fn crossing_lines_keep_separate_streams() {
        let lines = BTreeMap::from([
            wire(1, CellCoord { x: 0, y: 2 }, CellCoord { x: 4, y: 2 }, 0.3),
            wire(2, CellCoord { x: 2, y: 0 }, CellCoord { x: 2, y: 4 }, -0.7),
        ]);
        let paths = net_flow_paths(&lines);
        assert_eq!(paths.len(), 2);
        assert_eq!(
            paths[&(CellCoord { x: 0, y: 2 }, CellCoord { x: 4, y: 2 })].net_flow,
            0.3
        );
        assert_eq!(
            paths[&(CellCoord { x: 2, y: 0 }, CellCoord { x: 2, y: 4 })].net_flow,
            -0.7
        );
    }

    #[test]
    fn overlapping_stretches_sum_signed_transfers_and_hide_exact_cancellation() {
        let a = CellCoord { x: 0, y: 0 };
        let b = CellCoord { x: 2, y: 0 };
        let c = CellCoord { x: 4, y: 0 };
        // Both wires are stored backwards; clipping must still emit canonical geometry.
        let mut lines = BTreeMap::from([wire(1, c, a, -0.3), wire(2, b, a, 0.7)]);
        let paths = net_flow_paths(&lines);
        assert_eq!(paths.len(), 2);
        assert!((paths[&(a, b)].net_flow + 0.4).abs() < 1e-9);
        assert_eq!(paths[&(b, c)].net_flow, 0.3);
        assert_eq!(paths[&(a, b)].line, LineId(1));
        for (key, path) in paths {
            let first = pulse_transform(&path.parts, FlowDirection::Forward, 0.0, 0, 1).unwrap();
            let last = pulse_transform(&path.parts, FlowDirection::Reverse, 0.0, 0, 1).unwrap();
            assert!(
                first
                    .translation
                    .xz()
                    .distance(Vec2::new(f32::from(key.0.x), f32::from(key.0.y)))
                    < 1e-5
            );
            assert!(
                last.translation
                    .xz()
                    .distance(Vec2::new(f32::from(key.1.x), f32::from(key.1.y)))
                    < 1e-5
            );
        }
        lines.get_mut(&LineId(2)).unwrap().net_flow = 0.3;
        assert_eq!(direction(net_flow_paths(&lines)[&(a, b)].net_flow), None);
        lines.get_mut(&LineId(2)).unwrap().building = Some(1);
        let paths = net_flow_paths(&lines);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[&(a, c)].net_flow, 0.3);
    }
    #[test]
    fn routes_follow_generation_through_transit_cities_and_reverse_when_sources_change() {
        let lines = [line(1, 0, 1), line(2, 1, 2)];
        let forward = route_flows(
            &lines,
            &loads(&[(0, 100.0)]),
            &loads(&[(1, 40.0), (2, 60.0)]),
        );
        assert_eq!(
            forward,
            [
                (LineId(1), FlowDirection::Forward),
                (LineId(2), FlowDirection::Forward)
            ]
            .into()
        );
        let reverse = route_flows(&lines, &loads(&[(2, 100.0)]), &loads(&[(0, 100.0)]));
        assert!(reverse.values().all(|d| *d == FlowDirection::Reverse));
        assert_eq!(reverse.len(), 2);
    }
    #[test]
    fn loops_use_stable_shortest_routes_cancel_opposing_transfers_and_exclude_unfinished_lines() {
        let mut lines = vec![line(1, 0, 1), line(2, 1, 3), line(3, 0, 2), line(4, 2, 3)];
        let sources = loads(&[(0, 100.0)]);
        let sinks = loads(&[(3, 100.0)]);
        let chosen = route_flows(&lines, &sources, &sinks);
        assert_eq!(
            chosen.keys().copied().collect::<Vec<_>>(),
            vec![LineId(1), LineId(2)]
        );
        lines.reverse();
        assert_eq!(route_flows(&lines, &sources, &sinks), chosen);
        let cancelled = route_flows(
            &[line(1, 0, 1)],
            &loads(&[(0, 50.0), (1, 50.0)]),
            &loads(&[(0, 50.0), (1, 50.0)]),
        );
        assert!(cancelled.is_empty());
        lines
            .iter_mut()
            .for_each(|l| l.status = LineStatus::UnderConstruction { months_left: 1 });
        assert!(route_flows(&lines, &sources, &sinks).is_empty());
        assert!(route_flows(&[line(1, 0, 1)], &loads(&[]), &sinks).is_empty());
        assert!(route_flows(&[line(1, 0, 1)], &sources, &loads(&[])).is_empty());
        assert!(route_flows(&[line(1, 0, 1)], &sources, &sinks).is_empty());
    }
    proptest! {
        #[test]
        fn each_shared_stretch_has_one_stream_with_the_algebraic_net(
            intervals in prop::collection::vec((0u16..8, 1u16..8, any::<bool>(), -100i16..=100), 1..8),
            dx in -2i16..=2, dy in -2i16..=2,
        ) {
            prop_assume!(dx != 0 || dy != 0);
            let cell = |t: u16| CellCoord {
                x: (40 + dx * t as i16) as u16,
                y: (40 + dy * t as i16) as u16,
            };
            let lines: BTreeMap<_, _> = intervals.iter().enumerate().map(|(i, &(start, len, reverse, weight))| {
                let (a, b) = (cell(start), cell(start + len));
                wire(i as u32, if reverse { b } else { a }, if reverse { a } else { b }, f64::from(weight))
            }).collect();
            let paths = net_flow_paths(&lines);
            let axis = Vec2::new(f32::from(dx), f32::from(dy));
            let parameter = |at: CellCoord| (Vec2::new(f32::from(at.x), f32::from(at.y)) - Vec2::splat(40.0)).dot(axis) / axis.length_squared();
            for cut in 0..15 {
                let t = cut as f32 + 0.5;
                let covering: Vec<_> = paths.iter().filter(|((a, b), _)| {
                    let (a, b) = (parameter(*a), parameter(*b));
                    a.min(b) < t && t < a.max(b)
                }).collect();
                let expected: f64 = intervals.iter().filter(|(start, len, _, _)| *start <= cut && cut < start + len)
                    .map(|(_, _, reverse, weight)| f64::from(*weight) * if *reverse { -1.0 } else { 1.0 }).sum();
                let any_cover = intervals.iter().any(|(start, len, _, _)| *start <= cut && cut < start + len);
                prop_assert_eq!(covering.len(), usize::from(any_cover));
                if let Some(((a, b), path)) = covering.first() {
                    let net = path.net_flow * if parameter(*a) < parameter(*b) { 1.0 } else { -1.0 };
                    prop_assert!((net - expected).abs() < 1e-9);
                    prop_assert!(!path.parts.is_empty());
                }
            }
        }
        #[test]
        fn routed_chain_directions_match_each_cut_supply_and_demand_balance(
            output in prop::array::uniform4(0u16..1000),
            load in prop::array::uniform4(0u16..1000),
        ) {
            let total: f64 = output.iter().map(|v| f64::from(*v)).sum();
            let demand: f64 = load.iter().map(|v| f64::from(*v)).sum();
            prop_assume!(total > 0.0 && demand > 0.0);
            let sources = output.into_iter().enumerate().map(|(n, v)| (NodeId(n as u32), f64::from(v))).collect();
            let sinks = load.into_iter().enumerate().map(|(n, v)| (NodeId(n as u32), f64::from(v))).collect();
            let lines = [line(0, 0, 1), line(1, 1, 2), line(2, 2, 3)];
            let routed = route_flows(&lines, &sources, &sinks);
            for cut in 0..3 {
                let left_output: f64 = output[..=cut].iter().map(|v| f64::from(*v)).sum();
                let left_load: f64 = load[..=cut].iter().map(|v| f64::from(*v)).sum();
                let net = left_output / total - left_load / demand;
                let expected = if net.abs() <= 1e-9 { None } else if net > 0.0 { Some(FlowDirection::Forward) } else { Some(FlowDirection::Reverse) };
                prop_assert_eq!(routed.get(&LineId(cut as u32)).copied(), expected);
            }
        }
        #[test]
        fn pulse_stays_round_and_on_its_segment(distance in 0.0f32..10000.0, reverse in any::<bool>(), slot in 0u32..4) {
            let part = Part { center: Vec3::new(0.0, 2.0, 5.0), size: Vec3::new(0.1, 0.1, 10.06), rotation: Quat::IDENTITY, color: [1.0; 4] };
            let direction = if reverse { FlowDirection::Reverse } else { FlowDirection::Forward };
            let transform = pulse_transform(&[part], direction, distance, slot, 4).unwrap();
            prop_assert!(transform.translation.is_finite());
            prop_assert!(transform.translation.z >= 0.0 && transform.translation.z <= 10.0);
            prop_assert!((transform.translation.y - 2.07).abs() < 1e-5);
            prop_assert!(transform.scale.x > 0.0);
            prop_assert_eq!(transform.scale.x, transform.scale.y);
            prop_assert_eq!(transform.scale.y, transform.scale.z);
            let opposite = if reverse { FlowDirection::Forward } else { FlowDirection::Reverse };
            let opposite = pulse_transform(&[part], opposite, distance, slot, 4).unwrap();
            prop_assert!((transform.translation.z + opposite.translation.z - 10.0).abs() < 1e-5);
        }
    }
}
