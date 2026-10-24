//! The map grid.

use super::RegionError;
use crate::ids::CellCoord;
use crate::units::Ratio;

/// One map cell.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub land: bool,
    pub height: Ratio,
    pub irradiance: Ratio,
    pub wind_potential: Ratio,
    pub cooling_water: Ratio,
    pub hydro_potential: Ratio,
}

/// A rectangular grid of cells, stored row-major (`y * width + x`).
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainGrid {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
}

impl TerrainGrid {
    pub fn new(width: u16, height: u16, cells: Vec<Cell>) -> Result<Self, RegionError> {
        let expected = usize::from(width) * usize::from(height);
        if cells.len() != expected {
            return Err(RegionError::GridSizeMismatch {
                expected,
                actual: cells.len(),
            });
        }
        Ok(Self {
            width,
            height,
            cells,
        })
    }

    pub fn get(&self, at: CellCoord) -> Option<&Cell> {
        if at.x >= self.width || at.y >= self.height {
            return None;
        }
        self.cells
            .get(usize::from(at.y) * usize::from(self.width) + usize::from(at.x))
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }
}
