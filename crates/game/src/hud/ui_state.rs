//! The Planning panel's own state: which tab is open, which hour the grid highlights, and the
//! commitment pattern being edited.
//!
//! [`UiState`] is written in exactly one place, [`UiState::apply`], which [`dispatch`] calls for
//! every action. Nothing here is game state: the domain never sees a draft until the player
//! applies it, and the model for the grid is rebuilt from the domain's commitments every frame.
//!
//! [`dispatch`]: super::dispatch

use std::collections::BTreeMap;

use application::Command;
use bevy::prelude::*;
use domain::ids::PlantId;
use domain::time::SimMonth;

use super::action::UiAction;

/// The tabs of the bottom Planning panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanningTab {
    /// The commitment grid and the planning warnings.
    #[default]
    Commitments,
    /// The forecast chart.
    Forecast,
    /// The tariff and the projected rate review.
    Tariff,
    /// The plants and lines under construction (spec §16.9).
    Construction,
    /// Completed daily supply/demand curves, labelled by their represented month.
    History,
}

/// A commitment pattern the player is editing and has not applied yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitDraft {
    pub plant: PlantId,
    pub hours: [bool; 24],
}

/// A `CommitPlant` the HUD has sent and the game has not answered yet.
///
/// `CommitmentInfeasible` carries no plant id, so a refusal can only be matched to its row by the
/// command the row sent: this record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingCommit {
    pub plant: PlantId,
    pub hours: [bool; 24],
}

/// What the Planning panel remembers between frames.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default)]
pub struct UiState {
    pub tab: PlanningTab,
    /// The archived supply chart's month; `None` selects the latest completed month.
    pub supply_month: Option<SimMonth>,
    /// Whether Review is displaying the completed-day chart rather than the summary.
    pub review_supply: bool,
    /// The hour a warning pointed at: its column is tinted in the grid and marked in the chart.
    pub highlight_hour: Option<u8>,
    /// Whether the warnings past the first are listed.
    pub warnings_open: bool,
    /// The one row being edited. Editing another row sends this one first.
    pub draft: Option<CommitDraft>,
    /// The commit sent and not yet answered.
    pub pending: Option<PendingCommit>,
}

impl UiState {
    /// The draft of `plant`'s row, if that row is the one being edited.
    pub fn draft_of(&self, plant: PlantId) -> Option<[bool; 24]> {
        self.draft
            .filter(|draft| draft.plant == plant)
            .map(|draft| draft.hours)
    }

    /// Takes in one action. The only writer of this state.
    pub fn apply(&mut self, action: &UiAction) {
        match action {
            UiAction::SelectSupplyMonth(month) => self.supply_month = Some(*month),
            UiAction::ShowReviewSupply(show) => {
                self.review_supply = *show;
                self.supply_month = None;
            }
            UiAction::SetPlanningTab(tab) => self.tab = *tab,
            UiAction::ShowHour(hour) => self.highlight_hour = *hour,
            UiAction::ToggleWarnings => self.warnings_open = !self.warnings_open,
            UiAction::EditCommitment { plant, hours } => {
                self.draft = Some(CommitDraft {
                    plant: *plant,
                    hours: *hours,
                });
            }
            UiAction::DiscardCommitmentDraft => self.draft = None,
            // The draft stays up until the game has answered, so a refusal can take it back.
            UiAction::Command(Command::CommitPlant { plant, hours }) => {
                self.pending = Some(PendingCommit {
                    plant: *plant,
                    hours: *hours,
                });
            }
            // All-day and off replace the pattern outright, so an edit of that row is moot.
            UiAction::Command(Command::StartPlant { plant } | Command::StopPlant { plant })
                if self.draft.is_some_and(|draft| draft.plant == *plant) =>
            {
                self.draft = None;
            }
            _ => {}
        }
    }

    /// The game has answered the pending commit, one way or the other: the row goes back to what
    /// the domain holds. If it accepted the pattern the draft and the domain agree, so nothing is
    /// lost; if it refused (`CommitmentInfeasible`, or a phase that moved on) the session is
    /// unchanged, so this is the revert, and the toast says why. Another row's draft, started in
    /// the same frame, is left alone.
    pub fn settle_commit(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if self.draft.is_some_and(|draft| draft.plant == pending.plant) {
            self.draft = None;
        }
    }
}

/// The text of the rows' initial-target fields, by plant. The grid's first draw of a row puts the
/// plant's current target here, and the player's typing edits it from then on; the presenter reads
/// a plant that is not here yet (before that first draw, or after a reset) as its current target.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default)]
pub struct CommitmentForm {
    pub targets: BTreeMap<PlantId, String>,
}

pub(super) fn reset_ui_state(mut ui: ResMut<UiState>, mut form: ResMut<CommitmentForm>) {
    *ui = UiState::default();
    *form = CommitmentForm::default();
}

/// Answers the pending commit once the game has run this frame's commands.
pub(super) fn settle_commit(mut ui: ResMut<UiState>) {
    if ui.pending.is_some() {
        ui.settle_commit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLANT: PlantId = PlantId(3);
    const OTHER: PlantId = PlantId(4);

    fn day() -> [bool; 24] {
        let mut hours = [false; 24];
        hours[6..18].fill(true);
        hours
    }

    #[test]
    fn actions_write_the_tab_the_hour_and_the_draft() {
        let mut ui = UiState::default();

        ui.apply(&UiAction::SetPlanningTab(PlanningTab::Forecast));
        ui.apply(&UiAction::ShowHour(Some(18)));
        ui.apply(&UiAction::EditCommitment {
            plant: PLANT,
            hours: day(),
        });

        assert_eq!(ui.tab, PlanningTab::Forecast);
        assert_eq!(ui.highlight_hour, Some(18));
        assert_eq!(ui.draft_of(PLANT), Some(day()));
        assert_eq!(ui.draft_of(OTHER), None);

        ui.apply(&UiAction::ToggleWarnings);
        assert!(ui.warnings_open);
        ui.apply(&UiAction::ToggleWarnings);
        assert!(!ui.warnings_open);

        ui.apply(&UiAction::ShowHour(None));
        ui.apply(&UiAction::DiscardCommitmentDraft);
        assert_eq!(ui.highlight_hour, None);
        assert_eq!(ui.draft, None);
    }

    #[test]
    fn a_sent_commit_is_pending_until_the_game_answers_and_then_the_draft_goes() {
        let mut ui = UiState::default();
        ui.apply(&UiAction::EditCommitment {
            plant: PLANT,
            hours: day(),
        });

        ui.apply(&UiAction::Command(Command::CommitPlant {
            plant: PLANT,
            hours: day(),
        }));
        assert_eq!(
            ui.pending,
            Some(PendingCommit {
                plant: PLANT,
                hours: day()
            })
        );
        assert!(
            ui.draft.is_some(),
            "the draft stays up while the game thinks"
        );

        ui.settle_commit();

        assert_eq!(ui.pending, None);
        assert_eq!(ui.draft, None, "the row is back to what the domain holds");
    }

    #[test]
    fn settling_leaves_a_draft_started_on_another_row_in_the_same_frame() {
        let mut ui = UiState::default();
        // Leaving a row: its commit is sent, and the next row's draft starts.
        ui.apply(&UiAction::EditCommitment {
            plant: PLANT,
            hours: day(),
        });
        ui.apply(&UiAction::Command(Command::CommitPlant {
            plant: PLANT,
            hours: day(),
        }));
        ui.apply(&UiAction::EditCommitment {
            plant: OTHER,
            hours: [true; 24],
        });

        ui.settle_commit();

        assert_eq!(ui.draft_of(OTHER), Some([true; 24]));
    }

    #[test]
    fn all_day_and_off_drop_the_draft_of_their_own_row_only() {
        let mut ui = UiState::default();
        ui.apply(&UiAction::EditCommitment {
            plant: PLANT,
            hours: day(),
        });

        ui.apply(&UiAction::Command(Command::StartPlant { plant: OTHER }));
        assert_eq!(ui.draft_of(PLANT), Some(day()));

        ui.apply(&UiAction::Command(Command::StopPlant { plant: PLANT }));
        assert_eq!(ui.draft, None);
    }

    #[test]
    fn settling_with_nothing_pending_changes_nothing() {
        let mut ui = UiState::default();
        ui.apply(&UiAction::EditCommitment {
            plant: PLANT,
            hours: day(),
        });
        let before = ui.clone();

        ui.settle_commit();

        assert_eq!(ui, before);
    }
}
