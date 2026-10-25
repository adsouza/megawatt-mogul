//! The planning map (spec §8.4): a 2D egui painter over the region's terrain, in Planning and
//! Operating. Plan 3 replaces it with the 3D diorama, whose picking writes the same [`Selection`].
//!
//! Layout and hit-testing are plain functions ([`MapLayout`], [`hit_test`]) so they run without
//! egui. The terrain is painted one rect per cell straight from the region (not copied into the
//! model), so a frame allocates only the few dozen markers of [`MapObjects`].

use std::collections::BTreeMap;
use std::sync::Arc;

use application::Game;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::generation::PlantStatus;
use domain::grid::LineStatus;
use domain::ids::{CellCoord, CityId, LineId, NodeId, PlantId, RegionId};
use domain::region::{PlantKind, RegionDefinition};
use egui::{Color32, Pos2, Rect, Stroke, StrokeKind, vec2};

use super::action::{ActionSink, ButtonModel, ToggleModel, UiAction, dispatch, toggle_button};
use super::layout::HudArea;
use super::selection::Selection;
use crate::l10n::{Localize, Localizer};
use crate::sim::Session;

/// What the map colours its cells by.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MapLayer {
    #[default]
    Terrain,
    Irradiance,
    WindPotential,
    CoolingWater,
    HydroPotential,
    /// Where a plant of this kind may be built (the region's siting rule).
    Suitability(PlantKind),
}

impl MapLayer {
    /// Every selectable layer, in display order. Gas and peaker siting accepts any land cell,
    /// so their suitability maps add no useful information.
    pub fn all() -> impl Iterator<Item = Self> {
        [
            Self::Terrain,
            Self::Irradiance,
            Self::WindPotential,
            Self::CoolingWater,
            Self::HydroPotential,
        ]
        .into_iter()
        .chain(
            [
                PlantKind::Nuclear,
                PlantKind::Coal,
                PlantKind::Solar,
                PlantKind::Hydro,
            ]
            .into_iter()
            .map(Self::Suitability),
        )
    }
}

pub(super) fn reset_layer(mut layer: ResMut<MapLayer>) {
    *layer = MapLayer::default();
}

/// Whether the player has chosen to show the map legend: `None` until they click its toggle, and
/// then a corner that hides nothing decides ([`legend_corner`]). It lasts the whole game, through every phase
/// (unlike `UiState`, which a month's planning owns), and a new game starts undecided.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LegendChoice(pub Option<bool>);

pub(super) fn reset_legend(mut choice: ResMut<LegendChoice>) {
    *choice = LegendChoice::default();
}

// ---- layout and hit-testing: pure -----------------------------------------------------------

/// Where the grid sits on screen: the largest fit of `cols × rows` square cells in an area,
/// centred in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapLayout {
    origin: Pos2,
    cell: f32,
    cols: u16,
    rows: u16,
}

impl MapLayout {
    /// Fits a `cols × rows` grid into `area`, keeping square cells. An empty grid or area gives
    /// cells of size zero, which no position hits.
    pub fn fit(area: Rect, cols: u16, rows: u16) -> Self {
        let usable = cols > 0 && rows > 0 && area.width() > 0.0 && area.height() > 0.0;
        let cell = if usable {
            (area.width() / f32::from(cols)).min(area.height() / f32::from(rows))
        } else {
            0.0
        };
        let size = vec2(cell * f32::from(cols), cell * f32::from(rows));
        Self {
            origin: area.center() - size / 2.0,
            cell,
            cols,
            rows,
        }
    }

    /// The side of one cell, in points.
    pub fn cell_size(&self) -> f32 {
        self.cell
    }

    /// The rectangle the whole grid covers.
    pub fn bounds(&self) -> Rect {
        let size = vec2(
            self.cell * f32::from(self.cols),
            self.cell * f32::from(self.rows),
        );
        Rect::from_min_size(self.origin, size)
    }

    pub fn cell_rect(&self, at: CellCoord) -> Rect {
        let offset = vec2(f32::from(at.x), f32::from(at.y)) * self.cell;
        Rect::from_min_size(self.origin + offset, vec2(self.cell, self.cell))
    }

    pub fn cell_center(&self, at: CellCoord) -> Pos2 {
        self.cell_rect(at).center()
    }

    /// The cell under `pos`, if `pos` is on the grid.
    pub fn cell_at(&self, pos: Pos2) -> Option<CellCoord> {
        if self.cell <= 0.0 {
            return None;
        }
        let offset = (pos - self.origin) / self.cell;
        if offset.x < 0.0 || offset.y < 0.0 {
            return None;
        }
        // Truncation is the point: the whole cells from the origin. A huge offset saturates and
        // fails the bounds check below.
        let (x, y) = (offset.x as u32, offset.y as u32);
        (x < u32::from(self.cols) && y < u32::from(self.rows)).then_some(CellCoord {
            x: x as u16,
            y: y as u16,
        })
    }
}

/// A plant on the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlantMarker {
    pub id: PlantId,
    pub kind: PlantKind,
    pub cell: CellCoord,
    /// The months left while under construction, `None` once built. Drawn faded, and labelled
    /// with its months left (spec §16.9), until then.
    pub months_left: Option<u32>,
}

impl PlantMarker {
    pub fn built(&self) -> bool {
        self.months_left.is_none()
    }
}

/// A city on the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CityMarker {
    pub id: CityId,
    pub cell: CellCoord,
    /// Energised: drawn filled. An unconnected city is a hollow ring.
    pub connected: bool,
}

/// A transmission line, from cell to cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineMarker {
    pub id: LineId,
    pub from: CellCoord,
    pub to: CellCoord,
    /// The months left while under construction, `None` in service. Drawn dashed, and labelled
    /// with its months left, until then.
    pub months_left: Option<u32>,
}

impl LineMarker {
    pub fn in_service(&self) -> bool {
        self.months_left.is_none()
    }
}

/// What stands on the map, as drawn and as picked: a few dozen items, in id order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapObjects {
    pub plants: Vec<PlantMarker>,
    pub cities: Vec<CityMarker>,
    pub lines: Vec<LineMarker>,
    /// The cell of every network node: each city and each plant.
    pub nodes: BTreeMap<NodeId, CellCoord>,
}

impl MapObjects {
    /// The markers of `game`, read through `GameSession`'s queries.
    pub fn of(game: &Game) -> Self {
        let session = game.session();
        let mut nodes = BTreeMap::new();
        let plants: Vec<PlantMarker> = session
            .plant_views()
            .into_iter()
            .map(|plant| {
                nodes.insert(plant.node, plant.cell);
                PlantMarker {
                    id: plant.id,
                    kind: plant.kind,
                    cell: plant.cell,
                    months_left: match plant.status {
                        PlantStatus::UnderConstruction { months_left } => Some(months_left),
                        _ => None,
                    },
                }
            })
            .collect();
        let cities: Vec<CityMarker> = session
            .city_views()
            .into_iter()
            .map(|city| {
                nodes.insert(city.node, city.cell);
                CityMarker {
                    id: city.id,
                    cell: city.cell,
                    connected: city.connected,
                }
            })
            .collect();
        let lines = session
            .line_views()
            .into_iter()
            .filter_map(|line| {
                Some(LineMarker {
                    id: line.id,
                    from: *nodes.get(&line.from)?,
                    to: *nodes.get(&line.to)?,
                    months_left: match line.status {
                        LineStatus::UnderConstruction { months_left } => Some(months_left),
                        LineStatus::InService => None,
                    },
                })
            })
            .collect();
        Self {
            plants,
            cities,
            lines,
            nodes,
        }
    }
}

/// Half the side of a plant's drawn square, in points: well inside its cell, so the plants of a
/// dense cluster leave their neighbours' cells (and cities) clickable.
pub fn plant_half_size(layout: &MapLayout) -> f32 {
    (layout.cell_size() * 0.45).max(3.0)
}

/// The radius of a city's drawn circle, in points.
pub fn city_radius(layout: &MapLayout) -> f32 {
    (layout.cell_size() * 0.4).max(3.0)
}

/// What a click at `pos` selects: what is drawn there, topmost first. That is a plant (its drawn
/// square), else a city (its drawn circle), else the nearest line, else the cell's occupant,
/// else the empty cell, else nothing.
/// Among markers drawn on top of each other, the one drawn last is on top.
pub fn pick(layout: &MapLayout, pos: Pos2, objects: &MapObjects) -> Selection {
    if layout.cell_size() <= 0.0 {
        return Selection::None;
    }
    let half = plant_half_size(layout);
    let radius = city_radius(layout);
    let plant = objects.plants.iter().rev().find(|plant| {
        let offset = pos - layout.cell_center(plant.cell);
        offset.x.abs() <= half && offset.y.abs() <= half
    });
    let city = || {
        objects
            .cities
            .iter()
            .rev()
            .find(|city| (pos - layout.cell_center(city.cell)).length() <= radius)
    };
    plant
        .map(|plant| Selection::Plant(plant.id))
        .or_else(|| city().map(|city| Selection::City(city.id)))
        .or_else(|| {
            objects
                .lines
                .iter()
                .filter_map(|line| {
                    let a = layout.cell_center(line.from);
                    let delta = layout.cell_center(line.to) - a;
                    let along = ((pos - a).dot(delta) / delta.length_sq().max(f32::EPSILON))
                        .clamp(0.0, 1.0);
                    let distance = pos.distance(a + delta * along);
                    (distance <= 5.0).then_some((line.id, distance))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
                .map(|(id, _)| Selection::Line(id))
        })
        .or_else(|| {
            layout.cell_at(pos).map(|cell| {
                objects
                    .plants
                    .iter()
                    .find(|plant| plant.cell == cell)
                    .map(|plant| Selection::Plant(plant.id))
                    .or_else(|| {
                        objects
                            .cities
                            .iter()
                            .find(|city| city.cell == cell)
                            .map(|city| Selection::City(city.id))
                    })
                    .unwrap_or(Selection::Cell(cell))
            })
        })
        .unwrap_or(Selection::None)
}

/// What a click at `pos` selects in `game`: [`pick`] over the game's own markers. The reference
/// path for tests: the map itself draws, and picks from, the markers in its model.
pub fn hit_test(layout: &MapLayout, pos: Pos2, game: &Game) -> Selection {
    pick(layout, pos, &MapObjects::of(game))
}

// ---- colours: pure ---------------------------------------------------------------------------

/// An sRGB colour, kept free of egui types so the shading is testable.
pub type Rgb = [u8; 3];

/// The opaque egui colour of `rgb`, for the draw functions: the one place an [`Rgb`] becomes egui's.
pub(super) fn color32([r, g, b]: Rgb) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Open water, on every layer that shades land only.
pub const WATER: Rgb = [28, 52, 96];
/// A cell the plant can be built on, on the suitability layer.
pub const SUITABLE: Rgb = [70, 170, 90];
/// A land cell the plant cannot be built on.
pub const UNSUITABLE: Rgb = [120, 55, 55];
const LOWLAND: Rgb = [64, 128, 70];
const HIGHLAND: Rgb = [150, 130, 100];
const DARK: Rgb = [38, 40, 46];
const COOL: Rgb = [70, 170, 255];
const HYDRO: Rgb = [60, 220, 190];

fn lerp(from: Rgb, to: Rgb, t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as u8;
    [
        mix(from[0], to[0]),
        mix(from[1], to[1]),
        mix(from[2], to[2]),
    ]
}

pub(crate) fn solar_color(value: f64) -> Rgb {
    // A fixed 0–1 sequential scale shared by both regions, never stretched to a region's
    // local minimum/maximum. Viridis anchors distinguish cloudy blue/teal from sunny yellow
    // while retaining increasing luminance, including for red/green colour blindness.
    const STOPS: [Rgb; 5] = [
        [68, 1, 84],
        [59, 82, 139],
        [33, 145, 140],
        [94, 201, 98],
        [253, 231, 37],
    ];
    let position = value.clamp(0.0, 1.0) * (STOPS.len() - 1) as f64;
    let index = (position.floor() as usize).min(STOPS.len() - 2);
    lerp(STOPS[index], STOPS[index + 1], position - index as f64)
}

/// The colour of `at` on `layer`. A cell off the map is black.
pub fn cell_color(layer: MapLayer, region: &RegionDefinition, at: CellCoord) -> Rgb {
    let Some(cell) = region.terrain().get(at) else {
        return [0, 0, 0];
    };
    let land_shaded = |value: f64, bright: Rgb| {
        if cell.land {
            lerp(DARK, bright, value)
        } else {
            WATER
        }
    };
    match layer {
        MapLayer::Terrain if cell.land => lerp(LOWLAND, HIGHLAND, cell.height.value()),
        MapLayer::Terrain => WATER,
        MapLayer::Irradiance if cell.land => solar_color(region.solar_potential(at).value()),
        MapLayer::Irradiance => WATER,
        MapLayer::WindPotential if cell.land => solar_color(cell.wind_potential.value()),
        MapLayer::WindPotential => WATER,
        MapLayer::CoolingWater => lerp(DARK, COOL, cell.cooling_water.value()),
        MapLayer::HydroPotential => land_shaded(cell.hydro_potential.value(), HYDRO),
        MapLayer::Suitability(_) if !cell.land => WATER,
        MapLayer::Suitability(kind) if region.site_check(kind, at).is_ok() => SUITABLE,
        MapLayer::Suitability(_) => UNSUITABLE,
    }
}

/// The colour a kind of plant is drawn in.
pub fn kind_color(kind: PlantKind) -> Rgb {
    match kind {
        PlantKind::Nuclear => [190, 120, 230],
        PlantKind::Coal => [110, 110, 120],
        PlantKind::Ccgt => [235, 150, 70],
        PlantKind::Peaker => [225, 90, 80],
        PlantKind::Solar => [245, 215, 70],
        PlantKind::Hydro => [80, 170, 235],
        PlantKind::Battery => [45, 225, 195],
        PlantKind::Wind => [120, 190, 245],
    }
}

/// The ink of outlines and letters, and the fills of the city marks. The legend
/// draws the same.
const INK: Color32 = Color32::from_rgb(20, 22, 28);
const CITY: Color32 = Color32::from_rgb(255, 236, 160);
/// A line in service, and one under construction (drawn dashed).
const IN_SERVICE_LINE: Color32 = Color32::from_rgb(255, 230, 120);
const DASHED_LINE: Color32 = Color32::from_rgb(200, 200, 210);
/// The legend's under-construction square: no kind in particular, faded as the map fades it.
const UNBUILT: Color32 = Color32::from_rgb(200, 200, 210);
/// The smallest letter worth drawing, in points. Under it the square is drawn plain: kind reads by
/// colour (see the legend), and a smaller letter would only be a smudge.
pub const GLYPH_MIN_FONT: f32 = 9.0;

/// The font size of the letter on a plant's square, or `None` if it would be under
/// [`GLYPH_MIN_FONT`]. A capital is about 0.7 em tall, so this fits inside the square.
pub fn glyph_font_size(layout: &MapLayout) -> Option<f32> {
    let size = plant_half_size(layout) * 1.5;
    (size >= GLYPH_MIN_FONT).then_some(size)
}

fn glyph_key(kind: PlantKind) -> &'static str {
    match kind {
        PlantKind::Nuclear => "plant-glyph-nuclear",
        PlantKind::Coal => "plant-glyph-coal",
        PlantKind::Ccgt => "plant-glyph-ccgt",
        PlantKind::Peaker => "plant-glyph-peaker",
        PlantKind::Solar => "plant-glyph-solar",
        PlantKind::Hydro => "plant-glyph-hydro",
        PlantKind::Battery => "plant-glyph-battery",
        PlantKind::Wind => "plant-glyph-wind",
    }
}

/// How faded a plant under construction is drawn, and its legend swatch.
const PLANT_FADE: f32 = 0.45;

/// What a months-left label is drawn beside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelAnchor {
    /// A plant under construction: right of its faded square.
    Plant(CellCoord),
    /// A line under construction: above its dashed line's midpoint.
    Line { from: CellCoord, to: CellCoord },
}

/// A project under construction's months left, as the map labels it: `3 mo` (spec §16.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthsLeftLabel {
    pub anchor: LabelAnchor,
    pub text: String,
}

/// A months-left label placed on the screen: its text drawn at `pos`, aligned by `align`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedLabel<'a> {
    pub pos: Pos2,
    pub align: egui::Align2,
    /// The font size: the plant letters' size, so at least [`GLYPH_MIN_FONT`].
    pub size: f32,
    pub text: &'a str,
}

/// Where the months-left labels go on `layout`. Like the plant letters, none is drawn under
/// [`GLYPH_MIN_FONT`]. They are paint only: a click goes to what is drawn under them ([`pick`]).
pub fn months_left_labels<'a>(
    layout: &MapLayout,
    labels: &'a [MonthsLeftLabel],
) -> Vec<PlacedLabel<'a>> {
    let Some(size) = glyph_font_size(layout) else {
        return Vec::new();
    };
    let gap = LABEL_GAP;
    labels
        .iter()
        .map(|label| {
            let (pos, align) = match label.anchor {
                LabelAnchor::Plant(cell) => (
                    layout.cell_center(cell) + vec2(plant_half_size(layout) + gap, 0.0),
                    egui::Align2::LEFT_CENTER,
                ),
                LabelAnchor::Line { from, to } => (
                    layout.cell_center(from).lerp(layout.cell_center(to), 0.5) - vec2(0.0, gap),
                    egui::Align2::CENTER_BOTTOM,
                ),
            };
            PlacedLabel {
                pos,
                align,
                size,
                text: label.text.as_str(),
            }
        })
        .collect()
}

/// The space between a months-left label and its mark, in points.
const LABEL_GAP: f32 = 2.0;

/// The months-left labels of `objects`: each plant's, then each line's, in id order.
fn months_left_of(objects: &MapObjects, l10n: &Localizer) -> Vec<MonthsLeftLabel> {
    let text = |months: u32| l10n.args("map-months-left", &[("months", months.into())]);
    let plants = objects.plants.iter().filter_map(|plant| {
        plant.months_left.map(|months| MonthsLeftLabel {
            anchor: LabelAnchor::Plant(plant.cell),
            text: text(months),
        })
    });
    let lines = objects.lines.iter().filter_map(|line| {
        line.months_left.map(|months| MonthsLeftLabel {
            anchor: LabelAnchor::Line {
                from: line.from,
                to: line.to,
            },
            text: text(months),
        })
    });
    plants.chain(lines).collect()
}

// ---- the terrain texture ---------------------------------------------------------------------

/// What the terrain texture depends on: the region's terrain and the layer shading it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainKey {
    pub region: RegionId,
    pub layer: MapLayer,
}

impl TerrainKey {
    pub fn of(region: &RegionDefinition, layer: MapLayer) -> Self {
        Self {
            region: region.id().clone(),
            layer,
        }
    }
}

/// One pixel per cell, shaded by `layer`. Pure; the map draws it as a single textured quad.
pub fn terrain_image(layer: MapLayer, region: &RegionDefinition) -> egui::ColorImage {
    let terrain = region.terrain();
    let (width, height) = (terrain.width(), terrain.height());
    let mut rgb = Vec::with_capacity(usize::from(width) * usize::from(height) * 3);
    for y in 0..height {
        for x in 0..width {
            rgb.extend_from_slice(&cell_color(layer, region, CellCoord { x, y }));
        }
    }
    egui::ColorImage::from_rgb([usize::from(width), usize::from(height)], &rgb)
}

/// The uploaded terrain texture, rebuilt only when its [`TerrainKey`] changes. A resource, cleared
/// when a game starts or ends (the key names a region, not a file's contents).
#[derive(Resource, Default)]
pub struct MapCache {
    cached: Option<(TerrainKey, egui::TextureHandle)>,
    rebuilds: u32,
}

impl MapCache {
    /// The texture for `region` shaded by `layer`: the cached one unless the key changed.
    pub fn texture(
        &mut self,
        ctx: &egui::Context,
        region: &RegionDefinition,
        layer: MapLayer,
    ) -> egui::TextureId {
        let key = TerrainKey::of(region, layer);
        if let Some((cached, handle)) = &self.cached
            && *cached == key
        {
            return handle.id();
        }
        // Nearest filtering keeps each cell a crisp block, however far the map is scaled.
        let handle = ctx.load_texture(
            "terrain",
            terrain_image(layer, region),
            egui::TextureOptions::NEAREST,
        );
        let id = handle.id();
        self.cached = Some((key, handle));
        self.rebuilds += 1;
        id
    }

    /// How many times the texture has been built: tests watch it to see the cache hold.
    pub fn rebuilds(&self) -> u32 {
        self.rebuilds
    }

    pub(super) fn clear(&mut self) {
        self.cached = None;
    }
}

pub(super) fn reset_cache(mut cache: ResMut<MapCache>) {
    cache.clear();
}

// ---- the legend --------------------------------------------------------------------------------

/// What a legend entry's swatch looks like on the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegendSwatch {
    Plant(PlantKind),
    ConnectedCity,
    UnconnectedCity,
    /// A faded plant square.
    PlantUnderConstruction,
    /// A dashed line.
    LineUnderConstruction,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LegendEntry {
    pub swatch: LegendSwatch,
    pub label: String,
}

/// Every kind of plant, then the city marks and the under-construction marks.
fn legend_entries(l10n: &Localizer) -> Vec<LegendEntry> {
    let entry = |swatch, label: String| LegendEntry { swatch, label };
    PlantKind::ALL
        .into_iter()
        .map(|kind| entry(LegendSwatch::Plant(kind), kind.localize(l10n)))
        .chain([
            entry(
                LegendSwatch::ConnectedCity,
                l10n.text("legend-city-connected"),
            ),
            entry(
                LegendSwatch::UnconnectedCity,
                l10n.text("legend-city-unconnected"),
            ),
            entry(
                LegendSwatch::PlantUnderConstruction,
                l10n.text("legend-plant-under-construction"),
            ),
            entry(
                LegendSwatch::LineUnderConstruction,
                l10n.text("legend-line-under-construction"),
            ),
        ])
        .collect()
}

/// The corners of the map the legend tries, in order of preference.
pub const LEGEND_CORNERS: [egui::Align2; 4] = [
    egui::Align2::RIGHT_TOP,
    egui::Align2::RIGHT_BOTTOM,
    egui::Align2::LEFT_BOTTOM,
    egui::Align2::LEFT_TOP,
];

/// How far the legend sits in from the map's edges, in points.
pub const LEGEND_INSET: f32 = 8.0;

/// The rectangle a legend of `size` covers in `corner` of `map`, [`LEGEND_INSET`] in from its
/// edges.
pub fn legend_rect(map: Rect, size: egui::Vec2, corner: egui::Align2) -> Rect {
    let anchor = corner.pos_in_rect(&map.shrink(LEGEND_INSET));
    corner.anchor_size(anchor, size)
}

/// The first corner, in [`LEGEND_CORNERS`] order, where a legend of `size` fits inside `map` and
/// covers none of `marks`; `None` when every corner would hide one.
pub fn legend_placement(map: Rect, size: egui::Vec2, marks: &[Pos2]) -> Option<egui::Align2> {
    LEGEND_CORNERS.into_iter().find(|corner| {
        let rect = legend_rect(map, size, *corner);
        map.contains_rect(rect) && !marks.iter().any(|mark| rect.contains(*mark))
    })
}

/// What a click on the map can pick, at its centre: every city and every plant, built or not.
pub fn pickable_marks(layout: &MapLayout, objects: &MapObjects) -> Vec<Pos2> {
    let cities = objects.cities.iter().map(|city| city.cell);
    let plants = objects.plants.iter().map(|plant| plant.cell);
    cities
        .chain(plants)
        .map(|cell| layout.cell_center(cell))
        .collect()
}

/// Where the legend shows, if at all, given the corner that hides nothing (`clear`). Until the
/// player chooses, it shows only there, so it never hides a city or plant (a click on it selects
/// nothing); with no clear corner it is collapsed. Once they choose, their choice wins: opened, it
/// goes in the clear corner if there is one, else the top right.
pub fn legend_corner(choice: LegendChoice, clear: Option<egui::Align2>) -> Option<egui::Align2> {
    match choice.0 {
        None => clear,
        Some(true) => Some(clear.unwrap_or(egui::Align2::RIGHT_TOP)),
        Some(false) => None,
    }
}

/// The legend's toggle in the layer-button row: on while the legend shows, and a click asks for
/// the other state.
pub fn legend_toggle(label: &str, open: bool) -> ToggleModel {
    ToggleModel {
        button: ButtonModel::new(label.to_owned(), UiAction::ShowLegend(!open)),
        on: open,
    }
}

/// The most columns the legend is laid out in.
pub const LEGEND_COLUMNS: usize = 3;

/// How many columns, at most [`LEGEND_COLUMNS`], the legend's entries fit in within `max_width`:
/// `widths` are the entries' widths in order, laid out row by row, with `gap` between columns.
/// One column if even that is too wide.
pub fn legend_columns(widths: &[f32], gap: f32, max_width: f32) -> usize {
    let grid_width = |columns: usize| {
        let widest = |column: usize| {
            widths
                .iter()
                .skip(column)
                .step_by(columns)
                .fold(0.0_f32, |widest, width| widest.max(*width))
        };
        (0..columns).map(widest).sum::<f32>() + gap * (columns - 1) as f32
    };
    (2..=LEGEND_COLUMNS.min(widths.len()))
        .rev()
        .find(|columns| grid_width(*columns) <= max_width)
        .unwrap_or(1)
}

// ---- model, presenter, draw, system ---------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceScaleModel {
    pub label: String,
    pub ticks: [String; 3],
    pub help: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapModel {
    /// The map's accessible name.
    pub label: String,
    /// One button per [`MapLayer`]; the current layer's is on. Clicking it again changes nothing,
    /// so it stays clickable rather than disabled.
    pub layers: Vec<ToggleModel>,
    pub layer: MapLayer,
    pub resource_scale: Option<ResourceScaleModel>,
    /// The terrain is read from the region while painting: it is too big to copy every frame.
    pub region: Arc<RegionDefinition>,
    pub objects: MapObjects,
    /// The letter each kind of plant is drawn with, when the map is big enough to read it.
    pub glyphs: Vec<(PlantKind, String)>,
    /// The months left of each plant and line under construction, labelled beside it.
    pub months_left: Vec<MonthsLeftLabel>,
    pub line_names: Vec<MonthsLeftLabel>,
    /// What each kind of mark on the map is, in a corner of the map when it shows.
    pub legend: Vec<LegendEntry>,
    /// The legend's accessible name.
    pub legend_label: String,
    /// Whether the player chose to show the legend; the draw function places it by the marks on
    /// the drawn map ([`legend_corner`]).
    pub legend_choice: LegendChoice,
    /// The label of the legend's toggle in the layer-button row.
    pub legend_toggle: String,
    pub selection: Selection,
}

impl MapModel {
    /// The glyph of `kind`.
    pub fn glyph(&self, kind: PlantKind) -> &str {
        self.glyphs
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or("", |(_, glyph)| glyph.as_str())
    }
}

fn layer_label(layer: MapLayer, l10n: &Localizer) -> String {
    match layer {
        MapLayer::Terrain => l10n.text("map-layer-terrain"),
        MapLayer::Irradiance => l10n.text("map-layer-irradiance"),
        MapLayer::WindPotential => l10n.text("map-layer-wind"),
        MapLayer::CoolingWater => l10n.text("map-layer-cooling-water"),
        MapLayer::HydroPotential => l10n.text("map-layer-hydro-potential"),
        MapLayer::Suitability(kind) => l10n.args(
            "map-layer-suitability",
            &[("kind", kind.localize(l10n).into())],
        ),
    }
}

pub fn map_model(
    game: &Game,
    l10n: &Localizer,
    layer: MapLayer,
    legend: LegendChoice,
    selection: Selection,
) -> MapModel {
    let objects = MapObjects::of(game);
    let names = crate::l10n::Names::new(l10n, game.session());
    let line_views = game.session().line_views();
    MapModel {
        label: l10n.text("map-label"),
        layers: MapLayer::all()
            .map(|choice| ToggleModel {
                button: ButtonModel::new(layer_label(choice, l10n), UiAction::SetLayer(choice)),
                on: choice == layer,
            })
            .collect(),
        layer,
        resource_scale: matches!(layer, MapLayer::Irradiance | MapLayer::WindPotential).then(
            || ResourceScaleModel {
                label: l10n.text(if layer == MapLayer::WindPotential {
                    "wind-potential"
                } else {
                    "map-solar-scale"
                }),
                ticks: [
                    l10n.text("map-solar-low"),
                    l10n.text("map-solar-mid"),
                    l10n.text("map-solar-high"),
                ],
                help: l10n.text(if layer == MapLayer::WindPotential {
                    "wind-resource-help"
                } else {
                    "map-solar-help"
                }),
            },
        ),
        region: Arc::clone(game.session().region()),
        months_left: months_left_of(&objects, l10n),
        line_names: objects
            .lines
            .iter()
            .filter_map(|line| {
                let view = line_views.iter().find(|view| view.id == line.id)?;
                crate::l10n::starter_line_name(game.session(), view.from, view.to)?;
                Some(MonthsLeftLabel {
                    anchor: LabelAnchor::Line {
                        from: line.from,
                        to: line.to,
                    },
                    text: names.line(line.id),
                })
            })
            .collect(),
        objects,
        glyphs: PlantKind::ALL
            .into_iter()
            .map(|kind| (kind, l10n.text(glyph_key(kind))))
            .collect(),
        legend: legend_entries(l10n),
        legend_label: l10n.text("legend-label"),
        legend_choice: legend,
        legend_toggle: l10n.text("legend-toggle"),
        selection,
    }
}

pub fn draw_map(ui: &mut egui::Ui, model: &MapModel, cache: &mut MapCache) -> Vec<UiAction> {
    let mut actions = Vec::new();
    // The legend's toggle ends the row, but whether it is on depends on the map's size, known
    // only once the row is laid out: its place is kept here and it is drawn into it below.
    let mut toggle_slot = Rect::NOTHING;
    ui.horizontal_wrapped(|ui| {
        for layer in &model.layers {
            toggle_button(ui, layer, &mut actions);
        }
        let size = button_size(ui, &model.legend_toggle);
        toggle_slot = ui.allocate_space(size).1;
    });
    if let Some(scale) = &model.resource_scale {
        draw_resource_scale(ui, scale);
    }
    let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, model.label.as_str())
    });
    let terrain = model.region.terrain();
    let layout = MapLayout::fit(response.rect, terrain.width(), terrain.height());
    let texture = cache.texture(ui.ctx(), &model.region, model.layer);
    painter.image(
        texture,
        layout.bounds(),
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    paint_objects(&painter, &layout, model);
    let legend_layout = measure_legend(ui, response.rect, model);
    let marks = pickable_marks(&layout, &model.objects);
    let corner = legend_corner(
        model.legend_choice,
        legend_placement(response.rect, legend_layout.size, &marks),
    );
    let toggle = legend_toggle(&model.legend_toggle, corner.is_some());
    let clicked = ui.put(
        toggle_slot,
        egui::Button::new(toggle.button.label.as_str()).selected(toggle.on),
    );
    if clicked.clicked() {
        actions.push(toggle.button.action.clone());
    }
    // The legend floats over the map and is not part of it: a click on it picks nothing.
    let legend =
        corner.map(|corner| draw_legend(ui.ctx(), response.rect, corner, legend_layout, model));
    if let Some(pos) = response
        .interact_pointer_pos()
        .filter(|pos| response.clicked() && !legend.is_some_and(|legend| legend.contains(*pos)))
    {
        actions.push(UiAction::Select(pick(&layout, pos, &model.objects)));
    }
    actions
}

/// Outside the map viewport: the colour key cannot cover a plant or intercept a map click.
pub(crate) fn draw_resource_scale(ui: &mut egui::Ui, model: &ResourceScaleModel) {
    ui.horizontal(|ui| {
        ui.small(&model.label).on_hover_text(&model.help);
        let (rect, response) = ui.allocate_exact_size(
            vec2(ui.available_width().clamp(1.0, 240.0), 28.0),
            egui::Sense::hover(),
        );
        let bar = Rect::from_min_size(rect.min, vec2(rect.width(), 10.0));
        for i in 0..64 {
            let left = bar.left() + bar.width() * i as f32 / 64.0;
            let right = bar.left() + bar.width() * (i + 1) as f32 / 64.0;
            ui.painter().rect_filled(
                Rect::from_min_max(egui::pos2(left, bar.top()), egui::pos2(right, bar.bottom())),
                0.0,
                color32(solar_color(f64::from(i) / 63.0)),
            );
        }
        for (label, anchor, x) in [
            (&model.ticks[0], egui::Align2::LEFT_TOP, rect.left()),
            (&model.ticks[1], egui::Align2::CENTER_TOP, rect.center().x),
            (&model.ticks[2], egui::Align2::RIGHT_TOP, rect.right()),
        ] {
            ui.painter().text(
                egui::pos2(x, bar.bottom() + 2.0),
                anchor,
                label,
                egui::TextStyle::Small.resolve(ui.style()),
                ui.visuals().text_color(),
            );
        }
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &model.help));
        response.on_hover_text(&model.help);
    });
}

/// The size of a button labelled `label`: its text and the button padding, at least the height of
/// an interactive widget.
fn button_size(ui: &egui::Ui, label: &str) -> egui::Vec2 {
    let text = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::TextStyle::Button.resolve(ui.style()),
        Color32::PLACEHOLDER,
    );
    let size = text.size() + 2.0 * ui.spacing().button_padding;
    vec2(size.x, size.y.max(ui.spacing().interact_size.y))
}

/// The space between the legend's columns and between its rows, in points.
const LEGEND_SPACING: egui::Vec2 = vec2(12.0, 2.0);
/// A legend swatch's side, in points.
const SWATCH: f32 = 12.0;

/// The legend's shape on a map: its columns and its outer size, frame included.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LegendLayout {
    columns: usize,
    size: egui::Vec2,
}

/// Measures the legend before it is drawn, so it can be placed: as many columns, up to three, as
/// fit inside `map`, and the size those make. Each entry is a swatch and a small label in a row
/// at least an interactive widget tall, as the grid lays them out.
fn measure_legend(ui: &egui::Ui, map: Rect, model: &MapModel) -> LegendLayout {
    let style = ui.style();
    let frame = egui::Frame::popup(style);
    let font = egui::TextStyle::Small.resolve(style);
    let gap = ui.spacing().item_spacing.x;
    let mut text_height = 0.0_f32;
    let widths: Vec<f32> = model
        .legend
        .iter()
        .map(|entry| {
            let text = ui.painter().layout_no_wrap(
                entry.label.clone(),
                font.clone(),
                Color32::PLACEHOLDER,
            );
            text_height = text_height.max(text.size().y);
            SWATCH + gap + text.size().x
        })
        .collect();
    let margin = frame.total_margin().sum();
    let room = map.width() - 2.0 * LEGEND_INSET - margin.x;
    let columns = legend_columns(&widths, LEGEND_SPACING.x, room);
    let column_widths: f32 = (0..columns)
        .map(|column| {
            widths
                .iter()
                .skip(column)
                .step_by(columns)
                .fold(0.0_f32, |widest, width| widest.max(*width))
        })
        .sum();
    let rows = widths.len().div_ceil(columns);
    let row_height = SWATCH.max(text_height).max(ui.spacing().interact_size.y);
    let gaps = |n: usize| n.saturating_sub(1) as f32;
    let size = vec2(
        column_widths + LEGEND_SPACING.x * gaps(columns),
        row_height * rows as f32 + LEGEND_SPACING.y * gaps(rows),
    ) + margin;
    LegendLayout { columns, size }
}

/// The legend, in `corner` of `map`: an overlay that takes no clicks itself, laid out as
/// [`measure_legend`] measured it. Returns the rectangle it covers, which the map's own clicks
/// must leave out.
fn draw_legend(
    ctx: &egui::Context,
    map: Rect,
    corner: egui::Align2,
    layout: LegendLayout,
    model: &MapModel,
) -> Rect {
    let shown = egui::Area::new(egui::Id::new("map_legend"))
        .order(egui::Order::Middle)
        .pivot(corner)
        .fixed_pos(corner.pos_in_rect(&map.shrink(LEGEND_INSET)))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                egui::Grid::new("map_legend_entries")
                    .spacing(LEGEND_SPACING)
                    .show(ui, |ui| {
                        for (i, entry) in model.legend.iter().enumerate() {
                            ui.horizontal(|ui| {
                                paint_swatch(ui, entry.swatch);
                                ui.small(entry.label.as_str());
                            });
                            if (i + 1) % layout.columns == 0 {
                                ui.end_row();
                            }
                        }
                    });
            });
        });
    shown.response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, model.legend_label.as_str())
    });
    shown.response.rect
}

/// A [`SWATCH`]-sized swatch drawn the way the mark is on the map.
fn paint_swatch(ui: &mut egui::Ui, swatch: LegendSwatch) {
    let (rect, _) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), egui::Sense::hover());
    let painter = ui.painter();
    let centre = rect.center();
    match swatch {
        LegendSwatch::Plant(kind) => {
            painter.rect(
                rect.shrink(1.0),
                egui::CornerRadius::same(2),
                color32(kind_color(kind)),
                Stroke::new(1.0, INK),
                StrokeKind::Inside,
            );
        }
        LegendSwatch::ConnectedCity => {
            painter.circle(centre, 5.0, CITY, Stroke::new(1.5, INK));
        }
        LegendSwatch::UnconnectedCity => {
            painter.circle(centre, 5.0, Color32::TRANSPARENT, Stroke::new(1.5, CITY));
        }
        LegendSwatch::PlantUnderConstruction => {
            painter.rect(
                rect.shrink(1.0),
                egui::CornerRadius::same(2),
                UNBUILT.gamma_multiply(PLANT_FADE),
                Stroke::new(1.0, INK.gamma_multiply(PLANT_FADE)),
                StrokeKind::Inside,
            );
        }
        LegendSwatch::LineUnderConstruction => {
            painter.extend(egui::Shape::dashed_line(
                &[rect.left_center(), rect.right_center()],
                Stroke::new(1.5, DASHED_LINE),
                3.0,
                2.0,
            ));
        }
    }
}

/// Lines, then cities and plants on top, in the order a click prefers them, then
/// the months-left labels.
fn paint_objects(painter: &egui::Painter, layout: &MapLayout, model: &MapModel) {
    let objects = &model.objects;
    let half = plant_half_size(layout);
    let at = |cell| layout.cell_center(cell);
    for line in &objects.lines {
        let breaks = crate::world::line_breaks(line.from, line.to);
        let origin = vec2(f32::from(line.from.x) + 0.5, f32::from(line.from.y) + 0.5);
        let target = vec2(f32::from(line.to.x) + 0.5, f32::from(line.to.y) + 0.5);
        let mut spans: Vec<(f32, f32, bool)> = Vec::new();
        for interval in breaks.windows(2) {
            let midpoint = origin + (target - origin) * ((interval[0] + interval[1]) * 0.5);
            let water = model
                .region
                .terrain()
                .get(CellCoord {
                    x: midpoint.x.floor() as u16,
                    y: midpoint.y.floor() as u16,
                })
                .is_some_and(|cell| !cell.land);
            if !water && let Some((_, end, false)) = spans.last_mut() {
                *end = interval[1];
            } else {
                spans.push((interval[0], interval[1], water));
            }
        }
        for (start, end, water) in spans {
            let ends = [
                at(line.from).lerp(at(line.to), start),
                at(line.from).lerp(at(line.to), end),
            ];
            if water {
                painter.line_segment(
                    [ends[0], ends[0].lerp(ends[1], 0.5)],
                    Stroke::new(2.0, Color32::from_rgb(75, 100, 125)),
                );
            } else if line.in_service() {
                painter.line_segment(ends, Stroke::new(2.0, IN_SERVICE_LINE));
            } else {
                painter.extend(egui::Shape::dashed_line(
                    &ends,
                    Stroke::new(1.5, DASHED_LINE),
                    5.0,
                    4.0,
                ));
            }
        }
    }
    let ink = INK;
    for city in &objects.cities {
        let (fill, outline) = if city.connected {
            (CITY, ink)
        } else {
            (Color32::TRANSPARENT, CITY)
        };
        painter.circle(
            at(city.cell),
            city_radius(layout),
            fill,
            Stroke::new(2.0, outline),
        );
    }
    for plant in &objects.plants {
        let fill = color32(kind_color(plant.kind));
        let fade = if plant.built() { 1.0 } else { PLANT_FADE };
        let centre = at(plant.cell);
        painter.rect(
            Rect::from_center_size(centre, vec2(half * 2.0, half * 2.0)),
            egui::CornerRadius::same(3),
            fill.gamma_multiply(fade),
            Stroke::new(1.5, ink.gamma_multiply(fade)),
            StrokeKind::Inside,
        );
        if let Some(size) = glyph_font_size(layout) {
            super::theme::paint_label(
                painter,
                centre,
                egui::Align2::CENTER_CENTER,
                model.glyph(plant.kind),
                size,
            );
        }
    }
    paint_months_left(painter, layout, model);
    paint_selection(painter, layout, model, half);
}

/// Each project's months left beside it, on a dark backing. Paint only, so a click passes
/// through to the mark or cell under it.
fn paint_months_left(painter: &egui::Painter, layout: &MapLayout, model: &MapModel) {
    for label in months_left_labels(layout, &model.months_left)
        .into_iter()
        .chain(months_left_labels(layout, &model.line_names))
    {
        super::theme::paint_label(painter, label.pos, label.align, label.text, label.size);
    }
}

/// A white ring (or outline, for a cell) around what is selected.
fn paint_selection(painter: &egui::Painter, layout: &MapLayout, model: &MapModel, half: f32) {
    let ring = Stroke::new(2.0, Color32::WHITE);
    let objects = &model.objects;
    let around = |cell| painter.circle_stroke(layout.cell_center(cell), half * 1.5, ring);
    match model.selection {
        Selection::Line(id) => {
            if let Some(line) = objects.lines.iter().find(|line| line.id == id) {
                painter.line_segment(
                    [layout.cell_center(line.from), layout.cell_center(line.to)],
                    ring,
                );
            }
        }
        Selection::None => {}
        Selection::Cell(at) => {
            painter.rect_stroke(
                layout.cell_rect(at),
                egui::CornerRadius::ZERO,
                ring,
                StrokeKind::Outside,
            );
        }
        Selection::Plant(id) => {
            if let Some(plant) = objects.plants.iter().find(|p| p.id == id) {
                around(plant.cell);
            }
        }
        Selection::City(id) => {
            if let Some(city) = objects.cities.iter().find(|c| c.id == id) {
                around(city.cell);
            }
        }
        Selection::LineFrom(node) => {
            if let Some(cell) = objects.nodes.get(&node) {
                around(*cell);
            }
        }
    }
}

pub(super) fn map_panel(
    mut contexts: EguiContexts,
    area: Res<HudArea>,
    session: Res<Session>,
    l10n: Res<Localizer>,
    mut cache: ResMut<MapCache>,
    mut surface: Option<ResMut<crate::world::MapSurface>>,
    mut sink: ActionSink,
) -> Result {
    let model = map_model(
        &session.0,
        &l10n,
        sink.layer(),
        sink.legend_choice(),
        sink.selection(),
    );
    let mut ui = area.root_ui(contexts.ctx_mut()?, "map");
    let actions = if let Some(surface) = surface.as_mut() {
        crate::world::mark_blocked(&session, &sink, surface);
        let toolbar = crate::world::toolbar_model(
            &l10n,
            sink.world().overlay,
            sink.world().effects,
            sink.world().hologram(),
            session.0.session().phase() == domain::session::DayPhase::Operating,
        );
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(&mut ui, |ui| {
                crate::world::draw_surface(ui, &model, &toolbar, surface)
            })
            .inner
    } else {
        // Pure 2D reference adapter for HUD-only/headless consumers. The windowed game always
        // installs WorldViewPlugin and takes the transparent diorama branch above.
        egui::CentralPanel::default()
            .show(&mut ui, |ui| draw_map(ui, &model, &mut cache))
            .inner
    };
    dispatch(actions, &mut sink);
    Ok(())
}

/// Compact, explicitly opened 3D legend. Perspective changes make a permanently clear corner impossible.
pub(crate) fn draw_world_legend(ctx: &egui::Context, rect: Rect, model: &MapModel) -> Rect {
    let measure = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("world_legend_measure"),
        egui::UiBuilder::new().max_rect(rect),
    );
    let layout = measure_legend(&measure, rect, model);
    let frame = egui::Frame::popup(measure.style());
    let margin = frame.total_margin().sum();
    let height = (rect.height() - 2.0 * LEGEND_INSET - margin.y).max(1.0);
    let width = (rect.width() - 2.0 * LEGEND_INSET - margin.x).max(1.0);
    let shown = egui::Area::new(egui::Id::new("map_legend"))
        .order(egui::Order::Middle)
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos(rect.shrink(LEGEND_INSET).right_top())
        .show(ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_max_width(width);
                egui::ScrollArea::vertical()
                    .max_height(height)
                    .min_scrolled_height(0.0)
                    .show(ui, |ui| {
                        egui::Grid::new("world_legend_entries")
                            .spacing(LEGEND_SPACING)
                            .show(ui, |ui| {
                                for (i, entry) in model.legend.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        paint_swatch(ui, entry.swatch);
                                        ui.small(&entry.label);
                                    });
                                    if (i + 1) % layout.columns == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    });
            });
        });
    shown.response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &model.legend_label)
    });
    shown.response.rect
}

#[cfg(test)]
mod tests {
    use application::InMemoryCommandLog;
    use domain::session::LineView;
    use egui::{pos2, vec2};
    use proptest::prelude::*;

    use super::*;
    use crate::hud::test_support::{
        eastern_game, order_plant, order_short_line, shipped_game, shipped_l10n,
    };

    fn area(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(w, h))
    }

    #[test]
    fn an_rgb_becomes_the_same_opaque_egui_colour() {
        for rgb in [WATER, SUITABLE, [0, 0, 0], [255, 255, 255], [1, 2, 3]] {
            let colour = color32(rgb);
            assert_eq!(colour.to_array(), [rgb[0], rgb[1], rgb[2], 255], "{rgb:?}");
        }
    }

    #[test]
    fn reference_map_selects_a_line_order_and_keeps_its_endpoints_selectable() {
        let layout = MapLayout::fit(area(0.0, 0.0, 1000.0, 600.0), 100, 60);
        let from = CellCoord { x: 10, y: 10 };
        let to = CellCoord { x: 30, y: 10 };
        let line = LineId(7);
        let mut objects = objects_of(
            Vec::new(),
            vec![CityMarker {
                id: CityId(1),
                cell: from,
                connected: true,
            }],
        );
        objects.lines.push(LineMarker {
            id: line,
            from,
            to,
            months_left: Some(3),
        });
        assert_eq!(
            pick(
                &layout,
                layout.cell_center(CellCoord { x: 20, y: 10 }),
                &objects
            ),
            Selection::Line(line)
        );
        assert_eq!(
            pick(&layout, layout.cell_center(from), &objects),
            Selection::City(CityId(1))
        );
        let away = CellCoord { x: 20, y: 11 };
        assert_eq!(
            pick(&layout, layout.cell_center(away), &objects),
            Selection::Cell(away)
        );
    }

    proptest! {
        #[test]
        fn map_layout_keeps_aspect_and_fits(
            cols in 1u16..=200,
            rows in 1u16..=200,
            x in -500.0f32..500.0,
            y in -500.0f32..500.0,
            width in 50.0f32..4000.0,
            height in 50.0f32..4000.0,
            pick_x in 0u16..200,
            pick_y in 0u16..200,
        ) {
            // Any grid shape (portrait too) in any panel.
            let panel = area(x, y, width, height);
            let layout = MapLayout::fit(panel, cols, rows);
            let bounds = layout.bounds();

            prop_assert!(panel.expand(0.01).contains_rect(bounds), "{bounds:?} in {panel:?}");
            let shape = f32::from(cols) / f32::from(rows);
            prop_assert!(
                ((bounds.width() / bounds.height()) / shape - 1.0).abs() < 1e-3,
                "aspect {bounds:?} for {cols} x {rows}"
            );
            // It is the biggest fit: it fills the panel's width or its height.
            let slack = (panel.width() - bounds.width()).min(panel.height() - bounds.height());
            prop_assert!(slack.abs() < 0.01, "{bounds:?} fills {panel:?}");
            prop_assert!((bounds.center() - panel.center()).length() < 0.01);
            prop_assert!((layout.cell_size() * f32::from(cols) - bounds.width()).abs() < 0.01);

            // Every cell round-trips through its centre, and corners are on the grid.
            let at = CellCoord { x: pick_x % cols, y: pick_y % rows };
            prop_assert_eq!(layout.cell_at(layout.cell_center(at)), Some(at));
            for corner in [
                CellCoord { x: 0, y: 0 },
                CellCoord { x: cols - 1, y: rows - 1 },
            ] {
                prop_assert_eq!(layout.cell_at(layout.cell_center(corner)), Some(corner));
            }
            prop_assert_eq!(layout.cell_at(bounds.min - vec2(1.0, 0.0)), None);
            prop_assert_eq!(layout.cell_at(bounds.max + vec2(0.0, 1.0)), None);
        }
    }

    #[test]
    fn an_empty_area_or_grid_hits_nothing() {
        for layout in [
            MapLayout::fit(area(0.0, 0.0, 0.0, 0.0), 100, 60),
            MapLayout::fit(area(0.0, 0.0, 500.0, 300.0), 0, 60),
            MapLayout::fit(area(0.0, 0.0, 500.0, 300.0), 100, 0),
        ] {
            assert_eq!(layout.cell_size(), 0.0);
            assert_eq!(layout.cell_at(pos2(0.0, 0.0)), None);
            assert_eq!(layout.cell_at(layout.bounds().center()), None);
        }
    }

    fn cell(x: u16, y: u16) -> CellCoord {
        CellCoord { x, y }
    }

    #[test]
    fn hit_test_prefers_plants_over_cities_over_cells() {
        let layout = MapLayout::fit(area(0.0, 0.0, 1000.0, 600.0), 100, 60);
        let stacked = cell(20, 20);
        let mut objects = MapObjects {
            plants: vec![PlantMarker {
                id: PlantId(7),
                kind: PlantKind::Coal,
                cell: stacked,
                months_left: None,
            }],
            cities: vec![CityMarker {
                id: CityId(3),
                cell: stacked,
                connected: false,
            }],
            lines: Vec::new(),
            nodes: BTreeMap::new(),
        };
        let at = layout.cell_center(stacked);

        assert_eq!(pick(&layout, at, &objects), Selection::Plant(PlantId(7)));
        objects.plants.clear();
        assert_eq!(pick(&layout, at, &objects), Selection::City(CityId(3)));
        objects.cities.clear();
        assert_eq!(pick(&layout, at, &objects), Selection::Cell(stacked));

        // Off the grid and away from every marker selects nothing.
        assert_eq!(pick(&layout, pos2(-30.0, -30.0), &objects), Selection::None);
        assert_eq!(
            pick(&layout, pos2(1500.0, 900.0), &objects),
            Selection::None
        );
    }

    fn plant_at(id: u32, at: CellCoord) -> PlantMarker {
        PlantMarker {
            id: PlantId(id),
            kind: PlantKind::Coal,
            cell: at,
            months_left: None,
        }
    }

    fn objects_of(plants: Vec<PlantMarker>, cities: Vec<CityMarker>) -> MapObjects {
        MapObjects {
            plants,
            cities,
            lines: Vec::new(),
            nodes: BTreeMap::new(),
        }
    }

    #[test]
    fn a_marker_and_the_ground_of_its_site_select_the_same_occupant() {
        let layout = MapLayout::fit(area(0.0, 0.0, 1000.0, 600.0), 100, 60);
        let plant = plant_at(1, cell(40, 20));
        let city = CityMarker {
            id: CityId(2),
            cell: cell(42, 20),
            connected: true,
        };
        let objects = objects_of(vec![plant], vec![city]);
        let (half, radius) = (plant_half_size(&layout), city_radius(&layout));
        assert!(
            half < layout.cell_size() / 2.0,
            "the plant fits inside its cell"
        );
        assert!(
            radius < layout.cell_size() / 2.0,
            "the city fits inside its cell"
        );

        // The plant's drawn square, corner to corner, is the plant.
        let centre = layout.cell_center(plant.cell);
        for corner in [
            vec2(half, half),
            vec2(-half, half),
            vec2(half, -half),
            vec2(-half, -half),
        ] {
            assert_eq!(
                pick(&layout, centre + corner * 0.99, &objects),
                Selection::Plant(plant.id)
            );
        }
        // Bare ground inside the plant's cell still belongs to its occupied site.
        for outside in [
            vec2(half + 0.2, 0.0),
            vec2(0.0, -half - 0.2),
            vec2(half + 0.2, half + 0.2),
        ] {
            assert_eq!(
                pick(&layout, centre + outside, &objects),
                Selection::Plant(plant.id),
                "{outside:?}"
            );
        }
        // Both the city model and the bare corners of its site select the city.
        let city_centre = layout.cell_center(city.cell);
        assert_eq!(
            pick(&layout, city_centre + vec2(radius * 0.99, 0.0), &objects),
            Selection::City(city.id)
        );
        let diagonal = vec2(radius * 0.7, radius * 0.7);
        assert_eq!(
            pick(&layout, city_centre + diagonal, &objects),
            Selection::City(city.id)
        );
        assert_eq!(
            pick(&layout, city_centre + diagonal * 1.2, &objects),
            Selection::City(city.id)
        );
        // The next cell over, between the two, is just a cell.
        let between = layout.cell_center(cell(41, 20));
        assert_eq!(
            pick(&layout, between, &objects),
            Selection::Cell(cell(41, 20))
        );
    }

    #[test]
    fn of_objects_drawn_on_top_of_each_other_the_last_drawn_is_picked() {
        let layout = MapLayout::fit(area(0.0, 0.0, 1000.0, 600.0), 100, 60);
        let at = cell(10, 10);
        let objects = objects_of(
            vec![plant_at(5, at), plant_at(6, at)],
            vec![
                CityMarker {
                    id: CityId(1),
                    cell: at,
                    connected: true,
                },
                CityMarker {
                    id: CityId(2),
                    cell: at,
                    connected: true,
                },
            ],
        );
        let centre = layout.cell_center(at);

        assert_eq!(
            pick(&layout, centre, &objects),
            Selection::Plant(PlantId(6))
        );
        let cities = objects_of(Vec::new(), objects.cities.clone());
        assert_eq!(pick(&layout, centre, &cities), Selection::City(CityId(2)));
    }

    /// Whether a plant's drawn square covers `pos`, worked out from the plant's centre and half
    /// side rather than through `pick`.
    fn covers(layout: &MapLayout, plant: &PlantMarker, pos: Pos2) -> bool {
        let half = plant_half_size(layout);
        let centre = layout.cell_center(plant.cell);
        (pos.x - centre.x).abs() <= half && (pos.y - centre.y).abs() <= half
    }

    #[test]
    fn hit_test_reads_the_game_s_plants_cities_and_cells() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let layout = MapLayout::fit(area(20.0, 10.0, 1000.0, 600.0), 100, 60);
        let objects = MapObjects::of(&game);

        // A plant is hit at its own centre (the topmost of any on the same cell).
        for plant in session.plant_views() {
            let hit = hit_test(&layout, layout.cell_center(plant.cell), &game);
            let Selection::Plant(found) = hit else {
                panic!(
                    "plant {:?} at {:?} is not hit: {hit:?}",
                    plant.id, plant.cell
                );
            };
            let found = objects.plants.iter().find(|p| p.id == found).unwrap();
            assert_eq!(found.cell, plant.cell, "the plant on that cell");
        }
        // Every city is hit at its own centre, unless a plant is drawn over that point.
        let mut selectable = 0;
        for city in session.city_views() {
            let centre = layout.cell_center(city.cell);
            let covered = objects.plants.iter().any(|p| covers(&layout, p, centre));
            let hit = hit_test(&layout, centre, &game);
            if covered {
                assert!(
                    matches!(hit, Selection::Plant(_)),
                    "{:?}: {hit:?}",
                    city.cell
                );
            } else {
                assert_eq!(hit, Selection::City(city.id), "{:?}", city.cell);
                selectable += 1;
            }
        }
        assert!(
            selectable + 2 >= session.city_views().len(),
            "at most a couple of cities share a cell with a plant: {selectable}"
        );
    }

    #[test]
    fn a_dense_cluster_leaves_its_cells_clickable() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let layout = MapLayout::fit(area(0.0, 0.0, 1000.0, 600.0), 100, 60);
        let mut objects = MapObjects::of(&game);
        // Isolate marker bounds: lines crossing these cells are separately selectable.
        objects.lines.clear();
        let occupied: Vec<CellCoord> = objects
            .plants
            .iter()
            .map(|p| p.cell)
            .chain(objects.cities.iter().map(|c| c.cell))
            .collect();

        // Next to every plant, any cell that holds no marker is that cell, not the plant.
        let mut checked = 0;
        for plant in &objects.plants {
            for (dx, dy) in [
                (-1i32, -1i32),
                (0, -1),
                (1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ] {
                let (nx, ny) = (i32::from(plant.cell.x) + dx, i32::from(plant.cell.y) + dy);
                if !(0..100).contains(&nx) || !(0..60).contains(&ny) {
                    continue;
                }
                let neighbour = CellCoord {
                    x: nx as u16,
                    y: ny as u16,
                };
                if occupied.contains(&neighbour) {
                    continue;
                }
                let at = layout.cell_center(neighbour);
                assert_eq!(
                    pick(&layout, at, &objects),
                    Selection::Cell(neighbour),
                    "next to the plant at {:?}",
                    plant.cell
                );
                checked += 1;
            }
        }
        assert!(
            checked >= objects.plants.len() * 4,
            "{checked} neighbouring cells checked around {} plants",
            objects.plants.len()
        );
        // The corner of a plant's own cell selects its site without extending into neighbours.
        let plant = &objects.plants[0];
        let corner = layout.cell_rect(plant.cell).max - vec2(0.1, 0.1);
        assert!(!covers(&layout, plant, corner));
        assert_eq!(pick(&layout, corner, &objects), Selection::Plant(plant.id));
    }

    #[test]
    fn map_objects_mirror_the_session() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();

        let objects = MapObjects::of(&game);

        assert_eq!(objects.plants.len(), session.plant_views().len());
        assert!(
            objects.plants.iter().all(PlantMarker::built),
            "the start fleet is built"
        );
        assert_eq!(objects.cities.len(), session.city_views().len());
        assert_eq!(
            objects.cities.iter().filter(|c| c.connected).count(),
            session.city_views().iter().filter(|c| c.connected).count()
        );
        assert_eq!(
            objects.nodes.len(),
            objects.cities.len() + objects.plants.len()
        );
        let lines: Vec<LineView> = session.line_views();
        assert_eq!(objects.lines.len(), lines.len());
        assert!(objects.lines.iter().all(LineMarker::in_service));
        assert_eq!(
            objects.nodes.get(&session.anchor_node()),
            Some(
                &session
                    .city_views()
                    .into_iter()
                    .find(|city| city.id == session.home_city())
                    .unwrap()
                    .cell
            )
        );
        // Every node a line ends on has a cell, so the line can be drawn.
        for line in lines {
            assert!(objects.nodes.contains_key(&line.from));
            assert!(objects.nodes.contains_key(&line.to));
        }
        assert!(matches!(
            session.line_views()[0].status,
            LineStatus::InService
        ));
    }

    #[test]
    fn suitability_layer_shades_cells_by_the_siting_rule() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let region = game.session().region();
        let terrain = region.terrain();
        let mut seen_suitable = [false; PlantKind::ALL.len()];
        let mut seen_unsuitable = [false; PlantKind::ALL.len()];

        for y in 0..terrain.height() {
            for x in 0..terrain.width() {
                let at = cell(x, y);
                let land = terrain.get(at).unwrap().land;
                for (i, kind) in PlantKind::ALL.into_iter().enumerate() {
                    let shade = cell_color(MapLayer::Suitability(kind), region, at);
                    match (land, region.site_check(kind, at).is_ok()) {
                        (false, _) => assert_eq!(shade, WATER),
                        (true, true) => {
                            assert_eq!(shade, SUITABLE);
                            seen_suitable[i] = true;
                        }
                        (true, false) => {
                            assert_eq!(shade, UNSUITABLE);
                            seen_unsuitable[i] = true;
                        }
                    }
                }
            }
        }
        // Cooling water and hydro rules really do rule cells out, and every kind fits somewhere.
        assert!(seen_suitable.iter().all(|seen| *seen), "{seen_suitable:?}");
        assert!(
            seen_unsuitable[0] && seen_unsuitable[5],
            "{seen_unsuitable:?}"
        );
    }

    #[test]
    fn sunshine_uses_distinct_hues_for_low_medium_and_high_resource() {
        let colour = solar_color;
        let low = colour(0.25);
        let medium = colour(0.5);
        let high = colour(0.8);
        assert!(low[2] > low[1] && low[2] > low[0], "low: {low:?}");
        assert!(
            medium[1] > medium[0] && medium[2] > medium[0],
            "medium: {medium:?}"
        );
        assert!(high[1] > high[0] && high[0] > high[2], "high: {high:?}");
    }

    #[test]
    fn sunshine_distinguishes_sunny_and_cloudier_sites_in_both_shipped_regions() {
        for (id, lower, higher) in [
            ("western_interconnection", "city-seattle", "city-phoenix"),
            ("eastern_interconnection", "city-cleveland", "city-miami"),
        ] {
            let game = shipped_game(id, Box::new(InMemoryCommandLog::default()));
            let region = game.session().region();
            let at = |name| {
                region
                    .cities()
                    .iter()
                    .find(|c| c.name_key == name)
                    .unwrap()
                    .cell
            };
            let low = at(lower);
            let high = at(higher);
            let low_value = region.terrain().get(low).unwrap().irradiance.value();
            let high_value = region.terrain().get(high).unwrap().irradiance.value();
            assert!(
                high_value > low_value + 0.2,
                "{id}: {low_value} → {high_value}"
            );
            let low_color = cell_color(MapLayer::Irradiance, region, low);
            let high_color = cell_color(MapLayer::Irradiance, region, high);
            assert!(low_color[2] > low_color[0], "{lower}: {low_color:?}");
            assert!(
                high_color[1] > high_color[2] + 30,
                "{higher}: {high_color:?}"
            );
            // Exact cell values and the key use one absolute scale, including across regions.
            assert_eq!(low_color, solar_color(low_value));
            assert_eq!(high_color, solar_color(high_value));
            for y in 0..region.terrain().height() {
                for x in 0..region.terrain().width() {
                    let at = cell(x, y);
                    if !region.terrain().get(at).unwrap().land {
                        assert_eq!(cell_color(MapLayer::Irradiance, region, at), WATER);
                    }
                }
            }
            let model = map_model(
                &game,
                &shipped_l10n(),
                MapLayer::Irradiance,
                LegendChoice::default(),
                Selection::None,
            );
            let scale = model.resource_scale.unwrap();
            assert_eq!(scale.label, "Solar potential");
            assert_eq!(scale.ticks, ["0%", "50%", "100%"]);
            assert!(scale.help.contains("not today's weather"));
        }
    }

    proptest! {
        #[test]
        fn sunshine_brightness_increases_across_the_absolute_scale(a in 0.0f64..=1.0, b in 0.0f64..=1.0) {
            let luminance = |value| {
                let [r, g, b] = solar_color(value).map(|c| {
                    let c = f64::from(c) / 255.0;
                    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
                });
                0.2126 * r + 0.7152 * g + 0.0722 * b
            };
            // Permit sub-byte quantisation differences, not a reversal of the sequence.
            prop_assert!(luminance(a.min(b)) <= luminance(a.max(b)) + 0.004);
        }
    }

    #[test]
    fn value_layers_brighten_with_their_value() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let region = game.session().region();
        let terrain = region.terrain();
        let brightness = |rgb: Rgb| rgb.iter().map(|c| u32::from(*c)).sum::<u32>();
        for (layer, value) in [
            (
                MapLayer::Irradiance,
                (|c: &domain::region::Cell| c.irradiance.value()) as fn(&_) -> f64,
            ),
            (MapLayer::CoolingWater, |c| c.cooling_water.value()),
            (MapLayer::HydroPotential, |c| c.hydro_potential.value()),
        ] {
            let land: Vec<(CellCoord, f64)> = (0..terrain.height())
                .flat_map(|y| (0..terrain.width()).map(move |x| cell(x, y)))
                .filter(|at| terrain.get(*at).unwrap().land)
                .map(|at| (at, value(terrain.get(at).unwrap())))
                .collect();
            let (low, _) = land
                .iter()
                .copied()
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap();
            let (high, _) = land
                .iter()
                .copied()
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap();
            assert!(
                brightness(cell_color(layer, region, high))
                    > brightness(cell_color(layer, region, low)),
                "{layer:?}: {high:?} should be brighter than {low:?}"
            );
        }
    }

    #[test]
    fn layer_buttons_pick_a_layer_and_the_current_one_is_selected() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        let model = map_model(
            &game,
            &l10n,
            MapLayer::Terrain,
            LegendChoice::default(),
            Selection::None,
        );

        let layers: Vec<MapLayer> = MapLayer::all().collect();
        assert_eq!(model.layers.len(), layers.len());
        assert_eq!(
            layers,
            [
                MapLayer::Terrain,
                MapLayer::Irradiance,
                MapLayer::WindPotential,
                MapLayer::CoolingWater,
                MapLayer::HydroPotential,
                MapLayer::Suitability(PlantKind::Nuclear),
                MapLayer::Suitability(PlantKind::Coal),
                MapLayer::Suitability(PlantKind::Solar),
                MapLayer::Suitability(PlantKind::Hydro),
            ]
        );
        for (toggle, layer) in model.layers.iter().zip(&layers) {
            assert_eq!(toggle.button.action, UiAction::SetLayer(*layer));
            // Clicking the current layer again is harmless, so every button stays clickable.
            assert!(toggle.button.enabled, "{}", toggle.button.label);
            assert_eq!(
                toggle.on,
                *layer == MapLayer::Terrain,
                "{}",
                toggle.button.label
            );
        }
        let labels: Vec<&str> = model
            .layers
            .iter()
            .map(|t| t.button.label.as_str())
            .collect();
        assert_eq!(labels[0], l10n.text("map-layer-terrain"));
        assert_eq!(labels[1], "Sunshine");
        assert_eq!(labels[2], "Wind potential");
        assert_eq!(labels[3], "Cooling water");
        assert_eq!(labels[4], "Hydro potential");
        assert_eq!(labels[5], "Suitability: Nuclear plant");
        assert_eq!(labels[6], "Suitability: Coal plant");
        assert_eq!(labels[7], "Suitability: Solar farm");
        assert_eq!(labels[8], "Suitability: Hydro plant");
        assert_eq!(model.layer, MapLayer::Terrain);
        assert_eq!(l10n.problems(), vec![]);

        // Choosing another layer moves the selection.
        let model = map_model(
            &game,
            &l10n,
            MapLayer::Suitability(PlantKind::Solar),
            LegendChoice::default(),
            Selection::None,
        );
        let selected: Vec<&str> = model
            .layers
            .iter()
            .filter(|t| t.on)
            .map(|t| t.button.label.as_str())
            .collect();
        assert_eq!(selected, ["Suitability: Solar farm"]);
    }

    #[test]
    fn plants_get_a_translated_glyph_each() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        let model = map_model(
            &game,
            &l10n,
            MapLayer::Terrain,
            LegendChoice::default(),
            Selection::None,
        );

        let mut glyphs: Vec<&str> = PlantKind::ALL.iter().map(|k| model.glyph(*k)).collect();
        assert!(glyphs.iter().all(|g| g.chars().count() == 1), "{glyphs:?}");
        glyphs.sort_unstable();
        glyphs.dedup();
        assert_eq!(
            glyphs.len(),
            PlantKind::ALL.len(),
            "each kind has its own glyph"
        );
        assert_eq!(model.label, l10n.text("map-label"));
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_terrain_texture_key_follows_the_region_and_the_layer() {
        let eastern = eastern_game(Box::new(InMemoryCommandLog::default()));
        let western = shipped_game(
            "western_interconnection",
            Box::new(InMemoryCommandLog::default()),
        );
        let (east, west) = (eastern.session().region(), western.session().region());
        let key = |region: &RegionDefinition, layer| TerrainKey::of(region, layer);

        assert_eq!(key(east, MapLayer::Terrain), key(east, MapLayer::Terrain));
        assert_ne!(
            key(east, MapLayer::Terrain),
            key(east, MapLayer::Irradiance)
        );
        assert_ne!(
            key(east, MapLayer::Suitability(PlantKind::Coal)),
            key(east, MapLayer::Suitability(PlantKind::Solar))
        );
        assert_ne!(key(east, MapLayer::Terrain), key(west, MapLayer::Terrain));
    }

    #[test]
    fn the_terrain_image_has_one_pixel_per_cell_in_the_layer_s_colours() {
        for region in ["eastern_interconnection", "western_interconnection"] {
            let game = shipped_game(region, Box::new(InMemoryCommandLog::default()));
            let region = game.session().region();
            let terrain = region.terrain();
            for layer in [MapLayer::Terrain, MapLayer::Suitability(PlantKind::Hydro)] {
                let image = terrain_image(layer, region);

                assert_eq!(
                    image.size,
                    [usize::from(terrain.width()), usize::from(terrain.height())]
                );
                assert_eq!(image.pixels.len(), image.size[0] * image.size[1]);
                for at in [
                    cell(0, 0),
                    cell(terrain.width() - 1, terrain.height() - 1),
                    cell(37, 12),
                ] {
                    let [r, g, b] = cell_color(layer, region, at);
                    assert_eq!(
                        image.pixels[usize::from(at.y) * image.size[0] + usize::from(at.x)],
                        Color32::from_rgb(r, g, b)
                    );
                }
            }
        }
    }

    #[test]
    fn the_cache_rebuilds_the_texture_only_when_the_key_changes() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let region = game.session().region();
        let ctx = egui::Context::default();
        let mut cache = MapCache::default();
        assert_eq!(cache.rebuilds(), 0);

        let first = cache.texture(&ctx, region, MapLayer::Terrain);
        assert_eq!(cache.texture(&ctx, region, MapLayer::Terrain), first);
        assert_eq!(cache.rebuilds(), 1, "the second frame reuses it");

        cache.texture(&ctx, region, MapLayer::Irradiance);
        assert_eq!(cache.rebuilds(), 2, "another layer is another texture");
        cache.texture(&ctx, region, MapLayer::Irradiance);
        assert_eq!(cache.rebuilds(), 2);

        cache.clear();
        cache.texture(&ctx, region, MapLayer::Irradiance);
        assert_eq!(cache.rebuilds(), 3, "a cleared cache starts over");
    }

    #[test]
    fn glyph_letters_are_drawn_only_when_legible() {
        let layout_at =
            |cell: f32| MapLayout::fit(area(0.0, 0.0, cell * 100.0, cell * 60.0), 100, 60);

        // A 1280 x 720 window leaves the eastern map about 720 pt wide: 7.2 pt cells. No letters.
        assert_eq!(glyph_font_size(&layout_at(7.2)), None);
        // A big map has them.
        assert!(glyph_font_size(&layout_at(20.0)).is_some());

        let mut legible = false;
        for step in 4..160 {
            let cell = step as f32 * 0.25;
            let layout = layout_at(cell);
            match glyph_font_size(&layout) {
                Some(size) => {
                    legible = true;
                    assert!(size >= GLYPH_MIN_FONT, "{size} pt at {cell} pt cells");
                    // A capital is about 0.7 em tall: it fits inside the square.
                    assert!(size * 0.7 <= plant_half_size(&layout) * 2.0);
                }
                None => assert!(!legible, "letters vanish again at {cell} pt cells"),
            }
        }
        assert!(legible);
    }

    /// Where a kind sits in `PlantKind::ALL`. An exhaustive match, so adding a kind breaks the
    /// build here until the legend test below accounts for it.
    fn position(kind: PlantKind) -> usize {
        match kind {
            PlantKind::Nuclear => 0,
            PlantKind::Coal => 1,
            PlantKind::Ccgt => 2,
            PlantKind::Peaker => 3,
            PlantKind::Solar => 4,
            PlantKind::Hydro => 5,
            PlantKind::Battery => 6,
            PlantKind::Wind => 7,
        }
    }

    #[test]
    fn the_legend_lists_every_plant_kind_then_the_other_marks() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        let model = map_model(
            &game,
            &l10n,
            MapLayer::Terrain,
            LegendChoice::default(),
            Selection::None,
        );

        let swatches: Vec<LegendSwatch> = model.legend.iter().map(|e| e.swatch).collect();
        for kind in PlantKind::ALL {
            assert_eq!(
                swatches[position(kind)],
                LegendSwatch::Plant(kind),
                "{kind:?} in ALL order"
            );
            assert_eq!(model.legend[position(kind)].label, kind.localize(&l10n));
        }
        assert_eq!(
            swatches[PlantKind::ALL.len()..],
            [
                LegendSwatch::ConnectedCity,
                LegendSwatch::UnconnectedCity,
                LegendSwatch::PlantUnderConstruction,
                LegendSwatch::LineUnderConstruction,
            ]
        );
        let labels: Vec<&str> = model.legend[PlantKind::ALL.len()..]
            .iter()
            .map(|e| e.label.as_str())
            .collect();
        assert_eq!(
            labels,
            [
                "Connected city",
                "Unconnected city",
                "Plant under construction",
                "Line under construction",
            ]
        );
        assert_eq!(model.legend_label, "Map legend");
        assert_eq!(l10n.problems(), vec![]);
    }

    /// A game with a peaker and a short line ordered: both under construction.
    fn building() -> (Game, PlantId, u32) {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let plant = order_plant(&mut game, PlantKind::Peaker, 0);
        let (_, months) = order_short_line(&mut game);
        (game, plant, months)
    }

    #[test]
    fn projects_under_construction_carry_their_months_left() {
        let l10n = shipped_l10n();
        let (game, plant, line_months) = building();
        let session = game.session();
        let peaker = session.region().spec(PlantKind::Peaker).build_months;

        let model = map_model(
            &game,
            &l10n,
            MapLayer::Terrain,
            LegendChoice::default(),
            Selection::None,
        );

        let marker = model.objects.plants.iter().find(|p| p.id == plant).unwrap();
        assert_eq!(marker.months_left, Some(peaker));
        assert!(!marker.built());
        let building: Vec<&LineMarker> = model
            .objects
            .lines
            .iter()
            .filter(|l| !l.in_service())
            .collect();
        assert_eq!(building.len(), 1);
        assert_eq!(building[0].months_left, Some(line_months));
        assert_eq!(
            model.months_left,
            [
                MonthsLeftLabel {
                    anchor: LabelAnchor::Plant(marker.cell),
                    text: format!("{peaker} mo"),
                },
                MonthsLeftLabel {
                    anchor: LabelAnchor::Line {
                        from: building[0].from,
                        to: building[0].to,
                    },
                    text: format!("{line_months} mo"),
                },
            ]
        );
        // The start fleet and network are built: no labels.
        let fresh = eastern_game(Box::new(InMemoryCommandLog::default()));
        assert_eq!(
            map_model(
                &fresh,
                &l10n,
                MapLayer::Terrain,
                LegendChoice::default(),
                Selection::None
            )
            .months_left,
            []
        );
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_months_left_labels_are_drawn_only_at_9_pt_or_more() {
        let l10n = shipped_l10n();
        let (game, _, _) = building();
        let model = map_model(
            &game,
            &l10n,
            MapLayer::Terrain,
            LegendChoice::default(),
            Selection::None,
        );
        let layout_at =
            |cell: f32| MapLayout::fit(area(0.0, 0.0, cell * 100.0, cell * 60.0), 100, 60);

        // At the smallest window's map, about 5 pt cells, none.
        assert_eq!(months_left_labels(&layout_at(5.0), &model.months_left), []);
        for step in 4..160 {
            let cell = step as f32 * 0.25;
            let layout = layout_at(cell);
            let placed = months_left_labels(&layout, &model.months_left);
            match glyph_font_size(&layout) {
                None => assert_eq!(placed, [], "{cell} pt cells"),
                Some(size) => {
                    assert_eq!(placed.len(), 2, "{cell} pt cells");
                    for label in &placed {
                        assert!(label.size >= GLYPH_MIN_FONT, "{cell} pt cells");
                        assert_eq!(label.size, size);
                    }
                }
            }
        }
        // Beside the plant's square, and above the line's midpoint.
        let layout = layout_at(20.0);
        let placed = months_left_labels(&layout, &model.months_left);
        let LabelAnchor::Plant(at) = model.months_left[0].anchor else {
            panic!("the plant's label comes first");
        };
        let centre = layout.cell_center(at);
        assert_eq!(placed[0].text, model.months_left[0].text);
        assert_eq!(placed[0].align, egui::Align2::LEFT_CENTER);
        assert!(placed[0].pos.x > centre.x + plant_half_size(&layout));
        assert_eq!(placed[0].pos.y, centre.y);
        let LabelAnchor::Line { from, to } = model.months_left[1].anchor else {
            panic!("then the line's");
        };
        let middle = layout.cell_center(from).lerp(layout.cell_center(to), 0.5);
        assert_eq!(placed[1].align, egui::Align2::CENTER_BOTTOM);
        assert_eq!(placed[1].pos.x, middle.x);
        assert!(placed[1].pos.y < middle.y);
        assert_eq!(l10n.problems(), vec![]);
    }

    /// A label is paint only: a click on it goes to what is drawn there, not to its plant.
    #[test]
    fn a_months_left_label_takes_no_click() {
        let l10n = shipped_l10n();
        let (game, plant, _) = building();
        let model = map_model(
            &game,
            &l10n,
            MapLayer::Terrain,
            LegendChoice::default(),
            Selection::None,
        );
        let layout = MapLayout::fit(area(0.0, 0.0, 2000.0, 1200.0), 100, 60);
        let placed = months_left_labels(&layout, &model.months_left);
        assert_eq!(placed.len(), 2);
        let on_text = placed[0].pos + vec2(4.0, 0.0);
        let hit = pick(&layout, on_text, &model.objects);
        assert_ne!(hit, Selection::Plant(plant));
        let covered = model
            .objects
            .plants
            .iter()
            .any(|p| covers(&layout, p, on_text));
        if !covered {
            assert_eq!(hit, Selection::Cell(layout.cell_at(on_text).unwrap()));
        }
    }

    #[test]
    fn the_legend_takes_the_first_corner_that_hides_no_mark() {
        use egui::Align2;
        let map = area(100.0, 50.0, 400.0, 300.0);
        let size = vec2(120.0, 60.0);
        let rect = |corner| legend_rect(map, size, corner);

        // Each corner is inset from the map's edges by the same margin.
        assert_eq!(rect(Align2::RIGHT_TOP), area(372.0, 58.0, 120.0, 60.0));
        assert_eq!(rect(Align2::RIGHT_BOTTOM), area(372.0, 282.0, 120.0, 60.0));
        assert_eq!(rect(Align2::LEFT_BOTTOM), area(108.0, 282.0, 120.0, 60.0));
        assert_eq!(rect(Align2::LEFT_TOP), area(108.0, 58.0, 120.0, 60.0));

        // An empty map: the top right, the first preference.
        assert_eq!(legend_placement(map, size, &[]), Some(Align2::RIGHT_TOP));
        // Each corner blocked in turn sends it to the next, in order.
        let mut marks = Vec::new();
        for (blocked, next) in LEGEND_CORNERS.iter().zip(
            LEGEND_CORNERS
                .iter()
                .skip(1)
                .map(Some)
                .chain(std::iter::once(None)),
        ) {
            marks.push(rect(*blocked).center());
            assert_eq!(
                legend_placement(map, size, &marks),
                next.copied(),
                "after blocking {blocked:?}"
            );
        }
        // One mark in a lower corner only: the top right is still preferred.
        assert_eq!(
            legend_placement(map, size, &[rect(Align2::LEFT_BOTTOM).center()]),
            Some(Align2::RIGHT_TOP)
        );
        // Only the top left clear: the top left.
        let three: Vec<Pos2> = LEGEND_CORNERS[..3]
            .iter()
            .map(|c| rect(*c).left_top() + vec2(1.0, 1.0))
            .collect();
        assert_eq!(legend_placement(map, size, &three), Some(Align2::LEFT_TOP));
        // A mark just outside a corner's rect doesn't block it.
        let beside = rect(Align2::RIGHT_TOP).left_center() - vec2(0.5, 0.0);
        assert_eq!(
            legend_placement(map, size, &[beside]),
            Some(Align2::RIGHT_TOP)
        );
        // A map smaller than the legend has no room in any corner.
        assert_eq!(
            legend_placement(area(0.0, 0.0, 100.0, 40.0), size, &[]),
            None
        );
    }

    #[test]
    fn the_player_s_choice_wins_and_an_opened_legend_still_prefers_a_clear_corner() {
        use egui::Align2;
        let undecided = LegendChoice::default();
        assert_eq!(undecided, LegendChoice(None));
        for clear in LEGEND_CORNERS.into_iter().map(Some).chain([None]) {
            assert_eq!(legend_corner(undecided, clear), clear, "by default");
            assert_eq!(legend_corner(LegendChoice(Some(false)), clear), None);
            assert_eq!(
                legend_corner(LegendChoice(Some(true)), clear),
                Some(clear.unwrap_or(Align2::RIGHT_TOP)),
                "opened, with {clear:?} clear"
            );
        }
    }

    #[test]
    fn every_city_and_plant_is_a_pickable_mark() {
        let (game, plant, _) = building();
        let objects = MapObjects::of(&game);
        let layout = MapLayout::fit(area(0.0, 0.0, 1000.0, 600.0), 100, 60);

        let marks = pickable_marks(&layout, &objects);

        assert_eq!(marks.len(), objects.cities.len() + objects.plants.len());
        for city in &objects.cities {
            assert!(marks.contains(&layout.cell_center(city.cell)));
        }
        let unbuilt = objects.plants.iter().find(|p| p.id == plant).unwrap();
        assert!(!unbuilt.built());
        assert!(marks.contains(&layout.cell_center(unbuilt.cell)));
    }

    #[test]
    fn the_legend_toggle_shows_whether_it_is_open_and_asks_for_the_other_state() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let model = map_model(
            &game,
            &l10n,
            MapLayer::Terrain,
            LegendChoice(Some(false)),
            Selection::None,
        );
        assert_eq!(model.legend_toggle, "Legend");
        assert_eq!(model.legend_choice, LegendChoice(Some(false)));

        for open in [true, false] {
            let toggle = legend_toggle(&model.legend_toggle, open);
            assert_eq!(toggle.on, open);
            assert_eq!(
                toggle.button,
                ButtonModel::new("Legend".to_owned(), UiAction::ShowLegend(!open))
            );
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_legend_drops_columns_until_it_fits_the_width() {
        // Row by row: three columns are 80 + 90 + 70 wide, two are 90 + 80, one is 90.
        let widths = [50.0, 60.0, 70.0, 80.0, 90.0, 40.0, 30.0];
        let gap = 12.0;
        assert_eq!(legend_columns(&widths, gap, 1000.0), LEGEND_COLUMNS);
        assert_eq!(legend_columns(&widths, gap, 264.0), 3);
        assert_eq!(legend_columns(&widths, gap, 263.9), 2);
        assert_eq!(legend_columns(&widths, gap, 182.0), 2);
        assert_eq!(legend_columns(&widths, gap, 181.9), 1);
        assert_eq!(legend_columns(&widths, gap, 90.0), 1);
        // Too narrow for even one column: one column, the narrowest it gets.
        assert_eq!(legend_columns(&widths, gap, 10.0), 1);
        // Fewer entries than columns: no empty column counts.
        assert_eq!(legend_columns(&[50.0, 60.0], gap, 122.0), 2);
        assert_eq!(legend_columns(&[], gap, 0.0), 1);
    }

    #[test]
    fn plant_kinds_read_apart_by_colour_alone() {
        let mut colours: Vec<Rgb> = PlantKind::ALL.into_iter().map(kind_color).collect();
        colours.sort_unstable();
        colours.dedup();
        assert_eq!(colours.len(), PlantKind::ALL.len());
    }
}
