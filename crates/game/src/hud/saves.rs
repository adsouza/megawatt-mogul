//! Save/load presenters and the single persistence modal. No store calls from draw code.
use super::{ActionSink, ButtonModel, UiAction, add_enabled, button, dispatch};
use crate::{
    l10n::{Localize, Localizer, format},
    sim::{PersistAction, Persistence, SaveScreen},
};
use application::{SaveError, SaveKind};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

#[derive(Resource, Default)]
pub struct SaveForm {
    pub name: String,
}
pub(super) fn reset_form(mut form: ResMut<SaveForm>) {
    *form = SaveForm::default();
}
/// Freeze the overlay choice for the entire egui pass, before any button dispatches.
#[derive(Resource, Default)]
pub struct PersistenceShown(pub bool);
pub(super) fn snapshot(persistence: Res<Persistence>, mut shown: ResMut<PersistenceShown>) {
    shown.0 = persistence.screen != SaveScreen::Closed;
}
pub(super) fn other_overlays(shown: Res<PersistenceShown>) -> bool {
    !shown.0
}

#[derive(Debug, Clone, PartialEq)]
pub struct SaveRowModel {
    pub name: String,
    pub details: String,
    pub problem: Option<String>,
    pub action: Option<ButtonModel>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct SavesModel {
    pub title: String,
    pub body: String,
    pub busy: bool,
    pub error: Option<String>,
    pub name_label: Option<String>,
    pub rows: Vec<SaveRowModel>,
    pub buttons: Vec<ButtonModel>,
}

pub fn saves_model(p: &Persistence, form: &SaveForm, l10n: &Localizer) -> SavesModel {
    let saving = p.screen == SaveScreen::Save;
    let title = match p.screen {
        SaveScreen::Browser => "save-load",
        SaveScreen::Leave => "save-leave-title",
        SaveScreen::ConfirmOverwrite => "save-overwrite-title",
        SaveScreen::Automatic => "save-autosave",
        _ => "save-title",
    };
    let mut body = l10n.text(if p.busy() {
        "save-working"
    } else if p.screen == SaveScreen::Leave {
        "save-leave-body"
    } else if p.screen == SaveScreen::ConfirmOverwrite {
        "save-overwrite-body"
    } else if saving {
        "save-applied-only"
    } else {
        "save-choose"
    });
    if p.screen == SaveScreen::ConfirmOverwrite
        && let Some(entry) = p.overwrite_entry()
    {
        body = l10n.args(
            "save-overwrite-specific",
            &[
                ("name", entry.metadata.name.as_str().into()),
                ("month", format::month_year(entry.summary.month).into()),
                ("phase", entry.summary.phase.localize(l10n).into()),
                ("time", format::clock(entry.summary.tick).into()),
            ],
        );
    }
    if let Some(summary) = p.checkpoint_summary() {
        body.push('\n');
        body.push_str(&l10n.args(
            "save-boundary",
            &[
                ("month", format::month_year(summary.month).into()),
                ("phase", summary.phase.localize(l10n).into()),
                ("time", format::clock(summary.tick).into()),
            ],
        ));
    }
    let rows: Vec<_> = if matches!(p.screen, SaveScreen::Browser | SaveScreen::Save) {
        p.entries
            .iter()
            .map(|listing| match &listing.entry {
                Ok(entry) => {
                    let automatic = entry.metadata.kind == SaveKind::Automatic;
                    let name = if automatic {
                        l10n.text("save-autosave")
                    } else {
                        entry.metadata.name.clone()
                    };
                    let details = l10n.args(
                        "save-details",
                        &[
                            ("region", l10n.text(&entry.region_name_key).into()),
                            ("month", format::month_year(entry.summary.month).into()),
                            ("phase", entry.summary.phase.localize(l10n).into()),
                            ("time", format::clock(entry.summary.tick).into()),
                            (
                                "kind",
                                l10n.text(if automatic {
                                    "save-automatic-kind"
                                } else {
                                    "save-manual-kind"
                                })
                                .into(),
                            ),
                            (
                                "saved",
                                crate::sim::utc_stamp(entry.metadata.saved_at).into(),
                            ),
                            (
                                "status",
                                match entry.summary.status {
                                    domain::session::SessionStatus::Active => String::new(),
                                    domain::session::SessionStatus::GameOver(reason) => {
                                        reason.localize(l10n)
                                    }
                                }
                                .into(),
                            ),
                        ],
                    );
                    let action = if saving {
                        (!automatic).then(|| {
                            ButtonModel::new(
                                l10n.text("save-overwrite"),
                                UiAction::Persistence(PersistAction::ChooseOverwrite {
                                    slot: entry.metadata.slot.clone(),
                                    name: if form.name.trim().is_empty() {
                                        entry.metadata.name.clone()
                                    } else {
                                        form.name.clone()
                                    },
                                }),
                            )
                            .enabled_if(p.can_save())
                        })
                    } else {
                        Some(
                            ButtonModel::new(
                                l10n.text("save-resume"),
                                UiAction::Persistence(PersistAction::Load(
                                    entry.metadata.slot.clone(),
                                )),
                            )
                            .enabled_if(!p.busy()),
                        )
                    };
                    SaveRowModel {
                        name,
                        details,
                        problem: None,
                        action,
                    }
                }
                Err(error) => SaveRowModel {
                    name: listing.file_label.clone(),
                    details: String::new(),
                    problem: Some(save_error(error, l10n)),
                    action: None,
                },
            })
            .collect()
    } else {
        Vec::new()
    };
    if rows.is_empty() && matches!(p.screen, SaveScreen::Browser | SaveScreen::Save) && !p.busy() {
        body.push('\n');
        body.push_str(&l10n.text("save-empty"));
    }
    let action =
        |key: &str, action| ButtonModel::new(l10n.text(key), UiAction::Persistence(action));
    let mut buttons = Vec::new();
    if !p.busy() {
        if p.error.is_some() {
            buttons.push(action("save-retry", PersistAction::Retry));
        }
        if p.automatic_error() {
            buttons.push(action(
                "save-continue-unsaved",
                PersistAction::ContinueWithoutSaving,
            ));
        } else {
            match p.screen {
                SaveScreen::Save => {
                    buttons.push(
                        action("save-new", PersistAction::SaveNew(form.name.clone())).enabled_if(
                            p.can_save() && !form.name.trim().is_empty() && p.error.is_none(),
                        ),
                    );
                    buttons.push(action("save-refresh", PersistAction::Refresh));
                }
                SaveScreen::Browser => buttons.push(action("save-refresh", PersistAction::Refresh)),
                SaveScreen::Leave => {
                    buttons.push(action(
                        "save-and-menu",
                        PersistAction::OpenSave { leave: true },
                    ));
                    buttons.push(ButtonModel::new(
                        l10n.text("save-leave-unsaved"),
                        UiAction::ReturnToMenu,
                    ));
                }
                SaveScreen::ConfirmOverwrite if p.error.is_none() => {
                    buttons.push(action("save-overwrite", PersistAction::ConfirmOverwrite))
                }
                _ => (),
            }
        }
    }
    if p.screen != SaveScreen::Automatic {
        buttons.push(action("save-cancel", PersistAction::Cancel));
    }
    SavesModel {
        title: l10n.text(title),
        body,
        busy: p.busy(),
        error: p.error.as_ref().map(|e| save_error(e, l10n)),
        name_label: saving.then(|| l10n.text("save-name")),
        rows,
        buttons,
    }
}

/// Exhaustive typed mapping, with diagnostic details reserved for logs rather than raw UI copy.
pub fn save_error(error: &SaveError, l10n: &Localizer) -> String {
    let key = match error {
        SaveError::InvalidId => "save-error-id",
        SaveError::InvalidSaveName => "save-error-name",
        SaveError::SaveFormatVersionMismatch { .. } => "save-error-version",
        SaveError::ChecksumMismatch => "save-error-checksum",
        SaveError::SaveTooLarge => "save-error-size",
        SaveError::Malformed(_) => "save-error-malformed",
        SaveError::SlotChanged => "save-error-changed",
        SaveError::SlotBusy => "save-error-busy",
        SaveError::DurabilityUnconfirmed { .. } => "save-error-durability",
        SaveError::Cancelled => "save-error-cancelled",
        SaveError::Io { .. } => "save-error-io",
        SaveError::Restore(error) => match error {
            application::RestoreError::Replay(_) => "save-error-rules",
            application::RestoreError::RegionSourceMismatch
            | application::RestoreError::RegionIdMismatch => "save-error-region",
            application::RestoreError::CommandCountMismatch
            | application::RestoreError::StateHashMismatch
            | application::RestoreError::SummaryMismatch => "save-error-state",
            application::RestoreError::ReplayCommandFailed { .. } => "save-error-replay",
            application::RestoreError::Cancelled => "save-error-cancelled",
            application::RestoreError::Log(_) => "save-error-log",
        },
        SaveError::OtherVersionDiverged { .. } | SaveError::OtherVersionUnreadable { .. } => {
            "save-error-other-version"
        }
    };
    l10n.text(key)
}

pub fn draw_saves(ui: &mut egui::Ui, model: &SavesModel, form: &mut SaveForm) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.set_width(560.0_f32.min(ui.ctx().content_rect().width() - 48.0));
    ui.heading(&model.title);
    ui.label(&model.body);
    if model.busy {
        ui.spinner();
    }
    if let Some(error) = &model.error {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    if let Some(label) = &model.name_label {
        ui.label(label);
        add_enabled(
            ui,
            !model.busy,
            egui::TextEdit::singleline(&mut form.name)
                .char_limit(80)
                .desired_width(f32::INFINITY),
        );
    }
    let height = (ui.ctx().content_rect().height() * 0.35).clamp(80.0, 260.0);
    egui::ScrollArea::vertical()
        .id_salt("save_rows")
        .max_height(height)
        .show(ui, |ui| {
            for row in &model.rows {
                ui.separator();
                ui.strong(&row.name);
                ui.label(&row.details);
                if let Some(problem) = &row.problem {
                    ui.colored_label(ui.visuals().error_fg_color, problem);
                }
                if let Some(action) = &row.action {
                    button(ui, action, &mut actions);
                }
            }
        });
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        for b in &model.buttons {
            button(ui, b, &mut actions);
        }
    });
    actions
}

pub(super) fn saves_overlay(
    mut contexts: EguiContexts,
    shown: Res<PersistenceShown>,
    l10n: Res<Localizer>,
    mut form: ResMut<SaveForm>,
    mut sink: ActionSink,
) -> Result {
    if !shown.0 {
        return Ok(());
    }
    let model = saves_model(sink.persistence(), &form, &l10n);
    let actions = egui::Modal::new(egui::Id::new("saves"))
        .show(contexts.ctx_mut()?, |ui| draw_saves(ui, &model, &mut form))
        .inner;
    dispatch(actions, &mut sink);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::test_support::shipped_l10n;
    use application::{CommandError, ReplayCompatibility, RestoreError};
    use domain::error::DomainError;

    /// The four ways a replay fails to reproduce its recorded boundary.
    fn verification_failures() -> Vec<RestoreError> {
        vec![
            RestoreError::CommandCountMismatch,
            RestoreError::StateHashMismatch,
            RestoreError::SummaryMismatch,
            RestoreError::ReplayCommandFailed {
                index: 3,
                error: CommandError::Domain(DomainError::InvalidMonth { month: 13 }),
            },
        ]
    }

    #[test]
    fn a_failed_verification_names_the_other_version_only_when_the_save_came_from_one() {
        let l10n = shipped_l10n();
        let older = ReplayCompatibility {
            build: "an-older-build".into(),
            platform: "another-target".into(),
        };
        for error in verification_failures() {
            assert!(error.is_verification_failure());
            assert_eq!(
                save_error(
                    &SaveError::OtherVersionDiverged {
                        recorded: older.clone(),
                        error: error.clone(),
                    },
                    &l10n
                ),
                "This save was made by a different version of the game, and this version \
                 doesn't reproduce it. Your save file is unchanged.",
                "{error:?}"
            );
            let same_build = if matches!(error, RestoreError::ReplayCommandFailed { .. }) {
                "save-error-replay"
            } else {
                "save-error-state"
            };
            assert_eq!(
                save_error(&SaveError::Restore(error), &l10n),
                l10n.text(same_build)
            );
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn an_undecodable_save_names_the_other_version_only_when_the_save_came_from_one() {
        let l10n = shipped_l10n();
        let older = ReplayCompatibility {
            build: "an-older-build".into(),
            platform: "another-target".into(),
        };
        assert_eq!(
            save_error(
                &SaveError::OtherVersionUnreadable {
                    recorded: older,
                    cause: "unknown region field".into(),
                },
                &l10n
            ),
            l10n.text("save-error-other-version")
        );
        assert_eq!(
            save_error(&SaveError::Malformed("unknown region field".into()), &l10n),
            "This save could not be decoded."
        );
        assert_eq!(l10n.problems(), vec![]);
    }
}
