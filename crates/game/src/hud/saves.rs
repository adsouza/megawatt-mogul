//! Save/load presenters and the single persistence modal. No store calls from draw code.
use super::{ActionSink, ButtonModel, UiAction, add_enabled, dispatch};
use crate::{
    l10n::{Localize, Localizer, format},
    sim::{PersistAction, Persistence, SaveScreen},
};
use application::{SaveError, SaveKind, SaveListing};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

#[derive(Resource, Default)]
pub struct SaveForm {
    pub name: String,
    pub rename_name: String,
    rename_slot: Option<application::SaveId>,
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
    pub kind: Option<String>,
    pub automatic: bool,
    pub problem: Option<String>,
    pub action: Option<ButtonModel>,
    #[cfg(target_arch = "wasm32")]
    pub export: Option<ButtonModel>,
    pub rename: Option<ButtonModel>,
    pub delete: Option<ButtonModel>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct SavesModel {
    pub title: String,
    pub eyebrow: String,
    pub library: bool,
    pub named_heading: String,
    pub automatic_heading: String,
    pub unreadable_heading: String,
    pub named_empty: String,
    pub automatic_empty: String,
    pub body: String,
    pub busy: bool,
    pub error: Option<String>,
    pub empty: Option<String>,
    pub name_label: Option<String>,
    /// Operation availability before validating the name, which can change during this draw pass.
    pub name_submission_enabled: bool,
    pub renaming: bool,
    pub deleting: bool,
    pub rows: Vec<SaveRowModel>,
    pub buttons: Vec<ButtonModel>,
}

impl SavesModel {
    /// Bind name actions to this pass's edited form rather than the previous frame's text.
    fn edited_name_button(&self, button: &ButtonModel, form: &SaveForm) -> (ButtonModel, bool) {
        let mut button = button.clone();
        let name = match &mut button.action {
            UiAction::Persistence(PersistAction::Rename(name)) => {
                *name = form.rename_name.clone();
                Some(name)
            }
            UiAction::Persistence(PersistAction::SaveNew(name)) => {
                *name = form.name.clone();
                Some(name)
            }
            _ => None,
        };
        let primary = name.is_some();
        if let Some(name) = name {
            button.enabled = self.name_submission_enabled && !name.trim().is_empty();
        }
        (button, primary)
    }
}

pub(super) fn display_name(metadata: &application::SaveMetadata, l10n: &Localizer) -> String {
    if metadata.kind == SaveKind::Automatic && metadata.name == "save-autosave" {
        l10n.text("save-autosave")
    } else {
        metadata.name.clone()
    }
}

fn save_details(entry: &application::SaveEntry, l10n: &Localizer) -> String {
    l10n.args(
        "save-details",
        &[
            ("region", l10n.text(&entry.region_name_key).into()),
            ("month", format::month_year(entry.summary.month).into()),
            ("phase", entry.summary.phase.localize(l10n).into()),
            ("time", format::clock(entry.summary.tick).into()),
            (
                "saved",
                crate::sim::utc_date_time(entry.metadata.saved_at).into(),
            ),
            (
                "status",
                match entry.summary.status {
                    domain::session::SessionStatus::Active => String::new(),
                    domain::session::SessionStatus::GameOver(reason) => reason.localize(l10n),
                }
                .into(),
            ),
        ],
    )
}

const MAX_LIBRARY_AUTOSAVES: usize = 12;

/// The display limit applies only to readable autosaves. Manual and unreadable files stay
/// accessible, and this presentation never changes the store's retention policy.
fn library_entries(entries: &[SaveListing]) -> Vec<&SaveListing> {
    let mut ordered: Vec<_> = entries.iter().collect();
    let time = |entry: &SaveListing| {
        entry
            .entry
            .as_ref()
            .map_or(entry.modified_at, |save| save.metadata.saved_at)
    };
    ordered.sort_by(|a, b| {
        time(b)
            .cmp(&time(a))
            .then_with(|| a.file_label.cmp(&b.file_label))
    });
    let mut automatic = 0;
    ordered.retain(|listing| {
        if listing
            .entry
            .as_ref()
            .is_ok_and(|entry| entry.metadata.kind == SaveKind::Automatic)
        {
            automatic += 1;
            automatic <= MAX_LIBRARY_AUTOSAVES
        } else {
            true
        }
    });
    ordered
}

pub fn saves_model(p: &Persistence, form: &SaveForm, l10n: &Localizer) -> SavesModel {
    let saving = p.screen == SaveScreen::Save;
    let renaming = p.screen == SaveScreen::Rename;
    let deleting = p.screen == SaveScreen::ConfirmDelete;
    let title = match p.screen {
        SaveScreen::Browser => "save-load",
        SaveScreen::Loading => "save-resuming",
        SaveScreen::Rename => "save-rename-title",
        SaveScreen::ConfirmDelete => "save-delete-title",
        SaveScreen::Leave => "save-leave-title",
        SaveScreen::ConfirmOverwrite => "save-overwrite-title",
        SaveScreen::Automatic => "save-autosave",
        _ => "save-title",
    };
    let mut body = l10n.text(if p.screen == SaveScreen::Loading {
        "save-restoring"
    } else if p.screen == SaveScreen::Browser && !p.menu_scanned() && !p.entries.is_empty() {
        // Keep cached cards visible while their small metadata refresh completes.
        "save-choose"
    } else if p.screen == SaveScreen::Browser && !p.menu_scanned() && p.busy() {
        "save-loading-list"
    } else if p.screen == SaveScreen::Browser && p.busy() {
        "save-restoring"
    } else if p.busy() {
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
    if renaming
        && !p.busy()
        && let Some(entry) = p.rename_entry()
    {
        body = l10n.args(
            "save-rename-body",
            &[("name", display_name(&entry.metadata, l10n).into())],
        );
    }
    if deleting && let Some(listing) = p.deletion_listing() {
        let (name, details) = match &listing.entry {
            Ok(entry) => (
                display_name(&entry.metadata, l10n),
                save_details(entry, l10n),
            ),
            Err(error) => (listing.file_label.clone(), save_error(error, l10n)),
        };
        body = l10n.args(
            "save-delete-specific",
            &[("name", name.into()), ("details", details.into())],
        );
    }
    let delete_button = |listing: &SaveListing| {
        (p.screen == SaveScreen::Browser && listing.revision.is_some())
            .then_some(listing.id.as_ref())
            .flatten()
            .map(|id| {
                ButtonModel::new(
                    l10n.text("save-delete"),
                    UiAction::Persistence(PersistAction::BeginDelete(id.clone())),
                )
                .enabled_if(!p.busy())
            })
    };
    if let Some(summary) = p.checkpoint_summary().filter(|_| !renaming && !deleting) {
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
        let entries = if p.screen == SaveScreen::Browser {
            library_entries(&p.entries)
        } else {
            p.entries
                .iter()
                .filter(|listing| {
                    listing
                        .entry
                        .as_ref()
                        .is_ok_and(|entry| entry.metadata.kind == SaveKind::Manual)
                })
                .collect()
        };
        entries
            .into_iter()
            .map(|listing| match &listing.entry {
                Ok(entry) => {
                    let automatic = entry.metadata.kind == SaveKind::Automatic;
                    let name = display_name(&entry.metadata, l10n);
                    let details = save_details(entry, l10n);
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
                        kind: Some(l10n.text(if automatic {
                            "save-automatic-kind"
                        } else {
                            "save-manual-kind"
                        })),
                        automatic,
                        problem: None,
                        #[cfg(target_arch = "wasm32")]
                        export: (p.screen == SaveScreen::Browser).then(|| {
                            ButtonModel::new(
                                l10n.text("browser-export"),
                                UiAction::Persistence(PersistAction::Export(
                                    entry.metadata.slot.clone(),
                                )),
                            )
                            .enabled_if(!p.busy())
                        }),
                        delete: delete_button(listing),
                        action,
                        rename: Some(
                            ButtonModel::new(
                                l10n.text("save-rename"),
                                UiAction::Persistence(PersistAction::BeginRename(
                                    entry.metadata.slot.clone(),
                                )),
                            )
                            .enabled_if(!p.busy()),
                        ),
                    }
                }
                Err(error) => SaveRowModel {
                    name: listing.file_label.clone(),
                    details: String::new(),
                    kind: None,
                    automatic: false,
                    problem: Some(save_error(error, l10n)),
                    action: None,
                    #[cfg(target_arch = "wasm32")]
                    export: None,
                    rename: None,
                    delete: delete_button(listing),
                },
            })
            .collect()
    } else {
        Vec::new()
    };
    let empty = (rows.is_empty()
        && matches!(p.screen, SaveScreen::Browser | SaveScreen::Save)
        && !p.busy())
    .then(|| l10n.text("save-empty"));
    let action =
        |key: &str, action| ButtonModel::new(l10n.text(key), UiAction::Persistence(action));
    let mut buttons = Vec::new();
    #[cfg(target_arch = "wasm32")]
    if p.screen == SaveScreen::Browser {
        buttons.push(action("browser-import", PersistAction::Import).enabled_if(!p.busy()));
    }
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
                SaveScreen::Rename => buttons.push(
                    action(
                        "save-rename",
                        PersistAction::Rename(form.rename_name.clone()),
                    )
                    .enabled_if(!form.rename_name.trim().is_empty()),
                ),
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
                SaveScreen::ConfirmDelete if p.error.is_none() => {
                    buttons.push(action("save-delete-confirm", PersistAction::ConfirmDelete))
                }
                SaveScreen::ConfirmOverwrite if p.error.is_none() => {
                    buttons.push(action("save-overwrite", PersistAction::ConfirmOverwrite))
                }
                _ => (),
            }
        }
    }
    if p.screen != SaveScreen::Automatic {
        buttons.push(
            action("save-cancel", PersistAction::Cancel)
                .enabled_if(!(renaming || deleting) || !p.busy()),
        );
    }
    SavesModel {
        title: l10n.text(title),
        eyebrow: l10n.text(if p.screen == SaveScreen::Browser {
            "save-library-label"
        } else {
            "app-title"
        }),
        library: p.screen == SaveScreen::Browser,
        named_heading: l10n.text("save-named-heading"),
        automatic_heading: l10n.text("save-automatic-heading"),
        unreadable_heading: l10n.text("save-unreadable-heading"),
        named_empty: l10n.text("save-named-empty"),
        automatic_empty: l10n.text("save-automatic-empty"),
        body,
        busy: p.busy(),
        error: p.error.as_ref().map(|e| save_error(e, l10n)),
        empty,
        name_label: (saving || renaming).then(|| l10n.text("save-name")),
        name_submission_enabled: !p.busy()
            && (renaming || (saving && p.can_save() && p.error.is_none())),
        renaming,
        deleting,
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
        SaveError::DeleteDurabilityUnconfirmed { .. } => "save-error-delete-durability",
        SaveError::Cancelled => "save-error-cancelled",
        #[cfg(target_arch = "wasm32")]
        SaveError::Io {
            stage: "browser-version",
            ..
        } => "browser-error-newer",
        #[cfg(target_arch = "wasm32")]
        SaveError::Io {
            stage: "browser-blocked",
            ..
        } => "browser-error-blocked",
        SaveError::Io { .. } => {
            if cfg!(target_arch = "wasm32") {
                "browser-error-storage"
            } else {
                "save-error-io"
            }
        }
        SaveError::Restore(error) => match error {
            application::RestoreError::Replay(_) => "save-error-rules",
            application::RestoreError::RegionSourceMismatch
            | application::RestoreError::RegionIdMismatch => "save-error-region",
            application::RestoreError::InvalidSnapshot
            | application::RestoreError::CommandCountMismatch
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

fn save_action(ui: &mut egui::Ui, model: &ButtonModel, primary: bool, actions: &mut Vec<UiAction>) {
    card_action(ui, model, primary, false, actions);
}
fn card_action(
    ui: &mut egui::Ui,
    model: &ButtonModel,
    primary: bool,
    compact: bool,
    actions: &mut Vec<UiAction>,
) {
    let mut widget = egui::Button::new(egui::RichText::new(&model.label).size(if compact {
        12.0
    } else {
        14.0
    }))
    .min_size(egui::vec2(
        if compact { 74.0 } else { 106.0 },
        if compact {
            26.0
        } else if primary {
            38.0
        } else {
            28.0
        },
    ));
    if primary {
        widget = widget
            .fill(egui::Color32::from_rgb(18, 78, 81))
            .stroke(egui::Stroke::new(1.0, super::title_scene::ACCENT));
    }
    if matches!(
        model.action,
        UiAction::Persistence(PersistAction::BeginDelete(_) | PersistAction::ConfirmDelete)
    ) {
        widget = widget
            .fill(egui::Color32::from_rgb(69, 32, 40))
            .stroke(egui::Stroke::new(1.0, ui.visuals().error_fg_color));
    }
    let response = add_enabled(ui, model.enabled, widget);
    if matches!(model.action, UiAction::Persistence(PersistAction::Cancel))
        && ui.ctx().data(|d| {
            d.get_temp::<bool>(egui::Id::new("delete_default_focus"))
                .unwrap_or(false)
        })
    {
        response.request_focus();
        ui.ctx()
            .data_mut(|d| d.remove::<bool>(egui::Id::new("delete_default_focus")));
    }
    if response.clicked() {
        actions.push(model.action.clone());
    }
}

fn draw_save_card(
    ui: &mut egui::Ui,
    row: &SaveRowModel,
    compact: bool,
    actions: &mut Vec<UiAction>,
) {
    let accent = if row.problem.is_some() {
        ui.visuals().error_fg_color
    } else if row.automatic {
        egui::Color32::from_rgb(240, 190, 110)
    } else {
        super::title_scene::ACCENT
    };
    let wide = !compact && ui.available_width() >= 560.0;
    let frame = egui::Frame::new()
        .fill(egui::Color32::from_rgb(17, 36, 47))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(37, 66, 76)))
        .corner_radius(8.0)
        .inner_margin(if compact { 10.0 } else { 16.0 })
        .show(ui, |ui| {
            ui.set_min_width((ui.available_width() - 2.0).max(0.0));
            let copy = |ui: &mut egui::Ui| {
                if !compact && let Some(kind) = &row.kind {
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(27, 54, 64))
                        .corner_radius(4.0)
                        .inner_margin(egui::Margin::symmetric(7, 2))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(kind)
                                    .size(10.0)
                                    .color(super::theme::TEXT),
                            );
                        });
                    ui.add_space(4.0);
                }
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&row.name)
                            .size(if compact { 15.0 } else { 22.0 })
                            .strong(),
                    )
                    .wrap(),
                );
                if !row.details.is_empty() {
                    ui.add_space(5.0);
                    ui.label(
                        egui::RichText::new(&row.details)
                            .size(if compact { 11.0 } else { 13.0 })
                            .color(super::theme::TEXT),
                    );
                }
                if let Some(problem) = &row.problem {
                    ui.add_space(5.0);
                    ui.colored_label(ui.visuals().error_fg_color, problem);
                }
            };
            if wide {
                ui.horizontal_top(|ui| {
                    let width = (ui.available_width() - 128.0).max(100.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_min_width(width);
                            copy(ui);
                        },
                    );
                    ui.vertical(|ui| {
                        if let Some(action) = &row.action {
                            card_action(ui, action, true, compact, actions);
                        }
                        #[cfg(target_arch = "wasm32")]
                        if let Some(export) = &row.export {
                            card_action(ui, export, false, compact, actions);
                        }
                        if let Some(rename) = &row.rename {
                            card_action(ui, rename, false, compact, actions);
                        }
                        if let Some(delete) = &row.delete {
                            card_action(ui, delete, false, compact, actions);
                        }
                    });
                });
            } else {
                copy(ui);
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if let Some(action) = &row.action {
                        card_action(ui, action, true, compact, actions);
                    }
                    #[cfg(target_arch = "wasm32")]
                    if let Some(export) = &row.export {
                        card_action(ui, export, false, compact, actions);
                    }
                    if let Some(rename) = &row.rename {
                        card_action(ui, rename, false, compact, actions);
                    }
                    if let Some(delete) = &row.delete {
                        card_action(ui, delete, false, compact, actions);
                    }
                });
            }
        });
    ui.painter().line_segment(
        [
            frame.response.rect.left_top() + egui::vec2(1.0, 10.0),
            frame.response.rect.left_bottom() + egui::vec2(1.0, -10.0),
        ],
        egui::Stroke::new(3.0, accent),
    );
    ui.add_space(if compact { 7.0 } else { 12.0 });
}

fn draw_library_rows(
    ui: &mut egui::Ui,
    model: &SavesModel,
    automatic: bool,
    actions: &mut Vec<UiAction>,
) {
    let mut rows = model
        .rows
        .iter()
        .filter(|row| row.automatic == automatic && row.problem.is_none())
        .peekable();
    if rows.peek().is_none() {
        ui.label(if automatic {
            &model.automatic_empty
        } else {
            &model.named_empty
        });
        ui.add_space(10.0);
    }
    for row in rows {
        draw_save_card(ui, row, automatic, actions);
    }
    if !automatic {
        let mut unreadable = model
            .rows
            .iter()
            .filter(|row| row.problem.is_some())
            .peekable();
        if unreadable.peek().is_some() {
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(&model.unreadable_heading)
                    .size(14.0)
                    .strong(),
            );
            for row in unreadable {
                draw_save_card(ui, row, false, actions);
            }
        }
    }
}

fn draw_library_column(
    ui: &mut egui::Ui,
    model: &SavesModel,
    automatic: bool,
    height: f32,
    actions: &mut Vec<UiAction>,
) {
    ui.label(
        egui::RichText::new(if automatic {
            &model.automatic_heading
        } else {
            &model.named_heading
        })
        .size(if automatic { 16.0 } else { 20.0 })
        .strong(),
    );
    ui.add_space(6.0);
    egui::ScrollArea::vertical()
        .id_salt(if automatic {
            "load_automatic_rows"
        } else {
            "load_named_rows"
        })
        .max_height((height - 38.0).max(50.0))
        .auto_shrink([false, false])
        .show(ui, |ui| draw_library_rows(ui, model, automatic, actions));
}

fn draw_library(ui: &mut egui::Ui, model: &SavesModel, height: f32, actions: &mut Vec<UiAction>) {
    if let Some(empty) = &model.empty {
        ui.label(empty);
    }
    if ui.available_width() >= 520.0 {
        let gap = 20.0;
        let available = ui.available_width() - gap;
        let named_width = available * 0.60;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (width, automatic) in [(named_width, false), (available - named_width, true)] {
                ui.allocate_ui_with_layout(
                    egui::vec2(width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(width);
                        draw_library_column(ui, model, automatic, height, actions);
                    },
                );
            }
        });
    } else {
        // Preserve both groups at extreme UI zoom, inside one bounded scrolling surface.
        egui::ScrollArea::vertical()
            .id_salt("load_narrow_rows")
            .max_height(height)
            .show(ui, |ui| {
                ui.heading(&model.named_heading);
                draw_library_rows(ui, model, false, actions);
                ui.add_space(12.0);
                ui.heading(&model.automatic_heading);
                draw_library_rows(ui, model, true, actions);
            });
    }
}

pub fn draw_saves(ui: &mut egui::Ui, model: &SavesModel, form: &mut SaveForm) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let dismiss = model
        .buttons
        .iter()
        .find(|b| b.action == UiAction::Persistence(PersistAction::Cancel))
        .filter(|b| super::action::cancelled(ui, b))
        .map(|b| b.action.clone());
    let mut submit_name = false;
    // Default focus belongs to Cancel on each newly opened deletion confirmation.
    let id = ui.id().with("delete_confirmation");
    let was_deleting = ui.ctx().data_mut(|d| {
        let previous = d.get_temp::<bool>(id).unwrap_or(false);
        d.insert_temp(id, model.deleting);
        previous
    });
    if model.deleting && !was_deleting {
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("delete_default_focus"), true));
    }
    let screen = ui.ctx().content_rect();
    ui.set_width(
        (if model.library { 1120.0_f32 } else { 560.0 }).min((screen.width() - 64.0).max(160.0)),
    );
    ui.spacing_mut().item_spacing.y = 7.0;
    ui.label(
        egui::RichText::new(&model.eyebrow)
            .size(11.0)
            .strong()
            .color(super::title_scene::ACCENT),
    );
    ui.label(
        egui::RichText::new(&model.title)
            .size(if model.library { 30.0 } else { 24.0 })
            .strong(),
    );
    ui.label(&model.body);
    if model.busy {
        ui.spinner();
    }
    if let Some(error) = &model.error {
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(47, 30, 37))
            .corner_radius(6.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.colored_label(ui.visuals().error_fg_color, error);
            });
    }
    if let Some(label) = &model.name_label {
        ui.label(label);
        let response = add_enabled(
            ui,
            !model.busy,
            egui::TextEdit::singleline(if model.renaming {
                &mut form.rename_name
            } else {
                &mut form.name
            })
            .id_salt(if model.renaming {
                "rename_name"
            } else {
                "save_name"
            })
            .char_limit(80)
            .desired_width(f32::INFINITY),
        );
        submit_name = super::action::submitted(&response);
        if response.enabled()
            && !response.lost_focus()
            && ui.memory(|memory| memory.focused().is_none())
        {
            response.request_focus();
        }
    }
    ui.add_space(8.0);
    let used = ui.min_rect().height();
    // Reserve the header, frame margins and footer before sizing the scrolling library.
    let height = (screen.height() - used - 146.0).clamp(60.0, 420.0);
    if model.library {
        draw_library(ui, model, height, &mut actions);
    } else {
        egui::ScrollArea::vertical()
            .id_salt("save_rows")
            .max_height(height)
            .show(ui, |ui| {
                if let Some(empty) = &model.empty {
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(17, 36, 47))
                        .corner_radius(8.0)
                        .inner_margin(20.0)
                        .show(ui, |ui| {
                            ui.set_min_width((ui.available_width() - 2.0).max(0.0));
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new(empty).size(20.0).strong());
                            ui.add_space(12.0);
                        });
                }
                for row in &model.rows {
                    draw_save_card(ui, row, false, &mut actions);
                }
            });
    }
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for b in &model.buttons {
            let (b, primary) = model.edited_name_button(b, form);
            save_action(ui, &b, primary, &mut actions);
            if primary && submit_name && b.enabled && actions.is_empty() {
                actions.push(b.action);
            }
        }
    });
    dismiss.map_or(actions, |action| vec![action])
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
    if let Some(entry) = sink.persistence().rename_entry() {
        if form.rename_slot.as_ref() != Some(&entry.metadata.slot) {
            form.rename_name = display_name(&entry.metadata, &l10n);
            form.rename_slot = Some(entry.metadata.slot.clone());
        }
    } else {
        form.rename_slot = None;
    }
    let model = saves_model(sink.persistence(), &form, &l10n);
    let actions = egui::Modal::new(egui::Id::new("saves"))
        .frame(
            egui::Frame::new()
                .fill(super::title_scene::BACKGROUND)
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(49, 99, 105)))
                .corner_radius(12.0)
                .inner_margin(20.0),
        )
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

    fn listing(index: usize, at: u64, automatic: bool) -> SaveListing {
        let id = application::SaveId::new(format!("{index:032x}")).unwrap();
        SaveListing {
            id: Some(id.clone()),
            file_label: id.as_str().into(),
            modified_at: u64::MAX,
            revision: Some(application::SaveRevision([0; 32])),
            entry: Ok(application::SaveEntry {
                region: domain::ids::RegionId::new("test"),
                region_name_key: "region-test".into(),
                metadata: application::SaveMetadata {
                    slot: id.clone(),
                    run: id,
                    kind: if automatic {
                        SaveKind::Automatic
                    } else {
                        SaveKind::Manual
                    },
                    name: format!("Save {index}"),
                    saved_at: at,
                },
                summary: application::CheckpointSummary {
                    month: domain::time::SimMonth::new(2027, 1).unwrap(),
                    tick: 1440,
                    phase: domain::session::DayPhase::Review,
                    status: domain::session::SessionStatus::Active,
                },
                revision: application::SaveRevision([0; 32]),
                compatibility: ReplayCompatibility {
                    build: "test".into(),
                    platform: "test".into(),
                },
            }),
        }
    }

    #[test]
    fn only_autosaves_leave_the_save_dialog_empty() {
        let mut persistence = Persistence::default();
        persistence.screen = SaveScreen::Save;
        persistence.entries = vec![listing(0, 1, true), listing(1, 2, true)];
        let model = saves_model(&persistence, &SaveForm::default(), &shipped_l10n());
        assert!(model.rows.is_empty());
        assert!(model.empty.is_some());
        assert_eq!(persistence.entries.len(), 2);
    }

    proptest::proptest! {
        #[test]
        fn save_dialog_lists_exactly_the_readable_manual_games(
            saves in proptest::collection::vec((proptest::bool::ANY, proptest::bool::ANY), 0..40)
        ) {
            let l10n = shipped_l10n();
            let mut persistence = Persistence::default();
            persistence.screen = SaveScreen::Save;
            persistence.entries = saves.iter().enumerate().map(|(index, &(automatic, readable))| {
                let mut entry = listing(index, index as u64, automatic);
                if readable {
                    entry.entry.as_mut().unwrap().region_name_key = "region-eastern-interconnection".into();
                } else {
                    entry.entry = Err(SaveError::ChecksumMismatch);
                }
                entry
            }).collect();
            let before = persistence.entries.clone();
            let model = saves_model(&persistence, &SaveForm::default(), &l10n);
            let expected: Vec<_> = saves.iter().enumerate()
                .filter(|(_, (automatic, readable))| !automatic && *readable)
                .map(|(index, _)| format!("Save {index}"))
                .collect();
            let actual: Vec<_> = model.rows.iter().map(|row| row.name.clone()).collect();
            proptest::prop_assert_eq!(&actual, &expected);
            proptest::prop_assert!(model.rows.iter().all(|row| !row.automatic && row.action.is_some()));
            proptest::prop_assert_eq!(model.empty.is_some(), expected.is_empty());
            proptest::prop_assert_eq!(persistence.entries, before);
            proptest::prop_assert!(l10n.problems().is_empty());
        }
    }

    proptest::proptest! {
        #[test]
        fn library_limit_keeps_newest_twelve_automatic_entries_and_every_other_file(saves in proptest::collection::vec((0_u64..100, proptest::bool::ANY), 0..80)) {
            let mut entries: Vec<_> = saves.iter().enumerate().map(|(i, (at, automatic))| listing(i, *at, *automatic)).collect();
            entries.push(SaveListing { id: None, file_label: "broken".into(), modified_at: u64::MAX, revision: None, entry: Err(SaveError::ChecksumMismatch) });
            let selected = library_entries(&entries);
            let mut expected_times: Vec<_> = saves.iter().filter(|(_, automatic)| *automatic).map(|(at, _)| *at).collect();
            expected_times.sort_by(|a, b| b.cmp(a));
            expected_times.truncate(12);
            let actual_times: Vec<_> = selected.iter().filter_map(|listing| listing.entry.as_ref().ok()).filter(|entry| entry.metadata.kind == SaveKind::Automatic).map(|entry| entry.metadata.saved_at).collect();
            proptest::prop_assert_eq!(actual_times, expected_times);
            proptest::prop_assert_eq!(selected.iter().filter(|listing| listing.entry.as_ref().is_ok_and(|entry| entry.metadata.kind == SaveKind::Manual)).count(), saves.iter().filter(|(_, automatic)| !automatic).count());
            proptest::prop_assert!(selected.iter().any(|listing| listing.entry.is_err()));
            let ordered: Vec<_> = selected.iter().map(|listing| listing.file_label.clone()).collect();
            entries.reverse();
            proptest::prop_assert_eq!(library_entries(&entries).iter().map(|listing| listing.file_label.clone()).collect::<Vec<_>>(), ordered);
        }
    }
}
