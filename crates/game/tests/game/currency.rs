use std::sync::Arc;

use application::{
    Game, InMemoryCommandLog, LoadedRegion, RegionCatalog, RegionFingerprint, RegionListing,
    RegionLoadError,
};
use bevy_egui::egui;
use domain::ids::RegionId;
use domain::region::{RegionDefinition, fixtures};
use domain::session::GameSettings;
use domain::units::{Currency, Money};
use game::l10n::format::{MoneyStyle, money};
use game::sim::Session;
use tempfile::TempDir;

use crate::support::egui_pass::EguiPass;
use crate::support::{hud_app, shipped_l10n};

#[test]
fn every_currency_symbol_paints_without_a_replacement_glyph_or_uses_iso() {
    let ctx = egui::Context::default();
    let l10n = shipped_l10n();
    let output = ctx.run_ui(egui::RawInput::default(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            let font = egui::FontId::proportional(14.0);
            let replacement =
                ui.painter()
                    .layout_no_wrap("\u{fffd}".into(), font.clone(), egui::Color32::WHITE);
            let missing_uv = replacement.rows[0].glyphs[0].uv_rect;
            for currency in [
                Currency::Usd,
                Currency::Eur,
                Currency::Cny,
                Currency::Inr,
                Currency::Rub,
                Currency::Brl,
            ] {
                let style = MoneyStyle::resolve_egui(currency, &l10n, &ctx);
                let symbol = style.symbol();
                let galley =
                    ui.painter()
                        .layout_no_wrap(symbol.clone(), font.clone(), egui::Color32::WHITE);
                assert!(!galley.rows.is_empty());
                for glyph in &galley.rows[0].glyphs {
                    assert_ne!(glyph.uv_rect, missing_uv, "{currency:?}: {symbol}");
                }
                ui.painter().galley(
                    egui::pos2(10.0, ui.cursor().top()),
                    galley,
                    egui::Color32::WHITE,
                );
                ui.label(money(Money::from_cents(123), &style));
            }
        });
    });
    assert!(!output.shapes.is_empty());
    output.drop_without_applying_deltas();
}

struct CurrencyCatalog(Currency);
impl RegionCatalog for CurrencyCatalog {
    fn list(&self) -> Result<Vec<RegionListing>, RegionLoadError> {
        Ok(Vec::new())
    }
    fn load(&self, _: &RegionId) -> Result<LoadedRegion, RegionLoadError> {
        let mut parts = fixtures::tiny_region_parts();
        parts.currency = self.0;
        Ok(LoadedRegion {
            definition: Arc::new(RegionDefinition::try_from(parts).unwrap()),
            source_bytes: Vec::new(),
            fingerprint: RegionFingerprint::new([0; 32]),
        })
    }
}

#[test]
fn currency_style_refreshes_before_the_first_pass_after_session_replacement() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    crate::support::request(&mut app, "eastern_interconnection");
    let pass = EguiPass::attach(&mut app);
    for (currency, expected) in [
        (Currency::Eur, "€1.23"),
        (Currency::Brl, "R$1.23"),
        (Currency::Usd, "$1.23"),
    ] {
        let game = Game::start_new(
            &CurrencyCatalog(currency),
            &RegionId::new("tiny"),
            7,
            GameSettings::default(),
            Box::new(InMemoryCommandLog::default()),
        )
        .unwrap();
        app.world_mut().insert_resource(Session(game));
        // Deliberately no Update between replacing the session and its first paint.
        let widgets = pass.run(&mut app, 1024.0, 640.0);
        let style = app.world().resource::<MoneyStyle>();
        assert_eq!(money(Money::from_cents(123), style), expected);
        let model = game::hud::top_bar_model(
            &app.world().resource::<Session>().0,
            app.world().resource::<game::sim::Persistence>(),
            &shipped_l10n(),
            style,
        );
        assert_eq!(model.cash.value, format!("{}2.00B", style.prefix));
        assert!(
            widgets.iter().any(|widget| widget.text == model.cash.value),
            "cash must paint with the current region style"
        );
    }
}

#[test]
fn runtime_currency_fallback_uses_the_primary_context_fonts() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    let game = Game::start_new(
        &CurrencyCatalog(Currency::Eur),
        &RegionId::new("tiny"),
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    app.world_mut().insert_resource(Session(game));
    app.update();
    assert_eq!(app.world().resource::<MoneyStyle>().prefix, "€");
    let pass = EguiPass::attach(&mut app);
    // Deliberately limit the actual proportional family to emoji: the pre-frame default
    // supports the euro, but this context does not. This catches using defaults as the oracle.
    let mut fonts = egui::FontDefinitions::default();
    fonts.families.insert(
        egui::FontFamily::Proportional,
        vec!["NotoEmoji-Regular".into()],
    );
    pass.ctx().set_fonts(fonts);
    pass.run(&mut app, 1024.0, 640.0);
    assert_eq!(app.world().resource::<MoneyStyle>().prefix, "EUR ");
}
