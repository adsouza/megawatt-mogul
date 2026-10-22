//! What the player has selected on the map: the one thing the inspector and the build menu talk
//! about.

use bevy::prelude::*;
use domain::ids::{CellCoord, CityId, NodeId, PlantId};

/// The current selection. The diorama's bounds/terrain picking writes it through
/// [`UiAction::Select`], as does the headless 2D reference adapter. It is reset to `None` whenever a game starts.
///
/// [`UiAction::Select`]: super::UiAction::Select
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Selection {
    #[default]
    None,
    Cell(CellCoord),
    Plant(PlantId),
    City(CityId),
    /// The first end of a line being drawn: the next node picked is its other end.
    LineFrom(NodeId),
}

pub(super) fn reset_selection(mut selection: ResMut<Selection>) {
    *selection = Selection::None;
}
