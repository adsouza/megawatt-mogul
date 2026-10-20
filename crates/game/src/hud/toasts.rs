//! Toasts: short notices for refused commands and for the domain events worth telling the player.
//!
//! A `CommandError` becomes a toast here and nowhere else (spec §8.4). Toasts run on real time,
//! the one clock the HUD may read: [`collect_toasts`] reads `Time<Real>` and passes `now` (seconds)
//! on, so the queue and the presenter stay pure and testable.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use super::action::UiAction;
use crate::l10n::{LocalizeWith, Localizer, Names, Toast};
use crate::sim::{CommandFailed, DomainEventMessage, Session};

/// How long a toast stays up, in real seconds. A repeat restarts it.
pub const TOAST_SECONDS: f64 = 4.0;

/// The most toasts on screen at once. Older ones give way to newer.
pub const MAX_TOASTS: usize = 5;

#[derive(Debug, Clone, PartialEq)]
struct ToastEntry {
    text: String,
    /// How many times this text arrived while it was up.
    count: u32,
    expires_at: f64,
}

/// The toasts on screen, oldest first.
#[derive(Resource, Debug, Default)]
pub struct ToastQueue {
    toasts: Vec<ToastEntry>,
}

impl ToastQueue {
    /// Shows `text` from `now`. A text that is already up is not shown twice: its count goes up,
    /// its time restarts and it becomes the newest.
    pub fn push(&mut self, text: String, now: f64) {
        let expires_at = now + TOAST_SECONDS;
        let entry = match self.toasts.iter().position(|toast| toast.text == text) {
            Some(index) => {
                let mut entry = self.toasts.remove(index);
                entry.count += 1;
                entry.expires_at = expires_at;
                entry
            }
            None => ToastEntry {
                text,
                count: 1,
                expires_at,
            },
        };
        self.toasts.push(entry);
        let overflow = self.toasts.len().saturating_sub(MAX_TOASTS);
        self.toasts.drain(..overflow);
    }

    /// Drops the toasts whose time is up at `now`.
    pub fn expire(&mut self, now: f64) {
        self.toasts.retain(|toast| toast.expires_at > now);
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToastsModel {
    /// Newest first.
    pub toasts: Vec<String>,
}

pub fn toasts_model(queue: &ToastQueue, l10n: &Localizer, now: f64) -> ToastsModel {
    ToastsModel {
        toasts: queue
            .toasts
            .iter()
            .rev()
            .filter(|toast| toast.expires_at > now)
            .take(MAX_TOASTS)
            .map(|toast| match toast.count {
                1 => toast.text.clone(),
                count => l10n.args(
                    "toast-repeated",
                    &[
                        ("text", toast.text.as_str().into()),
                        ("count", count.to_string().into()),
                    ],
                ),
            })
            .collect(),
    }
}

/// Toasts ask for nothing; the return type keeps every panel's draw function the same shape.
pub fn draw_toasts(ui: &mut egui::Ui, model: &ToastsModel) -> Vec<UiAction> {
    for text in &model.toasts {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_max_width(360.0);
            ui.label(text.as_str());
        });
    }
    Vec::new()
}

/// Turns this frame's refusals and toast-worthy events into toasts, and retires old ones. Runs in
/// every state: a new game that fails to start is refused from the menu.
pub(super) fn collect_toasts(
    mut failures: MessageReader<CommandFailed>,
    mut events: MessageReader<DomainEventMessage>,
    session: Option<Res<Session>>,
    l10n: Res<Localizer>,
    time: Res<Time<Real>>,
    mut queue: ResMut<ToastQueue>,
) {
    let now = time.elapsed_secs_f64();
    queue.expire(now);
    if failures.is_empty() && events.is_empty() {
        return;
    }
    let names = match &session {
        Some(session) => Names::new(&l10n, session.0.session()),
        None => Names::empty(&l10n),
    };
    for CommandFailed(error) in failures.read() {
        queue.push(error.localize_with(&l10n, &names), now);
    }
    for DomainEventMessage(event) in events.read() {
        if let Some(text) = event.toast(&l10n, &names) {
            queue.push(text, now);
        }
    }
}

pub(super) fn toasts_overlay(
    mut contexts: EguiContexts,
    queue: Res<ToastQueue>,
    l10n: Res<Localizer>,
    time: Res<Time<Real>>,
) -> Result {
    let model = toasts_model(&queue, &l10n, time.elapsed_secs_f64());
    if model.toasts.is_empty() {
        return Ok(());
    }
    egui::Area::new(egui::Id::new("toasts"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -12.0))
        .interactable(false)
        .show(contexts.ctx_mut()?, |ui| draw_toasts(ui, &model));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::test_support::shipped_l10n;

    #[test]
    fn toasts_collapse_duplicates_and_cap_at_five() {
        let l10n = shipped_l10n();
        let mut queue = ToastQueue::default();
        for (i, text) in ["a", "b", "c", "d", "e", "f"].into_iter().enumerate() {
            queue.push(text.to_owned(), i as f64 * 0.1);
        }
        queue.push("e".to_owned(), 0.7);
        queue.push("e".to_owned(), 0.8);

        let model = toasts_model(&queue, &l10n, 1.0);

        let repeated = l10n.args(
            "toast-repeated",
            &[("text", "e".into()), ("count", "3".into())],
        );
        assert_eq!(model.toasts, vec![repeated.as_str(), "f", "d", "c", "b"]);
        assert_eq!(repeated, "e (×3)");
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn toasts_last_four_real_seconds_and_a_repeat_restarts_the_clock() {
        let l10n = shipped_l10n();
        let mut queue = ToastQueue::default();
        queue.push("old".to_owned(), 0.0);
        queue.push("again".to_owned(), 0.0);
        queue.push("again".to_owned(), 3.0);

        assert_eq!(toasts_model(&queue, &l10n, 3.9).toasts.len(), 2);
        let later = toasts_model(&queue, &l10n, 4.0);
        assert_eq!(later.toasts, vec!["again (×2)"]);

        queue.expire(6.9);
        assert_eq!(toasts_model(&queue, &l10n, 6.9).toasts, vec!["again (×2)"]);
        queue.expire(7.0);
        assert!(toasts_model(&queue, &l10n, 7.0).toasts.is_empty());
    }

    #[test]
    fn a_new_text_after_expiry_starts_again_at_one() {
        let l10n = shipped_l10n();
        let mut queue = ToastQueue::default();
        queue.push("x".to_owned(), 0.0);
        queue.expire(5.0);
        queue.push("x".to_owned(), 5.0);

        assert_eq!(toasts_model(&queue, &l10n, 5.0).toasts, vec!["x"]);
    }
}
