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

/// A weak currency late in a rich game shows a thousand billion in the top bar (₹1,234B,
/// ₽5,000B). Even then, at the smallest window and at the largest UI zoom, every label of the
/// bar must paint whole (egui truncates silently, and the AccessKit label still reads in full)
/// and inside the screen, with no region's longer names pushing a button off the edge.
#[test]
fn the_widest_top_bar_cash_fits_at_the_minimum_window() {
    use application::Game;
    use bevy::prelude::default;
    use game::hud::top_bar_model;
    use game::sim::Persistence;
    use infrastructure::region::RonRegionCatalog;

    let l10n = shipped_l10n();
    let catalog = RonRegionCatalog::new(crate::support::shipped_assets().join("regions"));
    let widest = [1_234_000_000_000_i64, 5_000_000_000_000]
        .into_iter()
        .flat_map(|dollars| [dollars, -dollars]);
    let widest: Vec<Money> = widest.map(Money::from_dollars).collect();
    for &region in crate::support::SHIPPED_REGIONS {
        let game = Game::start_new(
            &catalog,
            &RegionId::new(region),
            7,
            GameSettings::default(),
            Box::new(InMemoryCommandLog::default()),
        )
        .unwrap();
        for zoom in [1.0, 1.25] {
            let ctx = egui::Context::default();
            ctx.set_zoom_factor(zoom);
            // Fonts exist only once the context has run a frame.
            ctx.run_ui(egui::RawInput::default(), |_| {})
                .drop_without_applying_deltas();
            let screen = egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(
                    game::window::MIN_WINDOW_SIZE.x / zoom,
                    game::window::MIN_WINDOW_SIZE.y / zoom,
                ),
            );
            for currency in [
                Currency::Usd,
                Currency::Eur,
                Currency::Cny,
                Currency::Inr,
                Currency::Rub,
                Currency::Brl,
            ] {
                // The style the game picks from its fonts, and the symbol form.
                for style in [
                    MoneyStyle::resolve_egui(currency, &l10n, &ctx),
                    MoneyStyle::resolve(currency, &l10n, |_| true),
                ] {
                    for &amount in &widest {
                        let mut model =
                            top_bar_model(&game, &Persistence::default(), &l10n, &style);
                        model.cash.value = money(amount, &style);
                        let label =
                            format!("{region} {currency:?} {} zoom {zoom}", model.cash.value);
                        let mut checked = 0;
                        for frame in 0..4 {
                            let output = ctx.run_ui(
                                egui::RawInput {
                                    screen_rect: Some(screen),
                                    ..default()
                                },
                                |ui| {
                                    egui::Panel::top("top_bar")
                                        .show(ui, |ui| game::hud::draw_top_bar(ui, &model));
                                },
                            );
                            let shapes = output.shapes.clone();
                            output.drop_without_applying_deltas();
                            // The first frames lay out with the fonts still loading.
                            if frame < 2 {
                                continue;
                            }
                            for text in [
                                &model.date,
                                &model.time,
                                &model.season,
                                &model.phase,
                                &model.cash.label,
                                &model.cash.value,
                                &model.reliability.label,
                                &model.reliability.value,
                                &model.satisfaction.label,
                                &model.satisfaction.value,
                                &model.strikes.label,
                                &model.strikes.value,
                                &model.milestones.label,
                                &model.save.label,
                                &model.menu.label,
                            ] {
                                let painted = shapes
                                    .iter()
                                    .find_map(|shape| match &shape.shape {
                                        egui::Shape::Text(t) if t.galley.text() == *text => Some(t),
                                        _ => None,
                                    })
                                    .unwrap_or_else(|| panic!("{label}: {text:?} was not painted"));
                                let glyphs: String = painted
                                    .galley
                                    .rows
                                    .iter()
                                    .flat_map(|row| row.glyphs.iter().map(|g| g.chr))
                                    .collect();
                                assert_eq!(&glyphs, text, "{label}: painted whole");
                                let bounds = painted.visual_bounding_rect();
                                assert!(
                                    screen.contains_rect(bounds),
                                    "{label}: {text:?} at {bounds:?} leaves the {screen:?} screen"
                                );
                                checked += 1;
                            }
                        }
                        assert_eq!(checked, 2 * 15, "{label}");
                    }
                }
            }
        }
    }
}
