//! The tariff panel (spec §2.2, §2.5, §8.4): the regulated tariff and allowed return as they
//! stand, and what the December rate review would do on today's state. The player does not set
//! the tariff; the panel shows how the fleet they are planning will be judged.

use crate::l10n::format::MoneyStyle;
use application::Game;
use bevy_egui::egui;
use domain::time::SimMonth;

use super::action::UiAction;
use super::theme::{TextTone, heading_color};
use super::top_bar::Stat;
use crate::l10n::{Localizer, format};

#[derive(Debug, Clone, PartialEq)]
pub struct TariffModel {
    pub title: String,
    /// The tariff and the allowed return in force now.
    pub current: Vec<Stat>,
    pub current_tones: [TextTone; 2],
    pub review_heading: String,
    /// The projected review: allowed return, reserve margin, and the capital it would hold,
    /// release or write off.
    pub review: Vec<Stat>,
    pub review_tones: [TextTone; 6],
    /// How long until the review runs: `The December review is 9 months away.`
    pub timing: String,
}

/// The months from the one being planned until December, when the annual rate review runs as the
/// month closes. December itself is 0: the review runs when this month closes.
pub fn months_until_review(month: SimMonth) -> u32 {
    12 - u32::from(month.month())
}

pub fn tariff_model(game: &Game, l10n: &Localizer, style: &MoneyStyle) -> TariffModel {
    let session = game.session();
    let company = session.company_view();
    let regulator = session.region().regulator();
    // The projection is refreshed after every command that moves the estimate; the query is the
    // same figure, so it only stands in if the projection has not been seeded.
    let estimate = game
        .projections()
        .rate_review
        .estimate()
        .cloned()
        .unwrap_or_else(|| session.rate_review_estimate());
    let stat = |key: &str, value: String| Stat {
        label: l10n.text(key),
        value,
    };
    let return_tone = |value: domain::units::Ratio| {
        if value <= regulator.min_return {
            TextTone::Negative
        } else if value < regulator.base_return {
            TextTone::Caution
        } else {
            TextTone::Positive
        }
    };
    let nonzero = |amount: domain::units::Money, tone| {
        if amount > domain::units::Money::ZERO {
            tone
        } else {
            TextTone::Neutral
        }
    };
    TariffModel {
        title: l10n.text("tariff-title"),
        current: vec![
            stat("tariff-current", format::price(company.tariff, style)),
            stat(
                "tariff-allowed-return",
                format::percent(company.allowed_return),
            ),
        ],
        current_tones: [TextTone::Information, return_tone(company.allowed_return)],
        review_heading: l10n.text("tariff-review-heading"),
        review: vec![
            stat(
                "tariff-projected-return",
                format::percent(estimate.projected_return),
            ),
            stat(
                "tariff-satisfaction-adjustment",
                format::signed_points(session.satisfaction_return_adjustment()),
            ),
            stat(
                "tariff-reserve-margin",
                format::signed_percent(estimate.reserve_margin),
            ),
            stat(
                "tariff-held-capital",
                format::money(estimate.capital_to_hold, style),
            ),
            stat(
                "tariff-capital-release",
                format::money(estimate.capital_to_release, style),
            ),
            stat(
                "tariff-capital-at-risk",
                format::money(estimate.capital_at_risk, style),
            ),
        ],
        review_tones: [
            return_tone(estimate.projected_return),
            if session.satisfaction_return_adjustment() > 0.0 {
                TextTone::Positive
            } else if session.satisfaction_return_adjustment() < 0.0 {
                TextTone::Caution
            } else {
                TextTone::Neutral
            },
            if estimate.reserve_margin >= regulator.reserve_margin.value() {
                TextTone::Positive
            } else if estimate.reserve_margin < 0.0 {
                TextTone::Negative
            } else {
                TextTone::Caution
            },
            nonzero(estimate.capital_to_hold, TextTone::Caution),
            nonzero(estimate.capital_to_release, TextTone::Positive),
            nonzero(estimate.capital_at_risk, TextTone::Negative),
        ],
        timing: l10n.args(
            "tariff-review-months",
            &[(
                "months",
                months_until_review(session.clock().month()).into(),
            )],
        ),
    }
}

pub fn draw_tariff(ui: &mut egui::Ui, model: &TariffModel) -> Vec<UiAction> {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(&model.title)
                    .strong()
                    .color(heading_color()),
            );
            stat_grid(ui, "tariff_current", &model.current, &model.current_tones);
        });
        ui.add_space(24.0);
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(&model.review_heading)
                    .strong()
                    .color(heading_color()),
            );
            stat_grid(ui, "tariff_review", &model.review, &model.review_tones);
            ui.weak(model.timing.as_str());
        });
    });
    Vec::new()
}

fn stat_grid(ui: &mut egui::Ui, id: &str, stats: &[Stat], tones: &[TextTone]) {
    egui::Grid::new(id).show(ui, |ui| {
        for (stat, tone) in stats.iter().zip(tones) {
            ui.colored_label(TextTone::Neutral.color(), &stat.label);
            ui.label(
                egui::RichText::new(&stat.value)
                    .strong()
                    .color(tone.color()),
            );
            ui.end_row();
        }
    });
}

#[cfg(test)]
mod tests {
    use crate::hud::test_support::usd_style;
    use application::InMemoryCommandLog;

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_l10n};

    #[test]
    fn tariff_panel_counts_months_to_december() {
        let months = |month: u8| months_until_review(SimMonth::new(2027, month).unwrap());

        assert_eq!(months(1), 11);
        assert_eq!(months(4), 8);
        assert_eq!(months(11), 1);
        assert_eq!(months(12), 0, "December's own close runs the review");
    }

    #[test]
    fn the_timing_line_reads_naturally_for_zero_one_and_many() {
        let l10n = shipped_l10n();
        let text = |months: u32| l10n.args("tariff-review-months", &[("months", months.into())]);

        assert_eq!(text(0), "The review runs when this month closes.");
        assert_eq!(text(1), "The December review is 1 month away.");
        assert_eq!(text(11), "The December review is 11 months away.");
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_panel_shows_the_tariff_and_the_projected_review() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let company = session.company_view();
        let estimate = session.rate_review_estimate();

        let model = tariff_model(&game, &l10n, &usd_style());

        let rows = |stats: &[Stat]| -> Vec<(String, String)> {
            stats
                .iter()
                .map(|stat| (stat.label.clone(), stat.value.clone()))
                .collect()
        };
        assert_eq!(
            rows(&model.current),
            [
                (
                    "Current tariff".to_owned(),
                    format::price(company.tariff, &usd_style())
                ),
                (
                    "Allowed return".to_owned(),
                    format::percent(company.allowed_return)
                ),
            ]
        );
        assert_eq!(
            rows(&model.review),
            [
                (
                    "Projected allowed return".to_owned(),
                    format::percent(estimate.projected_return)
                ),
                ("Satisfaction adjustment".to_owned(), "−1.00 pp".to_owned()),
                (
                    "Reserve margin".to_owned(),
                    format::signed_percent(estimate.reserve_margin)
                ),
                (
                    "Capital held out of the rate base".to_owned(),
                    format::money(estimate.capital_to_hold, &usd_style())
                ),
                (
                    "Capital released back".to_owned(),
                    format::money(estimate.capital_to_release, &usd_style())
                ),
                (
                    "Capital at risk of write-off".to_owned(),
                    format::money(estimate.capital_at_risk, &usd_style())
                ),
            ]
        );
        // The shipped eastern game starts in January.
        assert_eq!(session.clock().month().month(), 1);
        assert_eq!(model.timing, "The December review is 11 months away.");
        assert!(
            model.current[0].value.ends_with("/MWh"),
            "{}",
            model.current[0].value
        );
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_projection_and_the_query_agree() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        assert_eq!(
            game.projections().rate_review.estimate(),
            Some(&game.session().rate_review_estimate())
        );
    }
}
