//! The Planning inspector: selected cells, cities, plants and lines, with construction progress.

use crate::l10n::format::MoneyStyle;
use application::Game;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::grid::LineStatus;
use domain::ids::{CellCoord, LineId};
use domain::region::PlantKind;
use domain::session::{DayPhase, GameSession};

use super::action::{ActionSink, ButtonModel, Prompt, UiAction, button, dispatch};
use super::build::BUILD_PANEL;
use super::city_card::{CityCardModel, city_card_model, draw_city_card};
use super::construction::{
    ConstructionCardModel, Project, construction_card_model, construction_model,
    draw_construction_card,
};
use super::layout::{Edge, HudArea, OuterPanel};
use super::plant_panel::{PlantCardModel, draw_plant_card, plant_card_model};
use super::selection::Selection;
use super::theme::{TextTone, heading_color};
use super::top_bar::Stat;
use crate::l10n::{Localize, Localizer, Names, format};
use crate::sim::Session;

/// The inspector's width, fixed so the map beside it never moves.
pub const INSPECTOR_PANEL_WIDTH: f32 = 260.0;
/// The inspector as an outer panel: on the left, resizable from [`INSPECTOR_PANEL_WIDTH`].
pub const INSPECTOR_PANEL: OuterPanel = OuterPanel {
    id: "inspector",
    edge: Edge::Left,
    size: INSPECTOR_PANEL_WIDTH,
};

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

/// What the inspector says about one cell.
#[derive(Debug, Clone, PartialEq)]
pub struct CellInspector {
    /// `Cell (12, 34)`.
    pub title: String,
    /// `Land` or `Water`.
    pub terrain: String,
    /// Height, sunshine, wind potential, cooling water and hydro potential, each a percentage.
    pub readings: Vec<Stat>,
    /// The heading over the suitability rows.
    pub suitability_heading: String,
    /// One row per plant kind, in `PlantKind::ALL` order.
    pub suitability: Vec<SuitabilityRow>,
    /// Clears the selection.
    pub close: ButtonModel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InspectorModel {
    Line(LineInspector),
    /// A cell on the map is selected.
    Cell(CellInspector),
    /// A city is selected (spec §16.8).
    City(CityCardModel),
    /// A plant under construction is selected (spec §16.9).
    Construction(ConstructionCardModel),
    /// A commissioned plant's connection and specifications.
    Plant(PlantCardModel),
    /// Nothing or an unknown object is selected. The panel keeps its place, so the map does not move.
    Empty {
        hint: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LineInspector {
    pub construction: Vec<super::plant_panel::PlantStat>,
    pub title: String,
    pub status: String,
    pub submarine: Option<String>,
    pub transfer_limits: String,
    pub in_service: Option<String>,
    pub cancel: Option<ButtonModel>,
    pub close: ButtonModel,
}

fn line_inspector(
    game: &Game,
    l10n: &Localizer,
    id: LineId,
    style: &MoneyStyle,
) -> Option<LineInspector> {
    let line = game
        .session()
        .line_views()
        .into_iter()
        .find(|line| line.id == id)?;
    let progress = construction_model(game, l10n)
        .rows
        .into_iter()
        .find(|row| row.project == Project::Line(id));
    let cells: std::collections::BTreeMap<_, _> = game
        .session()
        .city_views()
        .iter()
        .map(|c| (c.node, c.cell))
        .chain(
            game.session()
                .plant_views()
                .iter()
                .map(|p| (p.node, p.cell)),
        )
        .collect();
    let water = cells
        .get(&line.from)
        .zip(cells.get(&line.to))
        .map_or(0, |(a, b)| {
            game.session()
                .region()
                .terrain()
                .water_cells_on_segment(*a, *b)
                .len()
        });
    let construction = if game.session().region().has_economic_zones() {
        let quote = game.session().line_construction(id)?;
        let mut stats = super::build::construction_stats(
            game.session().region(),
            l10n,
            quote.capital,
            quote.months,
            style,
        );
        stats.extend(super::economics::route_stats(
            game.session().region(),
            &quote,
            l10n,
            style,
        ));
        stats
    } else {
        Vec::new()
    };
    Some(LineInspector {
        construction,
        submarine: (water > 0)
            .then(|| l10n.args("line-submarine", &[("cells", (water as u32).into())])),
        transfer_limits: l10n.text("line-copper-plate"),
        title: Names::new(l10n, game.session()).line(id),
        status: progress
            .as_ref()
            .map_or_else(|| l10n.text("line-in-service"), |row| row.months.clone()),
        in_service: progress.map(|row| row.in_service_text),
        cancel: matches!(line.status, LineStatus::UnderConstruction { .. }).then(|| {
            ButtonModel::new(
                l10n.text("line-cancel-construction"),
                UiAction::Prompt(Some(Prompt::CancelLineConstruction { line: id })),
            )
        }),
        close: ButtonModel::new(l10n.text("ui-close"), UiAction::Select(Selection::None)),
    })
}

/// The inspector (Planning only, spec §8.4): the selected cell, city, plant or line, or a hint to select
/// a cell. `None` outside Planning.
pub fn inspector_model(
    game: &Game,
    l10n: &Localizer,
    selection: Selection,
    style: &MoneyStyle,
) -> Option<InspectorModel> {
    if game.session().phase() != DayPhase::Planning {
        return None;
    }
    let model = match selection {
        Selection::Line(id) => line_inspector(game, l10n, id, style).map(InspectorModel::Line),
        Selection::Cell(at) => cell_inspector(game, l10n, at).map(InspectorModel::Cell),
        Selection::City(city) => city_card_model(game, l10n, city).map(InspectorModel::City),
        Selection::Plant(plant) => construction_card_model(game, l10n, plant, style)
            .map(InspectorModel::Construction)
            .or_else(|| plant_card_model(game, l10n, plant, style).map(InspectorModel::Plant)),
        _ => None,
    };
    Some(model.unwrap_or_else(|| InspectorModel::Empty {
        hint: l10n.text("inspector-select-cell"),
    }))
}

fn cell_inspector(game: &Game, l10n: &Localizer, at: CellCoord) -> Option<CellInspector> {
    let region = game.session().region();
    let cell = region.terrain().get(at)?;
    let reading = |key: &str, value| Stat {
        label: l10n.text(key),
        value: format::percent(value),
    };
    let market = region.has_economic_zones().then(|| {
        super::economics::zone_stat(
            region,
            region.economic_zone_at(at).expect("known cell"),
            l10n,
        )
    });
    let mut readings = vec![
        reading("inspector-height", cell.height),
        reading("inspector-irradiance", region.solar_potential(at)),
        reading("wind-potential", cell.wind_potential),
        reading("geothermal-potential", cell.geothermal_potential),
        reading("inspector-cooling-water", cell.cooling_water),
        reading("inspector-hydro-potential", cell.hydro_potential),
    ];
    if let Some(market) = market {
        readings.insert(
            0,
            Stat {
                label: market.label,
                value: market.value,
            },
        );
    }
    Some(CellInspector {
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
        readings,
        suitability_heading: l10n.text("inspector-can-build"),
        suitability: PlantKind::ALL
            .into_iter()
            .map(|kind| suitability_row(game.session(), kind, at, l10n))
            .collect(),
        close: ButtonModel::new(l10n.text("ui-close"), UiAction::Select(Selection::None)),
    })
}

/// One kind's verdict on `at`, including the session's current land occupancy.
pub fn suitability_row(
    session: &GameSession,
    kind: PlantKind,
    at: CellCoord,
    l10n: &Localizer,
) -> SuitabilityRow {
    let site = session.plant_site_check(kind, at);
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
    // The panel is only as tall as the window leaves it below the Planning panel, which at the
    // smallest window is less than a selected cell's readings and suitability rows.
    egui::ScrollArea::vertical()
        .id_salt((
            "inspector_scroll",
            match model {
                InspectorModel::Plant(card) => Some(card.plant.0),
                InspectorModel::Construction(card) => Some(card.plant.0),
                _ => None,
            },
        ))
        .auto_shrink([false, false])
        .show(ui, |ui| match model {
            InspectorModel::Line(line) => {
                ui.heading(egui::RichText::new(&line.title).color(heading_color()));
                let tone = if line.in_service.is_some() {
                    TextTone::Information
                } else {
                    TextTone::Positive
                };
                ui.colored_label(tone.color(), &line.status);
                if let Some(in_service) = &line.in_service {
                    ui.colored_label(TextTone::Caution.color(), in_service);
                }
                if let Some(submarine) = &line.submarine {
                    ui.colored_label(TextTone::Information.color(), submarine);
                }
                for stat in &line.construction {
                    ui.colored_label(TextTone::Neutral.color(), &stat.label);
                    ui.colored_label(stat.color.color(), &stat.value);
                }
                ui.colored_label(TextTone::Neutral.color(), &line.transfer_limits);
                if let Some(cancel) = &line.cancel {
                    button(ui, cancel, &mut actions);
                }
                button(ui, &line.close, &mut actions);
            }
            InspectorModel::Empty { hint } => {
                ui.weak(hint.as_str());
            }
            InspectorModel::Cell(cell) => draw_cell(ui, cell, &mut actions),
            InspectorModel::City(city) => draw_city_card(ui, city),
            InspectorModel::Construction(card) => draw_construction_card(ui, card),
            InspectorModel::Plant(card) => actions.extend(draw_plant_card(ui, card)),
        });
    actions
}

fn draw_cell(ui: &mut egui::Ui, model: &CellInspector, actions: &mut Vec<UiAction>) {
    ui.heading(egui::RichText::new(&model.title).color(heading_color()));
    ui.colored_label(TextTone::Information.color(), &model.terrain);
    egui::Grid::new("inspector_readings")
        .max_col_width((ui.available_width() - ui.spacing().item_spacing.x) / 2.0)
        .show(ui, |ui| {
            for stat in &model.readings {
                ui.colored_label(TextTone::Neutral.color(), &stat.label);
                ui.colored_label(TextTone::Information.color(), &stat.value);
                ui.end_row();
            }
        });
    ui.separator();
    ui.label(
        egui::RichText::new(&model.suitability_heading)
            .strong()
            .color(heading_color()),
    );
    for row in &model.suitability {
        // Wrapped, not a grid: a siting error is a sentence, and the panel is a fixed width.
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(TextTone::Neutral.color(), &row.label);
            if row.ok {
                ui.colored_label(TextTone::Positive.color(), row.verdict.as_str());
            } else {
                ui.colored_label(TextTone::Caution.color(), &row.verdict);
            }
        });
    }
    ui.separator();
    button(ui, &model.close, actions);
}

pub(super) fn inspector_panel(
    mut contexts: EguiContexts,
    mut area: ResMut<HudArea>,
    session: Res<Session>,
    l10n: Res<Localizer>,
    mut sink: ActionSink,
    style: Res<MoneyStyle>,
) -> Result {
    let Some(model) = inspector_model(&session.0, &l10n, sink.selection(), &style) else {
        return Ok(());
    };
    let ctx = contexts.ctx_mut()?;
    // The build menu claims its side after this panel: leave the map room beside it too.
    let reserved = BUILD_PANEL.current_size(ctx);
    let actions = area.show_panel(ctx, INSPECTOR_PANEL, reserved, |ui| {
        draw_inspector(ui, &model)
    });
    dispatch(actions, &mut sink);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::hud::test_support::usd_style;
    use application::{Command, InMemoryCommandLog};
    use domain::region::{RegionDefinition, SiteError};

    use super::*;
    use crate::hud::test_support::{eastern_game, order_plant, shipped_l10n};
    use crate::l10n::format;

    fn cells(region: &RegionDefinition) -> impl Iterator<Item = CellCoord> {
        let terrain = region.terrain();
        let (w, h) = (terrain.width(), terrain.height());
        (0..h).flat_map(move |y| (0..w).map(move |x| CellCoord { x, y }))
    }

    #[test]
    fn the_gibraltar_style_starter_line_is_named_and_drawn_as_a_cable() {
        let game = crate::hud::test_support::global_game();
        let l10n = crate::hud::test_support::global_l10n();
        let model = crate::world::WorldModel::of(&game);
        let line = model
            .lines
            .values()
            .find(|line| line.name_key.is_some())
            .unwrap();
        assert_eq!(
            Names::new(&l10n, game.session()).line(line.id),
            "Strait of Gibraltar"
        );
        let card = line_inspector(&game, &l10n, line.id, &usd_style()).unwrap();
        assert_eq!(card.submarine.as_deref(), Some("Submarine cable · 3 cells"));
        assert_eq!(
            card.transfer_limits,
            "Transfer limits are not simulated (copper plate)"
        );
        let water: Vec<_> = line.parts.iter().filter(|p| p.color[0] < 0.5).collect();
        assert!(!water.is_empty());
        assert!(water.iter().all(|p| p.size.z < 0.5), "water is dashed");
        let cell = CellCoord { x: 7, y: 3 };
        let potential = game.session().region().solar_potential(cell);
        let reading = cell_model(&game, &l10n, cell);
        assert_eq!(reading.readings[1].value, format::percent(potential));
        assert_ne!(
            potential,
            game.session()
                .region()
                .terrain()
                .get(cell)
                .unwrap()
                .irradiance
        );
        assert_eq!(
            crate::hud::cell_color(
                crate::hud::MapLayer::Irradiance,
                game.session().region(),
                cell
            ),
            crate::hud::map::solar_color(potential.value())
        );
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
            let row = suitability_row(game.session(), kind, water, &l10n);
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
        let row = suitability_row(game.session(), PlantKind::Nuclear, dry, &l10n);
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
            suitability_row(game.session(), PlantKind::Hydro, no_river, &l10n).verdict,
            SiteError::NoHydroSite.localize(&l10n)
        );
        let peaker = suitability_row(game.session(), PlantKind::Peaker, no_river, &l10n);
        assert_eq!(peaker.verdict, l10n.text("inspector-site-ok"));
        assert_eq!(peaker.verdict, "✓");
        assert!(peaker.ok);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn occupied_cells_are_unsuitable_in_the_inspector_even_for_a_stale_cell_selection() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        order_plant(&mut game, PlantKind::Peaker, 0);
        for at in game
            .session()
            .city_views()
            .iter()
            .map(|c| c.cell)
            .chain(game.session().plant_views().iter().map(|p| p.cell))
        {
            for row in cell_model(&game, &l10n, at).suitability {
                assert!(!row.ok);
                assert_eq!(row.verdict, SiteError::Occupied.localize(&l10n));
            }
        }
        assert!(l10n.problems().is_empty());
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

        let model = cell_model(&game, &l10n, at);

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
                    "Solar potential",
                    format::percent(region.solar_potential(at)).as_str()
                ),
                (
                    "Wind potential",
                    format::percent(terrain_cell.wind_potential).as_str()
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
            assert_eq!(row, &suitability_row(game.session(), row.kind, at, &l10n));
        }
        assert_eq!(model.close.action, UiAction::Select(Selection::None));
        assert_eq!(l10n.problems(), vec![]);

        // Water is called water.
        let water = cells(region)
            .find(|at| !region.terrain().get(*at).unwrap().land)
            .unwrap();
        let model = cell_model(&game, &l10n, water);
        assert_eq!(model.terrain, l10n.text("inspector-water"));
    }

    fn cell_model(game: &Game, l10n: &Localizer, at: CellCoord) -> CellInspector {
        match inspector_model(game, l10n, Selection::Cell(at), &usd_style()) {
            Some(InspectorModel::Cell(model)) => model,
            other => panic!("a cell is selected: {other:?}"),
        }
    }

    #[test]
    fn unknown_or_no_selection_keeps_the_panel_with_a_hint() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let outside = CellCoord { x: 5000, y: 5000 };

        for selection in [
            Selection::None,
            Selection::Plant(domain::ids::PlantId(u32::MAX)),
            Selection::LineFrom(session.anchor_node()),
            Selection::Cell(outside),
        ] {
            assert_eq!(
                inspector_model(&game, &l10n, selection, &usd_style()),
                Some(InspectorModel::Empty {
                    hint: "Select a cell to see what can be built there".to_owned()
                }),
                "{selection:?}"
            );
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    /// A selected plant shows either its construction progress or its specifications.
    #[test]
    fn a_selected_plant_under_construction_shows_its_card() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let built = game.session().plant_views()[0].id;
        let plant = order_plant(&mut game, domain::region::PlantKind::Peaker, 0);

        assert_eq!(
            inspector_model(&game, &l10n, Selection::Plant(plant), &usd_style()),
            Some(InspectorModel::Construction(
                construction_card_model(&game, &l10n, plant, &usd_style()).expect("a card")
            ))
        );
        assert!(matches!(
            inspector_model(&game, &l10n, Selection::Plant(built), &usd_style()),
            Some(InspectorModel::Plant(_))
        ));
        assert_eq!(l10n.problems(), vec![]);
    }

    /// A selected city shows its card in place of the cell hint (spec §16.8).
    #[test]
    fn a_selected_city_shows_its_card() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        for city in game.session().city_views() {
            assert_eq!(
                inspector_model(&game, &l10n, Selection::City(city.id), &usd_style()),
                Some(InspectorModel::City(
                    city_card_model(&game, &l10n, city.id).expect("a known city")
                )),
                "{:?}",
                city.id
            );
        }
        // A city the session doesn't know keeps the hint.
        assert!(matches!(
            inspector_model(
                &game,
                &l10n,
                Selection::City(domain::ids::CityId(9_999)),
                &usd_style()
            ),
            Some(InspectorModel::Empty { .. })
        ));
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_inspector_is_planning_only() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let at = CellCoord { x: 10, y: 10 };
        assert!(inspector_model(&game, &l10n, Selection::Cell(at), &usd_style()).is_some());

        game.execute(Command::BeginOperating).unwrap();

        assert_eq!(
            inspector_model(&game, &l10n, Selection::Cell(at), &usd_style()),
            None
        );
        assert_eq!(
            inspector_model(&game, &l10n, Selection::None, &usd_style()),
            None
        );
    }
}
