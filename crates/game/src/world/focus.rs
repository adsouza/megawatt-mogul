//! Physical local-grid bounds. HVDC routes never enlarge a zone to include another zone.
use super::{WorldModel, city_parts, plant_parts};
use bevy::prelude::*;
use domain::{grid::NodeKind, ids::NodeId, session::GameSession};

pub(super) fn zone_bounds(
    model: &WorldModel,
    session: &GameSession,
    zone: NodeId,
    scale: f32,
) -> Option<(Vec3, Vec3)> {
    let computed;
    let nodes = match session.power().island_nodes().get(&zone) {
        Some(nodes) => nodes,
        // A single-area region has no island map until its first topology recompute (at month
        // close), so a new game's grid is the energised AC component that holds `zone`.
        None if session.power().island_nodes().is_empty() => {
            computed = session
                .power()
                .network()
                .islands()
                .into_values()
                .find(|nodes| nodes.contains(&zone))?;
            &computed
        }
        None => return None,
    };
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut include = |low: Vec3, high: Vec3| {
        min = min.min(low);
        max = max.max(high);
    };
    let network = session.power().network();
    for node in nodes {
        let Some(at) = model.nodes.get(node) else {
            continue;
        };
        include(
            *at - Vec3::new(0.5, 0.1, 0.5),
            *at + Vec3::new(0.5, 0.6, 0.5),
        );
        let (parts, scale) = match network.node(*node)?.kind {
            NodeKind::Plant(id) => (plant_parts(model.plants.get(&id)?.kind), Vec3::splat(scale)),
            NodeKind::City(id) => (city_parts(), model.cities.get(&id)?.visual_scale(scale)),
        };
        for part in parts {
            let (low, high) = part.bounds();
            include(*at + low * scale, *at + high * scale);
        }
    }
    for line in model.lines.values() {
        if network
            .line(line.id)
            .is_some_and(|line| nodes.contains(&line.from) && nodes.contains(&line.to))
        {
            for part in &line.parts {
                let (low, high) = part.bounds();
                include(low, high);
            }
        }
    }
    (min.is_finite() && max.is_finite()).then_some((min, max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_bounds_stop_at_hvdc_terminals_and_include_inactive_single_bus_grids() {
        let game = crate::hud::test_support::sgcc_review_with_remote_island();
        let session = game.session();
        let model = WorldModel::of(&game);
        let remote = session
            .city_views()
            .into_iter()
            .find(|c| c.name_key == "city-shanghai")
            .unwrap()
            .node;
        assert!(!session.power().island_active(remote));
        let (low, high) = zone_bounds(&model, session, session.anchor_node(), 1.0).unwrap();
        let remote_at = model.nodes[&remote];
        assert!(
            !remote_at.cmpge(low).all() || !remote_at.cmple(high).all(),
            "HVDC must not enlarge the local grid to include Shanghai"
        );
        let (low, high) = zone_bounds(&model, session, remote, 1.0).unwrap();
        assert!(remote_at.cmpge(low).all() && remote_at.cmple(high).all());
        assert!(
            high.x - low.x <= 1.0 && high.z - low.z <= 1.0,
            "the dormant zone is one local bus"
        );
        assert!(zone_bounds(&model, session, NodeId(u32::MAX), 1.0).is_none());
    }

    /// A single-area region has no island map until its first topology recompute (month close),
    /// so a new game's home grid comes from the network's energised AC components.
    #[test]
    fn a_new_single_area_game_frames_the_starter_grid() {
        for region in ["western_interconnection", "eastern_interconnection"] {
            let game = crate::hud::test_support::shipped_game(
                region,
                Box::new(application::InMemoryCommandLog::default()),
            );
            let session = game.session();
            assert!(session.power().island_nodes().is_empty(), "{region}");
            let model = WorldModel::of(&game);
            let anchor = session.anchor_node();
            let (low, high) = zone_bounds(&model, session, anchor, 1.0)
                .unwrap_or_else(|| panic!("{region}: the starter grid has bounds"));
            let home = session
                .power()
                .network()
                .islands()
                .into_values()
                .find(|nodes| nodes.contains(&anchor))
                .unwrap();
            assert!(home.len() > 1, "{region}");
            for node in home {
                let at = model.nodes[&node];
                assert!(
                    at.cmpge(low).all() && at.cmple(high).all(),
                    "{region}: {node:?}"
                );
            }
            assert!(zone_bounds(&model, session, NodeId(u32::MAX), 1.0).is_none());
        }
    }
}
