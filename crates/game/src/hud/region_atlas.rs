//! Cached geographic artwork for the menu. No session or simulation state lives here.
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

use application::RegionCatalog;
use bevy::prelude::Resource;
use bevy_egui::egui::{self, Color32, Pos2, Rect, Stroke};
use domain::ids::RegionId;
use infrastructure::region::{Bounds, RegionOutline, region_outline};

use super::{RegionListings, RegionSelectModel, UiAction, theme};
use crate::l10n::{FluentArg, Localizer};

const WEST: f64 = -180.0;
const EAST: f64 = 180.0;
const NORTH: f64 = 75.0;
const SOUTH: f64 = -60.0;
const ROWS: usize = 1000;
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
    mesh: Vec<[[f32; 2]; 4]>,
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
                    regions.insert(listing.id.clone(), Arc::new(Artwork { outline, mesh }));
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

// Spherical Equal Earth on a unit-radius sphere. Inputs/outputs use degrees at the API
// boundary. Equations: Šavrič, Patterson & Jenny (2018); PROJ eqearth.cpp (see provenance).
const A1: f64 = 1.340264;
const A2: f64 = -0.081106;
const A3: f64 = 0.000893;
const A4: f64 = 0.003796;
const M: f64 = 0.8660254037844386;
const MAX_Y: f64 = 1.3173627591574;
const MAX_X: f64 = 2.7066299836960748;

fn derivative(theta: f64) -> f64 {
    let t2 = theta * theta;
    A1 + 3.0 * A2 * t2 + t2.powi(3) * (7.0 * A3 + 9.0 * A4 * t2)
}

fn equal_earth(lon: f64, lat: f64) -> (f64, f64) {
    let theta = (M * lat.to_radians().sin()).asin();
    let t2 = theta * theta;
    (
        lon.to_radians() * theta.cos() / (M * derivative(theta)),
        theta * (A1 + A2 * t2 + t2.powi(3) * (A3 + A4 * t2)),
    )
}

/// Reject points outside the curved Earth footprint, including non-finite pointer input.
fn equal_earth_inverse(x: f64, y: f64) -> Option<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() || y.abs() > MAX_Y + 1e-12 {
        return None;
    }
    let mut theta = y;
    for _ in 0..10 {
        let t2 = theta * theta;
        let residual = theta * (A1 + A2 * t2 + t2.powi(3) * (A3 + A4 * t2)) - y;
        let delta = residual / derivative(theta);
        theta -= delta;
        if delta.abs() < 1e-12 {
            let sin_lat = theta.sin() / M;
            let lon = (M * x * derivative(theta) / theta.cos()).to_degrees();
            if sin_lat.abs() > 1.0 + 1e-12 || lon.abs() > 180.0 + 1e-9 {
                return None;
            }
            return Some((
                lon.clamp(-180.0, 180.0),
                sin_lat.clamp(-1.0, 1.0).asin().to_degrees(),
            ));
        }
    }
    None
}

pub(super) fn aspect() -> f32 {
    (2.0 * MAX_X / (equal_earth(0.0, NORTH).1 - equal_earth(0.0, SOUTH).1)) as f32
}

fn project(rect: Rect, lon: f64, lat: f64) -> Pos2 {
    let (x, y) = equal_earth(lon, lat);
    let north = equal_earth(0.0, NORTH).1;
    let south = equal_earth(0.0, SOUTH).1;
    rect.min
        + egui::vec2(
            ((x + MAX_X) / (2.0 * MAX_X)) as f32 * rect.width(),
            ((north - y) / (north - south)) as f32 * rect.height(),
        )
}

fn unproject(rect: Rect, pos: Pos2) -> Option<(f64, f64)> {
    let north = equal_earth(0.0, NORTH).1;
    let south = equal_earth(0.0, SOUTH).1;
    let (lon, lat) = equal_earth_inverse(
        f64::from((pos.x - rect.left()) / rect.width()) * 2.0 * MAX_X - MAX_X,
        north - f64::from((pos.y - rect.top()) / rect.height()) * (north - south),
    )?;
    (SOUTH - 1e-6..=NORTH + 1e-6)
        .contains(&lat)
        .then_some((lon, lat))
}

// Scanline union/subtraction is independent of the playable grid size. Project all four
// corners of each strip so the curved meridians remain aligned with inverse picking.
fn mesh(outline: &RegionOutline) -> Vec<[[f32; 2]; 4]> {
    geographic_mesh(outline.bounds, &outline.land, &outline.exclude)
}

fn geographic_mesh(
    bounds: Bounds,
    land_rings: &[Vec<(f64, f64)>],
    excluded_rings: &[Vec<(f64, f64)>],
) -> Vec<[[f32; 2]; 4]> {
    let mut strips = Vec::new();
    let unit = Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
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
                strips.push([
                    project(unit, west, top).into(),
                    project(unit, x, top).into(),
                    project(unit, x, bottom).into(),
                    project(unit, west, bottom).into(),
                ]);
            }
        }
    }
    strips
}

fn paint_mesh(painter: &egui::Painter, strips: &[[[f32; 2]; 4]], rect: Rect, colour: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.vertices.reserve(strips.len() * 4);
    mesh.indices.reserve(strips.len() * 6);
    for strip in strips {
        let base = mesh.vertices.len() as u32;
        for point in strip {
            mesh.colored_vertex(
                rect.min + egui::vec2(point[0], point[1]) * rect.size(),
                colour,
            );
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
}

fn backdrop() -> &'static Vec<[[f32; 2]; 4]> {
    static BACKDROP: OnceLock<Vec<[[f32; 2]; 4]>> = OnceLock::new();
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

/// Returns the region under the pointer so the cards and map can share a hover treatment.
pub(super) fn draw_map(
    ui: &mut egui::Ui,
    model: &RegionSelectModel,
    card_hover: Option<usize>,
    actions: &mut Vec<UiAction>,
) -> Option<usize> {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, width / aspect()), egui::Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &model.map_label));
    let hovered = response.hover_pos().and_then(|pos| {
        let (lon, lat) = unproject(rect, pos)?;
        model.regions.iter().position(|choice| {
            choice.button.enabled
                && choice
                    .artwork
                    .as_ref()
                    .is_some_and(|art| art.outline.contains(lon, lat))
        })
    });
    let active = hovered.or(card_hover);
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect_filled(rect, 12.0, OCEAN);
    for lon in (-180..=180).step_by(30) {
        let points = (0..=54)
            .map(|i| {
                project(
                    rect,
                    f64::from(lon),
                    SOUTH + f64::from(i) * (NORTH - SOUTH) / 54.0,
                )
            })
            .collect();
        painter.add(egui::Shape::line(
            points,
            Stroke::new(0.5, Color32::from_rgb(26, 43, 54)),
        ));
    }
    for lat in (-60..=75).step_by(15) {
        painter.line_segment(
            [
                project(rect, WEST, f64::from(lat)),
                project(rect, EAST, f64::from(lat)),
            ],
            Stroke::new(0.5, Color32::from_rgb(26, 43, 54)),
        );
    }
    paint_mesh(&painter, backdrop(), rect, LAND);
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
                paint_mesh(&painter, &art.mesh, rect.translate(offset), Color32::WHITE);
            }
        }
        paint_mesh(&painter, &art.mesh, rect, colour);
        if let Some((_, lon, lat)) = &art.outline.home {
            let pos = project(rect, *lon, *lat);
            painter.circle_filled(pos, 7.0, theme::LABEL_BACKING);
            painter.circle_filled(pos, 3.5, Color32::WHITE);
            if active == Some(index) {
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
        }
    }
    theme::paint_label(
        &painter,
        rect.left_bottom() + egui::vec2(18.0, -18.0),
        egui::Align2::LEFT_BOTTOM,
        &model.map_note,
        12.0,
    );
    ui.weak(&model.land_note);
    if let Some(index) = hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if response.clicked() {
            actions.push(model.regions[index].button.action.clone());
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn equal_earth_matches_published_values() {
        for (lon, lat, expected) in [
            (0.0, 0.0, (0.0, 0.0)),
            (180.0, 0.0, (2.7066299836960748, 0.0)),
            (0.0, 90.0, (0.0, 1.3173627591574)),
        ] {
            let (x, y) = equal_earth(lon, lat);
            assert!((x - expected.0).abs() < 1e-12 && (y - expected.1).abs() < 1e-12);
        }
    }

    #[test]
    fn equal_earth_inverse_rejects_outside_the_curved_earth_and_nonfinite_input() {
        for (x, y) in [
            (3.0, 0.0),
            (2.7, 1.2),
            (0.0, 1.4),
            (f64::NAN, 0.0),
            (0.0, f64::INFINITY),
        ] {
            assert!(equal_earth_inverse(x, y).is_none());
        }
    }

    proptest! {
        #[test]
        fn equal_earth_round_trips(lon in WEST..EAST, lat in SOUTH..NORTH) {
            let (x, y) = equal_earth(lon, lat);
            let (got_lon, got_lat) = equal_earth_inverse(x, y).unwrap();
            prop_assert!((got_lon - lon).abs() < 1e-6);
            prop_assert!((got_lat - lat).abs() < 1e-6);
        }
        #[test]
        fn projection_round_trips_at_any_window_size(
            lon in -179.9_f64..179.9, lat in -59.9_f64..74.9,
            width in 300.0_f32..2000.0, height in 200.0_f32..1200.0,
        ) {
            let rect = Rect::from_min_size(egui::pos2(20.0, 45.0), egui::vec2(width, height));
            let (got_lon, got_lat) = unproject(rect, project(rect, lon, lat)).unwrap();
            prop_assert!((got_lon - lon).abs() < 0.0001);
            prop_assert!((got_lat - lat).abs() < 0.0001);
        }
    }
}
