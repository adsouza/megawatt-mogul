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
use domain::ids::{CellCoord, CityId, NodeId, PlantId};
use domain::region::{PlantKind, RegionDefinition};
use egui::{Color32, Pos2, Rect, Stroke, StrokeKind, vec2};

use super::action::{ActionSink, ButtonModel, UiAction, button, dispatch};
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
    CoolingWater,
    HydroPotential,
    /// Where a plant of this kind may be built (the region's siting rule).
    Suitability(PlantKind),
}

impl MapLayer {
    /// Every layer, in the order the buttons show them.
    pub fn all() -> impl Iterator<Item = Self> {
        [
            Self::Terrain,
            Self::Irradiance,
            Self::CoolingWater,
            Self::HydroPotential,
        ]
        .into_iter()
        .chain(PlantKind::ALL.into_iter().map(Self::Suitability))
    }
}

pub(super) fn reset_layer(mut layer: ResMut<MapLayer>) {
    *layer = MapLayer::default();
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
    /// False while under construction: drawn faded.
    pub built: bool,
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
    pub from: CellCoord,
    pub to: CellCoord,
    /// False while under construction: drawn dashed.
    pub in_service: bool,
}

/// What stands on the map, as drawn and as picked: a few dozen items, in id order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapObjects {
    pub plants: Vec<PlantMarker>,
    pub cities: Vec<CityMarker>,
    pub substation: CellCoord,
    pub lines: Vec<LineMarker>,
    /// The cell of every network node: the substation, each city and each plant.
    pub nodes: BTreeMap<NodeId, CellCoord>,
}

impl MapObjects {
    /// The markers of `game`, read through `GameSession`'s queries.
    pub fn of(game: &Game) -> Self {
        let session = game.session();
        let substation = session.region().start().substation;
        let mut nodes = BTreeMap::from([(session.substation(), substation)]);
        let plants: Vec<PlantMarker> = session
            .plant_views()
            .into_iter()
            .map(|plant| {
                nodes.insert(plant.node, plant.cell);
                PlantMarker {
                    id: plant.id,
                    kind: plant.kind,
                    cell: plant.cell,
                    built: !matches!(plant.status, PlantStatus::UnderConstruction { .. }),
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
                    from: *nodes.get(&line.from)?,
                    to: *nodes.get(&line.to)?,
                    in_service: line.status == LineStatus::InService,
                })
            })
            .collect();
        Self {
            plants,
            cities,
            substation,
            lines,
            nodes,
        }
    }
}

/// How close to a marker's centre a click counts as on it, in points: a cell and a half, and never
/// too small to hit on a big map.
pub fn marker_radius(layout: &MapLayout) -> f32 {
    (layout.cell_size() * 1.5).max(6.0)
}

/// What a click at `pos` selects: the nearest plant in reach, else the nearest city in reach, else
/// the cell, else nothing.
pub fn pick(layout: &MapLayout, pos: Pos2, objects: &MapObjects) -> Selection {
    if layout.cell_size() <= 0.0 {
        return Selection::None;
    }
    let reach = marker_radius(layout);
    let nearest = |markers: &mut dyn Iterator<Item = (Selection, CellCoord)>| {
        markers
            .map(|(selection, cell)| (selection, (layout.cell_center(cell) - pos).length()))
            .filter(|(_, distance)| *distance <= reach)
            // The first of equals wins, which is the lowest id: the lists are in id order.
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(selection, _)| selection)
    };
    nearest(
        &mut objects
            .plants
            .iter()
            .map(|plant| (Selection::Plant(plant.id), plant.cell)),
    )
    .or_else(|| {
        nearest(
            &mut objects
                .cities
                .iter()
                .map(|city| (Selection::City(city.id), city.cell)),
        )
    })
    .or_else(|| layout.cell_at(pos).map(Selection::Cell))
    .unwrap_or(Selection::None)
}

/// What a click at `pos` selects in `game`: [`pick`] over the game's own markers.
pub fn hit_test(layout: &MapLayout, pos: Pos2, game: &Game) -> Selection {
    pick(layout, pos, &MapObjects::of(game))
}

// ---- colours: pure ---------------------------------------------------------------------------

/// An sRGB colour, kept free of egui types so the shading is testable.
pub type Rgb = [u8; 3];

/// Open water, on every layer that shades land only.
pub const WATER: Rgb = [28, 52, 96];
/// A cell the plant can be built on, on the suitability layer.
pub const SUITABLE: Rgb = [70, 170, 90];
/// A land cell the plant cannot be built on.
pub const UNSUITABLE: Rgb = [120, 55, 55];
const LOWLAND: Rgb = [64, 128, 70];
const HIGHLAND: Rgb = [150, 130, 100];
const DARK: Rgb = [38, 40, 46];
const SUN: Rgb = [255, 214, 64];
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
        MapLayer::Irradiance => land_shaded(cell.irradiance.value(), SUN),
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
    }
}

fn glyph_key(kind: PlantKind) -> &'static str {
    match kind {
        PlantKind::Nuclear => "plant-glyph-nuclear",
        PlantKind::Coal => "plant-glyph-coal",
        PlantKind::Ccgt => "plant-glyph-ccgt",
        PlantKind::Peaker => "plant-glyph-peaker",
        PlantKind::Solar => "plant-glyph-solar",
        PlantKind::Hydro => "plant-glyph-hydro",
    }
}

fn color32([r, g, b]: Rgb) -> Color32 {
    Color32::from_rgb(r, g, b)
}

// ---- model, presenter, draw, system ---------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct MapModel {
    /// The map's accessible name.
    pub label: String,
    /// One button per [`MapLayer`]; the current layer's is disabled.
    pub layers: Vec<ButtonModel>,
    pub layer: MapLayer,
    /// The terrain is read from the region while painting: it is too big to copy every frame.
    pub region: Arc<RegionDefinition>,
    pub objects: MapObjects,
    /// The letter each kind of plant is drawn with.
    pub glyphs: Vec<(PlantKind, String)>,
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
        MapLayer::CoolingWater => l10n.text("map-layer-cooling-water"),
        MapLayer::HydroPotential => l10n.text("map-layer-hydro-potential"),
        MapLayer::Suitability(kind) => l10n.args(
            "map-layer-suitability",
            &[("kind", kind.localize(l10n).into())],
        ),
    }
}

pub fn map_model(game: &Game, l10n: &Localizer, layer: MapLayer, selection: Selection) -> MapModel {
    MapModel {
        label: l10n.text("map-label"),
        layers: MapLayer::all()
            .map(|choice| {
                ButtonModel::new(layer_label(choice, l10n), UiAction::SetLayer(choice))
                    .enabled_if(choice != layer)
            })
            .collect(),
        layer,
        region: Arc::clone(game.session().region()),
        objects: MapObjects::of(game),
        glyphs: PlantKind::ALL
            .into_iter()
            .map(|kind| (kind, l10n.text(glyph_key(kind))))
            .collect(),
        selection,
    }
}

pub fn draw_map(ui: &mut egui::Ui, model: &MapModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.horizontal_wrapped(|ui| {
        for layer in &model.layers {
            button(ui, layer, &mut actions);
        }
    });
    let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, model.label.as_str())
    });
    let terrain = model.region.terrain();
    let layout = MapLayout::fit(response.rect, terrain.width(), terrain.height());
    paint_cells(&painter, &layout, model);
    paint_objects(&painter, &layout, model);
    if let Some(pos) = response
        .interact_pointer_pos()
        .filter(|_| response.clicked())
    {
        actions.push(UiAction::Select(pick(&layout, pos, &model.objects)));
    }
    actions
}

/// One rect per cell, coloured by the layer. Each is grown a hair so the anti-aliased edges of
/// neighbours overlap instead of leaving seams.
fn paint_cells(painter: &egui::Painter, layout: &MapLayout, model: &MapModel) {
    let terrain = model.region.terrain();
    for y in 0..terrain.height() {
        for x in 0..terrain.width() {
            let at = CellCoord { x, y };
            painter.rect_filled(
                layout.cell_rect(at).expand(0.3),
                egui::CornerRadius::ZERO,
                color32(cell_color(model.layer, &model.region, at)),
            );
        }
    }
}

/// Lines, then the substation, cities and plants on top, in the order a click prefers them.
fn paint_objects(painter: &egui::Painter, layout: &MapLayout, model: &MapModel) {
    let objects = &model.objects;
    let half = marker_radius(layout) * 0.7;
    let at = |cell| layout.cell_center(cell);
    for line in &objects.lines {
        let ends = [at(line.from), at(line.to)];
        if line.in_service {
            painter.line_segment(ends, Stroke::new(2.0, Color32::from_rgb(255, 230, 120)));
        } else {
            painter.extend(egui::Shape::dashed_line(
                &ends,
                Stroke::new(1.5, Color32::from_rgb(200, 200, 210)),
                5.0,
                4.0,
            ));
        }
    }
    let ink = Color32::from_rgb(20, 22, 28);
    let centre = at(objects.substation);
    painter.add(egui::Shape::convex_polygon(
        vec![
            centre + vec2(0.0, -half),
            centre + vec2(half, 0.0),
            centre + vec2(0.0, half),
            centre + vec2(-half, 0.0),
        ],
        Color32::WHITE,
        Stroke::new(1.5, ink),
    ));
    for city in &objects.cities {
        let (fill, outline) = if city.connected {
            (Color32::from_rgb(255, 236, 160), ink)
        } else {
            (Color32::TRANSPARENT, Color32::from_rgb(255, 236, 160))
        };
        painter.circle(at(city.cell), half * 0.85, fill, Stroke::new(2.0, outline));
    }
    for plant in &objects.plants {
        let fill = color32(kind_color(plant.kind));
        let fade = if plant.built { 1.0 } else { 0.45 };
        let centre = at(plant.cell);
        painter.rect(
            Rect::from_center_size(centre, vec2(half * 2.0, half * 2.0)),
            egui::CornerRadius::same(3),
            fill.gamma_multiply(fade),
            Stroke::new(1.5, ink.gamma_multiply(fade)),
            StrokeKind::Inside,
        );
        painter.text(
            centre,
            egui::Align2::CENTER_CENTER,
            model.glyph(plant.kind),
            egui::FontId::proportional(half * 1.3),
            ink,
        );
    }
    paint_selection(painter, layout, model, half);
}

/// A white ring (or outline, for a cell) around what is selected.
fn paint_selection(painter: &egui::Painter, layout: &MapLayout, model: &MapModel, half: f32) {
    let ring = Stroke::new(2.0, Color32::WHITE);
    let objects = &model.objects;
    let around = |cell| painter.circle_stroke(layout.cell_center(cell), half * 1.45, ring);
    match model.selection {
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
    mut sink: ActionSink,
) -> Result {
    let model = map_model(&session.0, &l10n, sink.layer(), sink.selection());
    let mut ui = area.root_ui(contexts.ctx_mut()?, "map");
    let actions = egui::CentralPanel::default()
        .show(&mut ui, |ui| draw_map(ui, &model))
        .inner;
    dispatch(actions, &mut sink);
    Ok(())
}

#[cfg(test)]
mod tests {
    use application::InMemoryCommandLog;
    use domain::session::LineView;
    use egui::{pos2, vec2};

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_l10n};

    fn area(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(w, h))
    }

    #[test]
    fn map_layout_keeps_aspect_and_fits() {
        // A 100 x 60 grid in areas wider than, taller than, and exactly the grid's shape.
        for panel in [
            area(0.0, 0.0, 1000.0, 300.0),
            area(40.0, 20.0, 300.0, 900.0),
            area(0.0, 0.0, 800.0, 480.0),
            area(-50.0, 10.0, 77.0, 1234.0),
        ] {
            let layout = MapLayout::fit(panel, 100, 60);
            let bounds = layout.bounds();

            assert!(
                panel.expand(0.001).contains_rect(bounds),
                "{bounds:?} in {panel:?}"
            );
            assert!(
                (bounds.width() / bounds.height() - 100.0 / 60.0).abs() < 1e-4,
                "aspect {bounds:?}"
            );
            let touches_width = (bounds.width() - panel.width()).abs() < 1e-3;
            let touches_height = (bounds.height() - panel.height()).abs() < 1e-3;
            assert!(
                touches_width || touches_height,
                "{bounds:?} fills {panel:?}"
            );
            assert!(
                (bounds.center() - panel.center()).length() < 1e-3,
                "centred {bounds:?} in {panel:?}"
            );
            assert!((layout.cell_size() * 100.0 - bounds.width()).abs() < 1e-3);

            // Every cell round-trips through its centre, and the corners are on the grid.
            for at in [
                CellCoord { x: 0, y: 0 },
                CellCoord { x: 99, y: 59 },
                CellCoord { x: 37, y: 12 },
                CellCoord { x: 99, y: 0 },
            ] {
                assert_eq!(layout.cell_at(layout.cell_center(at)), Some(at));
                assert!(bounds.contains(layout.cell_rect(at).center()));
            }
            assert_eq!(layout.cell_at(bounds.min - vec2(1.0, 0.0)), None);
            assert_eq!(layout.cell_at(bounds.max + vec2(0.0, 1.0)), None);
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
                built: true,
            }],
            cities: vec![CityMarker {
                id: CityId(3),
                cell: stacked,
                connected: false,
            }],
            substation: cell(50, 30),
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

    #[test]
    fn a_marker_is_hit_within_its_radius_and_the_nearest_wins() {
        let layout = MapLayout::fit(area(0.0, 0.0, 1000.0, 600.0), 100, 60);
        let near = PlantMarker {
            id: PlantId(2),
            kind: PlantKind::Ccgt,
            cell: cell(40, 20),
            built: true,
        };
        let far = PlantMarker {
            id: PlantId(1),
            kind: PlantKind::Coal,
            cell: cell(41, 20),
            built: true,
        };
        let objects = MapObjects {
            plants: vec![far, near],
            cities: Vec::new(),
            substation: cell(0, 0),
            lines: Vec::new(),
            nodes: BTreeMap::new(),
        };
        let radius = marker_radius(&layout);
        assert!(radius >= layout.cell_size());

        // Just beside the nearer plant's centre, within its radius, still picks it...
        let beside = layout.cell_center(near.cell) + vec2(-radius * 0.9, 0.0);
        assert_eq!(pick(&layout, beside, &objects), Selection::Plant(near.id));
        // ...even though the other plant is also in reach from a point between them: the nearer
        // centre wins whatever the order of the list.
        let between = layout.cell_center(near.cell) + vec2(layout.cell_size() * 0.3, 0.0);
        assert_eq!(pick(&layout, between, &objects), Selection::Plant(near.id));
        // Well clear of both, it is just a cell.
        let clear = layout.cell_center(cell(70, 50));
        assert_eq!(
            pick(&layout, clear, &objects),
            Selection::Cell(cell(70, 50))
        );
    }

    #[test]
    fn hit_test_reads_the_game_s_plants_cities_and_cells() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let layout = MapLayout::fit(area(20.0, 10.0, 1000.0, 600.0), 100, 60);

        for plant in session.plant_views() {
            let Selection::Plant(found) = hit_test(&layout, layout.cell_center(plant.cell), &game)
            else {
                panic!("plant {:?} at {:?} is not hit", plant.id, plant.cell);
            };
            let found = session
                .plant_views()
                .into_iter()
                .find(|p| p.id == found)
                .unwrap();
            assert_eq!(found.cell, plant.cell, "the plant on that cell");
        }
        for city in session.city_views() {
            let hit = hit_test(&layout, layout.cell_center(city.cell), &game);
            assert!(
                matches!(hit, Selection::City(_) | Selection::Plant(_)),
                "{hit:?}"
            );
        }
        let terrain = session.region().terrain();
        let empty = (0..terrain.height())
            .flat_map(|y| (0..terrain.width()).map(move |x| cell(x, y)))
            .find(|c| {
                let pos = layout.cell_center(*c);
                let objects = MapObjects::of(&game);
                objects
                    .plants
                    .iter()
                    .map(|p| p.cell)
                    .chain(objects.cities.iter().map(|c| c.cell))
                    .all(|other| {
                        (layout.cell_center(other) - pos).length() > 3.0 * marker_radius(&layout)
                    })
            })
            .expect("some cell is clear of every marker");
        assert_eq!(
            hit_test(&layout, layout.cell_center(empty), &game),
            Selection::Cell(empty)
        );
    }

    #[test]
    fn map_objects_mirror_the_session() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();

        let objects = MapObjects::of(&game);

        assert_eq!(objects.plants.len(), session.plant_views().len());
        assert!(
            objects.plants.iter().all(|p| p.built),
            "the start fleet is built"
        );
        assert_eq!(objects.cities.len(), session.city_views().len());
        assert_eq!(
            objects.cities.iter().filter(|c| c.connected).count(),
            session.city_views().iter().filter(|c| c.connected).count()
        );
        assert_eq!(objects.substation, session.region().start().substation);
        let lines: Vec<LineView> = session.line_views();
        assert_eq!(objects.lines.len(), lines.len());
        assert!(objects.lines.iter().all(|l| l.in_service));
        assert_eq!(
            objects.nodes.get(&session.substation()),
            Some(&objects.substation)
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
        let mut seen_suitable = [false; 6];
        let mut seen_unsuitable = [false; 6];

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
    fn layer_buttons_pick_a_layer_and_the_current_one_is_disabled() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        let model = map_model(&game, &l10n, MapLayer::Terrain, Selection::None);

        let layers: Vec<MapLayer> = MapLayer::all().collect();
        assert_eq!(model.layers.len(), layers.len());
        assert_eq!(layers.len(), 10);
        for (button, layer) in model.layers.iter().zip(&layers) {
            assert_eq!(button.action, UiAction::SetLayer(*layer));
            assert_eq!(
                button.enabled,
                *layer != MapLayer::Terrain,
                "{}",
                button.label
            );
        }
        let labels: Vec<&str> = model.layers.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels[0], l10n.text("map-layer-terrain"));
        assert_eq!(labels[1], "Sunshine");
        assert_eq!(labels[2], "Cooling water");
        assert_eq!(labels[3], "Hydro potential");
        assert_eq!(labels[4], "Suitability: Nuclear plant");
        assert_eq!(labels[9], "Suitability: Hydro plant");
        assert_eq!(model.layer, MapLayer::Terrain);
        assert_eq!(l10n.problems(), vec![]);

        // Choosing another layer moves the disabled button.
        let model = map_model(
            &game,
            &l10n,
            MapLayer::Suitability(PlantKind::Solar),
            Selection::None,
        );
        let disabled: Vec<&str> = model
            .layers
            .iter()
            .filter(|b| !b.enabled)
            .map(|b| b.label.as_str())
            .collect();
        assert_eq!(disabled, ["Suitability: Solar farm"]);
    }

    #[test]
    fn plants_get_a_translated_glyph_each() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        let model = map_model(&game, &l10n, MapLayer::Terrain, Selection::None);

        let mut glyphs: Vec<&str> = PlantKind::ALL.iter().map(|k| model.glyph(*k)).collect();
        assert!(glyphs.iter().all(|g| g.chars().count() == 1), "{glyphs:?}");
        glyphs.sort_unstable();
        glyphs.dedup();
        assert_eq!(glyphs.len(), 6, "each kind has its own glyph");
        assert_eq!(model.label, l10n.text("map-label"));
        assert_eq!(l10n.problems(), vec![]);
    }
}
