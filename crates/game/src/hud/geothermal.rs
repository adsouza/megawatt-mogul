//! Pure geothermal presenter and action-only drawing, shared by Planning and Operating.
use super::action::{ButtonModel, UiAction, button};
use crate::l10n::{
    LocalizeWith, Localizer, Names,
    format::{self, MoneyStyle},
};
use application::{Command, Game};
use bevy_egui::egui;
use domain::{ids::PlantId, session::DayPhase, units::Ratio};
#[derive(Debug, Clone, PartialEq)]
pub struct GeothermalCard {
    pub field: String,
    pub progress: Option<String>,
    pub quote: Option<String>,
    pub action: Option<ButtonModel>,
    pub reason: Option<String>,
}
pub fn geothermal_model(
    game: &Game,
    l10n: &Localizer,
    id: PlantId,
    style: &MoneyStyle,
) -> Option<GeothermalCard> {
    let s = game.session();
    let p = s.power().plant(id)?;
    let g = p.geothermal()?;
    let params = s.region().geothermal()?;
    let annual_loss = 1.0 - (0..12).fold(1.0, |v, _| v * (1.0 - params.monthly_decline.value()));
    let field = l10n.args(
        "geothermal-field",
        &[
            (
                "field",
                format::percent(
                    Ratio::new((g.field.value() / g.initial_field.value()).clamp(0.0, 1.0))
                        .expect("field ratio"),
                )
                .into(),
            ),
            (
                "decline",
                format::percent(Ratio::new(annual_loss).expect("annual decline")).into(),
            ),
            ("rounds", g.rounds.into()),
        ],
    );
    if g.exploration_left > 0 {
        return Some(GeothermalCard {
            field,
            progress: Some(l10n.args(
                "geothermal-exploring",
                &[
                    ("months", g.exploration_left.into()),
                    ("odds", format::percent(g.odds).into()),
                    (
                        "capital",
                        format::money(g.exploration_capital, style).into(),
                    ),
                ],
            )),
            quote: None,
            action: None,
            reason: None,
        });
    }
    let progress = g
        .makeup
        .as_ref()
        .map(|m| l10n.args("geothermal-drilling", &[("months", m.months_left.into())]));
    // Makeup is ordered in Planning; elsewhere the card only reports the field, read-only.
    if s.phase() != DayPhase::Planning {
        return Some(GeothermalCard {
            field,
            progress,
            quote: None,
            action: None,
            reason: None,
        });
    }
    let quote = s.makeup_quote(id);
    Some(GeothermalCard {
        field,
        progress,
        quote: quote.as_ref().ok().map(|q| {
            l10n.args(
                "geothermal-makeup-quote",
                &[
                    ("capital", format::money(q.capital, style).into()),
                    ("months", q.months.into()),
                    ("from", format::megawatts(q.from).into()),
                    ("to", format::megawatts(q.to).into()),
                ],
            )
        }),
        action: Some(
            ButtonModel::new(
                l10n.text("geothermal-drill"),
                UiAction::Command(Command::DrillMakeupWells { plant: id }),
            )
            .enabled_if(quote.is_ok()),
        ),
        reason: quote
            .err()
            .map(|e| e.localize_with(l10n, &Names::new(l10n, s))),
    })
}
pub fn draw_geothermal(ui: &mut egui::Ui, model: &GeothermalCard, actions: &mut Vec<UiAction>) {
    ui.label(&model.field);
    for text in [&model.progress, &model.quote, &model.reason]
        .into_iter()
        .flatten()
    {
        ui.label(text);
    }
    if let Some(action) = &model.action {
        button(ui, action, actions);
    }
}

/// Reconcile view-only selection through the action funnel after a dry result.
pub(super) fn clean_dry_selection(mut sink: super::ActionSink) {
    let action = sink.game().and_then(|game| match sink.selection() {
        super::Selection::Plant(id) if game.session().power().plant(id).is_none() => game
            .session()
            .dry_cells()
            .iter()
            .find(|(_, site)| site.plant == id)
            .map(|(cell, _)| UiAction::Select(super::Selection::Cell(*cell))),
        super::Selection::LineFrom(node) | super::Selection::HvdcFrom(node)
            if game.session().power().network().node(node).is_none() =>
        {
            Some(UiAction::Select(super::Selection::None))
        }
        _ => None,
    });
    if let Some(action) = action {
        super::dispatch([action], &mut sink);
    }
}
