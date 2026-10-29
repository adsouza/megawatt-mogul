//! Focused HVDC equipment inspection. Presenters are pure; edits go through dispatch.
use super::theme::TextTone;
use super::top_bar::Stat;
use super::{ActionSink, ButtonModel, Prompt, ToggleModel, UiAction, UiState, button, dispatch};
use crate::{
    l10n::{Localizer, Names, format, format::MoneyStyle},
    sim::Session,
};
use application::{Command, Game};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::{
    grid::{ConverterStation, ConverterStationId, LineStatus},
    ids::{LinkId, NodeId},
    session::DayPhase,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HvdcTab {
    #[default]
    Overview,
    Dispatch,
    Costs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HvdcRangeForm {
    pub link: Option<LinkId>,
    pub from: u8,
    pub to: u8,
    pub mw: String,
}
impl Default for HvdcRangeForm {
    fn default() -> Self {
        Self {
            link: None,
            from: 0,
            to: 24,
            mw: "0".into(),
        }
    }
}

pub(super) fn schedule_range(
    schedule: &[f64; 24],
    form: &HvdcRangeForm,
    rating: f64,
) -> Option<[f64; 24]> {
    let mw: f64 = form.mw.trim().parse().ok()?;
    if !mw.is_finite() || mw.abs() > rating || form.from >= form.to || form.to > 24 {
        return None;
    }
    let mut result = *schedule;
    result[usize::from(form.from)..usize::from(form.to)].fill(mw);
    Some(result)
}

#[derive(Debug, Clone, PartialEq)]
pub struct HvdcTerminalModel {
    pub id: ConverterStationId,
    pub name: String,
    pub grid: String,
    pub service: String,
    pub support: String,
    pub tone: TextTone,
    pub focused: bool,
    pub select: ButtonModel,
    pub forecast: Option<ButtonModel>,
    pub activation: Option<ButtonModel>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct LinkCardModel {
    pub id: LinkId,
    pub title: String,
    pub status: String,
    pub tone: TextTone,
    pub stats: Vec<Stat>,
    pub costs: Vec<Stat>,
    pub terminals: [HvdcTerminalModel; 2],
    pub notices: Vec<String>,
    pub schedule: [f64; 24],
    pub auto_follow: bool,
    pub modes: Vec<ToggleModel>,
    pub control_status: String,
    pub rating: f64,
    pub target: f64,
    pub planning: bool,
    pub operating: bool,
    pub schedule_label: String,
    pub target_label: String,
    pub clear: ButtonModel,
    pub cancel: Option<ButtonModel>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct LinksModel {
    pub title: String,
    pub empty: String,
    pub cards: Vec<LinkCardModel>,
    pub activations: Vec<super::ActivationCardModel>,
    pub selected: Option<LinkId>,
    pub tabs: Vec<ToggleModel>,
    pub tab: HvdcTab,
    pub form: HvdcRangeForm,
    pub project_label: String,
    pub from_label: String,
    pub until_label: String,
    pub mw_label: String,
    pub apply_label: String,
    pub validation: String,
    pub direction: String,
    pub automatic_help: String,
    pub read_only: String,
    pub close: ButtonModel,
}
pub(super) fn planning_issues(
    session: &domain::session::GameSession,
    l10n: &Localizer,
    selected: Option<domain::ids::NodeId>,
) -> Vec<String> {
    session
        .power()
        .network()
        .links()
        .filter(|link| {
            selected.is_none_or(|id| {
                session
                    .power()
                    .link_planning_forecast(link.id)
                    .is_some_and(|plan| plan.from_island == Some(id) || plan.to_island == Some(id))
            })
        })
        .flat_map(|link| link_planning_issues(session, link.id, l10n))
        .collect()
}

/// The same eligibility used by the supply forecast, explained at the actual route.
pub(super) fn link_planning_issues(
    session: &domain::session::GameSession,
    id: LinkId,
    l10n: &Localizer,
) -> Vec<String> {
    let Some(link) = session.power().network().link(id) else {
        return Vec::new();
    };
    let Some(plan) = session.power().link_planning_forecast(id) else {
        return Vec::new();
    };
    let names = Names::new(l10n, session);
    let route = l10n.args(
        "hvdc-route",
        &[
            ("from", names.converter(link.from).into()),
            ("to", names.converter(link.to).into()),
        ],
    );
    let mut issues = Vec::new();
    for id in [plan.from_island, plan.to_island].into_iter().flatten() {
        if !session.power().island_active(id) {
            issues.push(l10n.args(
                "hvdc-service-inactive",
                &[
                    ("route", route.clone().into()),
                    ("island", names.island(session, id).into()),
                ],
            ));
        }
    }
    if let LineStatus::UnderConstruction { months_left } = link.status {
        issues.push(l10n.args(
            "hvdc-planning-construction",
            &[
                ("route", route.clone().into()),
                ("months", months_left.into()),
            ],
        ));
    } else {
        for (island, node, ready) in [
            (plan.from_island, link.from_grid(), plan.from_ready),
            (plan.to_island, link.to_grid(), plan.to_ready),
        ] {
            if ready.iter().all(|ready| *ready) {
                continue;
            }
            let mut ranges = Vec::new();
            let mut h = 0;
            while h < 24 {
                if ready[h] {
                    h += 1;
                    continue;
                }
                let start = h;
                while h < 24 && !ready[h] {
                    h += 1;
                }
                ranges.push(l10n.args(
                    "forecast-hour-range",
                    &[
                        ("start", format::clock(start as u32 * 60).into()),
                        ("end", format::clock(h as u32 * 60).into()),
                    ],
                ));
            }
            issues.push(
                l10n.args(
                    "hvdc-planning-terminal",
                    &[
                        ("route", route.clone().into()),
                        (
                            "island",
                            island
                                .map_or_else(|| names.node(node), |id| names.island(session, id))
                                .into(),
                        ),
                        ("hours", ranges.join(", ").into()),
                    ],
                ),
            );
        }
    }
    if !link.auto_follow && link.schedule.iter().all(|mw| *mw == 0.0) {
        issues.push(l10n.args("hvdc-planning-zero", &[("route", route.into())]));
    }
    issues
}

pub fn links_model(game: &Game, l10n: &Localizer, style: &MoneyStyle) -> Option<LinksModel> {
    links_model_with_ui(game, l10n, style, &UiState::default())
}
pub fn links_model_with_ui(
    game: &Game,
    l10n: &Localizer,
    style: &MoneyStyle,
    ui: &UiState,
) -> Option<LinksModel> {
    links_model_cached(game, l10n, style, None, ui)
}
fn links_model_cached(
    game: &Game,
    l10n: &Localizer,
    style: &MoneyStyle,
    cached: Option<&super::PlanningForecast>,
    ui: &UiState,
) -> Option<LinksModel> {
    let session = game.session();
    let spec = session.region().hvdc()?;
    let names = Names::new(l10n, session);
    let utilisation = session.link_utilisation(session.clock().month());
    let schedules = cached
        .map(|f| f.link_schedules.clone())
        .unwrap_or_else(|| session.link_schedules());
    let activations: Vec<_> = cached
        .map(|f| f.activations.values().cloned().collect())
        .unwrap_or_else(|| {
            session
                .power()
                .island_nodes()
                .keys()
                .filter(|id| !session.power().island_active(**id))
                .filter_map(|id| super::activation::activation_card(session, *id, l10n))
                .collect()
        });
    let cards: Vec<_> = session
        .power()
        .network()
        .links()
        .map(|link| {
            let plan = session
                .power()
                .link_planning_forecast(link.id)
                .expect("existing link");
            let schedule = schedules[&link.id];
            let building = matches!(link.status, LineStatus::UnderConstruction { .. });
            let inactive = [plan.from_island, plan.to_island]
                .iter()
                .any(|id| id.is_none_or(|id| !session.power().island_active(id)));
            let unsupported = (0..24).any(|h| schedule[h] != 0.0 && !plan.available(h));
            let zero = schedule.iter().all(|mw| *mw == 0.0);
            let (status, tone) = if let LineStatus::UnderConstruction { months_left } = link.status
            {
                (
                    l10n.args("hvdc-status-building", &[("months", months_left.into())]),
                    TextTone::Information,
                )
            } else if inactive {
                (l10n.text("hvdc-status-inactive"), TextTone::Caution)
            } else if session.phase() == DayPhase::Planning {
                let (key, tone) = if unsupported {
                    ("hvdc-status-unavailable", TextTone::Negative)
                } else if zero {
                    ("hvdc-status-standby", TextTone::Neutral)
                } else {
                    ("hvdc-status-ready", TextTone::Positive)
                };
                (l10n.text(key), tone)
            } else if link.blocked {
                (l10n.text("hvdc-status-blocked"), TextTone::Negative)
            } else if link.runback {
                (l10n.text("hvdc-status-runback"), TextTone::Caution)
            } else if link.flow.value() != 0.0 {
                (l10n.text("hvdc-status-transferring"), TextTone::Positive)
            } else {
                (l10n.text("hvdc-status-standby"), TextTone::Neutral)
            };
            let title = l10n.args(
                "hvdc-project-title",
                &[
                    ("id", link.id.0.to_string().into()),
                    ("from", names.node(link.from_grid()).into()),
                    ("to", names.node(link.to_grid()).into()),
                ],
            );
            let terminal = |station: ConverterStation,
                            island: Option<NodeId>,
                            ready: [bool; 24]| {
                let active = island.is_some_and(|id| session.power().island_active(id));
                let count = ready.iter().filter(|ready| **ready).count() as u32;
                let live_support = island.is_some_and(|id| {
                    active
                        && session
                            .power()
                            .island_states()
                            .get(&id)
                            .is_some_and(|state| state.mode() != domain::grid::GridMode::Blackout)
                        && session
                            .power()
                            .island_nodes()
                            .get(&id)
                            .is_some_and(|nodes| {
                                session.power().plants().values().any(|p| {
                                    nodes.contains(&p.node())
                                        && p.kind().is_dispatchable()
                                        && p.is_online()
                                })
                            })
                });
                let support = if session.phase() == DayPhase::Planning {
                    if count == 24 {
                        l10n.text("hvdc-support-all-day")
                    } else {
                        l10n.args(
                            "hvdc-support-missing",
                            &[(
                                "hours",
                                super::activation::hours_text(
                                    &std::array::from_fn(|h| !ready[h]),
                                    l10n,
                                )
                                .into(),
                            )],
                        )
                    }
                } else {
                    l10n.text(if live_support {
                        "hvdc-terminal-live-support"
                    } else {
                        "hvdc-terminal-live-blocked"
                    })
                };
                let supported = if session.phase() == DayPhase::Planning {
                    active && count == 24
                } else {
                    live_support
                };
                HvdcTerminalModel {
                    id: station.id,
                    name: names.converter(station),
                    grid: island.map_or_else(
                        || names.node(station.grid_node),
                        |id| names.island(session, id),
                    ),
                    service: l10n.text(if active {
                        "hvdc-service-active"
                    } else {
                        "hvdc-service-pending"
                    }),
                    support,
                    tone: if supported {
                        TextTone::Positive
                    } else {
                        TextTone::Caution
                    },
                    focused: ui.hvdc_terminal == Some(station.id),
                    select: ButtonModel::new(
                        names.converter(station),
                        UiAction::InspectHvdc {
                            link: link.id,
                            station: Some(station.id),
                        },
                    ),
                    forecast: island.map(|id| {
                        ButtonModel::new(
                            l10n.text("hvdc-open-grid"),
                            UiAction::InspectForecast {
                                island: id,
                                hour: 0,
                            },
                        )
                    }),
                    activation: island.and_then(|id| {
                        activations
                            .iter()
                            .find(|a| a.island == id)
                            .map(|a| a.review.clone())
                    }),
                }
            };
            let stat = |key: &str, value: String| Stat {
                label: l10n.text(key),
                value,
            };
            let quote = session
                .link_construction(link.id)
                .expect("immutable link terms");
            let mut notices = Vec::new();
            if session.phase() == DayPhase::Planning {
                if zero && !link.auto_follow {
                    notices.push(l10n.text("hvdc-no-schedule"));
                }
                if inactive {
                    notices.push(l10n.text("hvdc-inactive-summary"));
                }
                if unsupported {
                    notices.push(l10n.text("hvdc-unsupported-summary"));
                }
            } else if link.blocked {
                notices.push(l10n.text("hvdc-blocked"));
            } else if link.runback {
                notices.push(l10n.text("hvdc-runback"));
            }
            LinkCardModel {
                id: link.id,
                title,
                status,
                tone,
                stats: if session.phase() == DayPhase::Planning {
                    let low = schedule.iter().copied().fold(f64::INFINITY, f64::min);
                    let high = schedule.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    vec![
                        stat(
                            "hvdc-metric-rating",
                            format::megawatts(domain::units::Megawatts::new(spec.rating_mw)),
                        ),
                        stat(
                            "hvdc-metric-schedule-range",
                            l10n.args(
                                "hvdc-value-schedule-range",
                                &[
                                    (
                                        "low",
                                        format::megawatts(domain::units::Megawatts::new(low))
                                            .into(),
                                    ),
                                    (
                                        "high",
                                        format::megawatts(domain::units::Megawatts::new(high))
                                            .into(),
                                    ),
                                ],
                            ),
                        ),
                        stat(
                            "hvdc-metric-scheduled-hours",
                            l10n.args(
                                "hvdc-value-scheduled-hours",
                                &[(
                                    "hours",
                                    (schedule.iter().filter(|mw| **mw != 0.0).count() as u32)
                                        .into(),
                                )],
                            ),
                        ),
                    ]
                } else {
                    vec![
                        stat(
                            if session.phase() == DayPhase::Review {
                                "hvdc-metric-last-flow"
                            } else {
                                "hvdc-metric-flow"
                            },
                            format::megawatts(link.flow),
                        ),
                        stat(
                            "hvdc-metric-rating",
                            format::megawatts(domain::units::Megawatts::new(spec.rating_mw)),
                        ),
                        stat(
                            if session.phase() == DayPhase::Review {
                                "hvdc-metric-last-loss"
                            } else {
                                "hvdc-metric-loss"
                            },
                            format::megawatts(link.loss),
                        ),
                    ]
                },
                costs: vec![
                    stat("hvdc-metric-capital", format::money(quote.capital, style)),
                    stat(
                        "hvdc-metric-payment",
                        super::build::monthly_payment(quote.capital, quote.months, style),
                    ),
                    stat(
                        "hvdc-metric-utilisation",
                        format!(
                            "{:.1}%",
                            100.0 * utilisation.get(&link.id).copied().unwrap_or(0.0)
                        ),
                    ),
                ],
                terminals: [
                    terminal(link.from, plan.from_island, plan.from_ready),
                    terminal(link.to, plan.to_island, plan.to_ready),
                ],
                notices,
                schedule,
                auto_follow: link.auto_follow,
                modes: [(true, "hvdc-mode-automatic"), (false, "hvdc-mode-manual")]
                    .into_iter()
                    .map(|(on, key)| ToggleModel {
                        button: ButtonModel::new(
                            l10n.text(key),
                            UiAction::Command(Command::SetLinkAutoFollow { link: link.id, on }),
                        )
                        .enabled_if(
                            session.phase() == DayPhase::Planning
                                || (session.phase() == DayPhase::Operating && !building),
                        ),
                        on: link.auto_follow == on,
                    })
                    .collect(),
                control_status: if link.override_mw.is_some() {
                    l10n.text("hvdc-control-override")
                } else if link.auto_follow {
                    let target = if session.phase() == DayPhase::Planning {
                        schedule[0]
                    } else {
                        link.automatic_target
                    };
                    if target.abs() < 0.1 {
                        l10n.text("hvdc-control-standby")
                    } else {
                        let (sender, receiver) = if target > 0.0 {
                            (link.from_grid(), link.to_grid())
                        } else {
                            (link.to_grid(), link.from_grid())
                        };
                        l10n.args(
                            "hvdc-control-transfer",
                            &[
                                (
                                    "mw",
                                    format::megawatts(domain::units::Megawatts::new(target.abs()))
                                        .into(),
                                ),
                                ("from", names.node(sender).into()),
                                ("to", names.node(receiver).into()),
                            ],
                        )
                    }
                } else {
                    l10n.text("hvdc-control-manual")
                },
                rating: spec.rating_mw,
                target: link.target((session.clock().tick() / 60).min(23) as usize),
                planning: session.phase() == DayPhase::Planning,
                operating: session.phase() == DayPhase::Operating && !building,
                schedule_label: l10n.text(if link.auto_follow {
                    "hvdc-automatic-profile"
                } else {
                    "hvdc-schedule"
                }),
                target_label: l10n.text("hvdc-target"),
                clear: ButtonModel::new(
                    l10n.text(if link.auto_follow {
                        "hvdc-resume-automatic"
                    } else {
                        "hvdc-clear-override"
                    }),
                    UiAction::Command(Command::ClearLinkOverride { link: link.id }),
                )
                .enabled_if(link.override_mw.is_some() && session.phase() == DayPhase::Operating),
                cancel: (building && session.phase() == DayPhase::Planning).then(|| {
                    ButtonModel::new(
                        l10n.text("line-cancel-confirm"),
                        UiAction::Prompt(Some(Prompt::CancelLinkConstruction { link: link.id })),
                    )
                }),
            }
        })
        .collect();
    let selected = ui
        .hvdc_selected
        .filter(|id| cards.iter().any(|c| c.id == *id))
        .or_else(|| cards.first().map(|c| c.id));
    let form = if ui.hvdc_range.link == selected {
        ui.hvdc_range.clone()
    } else {
        HvdcRangeForm {
            link: selected,
            ..Default::default()
        }
    };
    Some(LinksModel {
        title: l10n.text("hvdc-title"),
        empty: l10n.text("hvdc-empty"),
        cards,
        activations,
        selected,
        tabs: [
            (HvdcTab::Overview, "hvdc-tab-overview"),
            (HvdcTab::Dispatch, "hvdc-tab-dispatch"),
            (HvdcTab::Costs, "hvdc-tab-costs"),
        ]
        .into_iter()
        .map(|(tab, key)| ToggleModel {
            button: ButtonModel::new(l10n.text(key), UiAction::SetHvdcTab(tab)),
            on: ui.hvdc_tab == tab,
        })
        .collect(),
        tab: ui.hvdc_tab,
        form,
        project_label: l10n.text("hvdc-project-picker"),
        from_label: l10n.text("hvdc-range-from"),
        until_label: l10n.text("hvdc-range-until"),
        mw_label: l10n.text("hvdc-range-mw"),
        apply_label: l10n.text("hvdc-range-apply"),
        validation: l10n.text("hvdc-range-validation"),
        direction: l10n.text("hvdc-direction-help"),
        automatic_help: l10n.text("hvdc-automatic-help"),
        read_only: l10n.text("hvdc-read-only"),
        close: ButtonModel::new(l10n.text("hvdc-close"), UiAction::ShowLinks(false)),
    })
}

fn draw_stats(ui: &mut egui::Ui, id: &str, stats: &[Stat]) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([18.0, 6.0])
        .show(ui, |ui| {
            for stat in stats {
                ui.weak(&stat.label);
                ui.label(&stat.value);
                ui.end_row();
            }
        });
}
fn draw_terminal(ui: &mut egui::Ui, terminal: &HvdcTerminalModel, actions: &mut Vec<UiAction>) {
    let border = if terminal.focused {
        TextTone::Information.color()
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke.color
    };
    egui::Frame::group(ui.style())
        .stroke(egui::Stroke::new(
            if terminal.focused { 2.0 } else { 1.0 },
            border,
        ))
        .show(ui, |ui| {
            ui.set_min_width(0.0);
            button(ui, &terminal.select, actions);
            ui.label(&terminal.grid);
            ui.colored_label(terminal.tone.color(), &terminal.service);
            ui.label(&terminal.support);
            if let Some(forecast) = &terminal.forecast {
                let before = actions.len();
                button(ui, forecast, actions);
                if actions.len() > before {
                    actions.push(UiAction::ShowLinks(false));
                }
            }
            if let Some(activate) = &terminal.activation {
                button(ui, activate, actions);
            }
        });
}
fn draw_profile(ui: &mut egui::Ui, card: &LinkCardModel) {
    use egui_plot::{Line, Plot};
    let points: Vec<[f64; 2]> = (0..24)
        .flat_map(|h| {
            [
                [h as f64, card.schedule[h]],
                [(h + 1) as f64, card.schedule[h]],
            ]
        })
        .collect();
    Plot::new(("hvdc_schedule", card.id.0))
        .height(125.0)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .show_axes([true, true])
        .include_x(0.0)
        .include_x(24.0)
        .include_y(0.0)
        .show(ui, |plot| {
            plot.line(
                Line::new(&card.schedule_label, points)
                    .color(TextTone::Information.color())
                    .width(2.0),
            );
        });
}
fn draw_range(
    ui: &mut egui::Ui,
    model: &LinksModel,
    card: &LinkCardModel,
    actions: &mut Vec<UiAction>,
) {
    let mut form = model.form.clone();
    form.link = Some(card.id);
    let before = form.clone();
    let mut submitted = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(&model.from_label);
        egui::ComboBox::from_id_salt("hvdc_range_from")
            .width(65.0)
            .selected_text(format::clock(u32::from(form.from) * 60))
            .show_ui(ui, |ui| {
                for hour in 0..24 {
                    ui.selectable_value(&mut form.from, hour, format::clock(u32::from(hour) * 60));
                }
            });
        ui.label(&model.until_label);
        egui::ComboBox::from_id_salt("hvdc_range_to")
            .width(65.0)
            .selected_text(format::clock(u32::from(form.to) * 60))
            .show_ui(ui, |ui| {
                for hour in 1..=24 {
                    ui.selectable_value(&mut form.to, hour, format::clock(u32::from(hour) * 60));
                }
            });
        ui.label(&model.mw_label);
        let field = ui.add(
            egui::TextEdit::singleline(&mut form.mw)
                .desired_width(75.0)
                .id_salt("hvdc_range_mw"),
        );
        let valid = schedule_range(&card.schedule, &form, card.rating).is_some();
        let apply = super::action::add_enabled(ui, valid, egui::Button::new(&model.apply_label));
        submitted = valid && (super::action::submitted(&field) || apply.clicked());
    });
    if schedule_range(&card.schedule, &form, card.rating).is_none() {
        ui.colored_label(TextTone::Caution.color(), &model.validation);
    }
    if form != before || submitted {
        actions.push(UiAction::EditHvdcRange(form));
    }
    if submitted {
        actions.push(UiAction::ApplyHvdcRange(card.id));
    }
}
fn draw_detail(
    ui: &mut egui::Ui,
    model: &LinksModel,
    card: &LinkCardModel,
    actions: &mut Vec<UiAction>,
) {
    match model.tab {
        HvdcTab::Overview => {
            for notice in &card.notices {
                ui.colored_label(TextTone::Caution.color(), notice);
            }
            ui.add_space(6.0);
            if ui.available_width() >= 510.0 {
                ui.columns(2, |columns| {
                    for (column, terminal) in columns.iter_mut().zip(&card.terminals) {
                        draw_terminal(column, terminal, actions);
                    }
                });
            } else {
                for terminal in &card.terminals {
                    draw_terminal(ui, terminal, actions);
                    ui.add_space(4.0);
                }
            }
        }
        HvdcTab::Dispatch => {
            ui.horizontal_wrapped(|ui| {
                for mode in &card.modes {
                    super::toggle_button(ui, mode, actions);
                }
            });
            ui.label(&card.control_status);
            if card.auto_follow {
                ui.weak(&model.automatic_help);
            }
            ui.label(&model.direction);
            draw_profile(ui, card);
            if card.planning && !card.auto_follow {
                draw_range(ui, model, card, actions);
            } else if card.operating {
                ui.label(&card.target_label);
                let mut target = card.target;
                if ui
                    .add(egui::Slider::new(&mut target, -card.rating..=card.rating))
                    .changed()
                {
                    actions.push(UiAction::Command(Command::SetLinkTarget {
                        link: card.id,
                        mw: target,
                    }));
                }
                button(ui, &card.clear, actions);
            } else {
                ui.weak(&model.read_only);
            }
        }
        HvdcTab::Costs => {
            draw_stats(ui, "hvdc_costs", &card.costs);
            if let Some(cancel) = &card.cancel {
                ui.add_space(12.0);
                button(ui, cancel, actions);
            }
        }
    }
}
pub fn draw_links(ui: &mut egui::Ui, model: &LinksModel) -> Vec<UiAction> {
    // Capture Escape before a text field consumes it. A popup gets Escape first.
    let dismiss = super::action::cancelled(ui, &model.close);
    let mut actions = Vec::new();
    let Some(card) = model
        .selected
        .and_then(|id| model.cards.iter().find(|card| card.id == id))
    else {
        ui.label(&model.empty);
        button(ui, &model.close, &mut actions);
        return if dismiss {
            vec![model.close.action.clone()]
        } else {
            actions
        };
    };
    let wide = ui.available_width() >= 700.0 && model.cards.len() > 1;
    if !wide {
        ui.horizontal_wrapped(|ui| {
            ui.weak(&model.project_label);
            egui::ComboBox::from_id_salt("hvdc_project_picker")
                .width(ui.available_width().max(80.0))
                .selected_text(&card.title)
                .show_ui(ui, |ui| {
                    for project in &model.cards {
                        if ui
                            .selectable_label(project.id == card.id, &project.title)
                            .clicked()
                        {
                            actions.push(UiAction::InspectHvdc {
                                link: project.id,
                                station: None,
                            });
                        }
                    }
                });
        });
    } else {
        ui.strong(&card.title);
    }
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(card.tone.color(), &card.status);
        for stat in &card.stats {
            ui.separator();
            ui.label(format!("{} {}", stat.label, stat.value));
        }
    });
    ui.horizontal_wrapped(|ui| {
        for tab in &model.tabs {
            super::toggle_button(ui, tab, &mut actions);
        }
    });
    ui.separator();
    let height = (ui.ctx().content_rect().height() - 250.0).clamp(140.0, 420.0);
    ui.push_id((card.id.0, model.tab as u8), |ui| {
        if wide {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(185.0);
                    egui::ScrollArea::vertical()
                        .id_salt("hvdc_catalogue")
                        .max_height(height)
                        .show(ui, |ui| {
                            for project in &model.cards {
                                if ui
                                    .selectable_label(project.id == card.id, &project.title)
                                    .clicked()
                                {
                                    actions.push(UiAction::InspectHvdc {
                                        link: project.id,
                                        station: None,
                                    });
                                }
                                ui.colored_label(project.tone.color(), &project.status);
                                ui.add_space(8.0);
                            }
                        });
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical()
                        .id_salt("hvdc_equipment_detail")
                        .max_height(height)
                        .show(ui, |ui| draw_detail(ui, model, card, &mut actions));
                });
            });
        } else {
            egui::ScrollArea::vertical()
                .id_salt("hvdc_equipment_detail")
                .max_height(height)
                .show(ui, |ui| draw_detail(ui, model, card, &mut actions));
        }
    });
    ui.separator();
    button(ui, &model.close, &mut actions);
    if dismiss {
        vec![model.close.action.clone()]
    } else {
        actions
    }
}

pub(super) fn links_panel(
    mut contexts: EguiContexts,
    session: Res<Session>,
    l10n: Res<Localizer>,
    style: Res<MoneyStyle>,
    forecast: Res<super::PlanningForecastCache>,
    mut sink: ActionSink,
) -> Result {
    if !sink.ui_state().links_open || sink.open_prompt().0.is_some() {
        return Ok(());
    }
    let Some(model) =
        links_model_cached(&session.0, &l10n, &style, forecast.get(), sink.ui_state())
    else {
        return Ok(());
    };
    let ctx = contexts.ctx_mut()?;
    let size = (ctx.content_rect().size() - egui::vec2(32.0, 32.0)).max(egui::vec2(200.0, 180.0));
    let mut open = true;
    let mut actions = Vec::new();
    egui::Window::new(&model.title)
        .id(egui::Id::new("hvdc_links"))
        .default_size(egui::vec2(780.0, 540.0))
        .max_size(size)
        .constrain(true)
        .open(&mut open)
        .show(ctx, |ui| {
            actions = draw_links(ui, &model);
        });
    if !open {
        actions.push(UiAction::ShowLinks(false));
    }
    dispatch(actions, &mut sink);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    proptest::proptest! {
        #[test]
        fn range_updates_preserve_other_hours_and_invalid_input_is_rejected(
            from in 0u8..24, to in 1u8..=24, mw in -1000.0f64..1000.0,
        ) {
            let schedule = std::array::from_fn(|h| h as f64 + 0.25);
            let form = HvdcRangeForm { link: Some(LinkId(1)), from, to, mw: mw.to_string() };
            let result = schedule_range(&schedule, &form, 1000.0);
            if from >= to { proptest::prop_assert!(result.is_none()); }
            else {
                let changed = result.unwrap();
                for h in 0..24 { proptest::prop_assert_eq!(changed[h], if (usize::from(from)..usize::from(to)).contains(&h) { mw } else { schedule[h] }); }
            }
            for text in ["NaN", "inf", "-inf", "1000.1", "", "bad"] {
                let invalid = HvdcRangeForm { mw: text.into(), ..form.clone() };
                proptest::prop_assert!(schedule_range(&schedule, &invalid, 1000.0).is_none());
            }
        }
    }
}
