//! HVDC presenter and controls. Every edit goes through the command dispatcher.
use super::{ActionSink, ButtonModel, Prompt, UiAction, button, dispatch};
use crate::{
    l10n::format::MoneyStyle,
    l10n::{Localizer, Names, format},
    sim::Session,
};
use application::{Command, Game};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::{grid::LineStatus, ids::LinkId, session::DayPhase};

#[derive(Debug, Clone, PartialEq)]
pub struct LinkCardModel {
    pub id: LinkId,
    pub title: String,
    pub stats: Vec<String>,
    pub schedule: [f64; 24],
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
    pub close: ButtonModel,
}
pub fn links_model(game: &Game, l10n: &Localizer, style: &MoneyStyle) -> Option<LinksModel> {
    let session = game.session();
    let spec = session.region().hvdc()?;
    let names = Names::new(l10n, session);
    let utilisation = session.link_utilisation(session.clock().month());
    let cards = session
        .power()
        .network()
        .links()
        .map(|link| {
            let title = l10n.args(
                "hvdc-route",
                &[
                    ("from", names.node(link.from).into()),
                    ("to", names.node(link.to).into()),
                ],
            );
            let quote = session
                .link_construction(link.id)
                .expect("immutable link terms");
            let mut stats = vec![
                l10n.args(
                    "hvdc-payment",
                    &[(
                        "value",
                        super::build::monthly_payment(quote.capital, quote.months, style).into(),
                    )],
                ),
                l10n.args(
                    "hvdc-rating",
                    &[(
                        "value",
                        format::megawatts(domain::units::Megawatts::new(spec.rating_mw)).into(),
                    )],
                ),
                l10n.args(
                    "hvdc-flow",
                    &[("value", format::megawatts(link.flow).into())],
                ),
                l10n.args(
                    "hvdc-loss",
                    &[("value", format::megawatts(link.loss).into())],
                ),
                l10n.args(
                    "hvdc-capital",
                    &[
                        ("value", format::money(quote.capital, style).into()),
                        ("months", quote.months.into()),
                    ],
                ),
                l10n.args(
                    "hvdc-utilisation",
                    &[(
                        "value",
                        format!(
                            "{:.1}",
                            100.0 * utilisation.get(&link.id).copied().unwrap_or(0.0)
                        )
                        .into(),
                    )],
                ),
            ];
            if link.blocked {
                stats.push(l10n.text("hvdc-blocked"));
            } else if link.runback {
                stats.push(l10n.text("hvdc-runback"));
            }
            let building = if let LineStatus::UnderConstruction { months_left } = link.status {
                stats.push(l10n.args("map-months-left", &[("months", months_left.into())]));
                true
            } else {
                false
            };
            LinkCardModel {
                id: link.id,
                title,
                stats,
                schedule: link.schedule,
                rating: spec.rating_mw,
                target: link.target((session.clock().tick() / 60).min(23) as usize),
                planning: session.phase() == DayPhase::Planning,
                operating: session.phase() == DayPhase::Operating && !building,
                schedule_label: l10n.text("hvdc-schedule"),
                target_label: l10n.text("hvdc-target"),
                clear: ButtonModel::new(
                    l10n.text("hvdc-clear-override"),
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
    Some(LinksModel {
        title: l10n.text("hvdc-title"),
        empty: l10n.text("hvdc-empty"),
        cards,
        close: ButtonModel::new(l10n.text("hvdc-close"), UiAction::ShowLinks(false)),
    })
}
pub fn draw_links(ui: &mut egui::Ui, model: &LinksModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    if model.cards.is_empty() {
        ui.label(&model.empty);
    }
    egui::ScrollArea::vertical()
        .max_height(400.0)
        .show(ui, |ui| {
            for card in &model.cards {
                ui.push_id(card.id.0, |ui| {
                    ui.collapsing(&card.title, |ui| {
                        for stat in &card.stats {
                            ui.label(stat);
                        }
                        if card.planning {
                            ui.label(&card.schedule_label);
                            let mut values = card.schedule;
                            let mut changed = false;
                            egui::Grid::new("schedule").num_columns(2).show(ui, |ui| {
                                for (hour, mw) in values.iter_mut().enumerate() {
                                    ui.label(format::clock(hour as u32 * 60));
                                    changed |= ui
                                        .add(egui::Slider::new(mw, -card.rating..=card.rating))
                                        .changed();
                                    ui.end_row();
                                }
                            });
                            if changed {
                                actions.push(UiAction::Command(Command::SetLinkSchedule {
                                    link: card.id,
                                    mw: Box::new(values),
                                }));
                            }
                        }
                        if card.operating {
                            ui.label(&card.target_label);
                            let mut mw = card.target;
                            if ui
                                .add(egui::Slider::new(&mut mw, -card.rating..=card.rating))
                                .changed()
                            {
                                actions.push(UiAction::Command(Command::SetLinkTarget {
                                    link: card.id,
                                    mw,
                                }));
                            }
                            button(ui, &card.clear, &mut actions);
                        }
                        if let Some(cancel) = &card.cancel {
                            button(ui, cancel, &mut actions);
                        }
                    });
                });
            }
        });
    button(ui, &model.close, &mut actions);
    if super::action::cancelled(ui, &model.close) {
        return vec![model.close.action.clone()];
    }
    actions
}
pub(super) fn links_panel(
    mut contexts: EguiContexts,
    session: Res<Session>,
    l10n: Res<Localizer>,
    style: Res<MoneyStyle>,
    mut sink: ActionSink,
) -> Result {
    if !sink.ui_state().links_open || sink.open_prompt().0.is_some() {
        return Ok(());
    }
    let Some(model) = links_model(&session.0, &l10n, &style) else {
        return Ok(());
    };
    let mut open = true;
    let mut actions = Vec::new();
    egui::Window::new(&model.title)
        .id(egui::Id::new("hvdc_links"))
        .default_width(340.0)
        .constrain(true)
        .open(&mut open)
        .show(contexts.ctx_mut()?, |ui| {
            actions = draw_links(ui, &model);
        });
    if !open {
        actions.push(UiAction::ShowLinks(false));
    }
    dispatch(actions, &mut sink);
    Ok(())
}
