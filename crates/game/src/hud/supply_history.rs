//! Archived played-day curves, rebuilt from the command journal on resume.
use crate::l10n::format::MoneyStyle;
use application::Game;
use bevy_egui::egui;
use domain::time::SimMonth;

use super::{SupplyModel, UiAction};
use crate::l10n::{Localizer, format};

#[derive(Debug, Clone, PartialEq)]
pub struct SupplyHistoryModel {
    pub month_label: String,
    pub months: Vec<(SimMonth, String)>,
    pub selected: Option<SimMonth>,
    pub note: String,
    pub empty: String,
    pub chart: Option<Box<SupplyModel>>,
}

impl SupplyHistoryModel {
    pub fn with_layout(mut self, layout: super::SupplyChartLayout) -> Self {
        if let Some(chart) = &mut self.chart {
            chart.layout = layout;
        }
        self
    }
}

pub fn supply_history_model(
    game: &Game,
    selected: Option<SimMonth>,
    l10n: &Localizer,
    style: &MoneyStyle,
) -> SupplyHistoryModel {
    let days = game.projections().timeline.completed_days();
    let day = selected
        .and_then(|month| days.iter().find(|day| day.month == month))
        .or_else(|| days.last());
    SupplyHistoryModel {
        month_label: l10n.text("supply-history-month"),
        months: days
            .iter()
            .rev()
            .map(|day| (day.month, format::month_year(day.month)))
            .collect(),
        selected: day.map(|day| day.month),
        note: l10n.text("supply-history-note"),
        empty: l10n.text("supply-history-empty"),
        chart: day.map(|day| {
            Box::new(super::supply_chart::completed_supply_model(
                game, day, l10n, style,
            ))
        }),
    }
}

pub fn draw_supply_history(ui: &mut egui::Ui, model: &SupplyHistoryModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let Some(chart) = &model.chart else {
        ui.colored_label(super::theme::TextTone::Neutral.color(), &model.empty);
        return actions;
    };
    ui.horizontal(|ui| {
        ui.colored_label(super::theme::TextTone::Neutral.color(), &model.month_label)
            .on_hover_text(&model.note);
        let selected = model
            .months
            .iter()
            .find(|(month, _)| Some(*month) == model.selected);
        egui::ComboBox::from_id_salt("supply_history_month")
            .selected_text(
                egui::RichText::new(selected.map_or("", |(_, label)| label.as_str()))
                    .color(super::theme::TextTone::Caution.color()),
            )
            .show_ui(ui, |ui| {
                for (month, label) in &model.months {
                    if ui
                        .selectable_label(Some(*month) == model.selected, label)
                        .clicked()
                    {
                        actions.push(UiAction::SelectSupplyMonth(*month));
                    }
                }
            });
    });
    // Keep visibility choices separate from the live chart and between represented months.
    ui.push_id(("supply_history", model.selected), |ui| {
        actions.extend(super::draw_supply_chart(ui, chart));
    });
    actions
}
