//! Currency styles cached for the shipped fonts, then verified against the actual HUD context.
//! The pre-frame cache supports headless event localization; the first paint replaces it with
//! styles resolved from the very fonts that will paint the HUD, before any panel runs.
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::units::Currency;

use super::{Localizer, format::MoneyStyle};
use crate::sim::Session;

const CURRENCIES: [Currency; 6] = [
    Currency::Usd,
    Currency::Eur,
    Currency::Cny,
    Currency::Inr,
    Currency::Rub,
    Currency::Brl,
];

#[derive(Resource)]
pub(crate) struct MoneyStyles {
    styles: Vec<(Currency, MoneyStyle)>,
    actual_fonts: bool,
}

impl MoneyStyles {
    fn resolve(l10n: &Localizer, ctx: &egui::Context, actual_fonts: bool) -> Self {
        Self {
            styles: CURRENCIES
                .into_iter()
                .map(|currency| (currency, MoneyStyle::resolve_egui(currency, l10n, ctx)))
                .collect(),
            actual_fonts,
        }
    }

    fn active(&self, session: Option<&Session>) -> &MoneyStyle {
        let currency = session.map_or(Currency::Usd, |session| {
            session.0.session().region().currency()
        });
        &self
            .styles
            .iter()
            .find(|(candidate, _)| *candidate == currency)
            .expect("all supported currencies are cached")
            .1
    }
}

fn install_active(
    commands: &mut Commands,
    cache: &MoneyStyles,
    session: Option<&Session>,
    current: Option<&MoneyStyle>,
) {
    let active = cache.active(session);
    if current != Some(active) {
        commands.insert_resource(active.clone());
    }
}

pub(crate) fn refresh_before_events(
    mut commands: Commands,
    session: Option<Res<Session>>,
    l10n: Res<Localizer>,
    cache: Option<Res<MoneyStyles>>,
    current: Option<Res<MoneyStyle>>,
) {
    if let Some(cache) = cache.filter(|_| !l10n.is_changed()) {
        install_active(
            &mut commands,
            &cache,
            session.as_deref(),
            current.as_deref(),
        );
        return;
    }
    // Egui uses its bundled fonts in this game; no window or GPU is needed to inspect them.
    let ctx = egui::Context::default();
    ctx.begin_pass(egui::RawInput::default());
    let cache = MoneyStyles::resolve(&l10n, &ctx, false);
    ctx.end_pass().drop_without_applying_deltas();
    install_active(
        &mut commands,
        &cache,
        session.as_deref(),
        current.as_deref(),
    );
    commands.insert_resource(cache);
}

pub(crate) fn refresh_before_paint(
    mut contexts: EguiContexts,
    mut commands: Commands,
    session: Option<Res<Session>>,
    l10n: Res<Localizer>,
    cache: Option<Res<MoneyStyles>>,
    current: Option<Res<MoneyStyle>>,
) -> Result {
    if let Some(cache) = cache.filter(|cache| cache.actual_fonts && !l10n.is_changed()) {
        install_active(
            &mut commands,
            &cache,
            session.as_deref(),
            current.as_deref(),
        );
        return Ok(());
    }
    let cache = MoneyStyles::resolve(&l10n, contexts.ctx_mut()?, true);
    install_active(
        &mut commands,
        &cache,
        session.as_deref(),
        current.as_deref(),
    );
    commands.insert_resource(cache);
    Ok(())
}
