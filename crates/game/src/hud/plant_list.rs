//! The plants the player can commit and dispatch, in the order every Planning panel shows them.
//!
//! The commitment grid lists them row by row and the forecast chart stacks their capacity by
//! kind, so both read one list: commissioned, dispatchable plants, grouped by kind in merit order
//! (cheapest fuel first, as the grid dispatches them).

use application::Game;
use domain::generation::PlantStatus;
use domain::region::{PlantKind, RegionDefinition};
use domain::session::PlantView;

/// The commissioned plants of one kind, by id.
#[derive(Debug, Clone, PartialEq)]
pub struct KindPlants {
    pub kind: PlantKind,
    pub plants: Vec<PlantView>,
}

/// Whether the player can commit and dispatch this plant: it has been commissioned and its output
/// is a choice (solar follows the sun).
pub fn is_commitable(plant: &PlantView) -> bool {
    plant.kind.is_dispatchable() && !matches!(plant.status, PlantStatus::UnderConstruction { .. })
}

/// Generating kinds in merit order, followed by storage as a separate group: ascending marginal cost, a tie going to the kind that comes
/// first in [`PlantKind::ALL`]. The one ordering every panel stacks or lists kinds in.
pub fn kinds_in_merit_order(region: &RegionDefinition) -> Vec<PlantKind> {
    let mut kinds = PlantKind::ALL.to_vec();
    kinds.sort_by(|a, b| {
        let cost = |kind: PlantKind| region.spec(kind).marginal_cost.value();
        (*a == PlantKind::Battery)
            .cmp(&(*b == PlantKind::Battery))
            .then_with(|| cost(*a).total_cmp(&cost(*b)))
            .then(a.cmp(b))
    });
    kinds
}

/// Every commissioned dispatchable plant, grouped by kind in merit order (see
/// [`kinds_in_merit_order`]). Kinds with no plant are left out.
pub fn plants_by_kind(game: &Game) -> Vec<KindPlants> {
    let session = game.session();
    let views = session.plant_views();
    kinds_in_merit_order(session.region())
        .into_iter()
        .filter(|kind| kind.is_dispatchable())
        .filter_map(|kind| {
            let plants: Vec<PlantView> = views
                .iter()
                .filter(|plant| plant.kind == kind && is_commitable(plant))
                .cloned()
                .collect();
            (!plants.is_empty()).then_some(KindPlants { kind, plants })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use application::{Command, InMemoryCommandLog};

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_game};

    #[test]
    fn plants_come_grouped_by_kind_cheapest_fuel_first() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let region = game.session().region();

        let groups = plants_by_kind(&game);

        let costs: Vec<f64> = groups
            .iter()
            .map(|group| region.spec(group.kind).marginal_cost.value())
            .collect();
        assert!(
            costs.windows(2).all(|pair| pair[0] <= pair[1]),
            "merit order: {costs:?}"
        );
        // All four starting kinds appear in merit order, with exactly the counts
        // specified by the region, independent of the presenter grouping.
        let counts: Vec<(PlantKind, usize)> = groups
            .iter()
            .map(|group| (group.kind, group.plants.len()))
            .collect();
        assert_eq!(
            counts,
            [
                PlantKind::Nuclear,
                PlantKind::Coal,
                PlantKind::Ccgt,
                PlantKind::Peaker,
            ]
            .map(|kind| (
                kind,
                region
                    .start()
                    .fleet
                    .iter()
                    .filter(|plant| plant.kind == kind)
                    .count()
            ))
        );
        for group in &groups {
            let ids: Vec<u32> = group.plants.iter().map(|plant| plant.id.0).collect();
            assert!(ids.windows(2).all(|pair| pair[0] < pair[1]), "{ids:?}");
        }
    }

    #[test]
    fn every_kind_has_a_place_in_the_merit_order_cheapest_first() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let region = game.session().region();

        let kinds = kinds_in_merit_order(region);

        let mut sorted = kinds.clone();
        sorted.sort();
        assert_eq!(sorted, PlantKind::ALL, "each kind once");
        let costs: Vec<f64> = kinds
            .iter()
            .filter(|kind| **kind != PlantKind::Battery)
            .map(|kind| region.spec(*kind).marginal_cost.value())
            .collect();
        assert!(costs.windows(2).all(|pair| pair[0] <= pair[1]), "{costs:?}");
        let dispatchable: Vec<PlantKind> = plants_by_kind(&game)
            .iter()
            .map(|group| group.kind)
            .collect();
        let filtered: Vec<PlantKind> = kinds
            .into_iter()
            .filter(|kind| dispatchable.contains(kind))
            .collect();
        assert_eq!(filtered, dispatchable, "the grid's order is the same one");
    }

    #[test]
    fn plants_under_construction_and_solar_are_not_listed() {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let region = session.region();
        let land = (0..region.terrain().height())
            .flat_map(|y| (0..region.terrain().width()).map(move |x| (x, y)))
            .map(|(x, y)| domain::ids::CellCoord { x, y })
            .find(|at| region.site_check(PlantKind::Peaker, *at).is_ok())
            .expect("the region has land");
        let before: usize = plants_by_kind(&game).iter().map(|g| g.plants.len()).sum();

        game.execute(Command::BuildPlant {
            kind: PlantKind::Peaker,
            cell: land,
        })
        .unwrap();

        let all = game.session().plant_views().len();
        let listed: usize = plants_by_kind(&game).iter().map(|g| g.plants.len()).sum();
        assert_eq!(all, before + 1, "the new plant exists");
        assert_eq!(listed, before, "but it is still under construction");
    }

    #[test]
    fn a_hydro_fleet_is_listed_with_its_kind() {
        let game = shipped_game(
            "western_interconnection",
            Box::new(InMemoryCommandLog::default()),
        );

        let kinds: Vec<PlantKind> = plants_by_kind(&game)
            .iter()
            .map(|group| group.kind)
            .collect();

        assert!(kinds.contains(&PlantKind::Hydro), "{kinds:?}");
        assert!(!kinds.contains(&PlantKind::Solar));
    }
}
