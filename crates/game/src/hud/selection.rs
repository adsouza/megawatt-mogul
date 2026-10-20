//! What the player has selected on the map: the one thing the inspector and the build menu talk
//! about.

use bevy::prelude::*;
use domain::ids::{CellCoord, CityId, NodeId, PlantId};

/// The current selection. Plan 2's 2D egui map writes it through [`UiAction::Select`]; Plan 3's 3D
/// picking writes the same resource. It is reset to `None` whenever a game starts.
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
