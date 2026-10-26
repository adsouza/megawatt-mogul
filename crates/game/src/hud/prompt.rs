//! Confirmation prompts: a question before something the player can't undo.
//!
//! A button opens one with `UiAction::Prompt(Some(..))`; [`dispatch`] stores it in [`OpenPrompt`],
//! and [`prompt_overlay`] draws whichever prompt is open as a modal. Adding a prompt means adding
//! a [`Prompt`] variant and its case in [`prompt_model`] (and, if it belongs to one phase, in
//! [`prompt_in_phase`]); nothing else.
//!
//! - **Leave game** (the top bar's *Menu*): *Leave* returns to the menu; *Stay* closes it.
//! - **Shortfall start** (*Start the day* on a short forecast, spec §16.4): *Start anyway* sends
//!   `BeginOperating`; *Keep planning* closes it. It is Planning's: the frame that begins the day
//!   closes it, before any Operating panel draws.
//! - **Cancel line construction**: removes an unfinished order in Planning; *Keep building* is
//!   the initial keyboard focus, and backing out preserves the order.
//!
//! A click outside a prompt is its `dismiss`, which always backs out.

use application::{Command, Game};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::session::{DayPhase, SessionStatus};

use super::action::{ActionSink, ButtonModel, OpenPrompt, Prompt, UiAction, button, dispatch};
use super::planning_warnings::time_of;
use crate::l10n::{Localizer, Names, format};
use crate::sim::Session;

#[derive(Debug, Clone, PartialEq)]
pub struct PromptModel {
    pub question: String,
    /// Does the thing asked about.
    pub confirm: ButtonModel,
    /// Backs out.
    pub cancel: ButtonModel,
    /// What a click outside the prompt means.
    pub dismiss: UiAction,
}

/// The prompt to show, if any. A prompt that no longer makes sense shows nothing: leaving a game
/// needs a game, once the game is over its own modal is the only one, and the shortfall prompt
/// shows only in Planning. The shortfall prompt names the figures it was opened with.
pub fn prompt_model(
    open: OpenPrompt,
    game: Option<&Game>,
    l10n: &Localizer,
) -> Option<PromptModel> {
    match open.0? {
        Prompt::Connection {
            kind,
            from,
            to,
            warning,
        } => {
            let session = game?.session();
            prompt_in_phase(prompt_after(open, session.status()), session.phase()).0?;
            match kind {
                domain::session::ConnectionKind::Ac => {
                    session.line_quote(from, to).ok()?;
                }
                domain::session::ConnectionKind::Dc => {
                    session.link_quote(from, to).ok()?;
                }
            }
            Some(PromptModel {
                question: l10n.args(
                    "connection-warning-question",
                    &[
                        ("cities", warning.cities.into()),
                        ("month", format::month_year(warning.month).into()),
                        ("added", format::megawatts(warning.added_peak).into()),
                        (
                            "island",
                            Names::new(l10n, session).node(warning.island).into(),
                        ),
                        ("time", time_of(warning.shortage.hour).into()),
                        ("demand", format::megawatts(warning.shortage.demand).into()),
                        (
                            "supply",
                            format::megawatts(warning.shortage.deliverable).into(),
                        ),
                        (
                            "shortfall",
                            format::megawatts(warning.shortage.shortfall).into(),
                        ),
                    ],
                ),
                confirm: ButtonModel::new(
                    l10n.text("connection-proceed"),
                    UiAction::ConfirmConnection { kind, from, to },
                ),
                cancel: ButtonModel::new(l10n.text("connection-cancel"), UiAction::Prompt(None)),
                dismiss: UiAction::Prompt(None),
            })
        }
        Prompt::CancelLinkConstruction { link } => {
            let session = game?.session();
            prompt_in_phase(prompt_after(open, session.status()), session.phase()).0?;
            let asset = session.power().network().link(link)?;
            if !matches!(
                asset.status,
                domain::grid::LineStatus::UnderConstruction { .. }
            ) {
                return None;
            }
            Some(PromptModel {
                question: l10n.text("hvdc-cancel-question"),
                confirm: ButtonModel::new(
                    l10n.text("line-cancel-confirm"),
                    UiAction::Command(Command::CancelLinkConstruction { link }),
                ),
                cancel: ButtonModel::new(l10n.text("line-cancel-keep"), UiAction::Prompt(None)),
                dismiss: UiAction::Prompt(None),
            })
        }
        Prompt::CancelLineConstruction { line } => {
            let session = game?.session();
            prompt_in_phase(prompt_after(open, session.status()), session.phase()).0?;
            session.line_views().into_iter().find(|view| {
                view.id == line
                    && matches!(
                        view.status,
                        domain::grid::LineStatus::UnderConstruction { .. }
                    )
            })?;
            let cancel = UiAction::Prompt(None);
            Some(PromptModel {
                question: l10n.args(
                    "line-cancel-question",
                    &[("line", Names::new(l10n, session).line(line).into())],
                ),
                confirm: ButtonModel::new(
                    l10n.text("line-cancel-confirm"),
                    UiAction::Command(Command::CancelLineConstruction { line }),
                ),
                cancel: ButtonModel::new(l10n.text("line-cancel-keep"), cancel.clone()),
                dismiss: cancel,
            })
        }
        Prompt::LeaveGame => {
            let game = game?;
            prompt_after(open, game.session().status()).0?;
            let stay = UiAction::Prompt(None);
            Some(PromptModel {
                question: l10n.text("leave-game-question"),
                confirm: ButtonModel::new(l10n.text("leave-game-leave"), UiAction::ReturnToMenu),
                cancel: ButtonModel::new(l10n.text("leave-game-stay"), stay.clone()),
                dismiss: stay,
            })
        }
        Prompt::ShortfallStart { hour, shortfall } => {
            let session = game?.session();
            prompt_in_phase(prompt_after(open, session.status()), session.phase()).0?;
            let keep_planning = UiAction::Prompt(None);
            Some(PromptModel {
                question: l10n.args(
                    "shortfall-start-question",
                    &[
                        ("amount", format::megawatts(shortfall).into()),
                        ("time", time_of(hour).into()),
                    ],
                ),
                confirm: ButtonModel::new(
                    l10n.text("shortfall-start-anyway"),
                    UiAction::Command(Command::BeginOperating),
                ),
                cancel: ButtonModel::new(
                    l10n.text("shortfall-start-keep-planning"),
                    keep_planning.clone(),
                ),
                dismiss: keep_planning,
            })
        }
    }
}

/// What stays open once the game is in `status`: an ended game closes any prompt, so no modal
/// stacks on the game-over one.
pub fn prompt_after(open: OpenPrompt, status: SessionStatus) -> OpenPrompt {
    match status {
        SessionStatus::Active => open,
        SessionStatus::GameOver(_) => OpenPrompt(None),
    }
}

/// Planning actions close once the day begins. The leave prompt is for every phase.
pub fn prompt_in_phase(open: OpenPrompt, phase: DayPhase) -> OpenPrompt {
    match open.0 {
        Some(
            Prompt::ShortfallStart { .. }
            | Prompt::Connection { .. }
            | Prompt::CancelLineConstruction { .. }
            | Prompt::CancelLinkConstruction { .. },
        ) if phase != DayPhase::Planning => OpenPrompt(None),
        _ => open,
    }
}

pub fn draw_prompt(ui: &mut egui::Ui, model: &PromptModel) -> Vec<UiAction> {
    let dismiss = super::action::cancelled(ui, &model.cancel);
    let mut actions = Vec::new();
    ui.label(model.question.as_str());
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if matches!(
            model.confirm.action,
            UiAction::ConfirmConnection { .. }
                | UiAction::Command(
                    Command::CancelLineConstruction { .. } | Command::CancelLinkConstruction { .. }
                )
        ) {
            let response = button(ui, &model.cancel, &mut actions);
            let frame = ui.ctx().cumulative_frame_nr();
            let id = ui.make_persistent_id(("prompt_cancel_focus", &model.question));
            let first = ui.data_mut(|data| {
                let last = data.get_temp::<u64>(id);
                data.insert_temp(id, frame);
                last != Some(frame) && last != Some(frame.saturating_sub(1))
            });
            if first {
                response.request_focus();
            }
            button(ui, &model.confirm, &mut actions);
        } else {
            button(ui, &model.confirm, &mut actions);
            button(ui, &model.cancel, &mut actions);
        }
    });
    if dismiss {
        vec![model.cancel.action.clone()]
    } else {
        actions
    }
}

pub(super) fn prompt_overlay(
    mut contexts: EguiContexts,
    session: Option<Res<Session>>,
    l10n: Res<Localizer>,
    mut sink: ActionSink,
) -> Result {
    let Some(model) = prompt_model(sink.open_prompt(), session.as_deref().map(|s| &s.0), &l10n)
    else {
        return Ok(());
    };
    let shown = egui::Modal::new(egui::Id::new("prompt"))
        .show(contexts.ctx_mut()?, |ui| draw_prompt(ui, &model));
    let mut actions = shown.inner;
    if shown.backdrop_response.clicked() && actions.is_empty() && model.cancel.enabled {
        actions.push(model.dismiss.clone());
    }
    dispatch(actions, &mut sink);
    Ok(())
}

/// Closes the open prompt once it no longer applies: when the game ends (see [`prompt_after`]),
/// and the shortfall prompt when the day begins (see [`prompt_in_phase`]). It runs after the
/// commands are applied, so the egui pass of the frame a phase changes already sees it closed.
pub(super) fn close_stale_prompt(session: Res<Session>, mut open: ResMut<OpenPrompt>) {
    let session = session.0.session();
    let after = prompt_in_phase(prompt_after(*open, session.status()), session.phase());
    let after = match after.0 {
        Some(Prompt::Connection { kind, from, to, .. })
            if match kind {
                domain::session::ConnectionKind::Ac => session.line_quote(from, to).is_err(),
                domain::session::ConnectionKind::Dc => session.link_quote(from, to).is_err(),
            } =>
        {
            OpenPrompt(None)
        }
        Some(Prompt::CancelLineConstruction { line })
            if !session.line_views().iter().any(|view| {
                view.id == line
                    && matches!(
                        view.status,
                        domain::grid::LineStatus::UnderConstruction { .. }
                    )
            }) =>
        {
            OpenPrompt(None)
        }
        _ => after,
    };
    if after != *open {
        *open = after;
    }
}

#[cfg(test)]
mod tests {
    use application::{Command, InMemoryCommandLog};
    use domain::session::{DayPhase, GameOverReason};
    use domain::time::TICKS_PER_HOUR;
    use domain::units::Megawatts;

    use super::*;
    use crate::hud::test_support::{eastern_game, empty_start, shipped_l10n};
    use crate::l10n::format;

    const LEAVE: OpenPrompt = OpenPrompt(Some(Prompt::LeaveGame));

    fn short_by(hour: u8, mw: f64) -> OpenPrompt {
        OpenPrompt(Some(Prompt::ShortfallStart {
            hour,
            shortfall: Megawatts::new(mw),
        }))
    }

    #[test]
    fn menu_asks_before_leaving() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        let prompt = prompt_model(LEAVE, Some(&game), &l10n).expect("the prompt is shown");

        assert_eq!(prompt.question, "Leave this game? It can't be resumed yet.");
        // Leave is the only way out of the game; Stay, or a click outside, just closes the prompt.
        assert_eq!(prompt.confirm.label, "Leave");
        assert_eq!(prompt.confirm.action, UiAction::ReturnToMenu);
        assert_eq!(prompt.cancel.label, "Stay");
        assert_eq!(prompt.cancel.action, UiAction::Prompt(None));
        assert_eq!(prompt.dismiss, UiAction::Prompt(None));
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn no_prompt_shows_when_none_is_open_or_there_is_no_game() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        assert_eq!(prompt_model(OpenPrompt(None), Some(&game), &l10n), None);
        assert_eq!(prompt_model(LEAVE, None, &l10n), None);
    }

    #[test]
    fn the_shortfall_prompt_offers_start_anyway_and_keep_planning() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        let worst = game
            .session()
            .capacity_shortfall_check()
            .expect("nothing is committed");
        let open = OpenPrompt(Some(Prompt::ShortfallStart {
            hour: worst.hour,
            shortfall: worst.shortfall,
        }));

        let prompt = prompt_model(open, Some(&game), &l10n).expect("the prompt is shown");

        // The figures the warnings strip shows, worded as likely rather than certain.
        assert_eq!(
            prompt.question,
            format!(
                "The forecast is short by up to {} (worst at {}). Expect rotating outages if you \
                 start the day now.",
                format::megawatts(worst.shortfall),
                format::clock(u32::from(worst.hour) * TICKS_PER_HOUR),
            )
        );
        // Start anyway begins the day; Keep planning, or a click outside, only closes the prompt.
        assert_eq!(prompt.confirm.label, "Start anyway");
        assert_eq!(
            prompt.confirm.action,
            UiAction::Command(Command::BeginOperating)
        );
        assert!(prompt.confirm.enabled);
        assert_eq!(prompt.cancel.label, "Keep planning");
        assert_eq!(prompt.cancel.action, UiAction::Prompt(None));
        assert!(prompt.cancel.enabled);
        assert_eq!(prompt.dismiss, UiAction::Prompt(None));
        assert_eq!(l10n.problems(), vec![]);
    }

    /// The prompt shows the figures it was opened with, not the forecast now.
    #[test]
    fn the_shortfall_prompt_names_the_hour_and_amount_it_holds() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        let prompt =
            prompt_model(short_by(18, 12_000.0), Some(&game), &l10n).expect("the prompt is shown");

        assert_eq!(
            prompt.question,
            "The forecast is short by up to 12.0 GW (worst at 18:00). Expect rotating outages if \
             you start the day now."
        );
        let early = prompt_model(short_by(7, 450.0), Some(&game), &l10n).unwrap();
        assert_eq!(
            early.question,
            "The forecast is short by up to 450 MW (worst at 07:00). Expect rotating outages if \
             you start the day now."
        );
        assert_eq!(prompt_model(short_by(18, 12_000.0), None, &l10n), None);
        assert_eq!(l10n.problems(), vec![]);
    }

    /// The shortfall prompt asks about starting the day, so it never shows outside Planning
    /// (spec §16.2). The leave prompt is for every phase.
    #[test]
    fn the_shortfall_prompt_is_planning_only() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        game.execute(Command::BeginOperating).unwrap();

        assert_eq!(prompt_model(short_by(18, 900.0), Some(&game), &l10n), None);
        assert!(prompt_model(LEAVE, Some(&game), &l10n).is_some());

        for phase in [DayPhase::Operating, DayPhase::Review] {
            assert_eq!(
                prompt_in_phase(short_by(18, 900.0), phase),
                OpenPrompt(None),
                "{phase:?}"
            );
            assert_eq!(prompt_in_phase(LEAVE, phase), LEAVE, "{phase:?}");
        }
        assert_eq!(
            prompt_in_phase(short_by(18, 900.0), DayPhase::Planning),
            short_by(18, 900.0)
        );
        assert_eq!(prompt_in_phase(LEAVE, DayPhase::Planning), LEAVE);
        assert_eq!(
            prompt_in_phase(OpenPrompt(None), DayPhase::Operating),
            OpenPrompt(None)
        );
    }

    #[test]
    fn a_finished_game_closes_the_leave_prompt() {
        for reason in [GameOverReason::Bankrupt, GameOverReason::LicenseRevoked] {
            assert_eq!(
                prompt_after(LEAVE, SessionStatus::GameOver(reason)),
                OpenPrompt(None)
            );
            assert_eq!(
                prompt_after(short_by(18, 900.0), SessionStatus::GameOver(reason)),
                OpenPrompt(None)
            );
        }
        assert_eq!(prompt_after(LEAVE, SessionStatus::Active), LEAVE);
        assert_eq!(
            prompt_after(OpenPrompt(None), SessionStatus::Active),
            OpenPrompt(None)
        );
    }
}
