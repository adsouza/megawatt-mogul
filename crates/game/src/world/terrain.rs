//! Centred X/Z coordinates, flat cell tops and the vertical faces joining unequal elevations.
//! These are visual dimensions only; no value participates in grid simulation or plant tuning.
use bevy::math::Vec3;
use domain::{ids::CellCoord, region::TerrainGrid};

const LAND_LIFT: f32 = 0.12;
const RELIEF: f32 = 2.0;
pub(super) const BASE: f32 = -0.25;
pub(super) const MAX_HEIGHT: f32 = LAND_LIFT + RELIEF;
const MAX_CELLS: usize = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridTransform {
    width: u16,
    height: u16,
}
impl GridTransform {
    pub fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }
    /// Half-open bounds: row zero lies at negative Z, and the right/far edges select no cell.
    pub fn cell_at(self, x: f32, z: f32) -> Option<CellCoord> {
        if !x.is_finite() || !z.is_finite() {
            return None;
        }
        let x = x + f32::from(self.width) / 2.0;
        let y = z + f32::from(self.height) / 2.0;
        if x < 0.0 || y < 0.0 || x >= f32::from(self.width) || y >= f32::from(self.height) {
            return None;
        }
        Some(CellCoord {
            x: x.floor() as u16,
            y: y.floor() as u16,
        })
    }
    pub fn cell_center(self, terrain: &TerrainGrid, at: CellCoord) -> Option<Vec3> {
        if self.width != terrain.width() || self.height != terrain.height() {
            return None;
        }
        Some(Vec3::new(
            f32::from(at.x) + 0.5 - f32::from(self.width) / 2.0,
            surface_height(terrain, at)?,
            f32::from(at.y) + 0.5 - f32::from(self.height) / 2.0,
        ))
    }
}
pub fn surface_height(terrain: &TerrainGrid, at: CellCoord) -> Option<f32> {
    terrain.get(at).map(|cell| {
        if cell.land {
            LAND_LIFT + cell.height.value() as f32 * RELIEF
        } else {
            0.0
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainGeometryError {
    PaletteLength { expected: usize, actual: usize },
    TooLarge,
}
/// Bevy mesh attributes in deterministic row/column/face order. Colours are opaque linear RGBA.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TerrainGeometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    /// Per cell in row-major order, which of its walls (west, east, north, south as bits 0 to 3)
    /// the mesh has. Recolouring replays this instead of re-measuring the relief.
    walls: Vec<u8>,
}
const SIDE_SHADE: f32 = 0.62;
const BOTTOM_COLOR: [f32; 4] = [0.08, 0.06, 0.04, 1.0];

/// Linear-light value of every sRGB byte, so a palette never calls `powf` per cell.
fn linear_table() -> [f32; 256] {
    std::array::from_fn(|v| {
        let value = v as f32 / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    })
}
/// The opaque top colour of a cell and the darker colour of its walls.
fn cell_colors(linear: &[f32; 256], rgb: [u8; 3]) -> ([f32; 4], [f32; 4]) {
    let color = [
        linear[usize::from(rgb[0])],
        linear[usize::from(rgb[1])],
        linear[usize::from(rgb[2])],
        1.0,
    ];
    let side = [
        color[0] * SIDE_SHADE,
        color[1] * SIDE_SHADE,
        color[2] * SIDE_SHADE,
        1.0,
    ];
    (color, side)
}
/// Where each wall of the cell at (`x`, `y`) stands, in west, east, north, south order: the
/// neighbour's surface, or the base off the grid. Only the higher cell owns a shared wall, so a
/// wall exists where the cell's own surface `h` is above this floor.
fn wall_floors(terrain: &TerrainGrid, x: u16, y: u16) -> [f32; 4] {
    let floor = |nx: i32, ny: i32| {
        if nx < 0 || ny < 0 || nx >= i32::from(terrain.width()) || ny >= i32::from(terrain.height())
        {
            BASE
        } else {
            surface_height(
                terrain,
                CellCoord {
                    x: nx as u16,
                    y: ny as u16,
                },
            )
            .expect("neighbor checked against grid bounds")
        }
    };
    let (x, y) = (i32::from(x), i32::from(y));
    [
        floor(x - 1, y),
        floor(x + 1, y),
        floor(x, y - 1),
        floor(x, y + 1),
    ]
}
fn check_palette(terrain: &TerrainGrid, palette: &[[u8; 3]]) -> Result<(), TerrainGeometryError> {
    let cells = usize::from(terrain.width()) * usize::from(terrain.height());
    if cells > MAX_CELLS {
        return Err(TerrainGeometryError::TooLarge);
    }
    if palette.len() != cells {
        return Err(TerrainGeometryError::PaletteLength {
            expected: cells,
            actual: palette.len(),
        });
    }
    Ok(())
}

impl TerrainGeometry {
    /// The vertex colours `build` would give `palette` for the same terrain, in the same order,
    /// without rebuilding the geometry. A map-layer change replaces only the colour attribute
    /// with this.
    pub fn colors_of(&self, palette: &[[u8; 3]]) -> Result<Vec<[f32; 4]>, TerrainGeometryError> {
        if palette.len() != self.walls.len() {
            return Err(TerrainGeometryError::PaletteLength {
                expected: self.walls.len(),
                actual: palette.len(),
            });
        }
        let mut colors = Vec::with_capacity(self.colors.len());
        if palette.is_empty() {
            return Ok(colors);
        }
        let linear = linear_table();
        for (&rgb, &walls) in palette.iter().zip(&self.walls) {
            let (color, side) = cell_colors(&linear, rgb);
            colors.extend_from_slice(&[color; 4]);
            for _ in 0..walls.count_ones() {
                colors.extend_from_slice(&[side; 4]);
            }
        }
        colors.extend_from_slice(&[BOTTOM_COLOR; 4]);
        Ok(colors)
    }
    pub fn build(terrain: &TerrainGrid, palette: &[[u8; 3]]) -> Result<Self, TerrainGeometryError> {
        check_palette(terrain, palette)?;
        let width = terrain.width();
        let height = terrain.height();
        let cells = usize::from(width) * usize::from(height);
        let mut mesh = Self::default();
        if cells == 0 {
            return Ok(mesh);
        }
        let half_x = f32::from(width) / 2.0;
        let half_z = f32::from(height) / 2.0;
        let linear = linear_table();
        for y in 0..height {
            for x in 0..width {
                let at = CellCoord { x, y };
                let h = surface_height(terrain, at).expect("loop stays on the validated grid");
                let x0 = f32::from(x) - half_x;
                let x1 = x0 + 1.0;
                let z0 = f32::from(y) - half_z;
                let z1 = z0 + 1.0;
                let (color, side) = cell_colors(
                    &linear,
                    palette[usize::from(y) * usize::from(width) + usize::from(x)],
                );
                mesh.quad(
                    [[x0, h, z0], [x0, h, z1], [x1, h, z1], [x1, h, z0]],
                    [0.0, 1.0, 0.0],
                    color,
                );
                let [west, east, north, south] = wall_floors(terrain, x, y);
                mesh.walls.push(
                    [west, east, north, south]
                        .into_iter()
                        .enumerate()
                        .fold(0, |bits, (bit, low)| bits | (u8::from(h > low) << bit)),
                );
                // Only the higher cell owns a shared wall; equal-height cells leave no duplicate faces.
                if h > west {
                    mesh.quad(
                        [[x0, west, z0], [x0, west, z1], [x0, h, z1], [x0, h, z0]],
                        [-1.0, 0.0, 0.0],
                        side,
                    );
                }
                if h > east {
                    mesh.quad(
                        [[x1, east, z1], [x1, east, z0], [x1, h, z0], [x1, h, z1]],
                        [1.0, 0.0, 0.0],
                        side,
                    );
                }
                if h > north {
                    mesh.quad(
                        [[x1, north, z0], [x0, north, z0], [x0, h, z0], [x1, h, z0]],
                        [0.0, 0.0, -1.0],
                        side,
                    );
                }
                if h > south {
                    mesh.quad(
                        [[x0, south, z1], [x1, south, z1], [x1, h, z1], [x0, h, z1]],
                        [0.0, 0.0, 1.0],
                        side,
                    );
                }
            }
        }
        mesh.quad(
            [
                [-half_x, BASE, -half_z],
                [half_x, BASE, -half_z],
                [half_x, BASE, half_z],
                [-half_x, BASE, half_z],
            ],
            [0.0, -1.0, 0.0],
            BOTTOM_COLOR,
        );
        Ok(mesh)
    }
    fn quad(&mut self, positions: [[f32; 3]; 4], normal: [f32; 3], color: [f32; 4]) {
        // MAX_CELLS bounds even the worst-case face count far below u32::MAX vertices.
        let first = self.positions.len() as u32;
        self.positions.extend(positions);
        self.normals.extend([normal; 4]);
        self.colors.extend([color; 4]);
        self.indices
            .extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    }
}
