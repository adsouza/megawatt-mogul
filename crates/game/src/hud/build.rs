//! The build menu (Planning only, spec §8.4): plants on the selected cell, and transmission lines
//! between two network nodes. It asks for nothing by itself: a button sends the build command, and
//! the domain checks it again.

use application::{Command, Game};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::grid::distance_cells;
use domain::ids::{CellCoord, NodeId};
use domain::region::{PlantKind, RegionDefinition};
use domain::session::DayPhase;
use domain::units::Money;

use super::action::{ActionSink, ButtonModel, UiAction, button, dispatch};
use super::layout::{Edge, HudArea, OuterPanel};
use super::plant_panel::PlantStat;
use super::selection::Selection;
use super::theme::{TextTone, ValueColor, heading_color, marginal_cost_fraction};
use crate::l10n::{Localize, LocalizeWith, Localizer, Names, format};
use crate::sim::Session;

/// The build menu's width, fixed so the map beside it never moves.
pub const BUILD_PANEL_WIDTH: f32 = 300.0;
/// The build menu as an outer panel: on the right, resizable from [`BUILD_PANEL_WIDTH`].
pub const BUILD_PANEL: OuterPanel = OuterPanel {
    id: "build",
    edge: Edge::Right,
    size: BUILD_PANEL_WIDTH,
};

/// One plant kind on the selected cell: what it is, what it costs, and the button to build it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlantRow {
    pub kind: PlantKind,
    pub name: String,
    pub help: Option<String>,
    /// Capacity, capital cost, build time and fuel cost, from the region's spec.
    pub stats: Vec<PlantStat>,
    /// Why the plant can't go on this cell: the localised siting error.
    pub blocked: Option<String>,
    /// Enabled only when the siting rule passes.
    pub build: ButtonModel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlantSection {
    /// No cell is selected yet.
    PickCell { hint: String },
    /// A row per kind for the selected cell.
    Kinds {
        heading: String,
        cell: CellCoord,
        rows: Vec<PlantRow>,
    },
}

/// What a line to one node would cost, or why it can't be built.
#[derive(Debug, Clone, PartialEq)]
pub enum LinePreview {
    /// Length, capital cost and build time, from `line_quote`.
    Quote(Vec<PlantStat>),
    /// The localised error `line_quote` returned.
    Rejected(String),
}

/// A node a line could end at.
#[derive(Debug, Clone, PartialEq)]
pub struct LineTarget {
    pub node: NodeId,
    /// `Line to Pittsburgh`.
    pub name: String,
    pub preview: LinePreview,
    /// Enabled only when the quote succeeded.
    pub build: ButtonModel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LineSection {
    /// Choosing where the line starts: one button per node.
    Start {
        heading: String,
        hint: String,
        from: Vec<ButtonModel>,
    },
    /// Choosing where it ends: every other node with its quote; buildable ones first, each group
    /// nearest first.
    End {
        heading: String,
        hint: String,
        targets: Vec<LineTarget>,
        cancel: ButtonModel,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct BuildModel {
    pub title: String,
    pub plant: PlantSection,
    pub line: LineSection,
}

/// A node of the network a line can start or end at, with its name and place.
struct NodeChoice {
    node: NodeId,
    name: String,
    cell: CellCoord,
}

/// The home city, then the other cities, then every plant.
fn node_choices(game: &Game, l10n: &Localizer) -> Vec<NodeChoice> {
    let session = game.session();
    let names = Names::new(l10n, session);
    let mut cities: Vec<_> = session
        .city_views()
        .into_iter()
        .map(|city| NodeChoice {
            node: city.node,
            name: names.city(city.id),
            cell: city.cell,
        })
        .collect();
    cities.sort_by_key(|city| city.node != session.anchor_node());
    let plants = session.plant_views().into_iter().map(|plant| NodeChoice {
        node: plant.node,
        name: names.plant(plant.id),
        cell: plant.cell,
    });
    cities.into_iter().chain(plants).collect()
}

/// The build menu for `selection`; `None` outside Planning, when nothing can be ordered.
pub fn build_model(game: &Game, l10n: &Localizer, selection: Selection) -> Option<BuildModel> {
    let session = game.session();
    if session.phase() != DayPhase::Planning {
        return None;
    }
    let nodes = node_choices(game, l10n);
    Some(BuildModel {
        title: l10n.text("build-title"),
        plant: plant_section(game, l10n, selection),
        line: line_section(game, l10n, selection, &nodes),
    })
}

fn months_value(l10n: &Localizer, months: u32) -> String {
    l10n.args("build-months-value", &[("months", months.into())])
}

/// Construction is paid at month close. Quotes are approximate because the final payment
/// includes any remaining cents; the ledger uses `Money::split_even`.
pub(super) fn monthly_payment(capital: Money, months: u32) -> String {
    format::money(Money::from_cents(capital.cents() / i64::from(months)))
}

fn construction_values(capital: Money, months: u32) -> [f64; 3] {
    [
        capital.cents() as f64,
        capital.cents() as f64 / f64::from(months),
        f64::from(months),
    ]
}

/// Each metric has its own regional reference, so monthly affordability reflects the payment
/// schedule. Line quotes use the same references as plants; changing the selected site or line
/// origin doesn't change what a colour means.
fn construction_stats(
    region: &RegionDefinition,
    l10n: &Localizer,
    capital: Money,
    months: u32,
) -> Vec<PlantStat> {
    let (minimum, maximum) = region.catalog().iter().fold(
        ([f64::INFINITY; 3], [0.0_f64; 3]),
        |(minimum, maximum), spec| {
            let values = construction_values(spec.capital_cost, spec.build_months);
            (
                std::array::from_fn(|i| minimum[i].min(values[i])),
                std::array::from_fn(|i| maximum[i].max(values[i])),
            )
        },
    );
    let values = construction_values(capital, months);
    [
        ("build-capital", format::money(capital)),
        ("build-monthly-payment", monthly_payment(capital, months)),
        ("build-months", months_value(l10n, months)),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (key, value))| {
        let fraction = if maximum[i] > minimum[i] {
            ((values[i] - minimum[i]) / (maximum[i] - minimum[i])).clamp(0.0, 1.0) as f32
        } else if values[i] > maximum[i] {
            1.0
        } else {
            0.0
        };
        PlantStat::new(l10n, key, value).with_color(ValueColor::Scale(fraction))
    })
    .collect()
}

fn plant_section(game: &Game, l10n: &Localizer, selection: Selection) -> PlantSection {
    let region = game.session().region();
    let cell = match selection {
        Selection::Cell(cell) if region.terrain().get(cell).is_some() => cell,
        _ => {
            return PlantSection::PickCell {
                hint: l10n.text("build-plant-select-cell"),
            };
        }
    };
    let stat = |key: &str, value: String| PlantStat::new(l10n, key, value);
    let rows = PlantKind::ALL
        .into_iter()
        .map(|kind| {
            let spec = region.spec(kind);
            let site = game.session().plant_site_check(kind, cell);
            let mut stats = vec![stat("build-capacity", format::megawatts(spec.capacity))];
            stats.extend(construction_stats(
                region,
                l10n,
                spec.capital_cost,
                spec.build_months,
            ));
            if let Some(b) = spec.battery {
                stats.extend([
                    stat("battery-charge-rating", format::megawatts(b.charge_power)),
                    stat(
                        "battery-energy-rating",
                        format::megawatt_hours(b.energy_capacity),
                    ),
                ]);
            } else {
                stats.push(
                    stat("build-marginal-cost", format::price(spec.marginal_cost)).with_color(
                        ValueColor::Scale(marginal_cost_fraction(region, spec.marginal_cost)),
                    ),
                );
            }
            if kind == PlantKind::Wind {
                let potential = region
                    .terrain()
                    .get(cell)
                    .expect("selected cell exists")
                    .wind_potential;
                stats.push(stat("wind-potential", format::percent(potential)));
                stats.push(stat(
                    "wind-forecast",
                    format::megawatts(
                        spec.capacity
                            * potential.value()
                            * region.wind().monthly[game.session().clock().month().index0()],
                    ),
                ));
                stats.push(stat(
                    "plant-capacity-credit",
                    format::percent(spec.capacity_credit),
                ));
            }
            PlantRow {
                kind,
                name: kind.localize(l10n),
                help: if kind == PlantKind::Wind {
                    Some(l10n.text("wind-build-help"))
                } else {
                    spec.battery
                        .map(|b| super::plant_panel::battery_hint(l10n, "battery-cycle-hint", b))
                },
                stats,
                blocked: site.as_ref().err().map(|error| error.localize(l10n)),
                build: ButtonModel::new(
                    l10n.text("build-plant"),
                    UiAction::Command(Command::BuildPlant { kind, cell }),
                )
                .enabled_if(site.is_ok()),
            }
        })
        .collect();
    PlantSection::Kinds {
        heading: l10n.args(
            "build-plant-heading",
            &[
                ("x", cell.x.to_string().into()),
                ("y", cell.y.to_string().into()),
            ],
        ),
        cell,
        rows,
    }
}

fn line_section(
    game: &Game,
    l10n: &Localizer,
    selection: Selection,
    nodes: &[NodeChoice],
) -> LineSection {
    let heading = l10n.text("build-line-heading");
    let from = match selection {
        Selection::LineFrom(node) => nodes.iter().find(|choice| choice.node == node),
        _ => None,
    };
    match from {
        Some(from) => line_end_section(game, l10n, heading, from, nodes),
        None => line_start_section(game, l10n, selection, heading, nodes),
    }
}

/// Where the line starts: a button per node, the selected plant's or city's first.
fn line_start_section(
    game: &Game,
    l10n: &Localizer,
    selection: Selection,
    heading: String,
    nodes: &[NodeChoice],
) -> LineSection {
    let session = game.session();
    let selected_node = match selection {
        Selection::Plant(id) => session
            .plant_views()
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.node),
        Selection::City(id) => session
            .city_views()
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.node),
        _ => None,
    };
    let mut from: Vec<&NodeChoice> = nodes.iter().collect();
    // Stable, so the rest keep their order.
    from.sort_by_key(|choice| Some(choice.node) != selected_node);
    LineSection::Start {
        heading,
        hint: l10n.text("build-line-hint-from"),
        from: from
            .into_iter()
            .map(|choice| {
                ButtonModel::new(
                    l10n.args("build-line-from", &[("node", choice.name.as_str().into())]),
                    UiAction::Select(Selection::LineFrom(choice.node)),
                )
            })
            .collect(),
    }
}

/// Where the line ends: every other node with what a line there would cost, or why it can't be
/// built. The buildable ones come first; each group is nearest first.
fn line_end_section(
    game: &Game,
    l10n: &Localizer,
    heading: String,
    from: &NodeChoice,
    nodes: &[NodeChoice],
) -> LineSection {
    let session = game.session();
    let names = Names::new(l10n, session);
    let mut others: Vec<(f64, &NodeChoice)> = nodes
        .iter()
        .filter(|choice| choice.node != from.node)
        .map(|choice| (distance_cells(from.cell, choice.cell), choice))
        .collect();
    others.sort_by(|a, b| a.0.total_cmp(&b.0));
    let stat = |key: &str, value: String| PlantStat::new(l10n, key, value);
    let mut targets: Vec<LineTarget> = others
        .into_iter()
        .map(|(_, to)| {
            let quote = session.line_quote(from.node, to.node);
            let build = ButtonModel::new(
                l10n.text("build-line"),
                UiAction::Command(Command::BuildLine {
                    from: from.node,
                    to: to.node,
                }),
            )
            .enabled_if(quote.is_ok());
            let preview = match quote {
                Ok(quote) => {
                    let mut stats = vec![stat(
                        "build-line-cells",
                        l10n.args("build-cells-value", &[("cells", quote.cells.into())]),
                    )];
                    stats.extend(construction_stats(
                        session.region(),
                        l10n,
                        quote.capital,
                        quote.months,
                    ));
                    LinePreview::Quote(stats)
                }
                Err(error) => LinePreview::Rejected(error.localize_with(l10n, &names)),
            };
            LineTarget {
                node: to.node,
                name: l10n.args("build-line-to", &[("node", to.name.as_str().into())]),
                preview,
                build,
            }
        })
        .collect();
    // Stable: the buildable lines first, each group nearest first.
    targets.sort_by_key(|target| matches!(target.preview, LinePreview::Rejected(_)));
    LineSection::End {
        heading,
        hint: l10n.args("build-line-hint-to", &[("node", from.name.as_str().into())]),
        targets,
        cancel: ButtonModel::new(
            l10n.text("build-line-cancel"),
            UiAction::Select(Selection::None),
        ),
    }
}

pub fn draw_build(ui: &mut egui::Ui, model: &BuildModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.heading(egui::RichText::new(&model.title).color(heading_color()));
    // Each line stage scrolls on its own, so switching stage starts at the top.
    let stage = match model.line {
        LineSection::Start { .. } => "build_line_start",
        LineSection::End { .. } => "build_line_end",
    };
    egui::ScrollArea::vertical().id_salt(stage).show(ui, |ui| {
        draw_plant_section(ui, &model.plant, &mut actions);
        ui.separator();
        draw_line_section(ui, &model.line, &mut actions);
    });
    actions
}

fn draw_plant_section(ui: &mut egui::Ui, section: &PlantSection, actions: &mut Vec<UiAction>) {
    match section {
        PlantSection::PickCell { hint } => {
            ui.weak(hint.as_str());
        }
        PlantSection::Kinds { heading, rows, .. } => {
            ui.label(egui::RichText::new(heading).strong().color(heading_color()));
            for row in rows {
                ui.group(|ui| {
                    ui.colored_label(heading_color(), &row.name);
                    egui::Grid::new(("plant_stats", row.kind as u8)).show(ui, |ui| {
                        for stat in &row.stats {
                            ui.colored_label(TextTone::Neutral.color(), &stat.label);
                            ui.colored_label(stat.color.color(), &stat.value);
                            ui.end_row();
                        }
                    });
                    if let Some(help) = &row.help {
                        ui.add(egui::Label::new(help).wrap());
                    }
                    if let Some(reason) = &row.blocked {
                        ui.colored_label(TextTone::Caution.color(), reason);
                    }
                    button(ui, &row.build, actions);
                });
            }
        }
    }
}

fn draw_line_section(ui: &mut egui::Ui, section: &LineSection, actions: &mut Vec<UiAction>) {
    match section {
        LineSection::Start {
            heading,
            hint,
            from,
        } => {
            ui.label(egui::RichText::new(heading).strong().color(heading_color()));
            ui.weak(hint.as_str());
            for choice in from {
                button(ui, choice, actions);
            }
        }
        LineSection::End {
            heading,
            hint,
            targets,
            cancel,
        } => {
            ui.label(egui::RichText::new(heading).strong().color(heading_color()));
            ui.weak(hint.as_str());
            button(ui, cancel, actions);
            for target in targets {
                ui.group(|ui| {
                    ui.colored_label(heading_color(), &target.name);
                    match &target.preview {
                        LinePreview::Quote(stats) => {
                            for stat in stats {
                                ui.horizontal(|ui| {
                                    ui.colored_label(TextTone::Neutral.color(), &stat.label);
                                    ui.colored_label(stat.color.color(), &stat.value);
                                });
                            }
                        }
                        LinePreview::Rejected(reason) => {
                            ui.colored_label(TextTone::Caution.color(), reason);
                        }
                    }
                    button(ui, &target.build, actions);
                });
            }
        }
    }
}

pub(super) fn build_panel(
    mut contexts: EguiContexts,
    mut area: ResMut<HudArea>,
    session: Res<Session>,
    l10n: Res<Localizer>,
    mut sink: ActionSink,
) -> Result {
    let Some(model) = build_model(&session.0, &l10n, sink.selection()) else {
        return Ok(());
    };
    let actions = area.show_panel(contexts.ctx_mut()?, BUILD_PANEL, 0.0, |ui| {
        draw_build(ui, &model)
    });
    dispatch(actions, &mut sink);
    Ok(())
}

#[cfg(test)]
mod tests {
    use application::InMemoryCommandLog;
    use domain::grid::distance_cells;
    use domain::region::{RegionDefinition, SiteError};

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_l10n};

    #[test]
    fn a_joined_target_shows_the_need_test_reason() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let from = session.plant_views()[0].node;
        let to = session.plant_views()[1].node;
        assert!(matches!(
            session.line_quote(from, to),
            Err(domain::error::DomainError::LineNotNeeded { .. })
        ));
        let model = build_model(&game, &l10n, Selection::LineFrom(from)).unwrap();
        let target = targets(&model)
            .iter()
            .find(|target| target.node == to)
            .unwrap();
        assert!(!target.build.enabled);
        let names = Names::new(&l10n, session);
        let expected = format!(
            "{} and {} are already joined by lines in service or under construction, so the regulator won't approve another line between them.",
            names.node(from),
            names.node(to)
        );
        assert_eq!(target.preview, LinePreview::Rejected(expected));
        assert!(l10n.problems().is_empty());
    }

    #[test]
    fn each_home_city_is_one_named_line_origin() {
        let l10n = shipped_l10n();
        for (region, home) in [
            ("eastern_interconnection", "Columbus"),
            ("western_interconnection", "Seattle"),
        ] {
            let game = crate::hud::test_support::shipped_game(
                region,
                Box::new(InMemoryCommandLog::default()),
            );
            let session = game.session();
            let model = build_model(&game, &l10n, Selection::None).unwrap();
            let LineSection::Start { from, .. } = model.line else {
                panic!("line origins")
            };
            assert_eq!(
                from.len(),
                session.city_views().len() + session.plant_views().len()
            );
            assert_eq!(from[0].label, format!("Line from {home}"));
            let home_action = UiAction::Select(Selection::LineFrom(session.anchor_node()));
            assert_eq!(
                from.iter()
                    .filter(|button| button.action == home_action)
                    .count(),
                1
            );
            let model =
                build_model(&game, &l10n, Selection::LineFrom(session.anchor_node())).unwrap();
            assert!(
                targets(&model)
                    .iter()
                    .all(|target| target.node != session.anchor_node())
            );
            let objects = super::super::map::MapObjects::of(&game);
            assert_eq!(
                objects
                    .cities
                    .iter()
                    .filter(|city| city.id == session.home_city())
                    .count(),
                1
            );
        }
        assert!(l10n.problems().is_empty());
    }

    fn cells(region: &RegionDefinition) -> impl Iterator<Item = CellCoord> {
        let terrain = region.terrain();
        let (w, h) = (terrain.width(), terrain.height());
        (0..h).flat_map(move |y| (0..w).map(move |x| CellCoord { x, y }))
    }

    fn rows(model: &BuildModel) -> &[PlantRow] {
        match &model.plant {
            PlantSection::Kinds { rows, .. } => rows,
            PlantSection::PickCell { .. } => panic!("no cell is selected: {model:?}"),
        }
    }

    #[test]
    fn construction_values_use_independent_cost_and_time_scales() {
        let l10n = shipped_l10n();
        for region_name in ["eastern_interconnection", "western_interconnection"] {
            let game = crate::hud::test_support::shipped_game(
                region_name,
                Box::new(InMemoryCommandLog::default()),
            );
            let region = game.session().region();
            let model =
                build_model(&game, &l10n, Selection::Cell(CellCoord { x: 0, y: 0 })).unwrap();
            let values: Vec<_> = PlantKind::ALL
                .into_iter()
                .map(|kind| {
                    let spec = region.spec(kind);
                    (
                        kind,
                        [
                            spec.capital_cost.cents() as f64,
                            spec.capital_cost.cents() as f64 / f64::from(spec.build_months),
                            f64::from(spec.build_months),
                        ],
                    )
                })
                .collect();
            for index in 0..3 {
                let mut ratings = Vec::new();
                for row in rows(&model) {
                    let ValueColor::Scale(fraction) = row.stats[index + 1].color else {
                        panic!("{} needs a cost/time scale", row.stats[index + 1].label)
                    };
                    let value = values.iter().find(|(kind, _)| *kind == row.kind).unwrap().1[index];
                    assert!((0.0..=1.0).contains(&fraction));
                    ratings.push((value, fraction, row.stats[index + 1].color.color()));
                }
                ratings.sort_by(|a, b| a.0.total_cmp(&b.0));
                for pair in ratings.windows(2) {
                    assert!(
                        pair[0].1 <= pair[1].1,
                        "more expensive/slower must not look better"
                    );
                }
                let cheapest = ratings.first().unwrap().2;
                let dearest = ratings.last().unwrap().2;
                assert!(
                    cheapest.g() > cheapest.r(),
                    "cheapest/fastest should be green"
                );
                assert!(dearest.r() > dearest.g(), "dearest/slowest should be red");
            }
            let row = |kind| rows(&model).iter().find(|row| row.kind == kind).unwrap();
            // The same capital spread over different schedules must give different payment colours.
            assert_eq!(
                row(PlantKind::Wind).stats[1].color,
                row(PlantKind::Ccgt).stats[1].color
            );
            assert_ne!(
                row(PlantKind::Wind).stats[2].color,
                row(PlantKind::Ccgt).stats[2].color
            );
        }
    }

    fn targets(model: &BuildModel) -> &[LineTarget] {
        match &model.line {
            LineSection::End { targets, .. } => targets,
            LineSection::Start { .. } => panic!("no line is started: {model:?}"),
        }
    }

    #[test]
    fn build_button_disabled_on_an_unsuitable_cell() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let region = game.session().region();

        // Open water takes nothing.
        let water = cells(region)
            .find(|at| !region.terrain().get(*at).unwrap().land)
            .unwrap();
        let model = build_model(&game, &l10n, Selection::Cell(water)).unwrap();
        assert_eq!(rows(&model).len(), PlantKind::ALL.len());
        for row in rows(&model) {
            assert!(!row.build.enabled, "{:?} on water", row.kind);
            assert_eq!(row.blocked, Some(SiteError::NotLand.localize(&l10n)));
        }

        // Dry land with no river: hydro is refused while a peaker is fine.
        let no_river = cells(region)
            .find(|at| region.site_check(PlantKind::Hydro, *at) == Err(SiteError::NoHydroSite))
            .unwrap();
        let model = build_model(&game, &l10n, Selection::Cell(no_river)).unwrap();
        let by_kind = |kind| rows(&model).iter().find(|row| row.kind == kind).unwrap();
        assert!(!by_kind(PlantKind::Hydro).build.enabled);
        assert_eq!(
            by_kind(PlantKind::Hydro).blocked,
            Some(SiteError::NoHydroSite.localize(&l10n))
        );
        assert!(by_kind(PlantKind::Peaker).build.enabled);
        assert_eq!(by_kind(PlantKind::Peaker).blocked, None);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn occupied_cells_never_offer_enabled_plant_construction() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        crate::hud::test_support::order_plant(&mut game, PlantKind::Peaker, 0);
        for at in game
            .session()
            .city_views()
            .iter()
            .map(|c| c.cell)
            .chain(game.session().plant_views().iter().map(|p| p.cell))
        {
            let model = build_model(&game, &l10n, Selection::Cell(at)).unwrap();
            for row in rows(&model) {
                assert!(
                    !row.build.enabled,
                    "offered {:?} on occupied {at:?}",
                    row.kind
                );
                assert_eq!(row.blocked, Some(SiteError::Occupied.localize(&l10n)));
            }
        }
        assert!(l10n.problems().is_empty());
    }

    #[test]
    fn build_actions_carry_the_selected_cell_and_kind() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let region = game.session().region();
        // A cell every kind could be built on, if there is one, else the most that can.
        let at = cells(region)
            .max_by_key(|at| {
                PlantKind::ALL
                    .iter()
                    .filter(|kind| region.site_check(**kind, *at).is_ok())
                    .count()
            })
            .unwrap();

        let model = build_model(&game, &l10n, Selection::Cell(at)).unwrap();

        let kinds: Vec<PlantKind> = rows(&model).iter().map(|row| row.kind).collect();
        assert_eq!(kinds, PlantKind::ALL);
        let mut enabled = 0;
        for row in rows(&model) {
            assert_eq!(
                row.build.action,
                UiAction::Command(Command::BuildPlant {
                    kind: row.kind,
                    cell: at
                })
            );
            assert_eq!(row.build.label, l10n.text("build-plant"));
            assert_eq!(row.build.enabled, region.site_check(row.kind, at).is_ok());
            enabled += usize::from(row.build.enabled);
        }
        assert!(
            enabled >= 3,
            "a peaker, a gas plant and solar fit most land"
        );

        // Each row quotes the region's own numbers.
        let coal = rows(&model)
            .iter()
            .find(|row| row.kind == PlantKind::Coal)
            .unwrap();
        let spec = region.spec(PlantKind::Coal);
        let stats: Vec<(&str, String)> = coal
            .stats
            .iter()
            .map(|s| (s.label.as_str(), s.value.clone()))
            .collect();
        assert_eq!(
            stats,
            [
                ("Capacity", format::megawatts(spec.capacity)),
                ("Capital cost", format::money(spec.capital_cost)),
                (
                    "Per month (approx.)",
                    format::money(spec.capital_cost.split_even(spec.build_months)[0])
                ),
                ("Build time", format!("{} months", spec.build_months)),
                ("Fuel cost", format::price(spec.marginal_cost)),
            ]
        );
        assert_eq!(coal.name, PlantKind::Coal.localize(&l10n));
        match &model.plant {
            PlantSection::Kinds { cell, heading, .. } => {
                assert_eq!(*cell, at);
                assert_eq!(heading, &format!("Plant at ({}, {})", at.x, at.y));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn line_preview_shows_the_quote_or_error() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let from = session.anchor_node();
        let names = Names::new(&l10n, session);

        let model = build_model(&game, &l10n, Selection::LineFrom(from)).unwrap();

        let targets = targets(&model);
        let nodes = session.plant_views().len() + session.city_views().len();
        assert_eq!(targets.len(), nodes - 1, "every node but the start");
        let mut quoted = 0;
        let mut rejected = 0;
        for target in targets {
            assert_eq!(
                target.build.label,
                l10n.text("build-line"),
                "{}",
                target.name
            );
            assert_eq!(
                target.build.action,
                UiAction::Command(Command::BuildLine {
                    from,
                    to: target.node
                })
            );
            match (session.line_quote(from, target.node), &target.preview) {
                (Ok(quote), LinePreview::Quote(stats)) => {
                    quoted += 1;
                    assert!(target.build.enabled);
                    assert!(
                        stats[1..]
                            .iter()
                            .all(|stat| matches!(stat.color, ValueColor::Scale(_)))
                    );
                    let stats: Vec<(&str, &str)> = stats
                        .iter()
                        .map(|s| (s.label.as_str(), s.value.as_str()))
                        .collect();
                    assert_eq!(
                        stats,
                        [
                            ("Length", format!("{} cells", quote.cells).as_str()),
                            ("Capital cost", format::money(quote.capital).as_str()),
                            (
                                "Per month (approx.)",
                                format::money(quote.capital.split_even(quote.months)[0]).as_str()
                            ),
                            (
                                "Build time",
                                format!(
                                    "{} {}",
                                    quote.months,
                                    if quote.months == 1 { "month" } else { "months" }
                                )
                                .as_str()
                            ),
                        ]
                    );
                }
                (Err(error), LinePreview::Rejected(text)) => {
                    rejected += 1;
                    assert!(!target.build.enabled);
                    assert_eq!(text, &error.localize_with(&l10n, &names));
                }
                (quote, preview) => panic!("{quote:?} previewed as {preview:?}"),
            }
        }
        // The home city already joins the other two starter cities and the whole fleet;
        // the rest of the cities can be quoted.
        assert!(quoted >= 20, "{quoted} quotes");
        assert_eq!(rejected, 2 + session.plant_views().len());
        assert!(rejected > 0);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn buildable_lines_come_first_and_each_group_is_nearest_first_and_named() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let from = session.anchor_node();

        let model = build_model(&game, &l10n, Selection::LineFrom(from)).unwrap();

        let cell_of = |node: NodeId| {
            session
                .plant_views()
                .into_iter()
                .find(|p| p.node == node)
                .map(|p| p.cell)
                .or_else(|| {
                    session
                        .city_views()
                        .into_iter()
                        .find(|c| c.node == node)
                        .map(|c| c.cell)
                })
                .expect("every target is a plant or a city")
        };
        let start = cell_of(session.anchor_node());
        let refused = |t: &LineTarget| matches!(t.preview, LinePreview::Rejected(_));
        let (buildable, joined): (Vec<&LineTarget>, Vec<&LineTarget>) =
            targets(&model).iter().partition(|t| !refused(t));
        assert!(!buildable.is_empty() && !joined.is_empty());
        let in_order: Vec<&LineTarget> = buildable.iter().chain(&joined).copied().collect();
        assert_eq!(
            in_order.iter().map(|t| t.node).collect::<Vec<_>>(),
            targets(&model).iter().map(|t| t.node).collect::<Vec<_>>(),
            "every refused line is listed after every buildable one"
        );
        for group in [&buildable, &joined] {
            let distances: Vec<f64> = group
                .iter()
                .map(|t| distance_cells(start, cell_of(t.node)))
                .collect();
            assert!(
                distances.windows(2).all(|pair| pair[0] <= pair[1]),
                "nearest first: {distances:?}"
            );
        }
        // An already-joined line says who is joined, by name, not by grid point.
        for target in &joined {
            let LinePreview::Rejected(reason) = &target.preview else {
                unreachable!()
            };
            let who = target.name.strip_prefix("Line to ").unwrap();
            assert_eq!(
                reason,
                &format!("Columbus and {who} are already connected.")
            );
        }
        let names: Vec<&str> = targets(&model).iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"Line to Pittsburgh"), "{names:?}");
        assert!(names.contains(&"Line to Nashville"), "{names:?}");
        match &model.line {
            LineSection::End {
                heading,
                hint,
                cancel,
                ..
            } => {
                assert_eq!(heading, &l10n.text("build-line-heading"));
                assert_eq!(hint, "Starting at Columbus. Choose where it ends.");
                assert_eq!(cancel.action, UiAction::Select(Selection::None));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn without_a_selection_the_menu_asks_for_a_cell_and_a_line_start() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();

        let model = build_model(&game, &l10n, Selection::None).unwrap();

        assert_eq!(model.title, l10n.text("build-title"));
        assert_eq!(
            model.plant,
            PlantSection::PickCell {
                hint: l10n.text("build-plant-select-cell")
            }
        );
        let LineSection::Start { from, .. } = &model.line else {
            panic!("{model:?}");
        };
        let labels: Vec<&str> = from.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels[0], "Line from Columbus");
        assert_eq!(
            from.len(),
            session.city_views().len() + session.plant_views().len()
        );
        assert!(labels.contains(&"Line from Columbus"), "{labels:?}");
        assert_eq!(
            from[0].action,
            UiAction::Select(Selection::LineFrom(session.anchor_node()))
        );
        assert!(from.iter().all(|b| b.enabled));
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn a_selected_city_or_plant_heads_the_list_of_line_starts() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let city = session.city_views().into_iter().last().unwrap();
        let plant = session.plant_views().into_iter().last().unwrap();

        for (selection, node) in [
            (Selection::City(city.id), city.node),
            (Selection::Plant(plant.id), plant.node),
        ] {
            let model = build_model(&game, &l10n, selection).unwrap();
            let LineSection::Start { from, .. } = &model.line else {
                panic!("{model:?}");
            };
            assert_eq!(from[0].action, UiAction::Select(Selection::LineFrom(node)));
        }
    }

    #[test]
    fn a_line_start_that_no_longer_exists_goes_back_to_choosing() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        let model = build_model(&game, &l10n, Selection::LineFrom(NodeId(99_999))).unwrap();

        assert!(matches!(model.line, LineSection::Start { .. }), "{model:?}");
    }

    #[test]
    fn the_build_menu_is_planning_only() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let at = CellCoord { x: 10, y: 10 };
        assert!(build_model(&game, &l10n, Selection::Cell(at)).is_some());

        game.execute(Command::BeginOperating).unwrap();

        assert_eq!(game.session().phase(), DayPhase::Operating);
        assert_eq!(build_model(&game, &l10n, Selection::Cell(at)), None);
        assert_eq!(build_model(&game, &l10n, Selection::None), None);
    }
}
