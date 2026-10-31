//! Where the widgets a tutorial step points at were drawn (spec §4.1, "Anchors").
//!
//! A panel marks its widget's rect as it draws it; the tutorial overlay, later in the same egui
//! pass, reads it back. A rect is only good for the pass that marked it: a widget that was not
//! drawn this pass has no anchor, so a spotlight waits for it rather than pointing at stale space.

use bevy_egui::egui;

use super::TutorialAnchor;

fn id(anchor: TutorialAnchor) -> egui::Id {
    egui::Id::new(("tutorial-anchor", anchor))
}

/// Records that `anchor`'s widget was drawn at `rect` in the current pass.
pub fn mark_anchor(ctx: &egui::Context, anchor: TutorialAnchor, rect: egui::Rect) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|data| data.insert_temp(id(anchor), (pass, rect)));
}

/// The rect `anchor`'s widget was drawn at, if it was marked in the current pass.
pub fn anchor_rect(ctx: &egui::Context, anchor: TutorialAnchor) -> Option<egui::Rect> {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|data| data.get_temp::<(u64, egui::Rect)>(id(anchor)))
        .filter(|(marked, _)| *marked == pass)
        .map(|(_, rect)| rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(30.0, 40.0))
    }

    #[test]
    fn a_marked_anchor_reads_back_in_the_same_pass_only() {
        let ctx = egui::Context::default();
        let mut seen = None;
        ctx.run_ui(egui::RawInput::default(), |ui| {
            assert_eq!(anchor_rect(ui.ctx(), TutorialAnchor::Cash), None);
            mark_anchor(ui.ctx(), TutorialAnchor::Cash, rect());
            seen = Some((
                anchor_rect(ui.ctx(), TutorialAnchor::Cash),
                anchor_rect(ui.ctx(), TutorialAnchor::Map),
            ));
        })
        .drop_without_applying_deltas();
        assert_eq!(seen, Some((Some(rect()), None)));

        let mut later = Some(Some(rect()));
        ctx.run_ui(egui::RawInput::default(), |ui| {
            later = Some(anchor_rect(ui.ctx(), TutorialAnchor::Cash));
        })
        .drop_without_applying_deltas();
        assert_eq!(later, Some(None));
    }

    #[test]
    fn each_anchor_keeps_its_own_rect() {
        let ctx = egui::Context::default();
        let other = egui::Rect::from_min_size(egui::pos2(1.0, 2.0), egui::vec2(3.0, 4.0));
        let mut seen = None;
        ctx.run_ui(egui::RawInput::default(), |ui| {
            mark_anchor(ui.ctx(), TutorialAnchor::StartDay, rect());
            mark_anchor(ui.ctx(), TutorialAnchor::SpeedControls, other);
            seen = Some((
                anchor_rect(ui.ctx(), TutorialAnchor::StartDay),
                anchor_rect(ui.ctx(), TutorialAnchor::SpeedControls),
            ));
        })
        .drop_without_applying_deltas();
        assert_eq!(seen, Some((Some(rect()), Some(other))));
    }
}
