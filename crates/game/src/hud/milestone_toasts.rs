//! Toasts for milestones: one when a milestone is earned, one when the hidden bonus appears.
//!
//! [`announce_milestones`] only reads the session: it compares the milestone projection with what
//! it announced last frame and sends a [`MilestoneToast`] for the difference. `collect_toasts`
//! turns those into text. A new game, a loaded save and a resumed replay each start a new
//! [`SessionGeneration`], and the cursor then adopts what the game already holds without a word:
//! a save that was awarded a milestone as it restored is not news.

use application::projections::MilestoneKind;
use bevy::prelude::*;

use crate::l10n::{Localize, Localizer};
use crate::sim::{Session, SessionGeneration};

/// A milestone worth a toast.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MilestoneToast(pub MilestoneNotice);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MilestoneNotice {
    /// The hidden bonus milestone has appeared in the panel.
    Revealed(MilestoneKind),
    /// A milestone has been earned.
    Reached(MilestoneKind),
}

impl MilestoneNotice {
    /// The toast's text.
    pub fn text(self, l10n: &Localizer) -> String {
        let (key, kind) = match self {
            Self::Revealed(kind) => ("toast-milestone-revealed", kind),
            Self::Reached(kind) => ("toast-milestone-reached", kind),
        };
        l10n.args(key, &[("name", kind.localize(l10n).into())])
    }
}

/// What has been announced for the current game.
#[derive(Resource, Debug, Default)]
pub(super) struct MilestoneCursor {
    /// The game the counts below belong to; `None` before the first game.
    generation: Option<u64>,
    /// How many milestones had been reached.
    reached: usize,
    /// Whether the bonus had appeared.
    revealed: bool,
}

/// Sends a toast for each milestone reached and for the bonus appearing since last frame. Never
/// changes the session. Runs after the frame's commands and any load, before the toasts are
/// collected.
pub(super) fn announce_milestones(
    session: Option<Res<Session>>,
    generation: Res<SessionGeneration>,
    mut cursor: ResMut<MilestoneCursor>,
    mut out: MessageWriter<MilestoneToast>,
) {
    let Some(session) = session else {
        return;
    };
    let milestones = &session.0.projections().milestones;
    let reached = milestones.reached();
    let bonus = milestones.bonus();
    if cursor.generation != Some(generation.0) {
        *cursor = MilestoneCursor {
            generation: Some(generation.0),
            reached: reached.len(),
            revealed: bonus.is_some(),
        };
        return;
    }
    let new = reached.get(cursor.reached..).unwrap_or_default();
    cursor.reached = reached.len();
    let is_bonus = |kind: &&MilestoneKind| **kind == MilestoneKind::FossilFreeYear;
    // The bonus appears when the third regional milestone is earned, and can be earned at once:
    // tell of the reveal before the award.
    for kind in new.iter().filter(|kind| !is_bonus(kind)) {
        out.write(MilestoneToast(MilestoneNotice::Reached(*kind)));
    }
    if let Some(kind) = bonus.filter(|_| !cursor.revealed) {
        cursor.revealed = true;
        out.write(MilestoneToast(MilestoneNotice::Revealed(kind)));
    }
    for kind in new.iter().filter(is_bonus) {
        out.write(MilestoneToast(MilestoneNotice::Reached(*kind)));
    }
}
