//! The cell inspector: what a selected map cell is like, and which plants could be built on it.

use application::Game;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::ids::CellCoord;
use domain::region::{PlantKind, RegionDefinition};

use super::action::{ActionSink, ButtonModel, UiAction, button, dispatch};
use super::layout::HudArea;
use super::selection::Selection;
use super::top_bar::Stat;
use crate::l10n::{Localize, Localizer, format};
use crate::sim::Session;

/// One plant kind's verdict on the selected cell.
#[derive(Debug, Clone, PartialEq)]
pub struct SuitabilityRow {
    pub kind: PlantKind,
    /// The kind's name.
    pub label: String,
    /// A tick when the plant can be built here, or why it cannot.
    pub verdict: String,
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InspectorModel {
    /// `Cell (12, 34)`.
    pub title: String,
    /// `Land` or `Water`.
    pub terrain: String,
    /// Height, sunshine, cooling water and hydro potential, each a percentage.
    pub readings: Vec<Stat>,
    /// The heading over the suitability rows.
    pub suitability_heading: String,
    /// One row per plant kind, in `PlantKind::ALL` order.
    pub suitability: Vec<SuitabilityRow>,
    /// Clears the selection.
    pub close: ButtonModel,
}

/// The inspector for the selected cell; `None` unless a cell on the map is selected.
pub fn inspector_model(
    game: &Game,
    l10n: &Localizer,
    selection: Selection,
) -> Option<InspectorModel> {
    let Selection::Cell(at) = selection else {
        return None;
    };
    let region = game.session().region();
    let cell = region.terrain().get(at)?;
    let reading = |key: &str, value| Stat {
        label: l10n.text(key),
        value: format::percent(value),
    };
    Some(InspectorModel {
        title: l10n.args(
            "inspector-title",
            &[
                ("x", at.x.to_string().into()),
                ("y", at.y.to_string().into()),
            ],
        ),
        terrain: l10n.text(if cell.land {
            "inspector-land"
        } else {
            "inspector-water"
        }),
        readings: vec![
            reading("inspector-height", cell.height),
            reading("inspector-irradiance", cell.irradiance),
            reading("inspector-cooling-water", cell.cooling_water),
            reading("inspector-hydro-potential", cell.hydro_potential),
        ],
        suitability_heading: l10n.text("inspector-can-build"),
        suitability: PlantKind::ALL
            .into_iter()
            .map(|kind| suitability_row(region, kind, at, l10n))
            .collect(),
        close: ButtonModel::new(l10n.text("ui-close"), UiAction::Select(Selection::None)),
    })
}

/// One kind's verdict on `at`, from the region's siting rule.
pub fn suitability_row(
    region: &RegionDefinition,
    kind: PlantKind,
    at: CellCoord,
    l10n: &Localizer,
) -> SuitabilityRow {
    let site = region.site_check(kind, at);
    SuitabilityRow {
        kind,
        label: kind.localize(l10n),
        verdict: match &site {
            Ok(()) => l10n.text("inspector-site-ok"),
            Err(error) => error.localize(l10n),
        },
        ok: site.is_ok(),
    }
}

pub fn draw_inspector(ui: &mut egui::Ui, model: &InspectorModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.heading(model.title.as_str());
    ui.label(model.terrain.as_str());
    egui::Grid::new("inspector_readings").show(ui, |ui| {
        for stat in &model.readings {
            ui.weak(stat.label.as_str());
            ui.label(stat.value.as_str());
            ui.end_row();
        }
    });
    ui.separator();
    ui.strong(model.suitability_heading.as_str());
    egui::Grid::new("inspector_suitability").show(ui, |ui| {
        for row in &model.suitability {
            ui.label(row.label.as_str());
            if row.ok {
                ui.colored_label(egui::Color32::from_rgb(90, 190, 110), row.verdict.as_str());
            } else {
                ui.weak(row.verdict.as_str());
            }
            ui.end_row();
        }
    });
    ui.separator();
    button(ui, &model.close, &mut actions);
    actions
}

pub(super) fn inspector_panel(
    mut contexts: EguiContexts,
    mut area: ResMut<HudArea>,
    session: Res<Session>,
    l10n: Res<Localizer>,
    mut sink: ActionSink,
) -> Result {
    let Some(model) = inspector_model(&session.0, &l10n, sink.selection()) else {
        return Ok(());
    };
    let mut ui = area.root_ui(contexts.ctx_mut()?, "inspector");
    let actions = egui::Panel::left("inspector")
        .default_size(260.0)
        .show(&mut ui, |ui| draw_inspector(ui, &model))
        .inner;
    area.claim(&ui);
    dispatch(actions, &mut sink);
    Ok(())
}

#[cfg(test)]
mod tests {
    use application::InMemoryCommandLog;
    use domain::region::SiteError;

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_l10n};
    use crate::l10n::format;

    fn cells(region: &RegionDefinition) -> impl Iterator<Item = CellCoord> {
        let terrain = region.terrain();
        let (w, h) = (terrain.width(), terrain.height());
        (0..h).flat_map(move |y| (0..w).map(move |x| CellCoord { x, y }))
    }

    #[test]
    fn suitability_row_reports_site_errors() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let region = game.session().region();

        // Open water: nothing can be built, and each row says so in words.
        let water = cells(region)
            .find(|at| !region.terrain().get(*at).unwrap().land)
            .expect("the region has water");
        for kind in PlantKind::ALL {
            let row = suitability_row(region, kind, water, &l10n);
            assert_eq!(row.kind, kind);
            assert_eq!(row.label, kind.localize(&l10n));
            assert_eq!(row.verdict, SiteError::NotLand.localize(&l10n));
            assert!(row.verdict.contains("water"), "{}", row.verdict);
            assert!(!row.ok);
        }

        // Dry land that fails the cooling-water rule names the shortfall.
        let dry = cells(region)
            .find(|at| {
                matches!(
                    region.site_check(PlantKind::Nuclear, *at),
                    Err(SiteError::InsufficientCoolingWater { .. })
                )
            })
            .expect("some land is too dry for a nuclear plant");
        let row = suitability_row(region, PlantKind::Nuclear, dry, &l10n);
        let Err(error) = region.site_check(PlantKind::Nuclear, dry) else {
            unreachable!()
        };
        assert_eq!(row.verdict, error.localize(&l10n));
        assert!(row.verdict.contains("Cooling water"), "{}", row.verdict);
        assert!(!row.ok);

        // Land with no river to dam can't take hydro, but a peaker goes anywhere on land.
        let no_river = cells(region)
            .find(|at| region.site_check(PlantKind::Hydro, *at) == Err(SiteError::NoHydroSite))
            .expect("some land has no hydro site");
        assert_eq!(
            suitability_row(region, PlantKind::Hydro, no_river, &l10n).verdict,
            SiteError::NoHydroSite.localize(&l10n)
        );
        let peaker = suitability_row(region, PlantKind::Peaker, no_river, &l10n);
        assert_eq!(peaker.verdict, l10n.text("inspector-site-ok"));
        assert_eq!(peaker.verdict, "✓");
        assert!(peaker.ok);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_inspector_describes_the_selected_cell() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let region = game.session().region();
        let at = cells(region)
            .find(|at| region.terrain().get(*at).unwrap().land)
            .unwrap();
        let terrain_cell = region.terrain().get(at).unwrap();

        let model = inspector_model(&game, &l10n, Selection::Cell(at)).expect("a cell is selected");

        assert_eq!(model.title, format!("Cell ({}, {})", at.x, at.y));
        assert_eq!(model.terrain, l10n.text("inspector-land"));
        let readings: Vec<(&str, &str)> = model
            .readings
            .iter()
            .map(|s| (s.label.as_str(), s.value.as_str()))
            .collect();
        assert_eq!(
            readings,
            [
                ("Height", format::percent(terrain_cell.height).as_str()),
                (
                    "Sunshine",
                    format::percent(terrain_cell.irradiance).as_str()
                ),
                (
                    "Cooling water",
                    format::percent(terrain_cell.cooling_water).as_str()
                ),
                (
                    "Hydro potential",
                    format::percent(terrain_cell.hydro_potential).as_str()
                ),
            ]
        );
        assert_eq!(model.suitability_heading, l10n.text("inspector-can-build"));
        let kinds: Vec<PlantKind> = model.suitability.iter().map(|row| row.kind).collect();
        assert_eq!(kinds, PlantKind::ALL);
        for row in &model.suitability {
            assert_eq!(row, &suitability_row(region, row.kind, at, &l10n));
        }
        assert_eq!(model.close.action, UiAction::Select(Selection::None));
        assert_eq!(l10n.problems(), vec![]);

        // Water is called water.
        let water = cells(region)
            .find(|at| !region.terrain().get(*at).unwrap().land)
            .unwrap();
        let model = inspector_model(&game, &l10n, Selection::Cell(water)).unwrap();
        assert_eq!(model.terrain, l10n.text("inspector-water"));
    }

    #[test]
    fn only_a_cell_on_the_map_has_an_inspector() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let plant = session.plant_views()[0].id;
        let city = session.city_views()[0].id;
        let outside = CellCoord { x: 5000, y: 5000 };

        for selection in [
            Selection::None,
            Selection::Plant(plant),
            Selection::City(city),
            Selection::LineFrom(session.substation()),
            Selection::Cell(outside),
        ] {
            assert_eq!(
                inspector_model(&game, &l10n, selection),
                None,
                "{selection:?}"
            );
        }
        assert_eq!(l10n.problems(), vec![]);
    }
}
