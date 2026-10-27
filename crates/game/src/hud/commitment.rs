//! The commitment grid (spec §8.4: "the primary planning tool"): one row per commissioned
//! dispatchable plant, grouped by local island and then kind in merit order, with 24 hour toggles.
//!
//! A toggle edits a draft pattern held in [`UiState`]. *Apply* sends it as `CommitPlant`; so does
//! starting to edit another row, which leaves this one. The domain validates the pattern (every
//! off-gap on the 24-hour circle must fit a shutdown and a start-up), and a refusal reverts the
//! row (see [`UiState::settle_commit`]) and shows the toast. *All day* and *Off* are the Planning
//! meaning of `StartPlant` and `StopPlant`. Each row also has the plant's AGC switch and its
//! initial target, which must lie between the plant's minimum stable output and its capacity.

use application::{Command, Game};
use bevy_egui::egui;
use domain::ids::{NodeId, PlantId};
use domain::region::PlantKind;
use domain::session::PlantView;
use domain::units::Megawatts;

use super::action::{ButtonModel, ToggleModel, UiAction, button, toggle_button};
use super::map::{Rgb, color32, kind_color};
use super::plant_list::plants_by_kind;
use super::theme::{TextTone, heading_color};
use super::ui_state::{CommitDraft, CommitmentForm, UiState};
use crate::l10n::{Localizer, Names, format};

/// One plant row's height. The grid has up to a few dozen rows, so they are kept compact.
pub const ROW_HEIGHT: f32 = 18.0;
/// The width of the plant name column.
pub const NAME_WIDTH: f32 = 120.0;
/// The width of one hour toggle.
pub const CELL_WIDTH: f32 = 18.0;

/// The label over one hour column. Clicking it tints the column (or clears the tint).
#[derive(Debug, Clone, PartialEq)]
pub struct HourHeader {
    pub hour: u8,
    /// `6`: the hour of day, compact enough for a narrow column.
    pub label: String,
    /// What a screen reader and the tooltip say: `Show 06:00 in the grid`.
    pub tip: String,
    pub highlighted: bool,
    pub action: UiAction,
}

/// One plant in one hour.
#[derive(Debug, Clone, PartialEq)]
pub struct HourCell {
    pub hour: u8,
    pub on: bool,
    pub highlighted: bool,
    /// `Coal plant 3 at 06:00: committed`.
    pub label: String,
    /// What clicking does: edit this row's draft, after sending the draft of any other row.
    pub toggle: Vec<UiAction>,
}

/// The initial-target field of a row.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetModel {
    /// `Initial target`.
    pub label: String,
    /// What the field holds: what was typed, or else the plant's current target.
    pub text: String,
    pub min: Megawatts,
    pub capacity: Megawatts,
    pub current: Megawatts,
    /// `240–600 MW`: what the domain accepts.
    pub range: String,
    /// Whether `text` is a number in that range.
    pub valid: bool,
    /// Sends `SetDispatchTarget`. Enabled only for a valid number that changes the target.
    pub set: ButtonModel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommitmentRow {
    pub plant: PlantId,
    pub kind: PlantKind,
    /// `Coal plant 3`.
    pub name: String,
    pub color: Rgb,
    /// 24 cells, hour 0 first: the draft if this row is being edited, else the committed pattern.
    pub cells: Vec<HourCell>,
    /// Whether the draft differs from the committed pattern.
    pub dirty: bool,
    pub all_day: ButtonModel,
    pub off: ButtonModel,
    pub agc: ToggleModel,
    pub agc_tip: String,
    pub target: TargetModel,
    /// Sends the draft. Enabled only while the row is dirty.
    pub apply: ButtonModel,
    /// Drops the draft. Enabled only while the row is dirty.
    pub revert: ButtonModel,
    /// The day's water, for a hydro plant (spec §2.2): `Water for the day: 1,234 MWh`.
    pub budget: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KindGroup {
    pub island: Option<NodeId>,
    /// Named local-grid section, shown before its first kind.
    pub heading: Option<String>,
    pub forecast: Option<ButtonModel>,
    pub kind: PlantKind,
    pub rows: Vec<CommitmentRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommitmentModel {
    pub island: Option<NodeId>,
    pub scope_label: String,
    pub scopes: Vec<ToggleModel>,
    /// An unapplied edit outside the selected grid remains recoverable.
    pub hidden_draft: Option<HiddenDraftModel>,
    pub time_axis: String,
    pub hours: Vec<HourHeader>,
    /// Islands by representative node, then kinds in merit order and plants by id.
    pub groups: Vec<KindGroup>,
    /// Shown instead of the grid when there is no plant to commit.
    pub empty: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HiddenDraftModel {
    pub message: String,
    pub reveal: ButtonModel,
}

/// Whether some row holds a draft the player has not applied. The day can't begin until it is
/// applied or reverted.
pub fn has_unsaved_draft(game: &Game, ui: &UiState) -> bool {
    ui.draft.is_some_and(|draft| {
        game.session()
            .plant_views()
            .iter()
            .find(|plant| plant.id == draft.plant)
            .is_some_and(|plant| plant.commitment != draft.hours)
    })
}

/// The target a typed `text` stands for, if it is a number the domain would accept for `plant`.
fn parse_target(text: &str, min: Megawatts, capacity: Megawatts) -> Option<Megawatts> {
    let value: f64 = text.trim().parse().ok()?;
    // Parsing accepts "NaN" and "inf"; the domain takes neither.
    if !value.is_finite() {
        return None;
    }
    let target = Megawatts::new(value);
    (target >= min && target <= capacity).then_some(target)
}

impl TargetModel {
    /// Refresh the pure presenter after a field edits its text during the current pass.
    fn edited(&self, plant: PlantId, text: &str) -> Self {
        let typed = parse_target(text, self.min, self.capacity);
        let mut model = self.clone();
        model.text = text.to_owned();
        model.valid = typed.is_some();
        model.set.enabled = typed.is_some_and(|target| target != self.current);
        model.set.action = UiAction::Command(Command::SetDispatchTarget {
            plant,
            mw: typed.unwrap_or(self.current),
        });
        model
    }
}

/// Commitments always belong to one physical local grid. The company-wide selection is a
/// forecast overview; re-entering the planner restores the last valid local selection.
pub(super) fn selected_commitment_island(
    session: &domain::session::GameSession,
    ui: &UiState,
) -> Option<NodeId> {
    session.region().areas()?;
    let islands = session.power().island_nodes();
    ui.island
        .filter(|id| islands.contains_key(id))
        .or_else(|| {
            ui.last_planning_island
                .filter(|id| islands.contains_key(id))
        })
        .or_else(|| {
            islands
                .iter()
                .find_map(|(id, nodes)| nodes.contains(&session.anchor_node()).then_some(*id))
        })
        .or_else(|| islands.keys().next().copied())
}

pub fn commitment_model(
    game: &Game,
    l10n: &Localizer,
    ui: &UiState,
    form: &CommitmentForm,
) -> CommitmentModel {
    let session = game.session();
    let region = session.region();
    let names = Names::new(l10n, session);
    let groups = plants_by_kind(game);
    let empty = std::collections::BTreeMap::new();
    let islands = if region.areas().is_some() {
        session.power().island_nodes()
    } else {
        &empty
    };
    let selected = selected_commitment_island(session, ui);
    let node_islands: std::collections::BTreeMap<_, _> = islands
        .iter()
        .flat_map(|(id, nodes)| nodes.iter().map(move |node| (*node, *id)))
        .collect();
    let plant_island = |plant: &PlantView| node_islands.get(&plant.node).copied();
    let island_name = |id| {
        let label = names.island(session, id);
        if session.power().island_active(id) {
            label
        } else {
            l10n.args("island-inactive-name", &[("island", label.into())])
        }
    };
    let scopes = if islands.is_empty() {
        Vec::new()
    } else {
        islands
            .keys()
            .map(|id| ToggleModel {
                button: ButtonModel::new(island_name(*id), UiAction::SelectIsland(Some(*id))),
                on: selected == Some(*id),
            })
            .collect()
    };

    // The draft is sent when another row starts being edited, if it differs from the plant's own.
    let flush = ui.draft.filter(|draft| {
        groups
            .iter()
            .flat_map(|group| &group.plants)
            .any(|plant| plant.id == draft.plant && plant.commitment != draft.hours)
    });

    let times: Vec<String> = (0..24u8)
        .map(|hour| format::clock(u32::from(hour) * domain::time::TICKS_PER_HOUR))
        .collect();
    let hours = (0..24u8)
        .map(|hour| {
            let highlighted = ui.highlight_hour == Some(hour);
            HourHeader {
                hour,
                label: hour.to_string(),
                tip: l10n.args(
                    "commitment-hour-header",
                    &[("time", times[usize::from(hour)].as_str().into())],
                ),
                highlighted,
                action: UiAction::ShowHour((!highlighted).then_some(hour)),
            }
        })
        .collect();

    let state_on = l10n.text("commitment-state-on");
    let state_off = l10n.text("commitment-state-off");
    let row = |plant: &PlantView| {
        let spec = region.spec(plant.kind);
        let draft = ui.draft_of(plant.id);
        let shown = draft.unwrap_or(plant.commitment);
        let dirty = draft.is_some_and(|draft| draft != plant.commitment);
        let name = names.plant(plant.id);
        let leave =
            flush
                .filter(|draft| draft.plant != plant.id)
                .map(|CommitDraft { plant, hours }| {
                    UiAction::Command(Command::CommitPlant { plant, hours })
                });
        let cells = (0..24u8)
            .map(|hour| {
                let on = shown[usize::from(hour)];
                let mut flipped = shown;
                flipped[usize::from(hour)] = !on;
                let edit = UiAction::EditCommitment {
                    plant: plant.id,
                    hours: flipped,
                };
                HourCell {
                    hour,
                    on,
                    highlighted: ui.highlight_hour == Some(hour),
                    label: l10n.args(
                        "commitment-cell",
                        &[
                            ("plant", name.as_str().into()),
                            ("time", times[usize::from(hour)].as_str().into()),
                            (
                                "state",
                                (if on { &state_on } else { &state_off }).as_str().into(),
                            ),
                        ],
                    ),
                    toggle: leave.iter().cloned().chain([edit]).collect(),
                }
            })
            .collect();

        let (min, capacity) = (spec.min_output(), spec.capacity);
        let text = form
            .targets
            .get(&plant.id)
            .cloned()
            .unwrap_or_else(|| format::plain_megawatts(plant.initial_target));
        let typed = parse_target(&text, min, capacity);
        // A disabled button still needs an action; this one would change nothing if it were sent.
        let set_action = UiAction::Command(Command::SetDispatchTarget {
            plant: plant.id,
            mw: typed.unwrap_or(plant.initial_target),
        });
        CommitmentRow {
            plant: plant.id,
            kind: plant.kind,
            name,
            color: kind_color(plant.kind),
            cells,
            dirty,
            all_day: ButtonModel::new(
                l10n.text("commitment-all-day"),
                UiAction::Command(Command::StartPlant { plant: plant.id }),
            )
            .enabled_if(shown != [true; 24]),
            off: ButtonModel::new(
                l10n.text("commitment-off"),
                UiAction::Command(Command::StopPlant { plant: plant.id }),
            )
            .enabled_if(shown != [false; 24]),
            agc: ToggleModel {
                button: ButtonModel::new(
                    l10n.text("commitment-agc"),
                    UiAction::Command(Command::SetAutoFollow {
                        plant: plant.id,
                        on: !plant.auto_follow,
                    }),
                ),
                on: plant.auto_follow,
            },
            agc_tip: l10n.text("commitment-agc-tip"),
            target: TargetModel {
                label: l10n.text("commitment-target"),
                min,
                capacity,
                current: plant.initial_target,
                valid: typed.is_some(),
                range: l10n.args(
                    "commitment-target-range",
                    &[
                        ("min", format::plain_megawatts(min).into()),
                        ("max", format::plain_megawatts(capacity).into()),
                    ],
                ),
                set: ButtonModel::new(l10n.text("commitment-target-set"), set_action)
                    .enabled_if(typed.is_some_and(|target| target != plant.initial_target)),
                text,
            },
            apply: ButtonModel::new(
                l10n.text("commitment-apply"),
                UiAction::Command(Command::CommitPlant {
                    plant: plant.id,
                    hours: shown,
                }),
            )
            .enabled_if(dirty),
            revert: ButtonModel::new(
                l10n.text("commitment-revert"),
                UiAction::DiscardCommitmentDraft,
            )
            .enabled_if(dirty),
            budget: plant.hydro_day_budget.map(|energy| {
                l10n.args(
                    "commitment-hydro-budget",
                    &[("energy", format::megawatt_hours(energy).into())],
                )
            }),
        }
    };

    let hidden_draft = flush.and_then(|draft| {
        let plant = groups
            .iter()
            .flat_map(|g| &g.plants)
            .find(|p| p.id == draft.plant)?;
        let island = plant_island(plant);
        (selected.is_some() && island != selected).then(|| HiddenDraftModel {
            message: l10n.args(
                "commitment-hidden-draft",
                &[
                    ("plant", names.plant(plant.id).into()),
                    (
                        "island",
                        island
                            .map_or_else(|| l10n.text("commitment-unconnected"), island_name)
                            .into(),
                    ),
                ],
            ),
            reveal: ButtonModel::new(
                l10n.text("commitment-return-to-draft"),
                UiAction::SelectIsland(island),
            ),
        })
    });
    let sections = [selected];
    let mut local_groups = Vec::new();
    for island in sections {
        let mut first = true;
        for group in &groups {
            let rows: Vec<_> = group
                .plants
                .iter()
                .filter(|plant| plant_island(plant) == island)
                .map(row)
                .collect();
            if rows.is_empty() {
                continue;
            }
            let show_heading = first && !islands.is_empty();
            local_groups.push(KindGroup {
                island,
                heading: show_heading.then(|| {
                    island.map_or_else(|| l10n.text("commitment-unconnected"), island_name)
                }),
                forecast: island.filter(|_| show_heading).map(|island| {
                    ButtonModel::new(
                        l10n.text("commitment-view-forecast"),
                        UiAction::InspectForecast {
                            island,
                            hour: ui.highlight_hour.unwrap_or(0),
                        },
                    )
                }),
                kind: group.kind,
                rows,
            });
            first = false;
        }
    }
    let groups = local_groups;
    CommitmentModel {
        island: selected,
        scope_label: l10n.text("commitment-grid"),
        scopes,
        hidden_draft,
        time_axis: region.system_zone().name_key.as_ref().map_or_else(
            || l10n.text("commitment-system-time"),
            |key| l10n.args("commitment-system-zone", &[("zone", l10n.text(key).into())]),
        ),
        hours,
        empty: groups.is_empty().then(|| l10n.text("commitment-empty")),
        groups,
    }
}

/// One hour's toggle or header, painted by hand: a few pixels wide, which no button can be. It
/// still reads as a button to a screen reader, named by `label`.
fn hour_square(
    ui: &mut egui::Ui,
    label: &str,
    fill: egui::Color32,
    text: Option<&str>,
    highlighted: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(CELL_WIDTH, ROW_HEIGHT - 2.0),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        let painter = ui.painter();
        painter.rect_filled(rect.shrink(0.5), 2.0, fill);
        if highlighted {
            painter.rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.5, visuals.warn_fg_color),
                egui::StrokeKind::Inside,
            );
        }
        if response.hovered() {
            painter.rect_stroke(
                rect,
                2.0,
                visuals.widgets.hovered.fg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        if let Some(text) = text {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(9.0),
                super::theme::contrasting_text(fill),
            );
        }
    }
    response.on_hover_text(label)
}

/// The header row: a column of space over the names, then one label per hour.
pub fn draw_commitment_header(
    ui: &mut egui::Ui,
    model: &CommitmentModel,
    actions: &mut Vec<UiAction>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        ui.add_sized(
            egui::vec2(NAME_WIDTH + 4.0, ROW_HEIGHT),
            egui::Label::new(
                egui::RichText::new(&model.time_axis).color(TextTone::Neutral.color()),
            )
            .truncate(),
        )
        .on_hover_text(&model.time_axis);
        for header in &model.hours {
            let fill = if header.highlighted {
                ui.visuals().widgets.active.weak_bg_fill
            } else {
                ui.visuals().widgets.inactive.weak_bg_fill
            };
            let response = hour_square(
                ui,
                &header.tip,
                fill,
                Some(&header.label),
                header.highlighted,
            );
            if response.clicked() {
                actions.push(header.action.clone());
            }
        }
    });
}

/// The grid: the rows by kind, scrolling when there are more than fit. The header stays put.
pub fn draw_commitment(
    ui: &mut egui::Ui,
    model: &CommitmentModel,
    form: &mut CommitmentForm,
) -> Vec<UiAction> {
    let mut actions = Vec::new();
    if model.scopes.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            ui.label(&model.scope_label);
            let selected = model
                .scopes
                .iter()
                .find(|scope| scope.on)
                .expect("selected scope");
            egui::ComboBox::from_id_salt("commitment_island_picker")
                .selected_text(&selected.button.label)
                .width(240.0)
                .show_ui(ui, |ui| {
                    for scope in &model.scopes {
                        if ui.selectable_label(scope.on, &scope.button.label).clicked() {
                            actions.push(scope.button.action.clone());
                            ui.close();
                        }
                    }
                });
        });
    }
    if let Some(draft) = &model.hidden_draft {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(TextTone::Caution.color(), &draft.message);
            button(ui, &draft.reveal, &mut actions);
        });
    }
    if let Some(empty) = &model.empty {
        ui.weak(empty.as_str());
        return actions;
    }
    draw_commitment_header(ui, model, &mut actions);
    egui::ScrollArea::vertical()
        .id_salt(("commitment_rows", model.island))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (index, group) in model.groups.iter().enumerate() {
                if index > 0 {
                    ui.add_space(3.0);
                }
                if let Some(heading) = &group.heading {
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(heading);
                        if let Some(forecast) = &group.forecast {
                            button(ui, forecast, &mut actions);
                        }
                    });
                }
                for row in &group.rows {
                    draw_row(ui, row, form, &mut actions);
                }
            }
        });
    actions
}

fn draw_row(
    ui: &mut egui::Ui,
    row: &CommitmentRow,
    form: &mut CommitmentForm,
    actions: &mut Vec<UiAction>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().interact_size.y = ROW_HEIGHT;
        ui.spacing_mut().button_padding = egui::vec2(4.0, 1.0);
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_sized(
            [NAME_WIDTH, ROW_HEIGHT],
            egui::Label::new(egui::RichText::new(&row.name).color(heading_color())).truncate(),
        );
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            for cell in &row.cells {
                let (fill, text) = if cell.on {
                    (color32(row.color), None)
                } else {
                    (ui.visuals().extreme_bg_color, None)
                };
                let response = hour_square(ui, &cell.label, fill, text, cell.highlighted);
                if response.clicked() {
                    actions.extend(cell.toggle.iter().cloned());
                }
            }
        });
        button(ui, &row.all_day, actions);
        button(ui, &row.off, actions);
        toggle_button(ui, &row.agc, actions).on_hover_text(row.agc_tip.as_str());
        let text = form
            .targets
            .entry(row.plant)
            .or_insert_with(|| row.target.text.clone());
        let response = ui
            .add(
                egui::TextEdit::singleline(text)
                    .desired_width(46.0)
                    .hint_text(row.target.label.as_str()),
            )
            .on_hover_text(row.target.label.as_str());
        let target = row.target.edited(row.plant, text);
        let clicked = button(ui, &target.set, actions).clicked();
        if !clicked && target.set.enabled && super::action::submitted(&response) {
            actions.push(target.set.action);
        }
        let range = egui::RichText::new(target.range.as_str());
        if target.valid {
            ui.colored_label(TextTone::Information.color(), range);
        } else {
            ui.colored_label(TextTone::Negative.color(), range);
        }
    });
    if row.dirty {
        ui.horizontal(|ui| {
            ui.add_space(NAME_WIDTH + 4.0);
            button(ui, &row.apply, actions);
            button(ui, &row.revert, actions);
        });
    }
    if let Some(budget) = &row.budget {
        ui.horizontal(|ui| {
            ui.add_space(NAME_WIDTH + 4.0);
            ui.colored_label(TextTone::Information.color(), budget);
        });
    }
}

#[cfg(test)]
mod tests {
    use application::{CommandError, InMemoryCommandLog};
    use domain::error::DomainError;

    use super::*;
    use crate::hud::test_support::{eastern_game, empty_start, shipped_game, shipped_l10n};

    #[test]
    fn commitment_axis_names_the_system_zone() {
        let game = crate::hud::test_support::global_game();
        let model = commitment_model(
            &game,
            &crate::hud::test_support::global_l10n(),
            &UiState::default(),
            &CommitmentForm::default(),
        );
        assert_eq!(model.time_axis, "System time (MSK)");
        assert_eq!(model.hours[14].label, "14");
    }

    fn model(game: &Game, ui: &UiState, form: &CommitmentForm) -> CommitmentModel {
        commitment_model(game, &shipped_l10n(), ui, form)
    }

    #[test]
    fn selected_forecast_island_limits_commitments_to_its_local_plants() {
        let game = shipped_game("sgcc", Box::new(InMemoryCommandLog::default()));
        let before = game.checkpoint();
        for (&island, nodes) in game.session().power().island_nodes() {
            let ui = UiState {
                island: Some(island),
                ..Default::default()
            };
            let built = model(&game, &ui, &CommitmentForm::default());
            let actual: Vec<_> = built
                .groups
                .iter()
                .flat_map(|g| &g.rows)
                .map(|r| r.plant)
                .collect();
            let expected: std::collections::BTreeSet<_> = game
                .session()
                .plant_views()
                .into_iter()
                .filter(|p| nodes.contains(&p.node) && super::super::plant_list::is_commitable(p))
                .map(|p| p.id)
                .collect();
            assert_eq!(
                actual.len(),
                expected.len(),
                "only local plants for {island:?}"
            );
            assert_eq!(
                actual
                    .into_iter()
                    .collect::<std::collections::BTreeSet<_>>(),
                expected
            );
        }
        assert_eq!(game.checkpoint(), before);
    }

    #[test]
    fn every_island_has_its_own_named_merit_ordered_fleet() {
        let game = shipped_game("sgcc", Box::new(InMemoryCommandLog::default()));
        let default = model(&game, &UiState::default(), &CommitmentForm::default());
        assert_eq!(default.island, Some(game.session().anchor_node()));
        assert_eq!(default.scopes.len(), 5);
        let mut all_plants = std::collections::BTreeSet::new();
        for &island in game.session().power().island_nodes().keys() {
            let ui = UiState {
                island: Some(island),
                ..Default::default()
            };
            let built = model(&game, &ui, &CommitmentForm::default());
            assert!(built.groups.iter().all(|g| g.island == Some(island)));
            assert_eq!(
                built.groups.iter().filter(|g| g.heading.is_some()).count(),
                1
            );
            assert_eq!(
                built.groups[0].heading.as_ref().unwrap(),
                &Names::new(&shipped_l10n(), game.session()).island(game.session(), island)
            );
            assert!(built.groups.windows(2).all(|pair| {
                game.session()
                    .region()
                    .spec(pair[0].kind)
                    .marginal_cost
                    .value()
                    <= game
                        .session()
                        .region()
                        .spec(pair[1].kind)
                        .marginal_cost
                        .value()
            }));
            for row in built.groups.iter().flat_map(|g| &g.rows) {
                assert!(
                    all_plants.insert(row.plant),
                    "a plant belongs to exactly one planner"
                );
            }
        }
        assert_eq!(
            all_plants.len(),
            game.session()
                .plant_views()
                .iter()
                .filter(|p| super::super::plant_list::is_commitable(p))
                .count()
        );
    }

    #[test]
    fn returning_to_commitments_skips_stale_island_ids() {
        let game = shipped_game("sgcc", Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let remote = *session.power().island_nodes().keys().last().unwrap();
        let mut ui = UiState {
            island: Some(NodeId(u32::MAX)),
            last_planning_island: Some(remote),
            ..Default::default()
        };
        assert_eq!(selected_commitment_island(session, &ui), Some(remote));
        ui.last_planning_island = Some(NodeId(u32::MAX));
        assert_eq!(
            selected_commitment_island(session, &ui),
            Some(session.anchor_node())
        );
    }

    #[test]
    fn switching_grids_preserves_and_reveals_hidden_edits_and_flushes_them_before_another_edit() {
        let game = shipped_game("sgcc", Box::new(InMemoryCommandLog::default()));
        let first = game.session().plant_views()[0].clone();
        let home = game.session().anchor_node();
        let remote = *game
            .session()
            .power()
            .island_nodes()
            .keys()
            .find(|id| **id != home)
            .unwrap();
        let mut ui = UiState::default();
        let hours = [false; 24];
        ui.apply(&UiAction::EditCommitment {
            plant: first.id,
            hours,
        });
        ui.apply(&UiAction::SelectIsland(Some(remote)));
        let before = ui.draft;
        let built = model(&game, &ui, &CommitmentForm::default());
        let hidden = built.hidden_draft.unwrap();
        assert!(
            hidden
                .message
                .contains(&Names::new(&shipped_l10n(), game.session()).plant(first.id))
        );
        assert_eq!(hidden.reveal.action, UiAction::SelectIsland(Some(home)));
        assert_eq!(ui.draft, before);
        assert!(has_unsaved_draft(&game, &ui));
        assert_eq!(
            built.groups[0].rows[0].cells[0].toggle[0],
            UiAction::Command(Command::CommitPlant {
                plant: first.id,
                hours
            })
        );
        ui.apply(&hidden.reveal.action);
        let restored = model(&game, &ui, &CommitmentForm::default());
        assert!(restored.hidden_draft.is_none());
        let row = restored
            .groups
            .iter()
            .flat_map(|g| &g.rows)
            .find(|r| r.plant == first.id)
            .unwrap();
        assert!(row.dirty && row.apply.enabled);
        assert!(row.cells.iter().all(|cell| !cell.on));
    }

    #[test]
    fn an_empty_island_keeps_the_picker_so_the_player_can_return_to_their_fleet() {
        let game = crate::hud::test_support::sgcc_review_with_remote_island();
        let remote = game
            .session()
            .city_views()
            .into_iter()
            .find(|c| c.name_key == "city-shanghai")
            .unwrap();
        let built = model(
            &game,
            &UiState {
                island: Some(remote.node),
                ..Default::default()
            },
            &CommitmentForm::default(),
        );
        assert!(built.groups.is_empty());
        assert!(built.empty.is_some());
        assert_eq!(built.scopes.len(), 6);
        assert!(
            built
                .scopes
                .iter()
                .any(|scope| scope.on && scope.button.label.contains("Shanghai"))
        );
    }

    fn first_row(model: &CommitmentModel, kind: PlantKind) -> &CommitmentRow {
        &model
            .groups
            .iter()
            .find(|group| group.kind == kind)
            .unwrap_or_else(|| panic!("a {kind:?} group"))
            .rows[0]
    }

    fn pattern(hours: std::ops::Range<usize>) -> [bool; 24] {
        let mut pattern = [false; 24];
        pattern[hours].fill(true);
        pattern
    }

    #[test]
    fn rows_are_grouped_by_kind_with_24_cells_and_a_name() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let l10n = shipped_l10n();

        let model = commitment_model(
            &game,
            &l10n,
            &UiState::default(),
            &CommitmentForm::default(),
        );

        let kinds: Vec<PlantKind> = model.groups.iter().map(|group| group.kind).collect();
        assert_eq!(
            kinds,
            [
                PlantKind::Nuclear,
                PlantKind::Coal,
                PlantKind::Ccgt,
                PlantKind::Peaker
            ]
        );
        assert_eq!(model.hours.len(), 24);
        assert_eq!(model.hours[6].label, "6");
        assert_eq!(model.hours[6].tip, "Show 06:00 in the grid");
        assert_eq!(model.empty, None);
        let rows = model.groups.iter().flat_map(|group| &group.rows);
        for row in rows {
            assert_eq!(row.cells.len(), 24, "{}", row.name);
            // The grid opens on the previous operator's schedule (spec §16.3).
            assert!(
                row.cells.iter().all(|cell| cell.on),
                "the inherited schedule commits every row all day"
            );
            assert!(row.agc.on, "and puts it on AGC: {}", row.name);
            assert_eq!(row.name, Names::new(&l10n, game.session()).plant(row.plant));
        }
        let nuclear = first_row(&model, PlantKind::Nuclear);
        assert_eq!(
            nuclear.cells[6].label,
            format!("{} at 06:00: committed", nuclear.name)
        );
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn a_toggle_edits_the_draft_and_apply_sends_it_as_commit_plant() {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        let mut ui = UiState::default();
        let form = CommitmentForm::default();
        let plant = first_row(&model(&game, &ui, &form), PlantKind::Coal).plant;

        // Nothing to apply on a row nobody has touched.
        let clean = model(&game, &ui, &form);
        let row = first_row(&clean, PlantKind::Coal);
        assert!(!row.dirty);
        assert!(!row.apply.enabled && !row.revert.enabled);

        // Toggling hour 6 edits that row's draft and sends nothing to the game.
        let toggle = &row.cells[6].toggle;
        let mut expected = [false; 24];
        expected[6] = true;
        assert_eq!(
            toggle,
            &vec![UiAction::EditCommitment {
                plant,
                hours: expected
            }]
        );
        for action in toggle {
            ui.apply(action);
        }

        let edited = model(&game, &ui, &form);
        let row = first_row(&edited, PlantKind::Coal);
        assert!(row.dirty);
        assert!(row.cells[6].on, "the cell shows the draft");
        assert!(row.cells[6].label.ends_with("06:00: committed"));
        assert!(row.apply.enabled && row.revert.enabled);
        assert_eq!(
            row.apply.action,
            UiAction::Command(Command::CommitPlant {
                plant,
                hours: expected
            })
        );
        assert_eq!(row.revert.action, UiAction::DiscardCommitmentDraft);
        assert!(
            game.session()
                .plant_views()
                .iter()
                .all(|p| p.commitment == [false; 24]),
            "the game has heard nothing"
        );
    }

    #[test]
    fn commitment_row_emits_commit_plant_on_apply() {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        let mut ui = UiState::default();
        let form = CommitmentForm::default();
        let plant = first_row(&model(&game, &ui, &form), PlantKind::Nuclear).plant;
        // A feasible pattern: nuclear is all-day or all-off, so use the everything-on edit.
        ui.apply(&UiAction::EditCommitment {
            plant,
            hours: [true; 24],
        });
        let edited = model(&game, &ui, &form);
        let apply = first_row(&edited, PlantKind::Nuclear).apply.clone();

        assert_eq!(
            apply.action,
            UiAction::Command(Command::CommitPlant {
                plant,
                hours: [true; 24]
            })
        );
        let UiAction::Command(command) = apply.action.clone() else {
            unreachable!()
        };
        game.execute(command).unwrap();
        ui.apply(&apply.action);
        ui.settle_commit();

        let applied = model(&game, &ui, &form);
        let row = first_row(&applied, PlantKind::Nuclear);
        assert!(
            row.cells.iter().all(|cell| cell.on),
            "the domain now holds it"
        );
        assert!(!row.dirty);
        assert_eq!(ui.draft, None);
    }

    #[test]
    fn infeasible_commit_reverts_the_pending_row() {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        let mut ui = UiState::default();
        let form = CommitmentForm::default();
        let nuclear = first_row(&model(&game, &ui, &form), PlantKind::Nuclear).plant;
        // Nuclear needs 48 hours to start: any off-gap is infeasible.
        let infeasible = pattern(0..12);
        ui.apply(&UiAction::EditCommitment {
            plant: nuclear,
            hours: infeasible,
        });
        let edited = model(&game, &ui, &form);
        let apply = first_row(&edited, PlantKind::Nuclear).apply.clone();
        assert!(first_row(&edited, PlantKind::Nuclear).dirty);

        // The row sends it: the draft is still up, now pending.
        ui.apply(&apply.action);
        assert_eq!(ui.pending.map(|pending| pending.plant), Some(nuclear));
        assert!(ui.draft.is_some());

        // The domain refuses, and the session is unchanged.
        let UiAction::Command(command) = apply.action else {
            unreachable!()
        };
        let before = game.session().state_hash();
        let refusal = game.execute(command).unwrap_err();
        assert!(
            matches!(
                refusal,
                CommandError::Domain(DomainError::CommitmentInfeasible { .. })
            ),
            "{refusal:?}"
        );
        assert_eq!(game.session().state_hash(), before);

        // The glue settles the pending commit this frame, and the row is the domain's again.
        ui.settle_commit();
        let reverted = model(&game, &ui, &form);
        let row = first_row(&reverted, PlantKind::Nuclear);
        assert!(!row.dirty);
        assert!(row.cells.iter().all(|cell| !cell.on));
        assert!(!row.apply.enabled);
        assert_eq!(ui.pending, None);
        assert_eq!(ui.draft, None);
    }

    #[test]
    fn editing_another_row_sends_the_row_being_left() {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        let mut ui = UiState::default();
        let form = CommitmentForm::default();
        let clean = model(&game, &ui, &form);
        let (coal, peaker) = (
            first_row(&clean, PlantKind::Coal).plant,
            first_row(&clean, PlantKind::Peaker).plant,
        );
        ui.apply(&UiAction::EditCommitment {
            plant: coal,
            hours: pattern(6..18),
        });

        let editing = model(&game, &ui, &form);

        // A cell of the same row only edits it.
        let own = &first_row(&editing, PlantKind::Coal).cells[20].toggle;
        assert_eq!(own.len(), 1);
        // A cell of any other row first sends the draft being left, then starts its own.
        let other = &first_row(&editing, PlantKind::Peaker).cells[3].toggle;
        let mut flipped = [false; 24];
        flipped[3] = true;
        assert_eq!(
            other,
            &vec![
                UiAction::Command(Command::CommitPlant {
                    plant: coal,
                    hours: pattern(6..18)
                }),
                UiAction::EditCommitment {
                    plant: peaker,
                    hours: flipped
                },
            ]
        );
    }

    #[test]
    fn a_draft_equal_to_the_committed_pattern_is_not_sent_when_leaving() {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        let mut ui = UiState::default();
        let form = CommitmentForm::default();
        let clean = model(&game, &ui, &form);
        let coal = first_row(&clean, PlantKind::Coal).plant;
        // Toggled on and off again: the same as the committed pattern.
        ui.apply(&UiAction::EditCommitment {
            plant: coal,
            hours: [false; 24],
        });

        let editing = model(&game, &ui, &form);

        let other = &first_row(&editing, PlantKind::Peaker).cells[3].toggle;
        assert_eq!(other.len(), 1, "{other:?}");
        assert!(!has_unsaved_draft(&game, &ui));
    }

    #[test]
    fn all_day_and_off_are_start_and_stop() {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        let ui = UiState::default();
        let form = CommitmentForm::default();
        let clean = model(&game, &ui, &form);
        let row = first_row(&clean, PlantKind::Coal);
        let plant = row.plant;
        assert_eq!(
            row.all_day.action,
            UiAction::Command(Command::StartPlant { plant })
        );
        assert_eq!(
            row.off.action,
            UiAction::Command(Command::StopPlant { plant })
        );
        assert!(row.all_day.enabled);
        assert!(!row.off.enabled, "it is off already");

        game.execute(Command::StartPlant { plant }).unwrap();

        let all_day = model(&game, &ui, &form);
        let row = first_row(&all_day, PlantKind::Coal);
        assert!(row.cells.iter().all(|cell| cell.on));
        assert!(!row.all_day.enabled);
        assert!(row.off.enabled);
    }

    #[test]
    fn the_agc_switch_flips_the_plants_setting() {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        let ui = UiState::default();
        let form = CommitmentForm::default();
        let clean = model(&game, &ui, &form);
        let row = first_row(&clean, PlantKind::Ccgt);
        let plant = row.plant;
        assert!(!row.agc.on);
        assert_eq!(
            row.agc.button.action,
            UiAction::Command(Command::SetAutoFollow { plant, on: true })
        );

        game.execute(Command::SetAutoFollow { plant, on: true })
            .unwrap();

        let on = model(&game, &ui, &form);
        let row = first_row(&on, PlantKind::Ccgt);
        assert!(row.agc.on);
        assert_eq!(
            row.agc.button.action,
            UiAction::Command(Command::SetAutoFollow { plant, on: false })
        );
    }

    #[test]
    fn initial_target_outside_bounds_is_not_sent() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let ui = UiState::default();
        let clean = model(&game, &ui, &CommitmentForm::default());
        let plant = first_row(&clean, PlantKind::Coal).plant;
        let spec = session.region().spec(PlantKind::Coal);
        let (min, capacity) = (spec.min_output().value(), spec.capacity.value());
        let current = session
            .plant_views()
            .iter()
            .find(|p| p.id == plant)
            .unwrap()
            .initial_target;
        let with = |text: &str| {
            let mut form = CommitmentForm::default();
            form.targets.insert(plant, text.to_owned());
            let built = commitment_model(&game, &l10n, &ui, &form);
            first_row(&built, PlantKind::Coal).target.clone()
        };

        // The field starts on the plant's own target, and the range is shown beside it.
        let untouched = first_row(&clean, PlantKind::Coal).target.clone();
        assert_eq!(untouched.text, format::plain_megawatts(current));
        assert_eq!(
            untouched.range,
            format!(
                "{}–{} MW",
                format::plain_megawatts(spec.min_output()),
                format::plain_megawatts(spec.capacity)
            )
        );
        assert!(untouched.valid);
        assert!(!untouched.set.enabled, "it would change nothing");

        // Out of range, not a number, not finite: nothing can be sent.
        for text in [
            format!("{}", min - 1.0),
            format!("{}", capacity + 1.0),
            "0".to_owned(),
            "-5".to_owned(),
            "abc".to_owned(),
            String::new(),
            "NaN".to_owned(),
            "inf".to_owned(),
            "1e999".to_owned(),
        ] {
            let target = with(&text);
            assert!(!target.valid, "{text:?}");
            assert!(!target.set.enabled, "{text:?}");
            assert_eq!(target.text, text, "what was typed stays in the field");
        }

        // The bounds themselves, and anything between, can be.
        for value in [min, capacity, (min + capacity) / 2.0 + 0.5] {
            let text = format!("{value}");
            let target = with(&text);
            assert!(target.valid, "{text}");
            if value != current.value() {
                assert!(target.set.enabled, "{text}");
                assert_eq!(
                    target.set.action,
                    UiAction::Command(Command::SetDispatchTarget {
                        plant,
                        mw: Megawatts::new(value)
                    })
                );
            }
        }
        // Whitespace around a number is fine.
        assert!(with(&format!("  {}  ", capacity)).valid);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn a_valid_target_is_accepted_by_the_domain() {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let l10n = shipped_l10n();
        let clean = model(&game, &UiState::default(), &CommitmentForm::default());
        let plant = first_row(&clean, PlantKind::Coal).plant;
        let spec = game.session().region().spec(PlantKind::Coal).clone();
        let mut form = CommitmentForm::default();
        let mid = (spec.min_output().value() + spec.capacity.value()) / 2.0 + 0.5;
        form.targets.insert(plant, format!("{mid}"));
        let built = commitment_model(&game, &l10n, &UiState::default(), &form);
        let set = first_row(&built, PlantKind::Coal).target.set.clone();
        assert!(set.enabled);

        let UiAction::Command(command) = set.action else {
            unreachable!()
        };
        game.execute(command).unwrap();

        let after = commitment_model(&game, &l10n, &UiState::default(), &form);
        let target = &first_row(&after, PlantKind::Coal).target;
        assert!(!target.set.enabled, "now it is the target");
        assert_eq!(
            game.session()
                .plant_views()
                .iter()
                .find(|p| p.id == plant)
                .unwrap()
                .initial_target,
            Megawatts::new(mid)
        );
    }

    #[test]
    fn hydro_rows_show_the_days_water_and_the_others_do_not() {
        let game = shipped_game(
            "western_interconnection",
            Box::new(InMemoryCommandLog::default()),
        );
        let l10n = shipped_l10n();

        let built = commitment_model(
            &game,
            &l10n,
            &UiState::default(),
            &CommitmentForm::default(),
        );

        let rows: Vec<&CommitmentRow> = built.groups.iter().flat_map(|group| &group.rows).collect();
        let mut hydro_rows = 0;
        for row in rows {
            if row.kind == PlantKind::Hydro {
                hydro_rows += 1;
                let view = game
                    .session()
                    .plant_views()
                    .into_iter()
                    .find(|p| p.id == row.plant)
                    .unwrap();
                let energy = view.hydro_day_budget.expect("a commissioned hydro plant");
                assert!(energy.value() > 0.0, "{energy:?}");
                assert_eq!(
                    row.budget,
                    Some(format!(
                        "Water for the day: {}",
                        format::megawatt_hours(energy)
                    ))
                );
            } else {
                assert_eq!(row.budget, None, "{}", row.name);
            }
        }
        assert!(
            hydro_rows > 0,
            "the hydro-led region must exercise budget rows"
        );
        assert_eq!(
            hydro_rows,
            game.session()
                .plant_views()
                .iter()
                .filter(|plant| plant.kind == PlantKind::Hydro)
                .count(),
            "every hydro plant has a water-budget row"
        );
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn a_header_click_tints_its_column_and_a_second_click_clears_it() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let mut ui = UiState::default();
        let form = CommitmentForm::default();

        let plain = model(&game, &ui, &form);
        assert_eq!(plain.hours[18].action, UiAction::ShowHour(Some(18)));
        ui.apply(&plain.hours[18].action);

        let tinted = model(&game, &ui, &form);
        assert!(tinted.hours[18].highlighted);
        assert!(!tinted.hours[17].highlighted);
        assert!(first_row(&tinted, PlantKind::Coal).cells[18].highlighted);
        assert!(!first_row(&tinted, PlantKind::Coal).cells[17].highlighted);
        assert_eq!(tinted.hours[18].action, UiAction::ShowHour(None));
    }
}
