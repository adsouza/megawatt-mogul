//! Illustrative source-to-load routes for the copper-plate view, never electrical load flow.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bevy::prelude::*;
use domain::{
    grid::LineStatus,
    ids::{LineId, NodeId},
    session::{DayPhase, LineView},
};

use super::Part;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowDirection {
    Forward,
    Reverse,
}

pub(super) fn route_flows(
    lines: &[LineView],
    sources: &BTreeMap<NodeId, f64>,
    sinks: &BTreeMap<NodeId, f64>,
) -> BTreeMap<LineId, FlowDirection> {
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
    net.into_iter()
        .filter_map(|(line, net)| {
            (net.abs() > 1e-9).then_some((
                line,
                if net > 0.0 {
                    FlowDirection::Forward
                } else {
                    FlowDirection::Reverse
                },
            ))
        })
        .collect()
}

pub(super) fn arrow_transform(
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
            let rotation = if direction == FlowDirection::Reverse {
                part.rotation * Quat::from_rotation_y(std::f32::consts::PI)
            } else {
                part.rotation
            };
            return Some(Transform::from_translation(center).with_rotation(rotation));
        }
        remaining -= segment;
    }
    None
}

/// A small shared chevron; +Z is its travel direction.
pub(super) fn arrow_parts() -> [Part; 2] {
    [-1.0, 1.0].map(|side| {
        let mut part = Part::block(
            Vec3::new(side * 0.07, 0.0, 0.0),
            Vec3::new(0.045, 0.035, 0.28),
            [1.0; 4],
        );
        part.rotation = Quat::from_rotation_y(-side * 0.52);
        part
    })
}

#[derive(Component)]
pub struct FlowArrow {
    pub line: LineId,
    pub(super) slot: u32,
    pub(super) count: u32,
}
#[derive(Resource, Default)]
pub(super) struct FlowAnimation {
    distance: f32,
    generation: Option<u64>,
}

pub(super) fn update_arrows(
    scene: Res<super::WorldScene>,
    controls: Res<super::WorldControls>,
    speed: Option<Res<crate::sim::SimSpeed>>,
    persistence: Option<Res<crate::sim::Persistence>>,
    time: Res<Time>,
    mut animation: ResMut<FlowAnimation>,
    mut arrows: Query<(&FlowArrow, &mut Transform, &mut Visibility)>,
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
    for (arrow, mut transform, mut visibility) in &mut arrows {
        let pose = scene
            .model
            .as_ref()
            .and_then(|m| m.lines.get(&arrow.line))
            .filter(|line| controls.hologram() && line.energized && line.building.is_none())
            .and_then(|line| {
                arrow_transform(
                    &line.parts,
                    line.flow?,
                    animation.distance,
                    arrow.slot,
                    arrow.count,
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
        fn arrow_stays_on_its_segment_and_points_in_the_travel_direction(distance in 0.0f32..10000.0, reverse in any::<bool>(), slot in 0u32..4) {
            let part = Part { center: Vec3::new(0.0, 2.0, 5.0), size: Vec3::new(0.1, 0.1, 10.06), rotation: Quat::IDENTITY, color: [1.0; 4] };
            let direction = if reverse { FlowDirection::Reverse } else { FlowDirection::Forward };
            let transform = arrow_transform(&[part], direction, distance, slot, 4).unwrap();
            prop_assert!(transform.translation.is_finite());
            prop_assert!(transform.translation.z >= 0.0 && transform.translation.z <= 10.0);
            prop_assert!((transform.translation.y - 2.07).abs() < 1e-5);
            let facing = transform.rotation * Vec3::Z;
            prop_assert!(if reverse { facing.z < -0.99 } else { facing.z > 0.99 }, "arrow faces travel direction");
        }
    }
}
