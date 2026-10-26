//! Cached geographic artwork for the menu. No session or simulation state lives here.
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

use super::region_globe::{GlobeSpin, GlobeView, clip_limb, sphere};
use application::RegionCatalog;
use bevy::{math::Vec3, prelude::Resource};
use bevy_egui::egui::{self, Color32, Rect, Stroke};
use domain::ids::RegionId;
use infrastructure::region::{Bounds, RegionOutline, region_outline};

use super::{RegionListings, RegionSelectModel, UiAction, theme};
use crate::l10n::{FluentArg, Localizer};

const WEST: f64 = -180.0;
const EAST: f64 = 180.0;
const NORTH: f64 = 90.0;
const SOUTH: f64 = -90.0;
const ROWS: usize = 720;
const OCEAN: Color32 = Color32::from_rgb(12, 25, 36);
const LAND: Color32 = Color32::from_rgb(39, 55, 63);
const COLOURS: [Color32; 4] = [
    Color32::from_rgb(69, 178, 179),
    Color32::from_rgb(226, 164, 82),
    Color32::from_rgb(141, 157, 222),
    Color32::from_rgb(184, 139, 191),
];

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Artwork {
    outline: RegionOutline,
    mesh: Vec<[[f32; 3]; 4]>,
    boundaries: Vec<[[f32; 3]; 2]>,
}

/// Optional RON geography cached once on entering the menu. Unknown catalog formats still have
/// ordinary, accessible region buttons; they never get a guessed geographic click target.
#[derive(Resource, Default)]
pub struct RegionAtlas(BTreeMap<RegionId, Arc<Artwork>>);

impl RegionAtlas {
    pub fn load(catalog: &dyn RegionCatalog, listings: &RegionListings) -> Self {
        let mut regions = BTreeMap::new();
        if let Ok(listings) = &listings.0 {
            for listing in listings.iter().filter(|listing| listing.entry.is_ok()) {
                if let Ok(loaded) = catalog.load(&listing.id)
                    && let Ok(outline) = region_outline(&loaded.source_bytes, &listing.id)
                {
                    let mesh = mesh(&outline);
                    let boundaries = area_boundaries(&loaded.definition);
                    regions.insert(
                        listing.id.clone(),
                        Arc::new(Artwork {
                            outline,
                            mesh,
                            boundaries,
                        }),
                    );
                }
            }
        }
        Self(regions)
    }
}

impl RegionSelectModel {
    pub fn with_atlas(mut self, atlas: &RegionAtlas, l10n: &Localizer) -> Self {
        for choice in &mut self.regions {
            if let UiAction::NewGame { region, .. } = &choice.button.action
                && let Some(art) = atlas.0.get(region)
            {
                choice.area_names = art
                    .outline
                    .areas
                    .iter()
                    .map(|a| (a.name_key.clone(), l10n.text(&a.name_key)))
                    .collect();
                let offset = |minutes: i32| {
                    l10n.args(
                        "region-select-zone",
                        &[(
                            "offset",
                            format!(
                                "{}{:02}:{:02}",
                                if minutes < 0 { "−" } else { "+" },
                                minutes.abs() / 60,
                                minutes.abs() % 60
                            )
                            .into(),
                        )],
                    )
                };
                let (min, max) = art.outline.zone_span;
                let zones = if min == max {
                    offset(min)
                } else {
                    l10n.args(
                        "region-select-zone-span",
                        &[("west", offset(min).into()), ("east", offset(max).into())],
                    )
                };
                let hemisphere = l10n.text(match art.outline.hemisphere {
                    infrastructure::region::Hemisphere::Northern => "region-select-northern",
                    infrastructure::region::Hemisphere::Southern => "region-select-southern",
                    infrastructure::region::Hemisphere::Both => "region-select-both-hemispheres",
                });
                choice.metadata = Some(l10n.args(
                    "region-select-metadata",
                    &[
                        ("frequency", FluentArg::Number(art.outline.frequency_hz)),
                        ("currency", art.outline.currency.iso_code().into()),
                        ("zones", zones.into()),
                        ("hemisphere", hemisphere.into()),
                    ],
                ));
                choice.artwork = Some(art.clone());
                if let Some((name, _, _)) = &art.outline.home {
                    choice.detail = Some(l10n.args(
                        "region-select-home",
                        &[
                            ("city", l10n.text(name).into()),
                            ("cities", FluentArg::Number(art.outline.cities as f64)),
                        ],
                    ));
                }
            }
        }
        self
    }
}

pub(super) fn aspect() -> f32 {
    1.0
}

// Cache a scanline union/subtraction on the unit sphere, independent of the playable grid.
// Longitude subdivisions keep curved shorelines and limb clipping accurate during rotation.
fn mesh(outline: &RegionOutline) -> Vec<[[f32; 3]; 4]> {
    geographic_mesh(outline.bounds, &outline.land, &outline.exclude)
}

fn geographic_mesh(
    bounds: Bounds,
    land_rings: &[Vec<(f64, f64)>],
    excluded_rings: &[Vec<(f64, f64)>],
) -> Vec<[[f32; 3]; 4]> {
    let mut strips = Vec::new();
    let step = (NORTH - SOUTH) / ROWS as f64;
    for row in 0..ROWS {
        let lat = NORTH - (row as f64 + 0.5) * step;
        if lat < bounds.south || lat > bounds.north {
            continue;
        }
        let mut events = Vec::new();
        for (rings, exclusion) in [(land_rings, false), (excluded_rings, true)] {
            for ring in rings {
                let mut xs = Vec::new();
                if let Some(mut prev) = ring.last().copied() {
                    for &next in ring {
                        if (prev.1 > lat) != (next.1 > lat) {
                            xs.push(
                                prev.0 + (lat - prev.1) / (next.1 - prev.1) * (next.0 - prev.0),
                            );
                        }
                        prev = next;
                    }
                }
                xs.sort_by(f64::total_cmp);
                for pair in xs.as_chunks::<2>().0 {
                    let west = pair[0].max(bounds.west).max(WEST);
                    let east = pair[1].min(bounds.east).min(EAST);
                    if west < east {
                        events.extend([(west, exclusion, 1_i32), (east, exclusion, -1)]);
                    }
                }
            }
        }
        events.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut land, mut excluded) = (0, 0);
        let mut start = None;
        for (x, exclusion, delta) in events {
            let before = land > 0 && excluded == 0;
            if exclusion {
                excluded += delta;
            } else {
                land += delta;
            }
            let after = land > 0 && excluded == 0;
            if !before && after {
                start = Some(x);
            }
            if before
                && !after
                && let Some(west) = start.take()
                && west < x
            {
                let top = (lat + step * 0.5).min(bounds.north);
                let bottom = (lat - step * 0.5).max(bounds.south);
                let columns = (x - west).ceil() as usize;
                for column in 0..columns {
                    let left = west + (x - west) * column as f64 / columns as f64;
                    let right = west + (x - west) * (column + 1) as f64 / columns as f64;
                    strips.push([
                        sphere(left, top).to_array(),
                        sphere(right, top).to_array(),
                        sphere(right, bottom).to_array(),
                        sphere(left, bottom).to_array(),
                    ]);
                }
            }
        }
    }
    strips
}

fn paint_mesh(
    painter: &egui::Painter,
    strips: &[[[f32; 3]; 4]],
    rect: Rect,
    view: &GlobeView,
    colour: Color32,
    shade: bool,
    offset: egui::Vec2,
) {
    let mut mesh = egui::Mesh::default();
    mesh.vertices.reserve(strips.len() * 2);
    mesh.indices.reserve(strips.len() * 3);
    for strip in strips {
        let face = strip.map(|p| view.rotate(Vec3::from_array(p)));
        if face.iter().all(|p| p.z < 0.0) {
            continue;
        }
        let clipped;
        let points = if face.iter().all(|p| p.z >= 0.0) {
            &face[..]
        } else {
            clipped = clip_limb(&face);
            &clipped
        };
        let base = mesh.vertices.len() as u32;
        for p in points {
            let gain = if shade {
                0.62 + 0.38 * p.z.max(0.0).sqrt()
            } else {
                1.0
            };
            let colour = Color32::from_rgb(
                (f32::from(colour.r()) * gain) as u8,
                (f32::from(colour.g()) * gain) as u8,
                (f32::from(colour.b()) * gain) as u8,
            );
            mesh.colored_vertex(view.screen(rect, *p) + offset, colour);
        }
        for i in 1..points.len().saturating_sub(1) {
            mesh.add_triangle(base, base + i as u32, base + i as u32 + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

fn graticule(
    painter: &egui::Painter,
    rect: Rect,
    view: &GlobeView,
    points: impl Iterator<Item = Vec3>,
) {
    let mut previous = None;
    for point in points.map(|p| view.rotate(p)) {
        if let Some(a) = previous {
            let a: Vec3 = a;
            if a.z >= 0.0 || point.z >= 0.0 {
                let mut a = a;
                let mut b = point;
                if a.z < 0.0 {
                    a = a.lerp(b, a.z / (a.z - b.z)).normalize_or_zero();
                }
                if b.z < 0.0 {
                    b = b.lerp(a, b.z / (b.z - a.z)).normalize_or_zero();
                }
                painter.line_segment(
                    [view.screen(rect, a), view.screen(rect, b)],
                    Stroke::new(0.6, Color32::from_rgb(39, 65, 79)),
                );
            }
        }
        previous = Some(point);
    }
}

fn backdrop() -> &'static Vec<[[f32; 3]; 4]> {
    static BACKDROP: OnceLock<Vec<[[f32; 3]; 4]>> = OnceLock::new();
    BACKDROP.get_or_init(|| {
        let land = super::world_coastline::LAND
            .iter()
            .map(|ring| ring.to_vec())
            .collect::<Vec<_>>();
        geographic_mesh(
            Bounds {
                west: WEST,
                east: EAST,
                south: SOUTH,
                north: NORTH,
            },
            &land,
            &[],
        )
    })
}

/// Card bodies change only the transient preview, never the selected simulation region.
pub(super) fn focus_region(ctx: &egui::Context, choice: &super::menu::RegionChoice) {
    let Some(art) = choice.artwork.as_ref().filter(|_| choice.button.enabled) else {
        return;
    };
    let UiAction::NewGame { region, .. } = &choice.button.action else {
        return;
    };
    let now = ctx.input(|input| input.time);
    ctx.data_mut(|data| {
        let id = egui::Id::new("region_globe_motion");
        let mut spin = data.get_temp::<GlobeSpin>(id).unwrap_or_default();
        spin.focus(
            now,
            (art.outline.bounds.west + art.outline.bounds.east) * 0.5,
        );
        data.insert_temp(id, spin);
        data.insert_temp(egui::Id::new("region_globe_selected"), region.clone());
    });
    ctx.request_repaint();
}

/// Returns the region under the pointer so the cards and map can share a hover treatment.
pub(super) fn draw_globe(
    ui: &mut egui::Ui,
    model: &RegionSelectModel,
    card_hover: Option<usize>,
    actions: &mut Vec<UiAction>,
) -> Option<usize> {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, width / aspect()),
        egui::Sense::click_and_drag(),
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &model.map_label));
    let id = egui::Id::new("region_globe_motion");
    let mut spin = ui
        .ctx()
        .data_mut(|data| data.get_temp::<GlobeSpin>(id).unwrap_or_default());
    spin.advance(
        ui.input(|input| input.time),
        response.is_pointer_button_down_on() || response.dragged(),
        response.drag_delta().x,
        rect.width(),
    );
    let view = GlobeView::new(spin.longitude);
    ui.ctx().data_mut(|data| data.insert_temp(id, spin));
    ui.ctx().request_repaint();
    let hovered = response
        .hover_pos()
        .filter(|_| !response.dragged())
        .and_then(|pos| {
            let (lon, lat) = view.unproject(rect, pos)?;
            model.regions.iter().position(|choice| {
                choice.button.enabled
                    && choice
                        .artwork
                        .as_ref()
                        .is_some_and(|art| art.outline.contains(lon, lat))
            })
        });
    if let Some(index) = hovered
        && let Some(pos) = response.hover_pos()
        && let Some((lon, lat)) = view.unproject(rect, pos)
        && let Some(art) = &model.regions[index].artwork
        && let Some(area) = art
            .outline
            .areas
            .iter()
            .find(|a| a.polygons.iter().any(|p| point_in_ring(p, lon, lat)))
        && let Some(name) = model.regions[index].area_names.get(&area.name_key)
    {
        response.clone().on_hover_text(name);
    }
    let selected_id = egui::Id::new("region_globe_selected");
    if response.dragged() {
        ui.ctx()
            .data_mut(|data| data.remove::<RegionId>(selected_id));
    }
    let selected = ui
        .ctx()
        .data_mut(|data| data.get_temp::<RegionId>(selected_id));
    let selected = model.regions.iter().position(|choice| {
        choice.button.enabled
            && matches!(&choice.button.action,
            UiAction::NewGame { region, .. } if Some(region) == selected.as_ref())
    });
    let active = hovered.or(card_hover).or(selected);
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let radius = rect.width() * 0.5;
    painter.circle_filled(rect.center(), radius, OCEAN);
    for band in 0..48 {
        let fraction = band as f32 / 48.0;
        painter.circle_filled(
            rect.center(),
            radius * (1.0 - fraction),
            OCEAN.lerp_to_gamma(Color32::from_rgb(25, 51, 65), fraction),
        );
    }
    for lon in (-180..180).step_by(30) {
        graticule(
            &painter,
            rect,
            &view,
            (0..=180).map(|i| sphere(f64::from(lon), -90.0 + f64::from(i))),
        );
    }
    for lat in (-75..=75).step_by(15) {
        graticule(
            &painter,
            rect,
            &view,
            (0..=360).map(|i| sphere(-180.0 + f64::from(i), f64::from(lat))),
        );
    }
    paint_mesh(
        &painter,
        backdrop(),
        rect,
        &view,
        LAND,
        true,
        egui::Vec2::ZERO,
    );
    for (index, choice) in model.regions.iter().enumerate() {
        let Some(art) = &choice.artwork else {
            continue;
        };
        let base = colour(choice, index);
        let colour = if !choice.button.enabled {
            Color32::from_gray(85)
        } else if active == Some(index) || art.outline.atlas.is_some() {
            base
        } else {
            Color32::from_rgb(
                (u16::from(base.r()) * 2 / 3) as u8,
                (u16::from(base.g()) * 2 / 3) as u8,
                (u16::from(base.b()) * 2 / 3) as u8,
            )
        };
        if active == Some(index) {
            // Expand the union silhouette, then cover its interior. Ring-by-ring strokes would
            // expose the seams between overlapping land polygons and outline excluded ocean.
            for offset in [
                egui::vec2(-1.5, 0.0),
                egui::vec2(1.5, 0.0),
                egui::vec2(0.0, -1.5),
                egui::vec2(0.0, 1.5),
            ] {
                paint_mesh(
                    &painter,
                    &art.mesh,
                    rect,
                    &view,
                    Color32::WHITE,
                    false,
                    offset,
                );
            }
        }
        paint_mesh(
            &painter,
            &art.mesh,
            rect,
            &view,
            colour,
            active != Some(index),
            egui::Vec2::ZERO,
        );
    }
    for choice in &model.regions {
        if let Some(art) = &choice.artwork {
            for edge in &art.boundaries {
                let a = view.rotate(Vec3::from_array(edge[0]));
                let b = view.rotate(Vec3::from_array(edge[1]));
                if a.z > 0.0 && b.z > 0.0 {
                    painter.line_segment(
                        [view.screen(rect, a), view.screen(rect, b)],
                        Stroke::new(0.8, Color32::from_rgb(250, 220, 200)),
                    );
                }
            }
        }
    }
    painter.circle_stroke(
        rect.center(),
        radius,
        Stroke::new(1.5, Color32::from_rgb(66, 101, 120)),
    );
    // All geographic fills precede the markers, and the active title is the final overlay.
    // Otherwise a later region can cover a neighbouring region's marker or a title beside it.
    for choice in &model.regions {
        if let Some(art) = &choice.artwork
            && let Some((_, lon, lat)) = &art.outline.home
            && let Some(pos) = view.project(rect, *lon, *lat)
        {
            painter.circle_filled(pos, 7.0, theme::LABEL_BACKING);
            painter.circle_filled(pos, 3.5, Color32::WHITE);
        }
    }
    theme::paint_label(
        &painter,
        rect.left_bottom() + egui::vec2(18.0, -18.0),
        egui::Align2::LEFT_BOTTOM,
        &model.map_note,
        12.0,
    );
    if let Some(choice) = active.and_then(|index| model.regions.get(index))
        && let Some(art) = &choice.artwork
        && let Some((_, lon, lat)) = &art.outline.home
        && let Some(pos) = view.project(rect, *lon, *lat)
    {
        theme::paint_label(
            &painter,
            pos + egui::vec2(if pos.x > rect.center().x { -12.0 } else { 12.0 }, 0.0),
            if pos.x > rect.center().x {
                egui::Align2::RIGHT_CENTER
            } else {
                egui::Align2::LEFT_CENTER
            },
            &choice.button.label,
            13.0,
        );
    }
    ui.weak(&model.land_note);
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if let Some(index) = hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if response.clicked() {
            actions.push(model.regions[index].button.action.clone());
        }
    } else if response
        .hover_pos()
        .is_some_and(|at| view.unproject(rect, at).is_some())
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    active
}

pub(super) fn colour(choice: &super::menu::RegionChoice, index: usize) -> Color32 {
    choice
        .artwork
        .as_ref()
        .and_then(|art| art.outline.atlas.as_ref())
        .and_then(|atlas| atlas.colour.strip_prefix('#'))
        .filter(|hex| hex.len() == 6 && hex.is_ascii())
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .map(|rgb| Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8))
        .unwrap_or(COLOURS[index % COLOURS.len()])
}

fn point_in_ring(ring: &[(f64, f64)], x: f64, y: f64) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = ring[ring.len() - 1];
    for &(bx, by) in ring {
        let (ax, ay) = previous;
        if (ay > y) != (by > y) && x < (bx - ax) * (y - ay) / (by - ay) + ax {
            inside = !inside;
        }
        previous = (bx, by);
    }
    inside
}
fn area_boundaries(region: &domain::region::RegionDefinition) -> Vec<[[f32; 3]; 2]> {
    if region.areas().is_none() {
        return Vec::new();
    }
    let grid = region.terrain();
    let geo = region.geo();
    let point = |x: f64, y: f64| {
        sphere(
            geo.west + x / f64::from(grid.width()) * (geo.east - geo.west),
            geo.north - y / f64::from(grid.height()) * (geo.north - geo.south),
        )
        .to_array()
    };
    let mut edges = Vec::new();
    for y in 0..grid.height() {
        for x in 0..grid.width() {
            let cell = domain::ids::CellCoord { x, y };
            let id = region.area_at(cell);
            if id.0 == 0 {
                continue;
            }
            for (next, a, b) in [
                (
                    domain::ids::CellCoord { x: x + 1, y },
                    (f64::from(x) + 1.0, f64::from(y)),
                    (f64::from(x) + 1.0, f64::from(y) + 1.0),
                ),
                (
                    domain::ids::CellCoord { x, y: y + 1 },
                    (f64::from(x), f64::from(y) + 1.0),
                    (f64::from(x) + 1.0, f64::from(y) + 1.0),
                ),
            ] {
                if grid.get(next).is_some()
                    && region.area_at(next).0 != 0
                    && region.area_at(next) != id
                {
                    edges.push([point(a.0, a.1), point(b.0, b.1)]);
                }
            }
        }
    }
    edges
}
