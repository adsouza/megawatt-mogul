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
    pub geothermal_potential: Ratio,
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

    /// Water cells touched by the closed segment between cell centres. Corner crossings
    /// include both adjacent cells. Invalid endpoints produce no route.
    pub fn water_cells_on_segment(&self, a: CellCoord, b: CellCoord) -> Vec<CellCoord> {
        if self.get(a).is_none() || self.get(b).is_none() {
            return Vec::new();
        }
        let (mut x, mut y) = (i32::from(a.x), i32::from(a.y));
        let dx = i32::from(b.x) - x;
        let dy = i32::from(b.y) - y;
        let (nx, ny) = (i64::from(dx.abs()), i64::from(dy.abs()));
        let (sx, sy) = (dx.signum(), dy.signum());
        let (mut ix, mut iy) = (0i64, 0i64);
        let mut water = Vec::new();
        let mut visit = |x: i32, y: i32| {
            if let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) {
                let at = CellCoord { x, y };
                if self.get(at).is_some_and(|cell| !cell.land) {
                    water.push(at);
                }
            }
        };
        visit(x, y);
        while ix < nx || iy < ny {
            match ((1 + 2 * ix) * ny).cmp(&((1 + 2 * iy) * nx)) {
                std::cmp::Ordering::Equal => {
                    visit(x + sx, y);
                    visit(x, y + sy);
                    x += sx;
                    y += sy;
                    ix += 1;
                    iy += 1;
                }
                std::cmp::Ordering::Less => {
                    x += sx;
                    ix += 1;
                }
                std::cmp::Ordering::Greater => {
                    y += sy;
                    iy += 1;
                }
            }
            visit(x, y);
        }
        water
    }

    pub fn has_geothermal(&self, min: Ratio) -> bool {
        self.cells
            .iter()
            .any(|c| c.land && c.geothermal_potential >= min)
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    fn grid(land: bool) -> TerrainGrid {
        TerrainGrid::new(
            8,
            8,
            vec![
                Cell {
                    land,
                    height: Ratio::ZERO,
                    irradiance: Ratio::ZERO,
                    wind_potential: Ratio::ZERO,
                    cooling_water: Ratio::ZERO,
                    hydro_potential: Ratio::ZERO,
                    geothermal_potential: Ratio::ZERO,
                };
                64
            ],
        )
        .unwrap()
    }
    #[test]
    fn water_supercover_includes_both_sides_of_corner_and_boundary_cells() {
        let g = grid(false);
        assert_eq!(
            g.water_cells_on_segment(CellCoord { x: 0, y: 0 }, CellCoord { x: 1, y: 1 }),
            vec![
                CellCoord { x: 0, y: 0 },
                CellCoord { x: 1, y: 0 },
                CellCoord { x: 0, y: 1 },
                CellCoord { x: 1, y: 1 }
            ]
        );
        assert_eq!(
            g.water_cells_on_segment(CellCoord { x: 7, y: 7 }, CellCoord { x: 7, y: 7 }),
            vec![CellCoord { x: 7, y: 7 }]
        );
        assert!(
            TerrainGrid::new(0, 0, vec![])
                .unwrap()
                .water_cells_on_segment(CellCoord { x: 0, y: 0 }, CellCoord { x: 0, y: 0 })
                .is_empty()
        );
        assert!(
            g.water_cells_on_segment(CellCoord { x: 8, y: 0 }, CellCoord { x: 0, y: 0 })
                .is_empty()
        );
    }
    proptest! {
        #[test]
        fn water_supercover_is_symmetric_includes_endpoints_and_ignores_land(ax in 0u16..8, ay in 0u16..8, bx in 0u16..8, by in 0u16..8) {
            let a=CellCoord{x:ax,y:ay}; let b=CellCoord{x:bx,y:by};
            let forward=grid(false).water_cells_on_segment(a,b);
            let backward=grid(false).water_cells_on_segment(b,a);
            prop_assert!(forward.contains(&a) && forward.contains(&b));
            prop_assert_eq!(forward.iter().copied().collect::<std::collections::BTreeSet<_>>(), backward.into_iter().collect());
            prop_assert!(grid(true).water_cells_on_segment(a,b).is_empty());
        }
    }
}
