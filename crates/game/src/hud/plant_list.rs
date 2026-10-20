//! The plants the player can commit and dispatch, in the order every Planning panel shows them.
//!
//! The commitment grid lists them row by row and the forecast chart stacks their capacity by
//! kind, so both read one list: commissioned, dispatchable plants, grouped by kind in merit order
//! (cheapest fuel first, as the grid dispatches them).

use application::Game;
use domain::generation::PlantStatus;
use domain::region::PlantKind;
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

/// Every commissioned dispatchable plant, grouped by kind in merit order: ascending marginal
/// cost, a tie going to the kind that comes first in [`PlantKind::ALL`]. Kinds with no plant are
/// left out.
pub fn plants_by_kind(game: &Game) -> Vec<KindPlants> {
    let session = game.session();
    let region = session.region();
    let views = session.plant_views();
    let mut kinds: Vec<PlantKind> = PlantKind::ALL
        .into_iter()
        .filter(|kind| kind.is_dispatchable())
        .collect();
    kinds.sort_by(|a, b| {
        let cost = |kind: PlantKind| region.spec(kind).marginal_cost.value();
        cost(*a).total_cmp(&cost(*b)).then(a.cmp(b))
    });
    kinds
        .into_iter()
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
        // The shipped eastern fleet: 2 nuclear, 4 coal, 9 combined cycle, 9 peakers. No solar,
        // which nobody commits, and no hydro.
        let counts: Vec<(PlantKind, usize)> = groups
            .iter()
            .map(|group| (group.kind, group.plants.len()))
            .collect();
        assert_eq!(
            counts,
            [
                (PlantKind::Nuclear, 2),
                (PlantKind::Coal, 4),
                (PlantKind::Ccgt, 9),
                (PlantKind::Peaker, 9),
            ]
        );
        for group in &groups {
            let ids: Vec<u32> = group.plants.iter().map(|plant| plant.id.0).collect();
            assert!(ids.windows(2).all(|pair| pair[0] < pair[1]), "{ids:?}");
        }
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
