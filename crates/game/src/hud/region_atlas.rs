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

const WEST: f64 = -168.0;
const EAST: f64 = -52.0;
const NORTH: f64 = 74.0;
const SOUTH: f64 = 12.0;
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
    mesh: Vec<[[f32; 2]; 2]>,
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

fn project(rect: Rect, lon: f64, lat: f64) -> Pos2 {
    rect.min
        + egui::vec2(
            ((lon - WEST) / (EAST - WEST)) as f32 * rect.width(),
            ((NORTH - lat) / (NORTH - SOUTH)) as f32 * rect.height(),
        )
}

fn unproject(rect: Rect, pos: Pos2) -> (f64, f64) {
    (
        WEST + f64::from((pos.x - rect.left()) / rect.width()) * (EAST - WEST),
        NORTH - f64::from((pos.y - rect.top()) / rect.height()) * (NORTH - SOUTH),
    )
}

// Horizontal strips handle concavity, overlapping land rings and excluded holes without a new
// triangulation dependency. Only cached geometry is sampled; pointer hit tests use exact rings.
fn mesh(outline: &RegionOutline) -> Vec<[[f32; 2]; 2]> {
    let mut mesh = Vec::new();
    let unit = Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
    let step = (NORTH - SOUTH) / ROWS as f64;
    for row in 0..ROWS {
        let lat = NORTH - (row as f64 + 0.5) * step;
        if lat < outline.bounds.south || lat > outline.bounds.north {
            continue;
        }
        let mut xs = vec![outline.bounds.west.max(WEST), outline.bounds.east.min(EAST)];
        for ring in outline.land.iter().chain(&outline.exclude) {
            if let Some(mut prev) = ring.last().copied() {
                for &next in ring {
                    if (prev.1 > lat) != (next.1 > lat) {
                        let x = prev.0 + (lat - prev.1) / (next.1 - prev.1) * (next.0 - prev.0);
                        if x > xs[0] && x < xs[1] {
                            xs.push(x);
                        }
                    }
                    prev = next;
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        for pair in xs.windows(2) {
            if outline.contains((pair[0] + pair[1]) * 0.5, lat) {
                mesh.push([
                    project(unit, pair[0], (lat + step * 0.5).min(outline.bounds.north)).into(),
                    project(unit, pair[1], (lat - step * 0.5).max(outline.bounds.south)).into(),
                ]);
            }
        }
    }
    mesh
}

fn paint_mesh(painter: &egui::Painter, strips: &[[[f32; 2]; 2]], rect: Rect, colour: Color32) {
    let mut mesh = egui::Mesh::default();
    for strip in strips {
        let [min, max] = *strip;
        mesh.add_colored_rect(
            Rect::from_min_max(
                rect.min + egui::vec2(min[0], min[1]) * rect.size(),
                rect.min + egui::vec2(max[0], max[1]) * rect.size(),
            ),
            colour,
        );
    }
    painter.add(egui::Shape::mesh(mesh));
}

// An intentionally simplified context silhouette, authored for this overview, not used for
// gameplay or picking. Selectable regions always come from their authoritative region files.
fn backdrop() -> &'static Vec<[[f32; 2]; 2]> {
    static BACKDROP: OnceLock<Vec<[[f32; 2]; 2]>> = OnceLock::new();
    BACKDROP.get_or_init(|| {
        mesh(&RegionOutline {
            atlas: None,
            bounds: Bounds {
                west: WEST,
                east: EAST,
                south: SOUTH,
                north: NORTH,
            },
            land: vec![vec![
                (-168., 66.),
                (-164., 71.),
                (-155., 72.),
                (-142., 69.),
                (-130., 70.),
                (-122., 72.),
                (-109., 70.),
                (-96., 74.),
                (-84., 70.),
                (-78., 64.),
                (-65., 60.),
                (-59., 55.),
                (-53., 52.),
                (-57., 48.),
                (-64., 46.),
                (-67., 45.),
                (-70., 43.),
                (-74., 40.),
                (-76., 35.),
                (-81., 31.),
                (-80., 26.),
                (-81., 25.),
                (-83., 29.),
                (-90., 29.),
                (-97., 26.),
                (-97., 22.),
                (-91., 18.),
                (-87., 21.),
                (-86., 16.),
                (-83., 15.),
                (-85., 12.),
                (-89., 14.),
                (-95., 16.),
                (-103., 18.),
                (-106., 23.),
                (-110., 28.),
                (-110., 24.),
                (-112., 23.),
                (-115., 28.),
                (-117., 33.),
                (-121., 35.),
                (-124., 40.),
                (-125., 48.),
                (-128., 52.),
                (-135., 58.),
                (-141., 60.),
                (-150., 60.),
                (-154., 57.),
                (-162., 55.),
                (-158., 59.),
                (-166., 61.),
                (-168., 66.),
            ]],
            exclude: vec![vec![
                (-96., 63.),
                (-89., 65.),
                (-83., 63.),
                (-78., 58.),
                (-80., 53.),
                (-82., 52.),
                (-82., 55.),
                (-88., 57.),
                (-94., 60.),
            ]],
            home: None,
            cities: 0,
        })
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
        ui.allocate_exact_size(egui::vec2(width, width / 1.6), egui::Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &model.map_title));
    let hovered = response.hover_pos().and_then(|pos| {
        let (lon, lat) = unproject(rect, pos);
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
    for lon in (-160..=-60).step_by(20) {
        painter.line_segment(
            [
                project(rect, f64::from(lon), SOUTH),
                project(rect, f64::from(lon), NORTH),
            ],
            Stroke::new(0.5, Color32::from_rgb(26, 43, 54)),
        );
    }
    for lat in (20..=70).step_by(10) {
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
        let base = COLOURS[index % COLOURS.len()];
        let colour = if !choice.button.enabled {
            Color32::from_gray(85)
        } else if active == Some(index) {
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
        rect.left_top() + egui::vec2(18.0, 18.0),
        egui::Align2::LEFT_TOP,
        &model.map_title,
        16.0,
    );
    theme::paint_label(
        &painter,
        rect.left_bottom() + egui::vec2(18.0, -18.0),
        egui::Align2::LEFT_BOTTOM,
        &model.map_note,
        12.0,
    );
    if let Some(index) = hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if response.clicked() {
            actions.push(model.regions[index].button.action.clone());
        }
    }
    active
}

pub(super) fn colour(index: usize) -> Color32 {
    COLOURS[index % COLOURS.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn projection_round_trips_at_any_window_size(
            lon in WEST..EAST, lat in SOUTH..NORTH,
            width in 300.0_f32..2000.0, height in 200.0_f32..1200.0,
        ) {
            let rect = Rect::from_min_size(egui::pos2(20.0, 45.0), egui::vec2(width, height));
            let (got_lon, got_lat) = unproject(rect, project(rect, lon, lat));
            prop_assert!((got_lon - lon).abs() < 0.00003);
            prop_assert!((got_lat - lat).abs() < 0.00003);
        }
    }
}
