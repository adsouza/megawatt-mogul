//! A19 through actual widgets/dispatch and the real egui pass.
use crate::{
    hud_smoke::dispatch_in,
    support::{self, egui_harness::Harness, egui_pass::EguiPass},
};
use application::Command;
use domain::{ids::CellCoord, region::PlantKind};
use game::{
    hud::{self, Selection, UiAction},
    sim::Session,
};

#[test]
fn geothermal_build_row_and_exploration_card_use_real_controls_and_disable_connections() {
    let dir = tempfile::TempDir::new().unwrap();
    let mut app = support::hud_app(&dir.path().join("logs"));
    app.insert_resource(game::paths::SaveDir(dir.path().join("saves")));
    support::request(&mut app, "western_interconnection");
    let l10n = support::shipped_l10n();
    let style = support::usd_style();
    let cell = {
        let game = &app.world().resource::<Session>().0;
        let s = game.session();
        let t = s.region().terrain();
        (0..t.height())
            .flat_map(|y| (0..t.width()).map(move |x| CellCoord { x, y }))
            .find(|at| s.plant_site_check(PlantKind::Geothermal, *at).is_ok())
            .unwrap()
    };
    let mut model = hud::build_model(
        &app.world().resource::<Session>().0,
        &l10n,
        Selection::Cell(cell),
        &style,
    )
    .unwrap();
    if let hud::PlantSection::Kinds { rows, .. } = &mut model.plant {
        assert_eq!(rows.len(), 9);
        let row = rows
            .iter()
            .find(|r| r.kind == PlantKind::Geothermal)
            .unwrap();
        assert!(row.build.enabled);
        assert!(
            row.stats
                .iter()
                .any(|s| s.label == l10n.text("geothermal-risk"))
        );
        rows.retain(|r| r.kind == PlantKind::Geothermal);
    } else {
        panic!("selected cell");
    }
    let harness = Harness::with_screen(
        game::window::MIN_WINDOW_SIZE.x,
        game::window::MIN_WINDOW_SIZE.y,
    );
    let actions = harness.click(&l10n.text("build-plant"), |ui| hud::draw_build(ui, &model));
    assert_eq!(actions.len(), 1);
    dispatch_in(&mut app, actions);
    support::update_n(&mut app, 2);
    let (id, node, anchor) = {
        let s = app.world().resource::<Session>().0.session();
        let p = s
            .plant_views()
            .into_iter()
            .find(|p| p.kind == PlantKind::Geothermal)
            .unwrap();
        (p.id, p.node, s.anchor_node())
    };
    assert!(
        app.world()
            .resource::<Session>()
            .0
            .session()
            .line_quote(anchor, node)
            .is_err()
    );
    let model =
        hud::construction_card_model(&app.world().resource::<Session>().0, &l10n, id, &style)
            .unwrap();
    assert!(model.status.contains("Exploration"));
    dispatch_in(&mut app, vec![UiAction::InspectPlant(id)]);
    support::update_n(&mut app, 2);
    let pass = EguiPass::attach(&mut app);
    pass.settled(
        &mut app,
        game::window::MIN_WINDOW_SIZE.x,
        game::window::MIN_WINDOW_SIZE.y,
    );
    assert!(l10n.problems().is_empty());
}
#[test]
fn geothermal_all_region_build_rows_show_the_correct_disabled_reason_and_localization() {
    use application::{Game, InMemoryCommandLog};
    use infrastructure::region::RonRegionCatalog;
    let l10n = support::shipped_l10n();
    let style = support::usd_style();
    let game = Game::start_new(
        &RonRegionCatalog::new(support::shipped_assets().join("regions")),
        &domain::ids::RegionId::new("eastern_interconnection"),
        7,
        domain::session::GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    let at = CellCoord { x: 20, y: 20 };
    let model = hud::build_model(&game, &l10n, Selection::Cell(at), &style).unwrap();
    let hud::PlantSection::Kinds { rows, .. } = model.plant else {
        panic!()
    };
    let row = rows
        .iter()
        .find(|r| r.kind == PlantKind::Geothermal)
        .unwrap();
    assert!(!row.build.enabled);
    assert_eq!(
        row.blocked,
        Some(l10n.text("geothermal-no-region-resource"))
    );
    assert!(l10n.problems().is_empty());
}
#[test]
fn geothermal_model_and_map_kind_style_have_distinct_resource_and_geometry() {
    let parts = game::world::plant_parts(PlantKind::Geothermal);
    assert!(!parts.is_empty());
    assert_ne!(parts, game::world::plant_parts(PlantKind::Coal));
    assert!(hud::MapLayer::all().any(|l| l == hud::MapLayer::GeothermalPotential));
    // Keep the game integration module in the existing binary.
    let _ = Command::DismissFieldNotice {
        plant: domain::ids::PlantId(1),
    };
}

fn operating_fixture() -> (tempfile::TempDir, application::Game, domain::ids::PlantId) {
    use application::{Game, InMemoryCommandLog, RegionCatalog};
    use infrastructure::region::RonRegionCatalog;
    let dir = tempfile::TempDir::new().unwrap();
    let source = support::shipped_assets().join("regions");
    let mut source_text =
        std::fs::read_to_string(source.join("western_interconnection.ron")).unwrap();
    let loaded = RonRegionCatalog::new(&source)
        .load(&domain::ids::RegionId::new("western_interconnection"))
        .unwrap();
    let game = Game::start_new(
        &RonRegionCatalog::new(&source),
        loaded.definition.id(),
        7,
        domain::session::GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    let t = loaded.definition.terrain();
    let at = (0..t.height())
        .flat_map(|y| (0..t.width()).map(move |x| CellCoord { x, y }))
        .filter(|at| {
            game.session()
                .plant_site_check(PlantKind::Geothermal, *at)
                .is_ok()
        })
        .max_by(|a, b| {
            t.get(*a)
                .unwrap()
                .geothermal_potential
                .value()
                .total_cmp(&t.get(*b).unwrap().geothermal_potential.value())
        })
        .unwrap();
    let (lat, lon) = loaded.definition.cell_lat_lon(at);
    // Accelerated acceptance data stays in this test's own temporary region archive.
    source_text = source_text.replace(
        "monthly_decline: 0.002535048613836126",
        "monthly_decline: 0.04",
    );
    source_text = source_text.replacen(
        "fleet: [",
        &format!("fleet: [\n(Geothermal, {lon}, {lat}),"),
        1,
    );
    std::fs::create_dir_all(dir.path().join("western_interconnection")).unwrap();
    std::fs::copy(
        source.join("western_interconnection/solar_clearness_cells.ron"),
        dir.path()
            .join("western_interconnection/solar_clearness_cells.ron"),
    )
    .unwrap();
    std::fs::write(dir.path().join("western_interconnection.ron"), source_text).unwrap();
    let game = Game::start_new(
        &RonRegionCatalog::new(dir.path()),
        &domain::ids::RegionId::new("western_interconnection"),
        7,
        domain::session::GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    let id = game
        .session()
        .plant_views()
        .into_iter()
        .find(|p| p.kind == PlantKind::Geothermal)
        .unwrap()
        .id;
    (dir, game, id)
}
fn play_at_32x(app: &mut bevy::prelude::App) {
    use bevy::time::TimeUpdateStrategy;
    use domain::session::DayPhase;
    dispatch_in(app, vec![UiAction::Command(Command::BeginOperating)]);
    support::update_n(app, 2);
    dispatch_in(app, vec![UiAction::SetSpeed(game::sim::SimSpeed::X32)]);
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    for _ in 0..70 {
        app.update();
        if app.world().resource::<Session>().0.session().phase() == DayPhase::Review {
            break;
        }
    }
    assert_eq!(
        app.world().resource::<Session>().0.session().phase(),
        DayPhase::Review
    );
    assert_eq!(
        *app.world().resource::<game::sim::SimSpeed>(),
        game::sim::SimSpeed::Paused
    );
    crate::player_workflows::settle_save(app);
}
#[test]
fn geothermal_32x_notices_real_makeup_and_dismissal_controls_survive_save_resume_and_fit() {
    use application::RegionCatalog;
    use application::{Game, InMemoryCommandLog};
    use bevy_egui::egui;
    use infrastructure::region::RonRegionCatalog;
    let (dir, game, id) = operating_fixture();
    let mut app = support::hud_app(&dir.path().join("logs"));
    app.insert_resource(game::paths::SaveDir(dir.path().join("saves")));
    support::request(&mut app, "western_interconnection");
    app.insert_resource(Session(game));
    let l10n = support::shipped_l10n();
    let style = support::usd_style();
    for month in 0..3 {
        if month > 0 {
            dispatch_in(&mut app, vec![UiAction::Command(Command::FinishReview)]);
            support::update_n(&mut app, 2);
        }
        play_at_32x(&mut app);
    }
    assert!(
        app.world()
            .resource::<Session>()
            .0
            .session()
            .power()
            .plant(id)
            .unwrap()
            .geothermal()
            .unwrap()
            .notice
            .is_some()
    );
    dispatch_in(&mut app, vec![UiAction::Command(Command::FinishReview)]);
    support::update_n(&mut app, 2);
    let mut warnings = hud::warnings_model(
        &app.world().resource::<Session>().0,
        &l10n,
        &hud::UiState::default(),
    );
    warnings
        .rows
        .retain(|r| matches!(r.kind, hud::WarningKind::Geothermal(_)));
    assert_eq!(warnings.rows.len(), 1);
    let harness = Harness::with_screen(1024.0, 640.0);
    let actions = harness.click(&l10n.text("geothermal-show-plant"), |ui| {
        hud::draw_warnings(ui, &warnings)
    });
    assert_eq!(actions, vec![UiAction::InspectPlant(id)]);
    dispatch_in(&mut app, actions);
    support::update_n(&mut app, 2);
    assert_eq!(*app.world().resource::<Selection>(), Selection::Plant(id));
    let actions = harness.click(&l10n.text("geothermal-not-now"), |ui| {
        hud::draw_warnings(ui, &warnings)
    });
    assert_eq!(
        actions,
        vec![UiAction::Command(Command::DismissFieldNotice { plant: id })]
    );
    dispatch_in(&mut app, actions);
    support::update_n(&mut app, 2);
    assert!(
        app.world()
            .resource::<Session>()
            .0
            .session()
            .power()
            .plant(id)
            .unwrap()
            .geothermal()
            .unwrap()
            .notice
            .is_none()
    );
    let checkpoint = app.world().resource::<Session>().0.checkpoint();
    let loaded = RonRegionCatalog::new(dir.path())
        .load(&domain::ids::RegionId::new("western_interconnection"))
        .unwrap();
    let restored = Game::restore(
        loaded,
        checkpoint.clone(),
        Box::new(InMemoryCommandLog::default()),
        &|| false,
    )
    .unwrap();
    assert_eq!(restored.checkpoint(), checkpoint);
    app.insert_resource(Session(restored));
    let card =
        hud::geothermal::geothermal_model(&app.world().resource::<Session>().0, &l10n, id, &style)
            .unwrap();
    assert!(card.action.as_ref().unwrap().enabled);
    let actions = harness.click(&l10n.text("geothermal-drill"), |ui| {
        let mut actions = Vec::new();
        hud::geothermal::draw_geothermal(ui, &card, &mut actions);
        actions
    });
    assert_eq!(actions.len(), 1);
    dispatch_in(&mut app, actions);
    support::update_n(&mut app, 2);
    assert!(
        app.world()
            .resource::<Session>()
            .0
            .session()
            .power()
            .plant(id)
            .unwrap()
            .geothermal()
            .unwrap()
            .makeup
            .is_some()
    );
    let pass = EguiPass::attach(&mut app);
    for zoom in [1.0, 1.25] {
        pass.ctx().set_zoom_factor(zoom);
        let widgets = pass.settled(&mut app, 1024.0 / zoom, 640.0 / zoom);
        let rect =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0 / zoom, 640.0 / zoom));
        assert!(
            widgets.iter().any(|w| w.text.contains("Field:")),
            "selected geothermal card must be visible"
        );
        assert!(
            widgets
                .iter()
                .filter(|w| w.text.contains("Field:"))
                .all(|w| rect.contains_rect(w.rect))
        );
    }
    pass.ctx().set_zoom_factor(1.0);
    for month in 0..3 {
        if month > 0 {
            dispatch_in(&mut app, vec![UiAction::Command(Command::FinishReview)]);
            support::update_n(&mut app, 2);
        }
        play_at_32x(&mut app);
    }
    let state = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .power()
        .plant(id)
        .unwrap()
        .geothermal()
        .unwrap();
    assert_eq!(state.rounds, 1);
    assert!((state.field.value() / state.initial_field.value() - 0.95).abs() < 1e-12);
    assert!(l10n.problems().is_empty());
}

/// Closes `months` whole months through the real command pipeline, ending in Planning.
fn close_months(game: &mut application::Game, months: u32) {
    for _ in 0..months {
        for command in [
            Command::BeginOperating,
            Command::AdvanceTicks { n: 1440 },
            Command::FinishReview,
        ] {
            game.execute(command).unwrap();
        }
    }
}

fn western_site(game: &application::Game) -> CellCoord {
    let s = game.session();
    let t = s.region().terrain();
    (0..t.height())
        .flat_map(|y| (0..t.width()).map(move |x| CellCoord { x, y }))
        .filter(|at| s.plant_site_check(PlantKind::Geothermal, *at).is_ok())
        .max_by(|a, b| {
            t.get(*a)
                .unwrap()
                .geothermal_potential
                .value()
                .total_cmp(&t.get(*b).unwrap().geothermal_potential.value())
        })
        .unwrap()
}

fn western_game(seed: u64) -> application::Game {
    use application::{Game, InMemoryCommandLog};
    use infrastructure::region::RonRegionCatalog;
    Game::start_new(
        &RonRegionCatalog::new(support::shipped_assets().join("regions")),
        &domain::ids::RegionId::new("western_interconnection"),
        seed,
        domain::session::GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap()
}

/// The quote the player orders from shows the two real instalment rates: the exploration share
/// over the exploration months, then the rest over the remaining construction months.
#[test]
fn geothermal_build_quote_shows_staged_exploration_then_construction_payments() {
    let l10n = support::shipped_l10n();
    let style = support::usd_style();
    let game = western_game(7);
    let cell = western_site(&game);
    let quote = game
        .session()
        .plant_quote(PlantKind::Geothermal, cell)
        .unwrap();
    let exploration = quote.exploration.unwrap();
    let expected = l10n.args(
        "geothermal-staged-payment",
        &[
            (
                "exploration",
                game::l10n::format::money(
                    domain::units::Money::from_cents(
                        exploration.capital_at_risk.cents() / i64::from(exploration.months),
                    ),
                    &style,
                )
                .into(),
            ),
            ("months", exploration.months.into()),
            (
                "construction",
                game::l10n::format::money(
                    domain::units::Money::from_cents(
                        (quote.capital - exploration.capital_at_risk).cents()
                            / i64::from(quote.months - exploration.months),
                    ),
                    &style,
                )
                .into(),
            ),
        ],
    );
    let model = hud::build_model(&game, &l10n, Selection::Cell(cell), &style).unwrap();
    let hud::PlantSection::Kinds { rows, .. } = model.plant else {
        panic!("kinds")
    };
    let row = rows
        .iter()
        .find(|r| r.kind == PlantKind::Geothermal)
        .unwrap();
    let payment = row
        .stats
        .iter()
        .find(|s| s.label == l10n.text("build-monthly-payment"))
        .unwrap();
    assert_eq!(payment.value, expected);
    assert!(l10n.problems().is_empty());
}

/// Planning offers the quote; while a round drills the button stays, disabled with the busy
/// reason; Operating shows only the field and progress lines, read-only.
#[test]
fn geothermal_card_is_disabled_while_drilling_and_read_only_in_operating() {
    let l10n = support::shipped_l10n();
    let style = support::usd_style();
    let (_dir, mut game, id) = operating_fixture();
    close_months(&mut game, 3);
    let card = hud::geothermal::geothermal_model(&game, &l10n, id, &style).unwrap();
    assert!(card.quote.is_some());
    assert!(card.action.as_ref().unwrap().enabled);
    game.execute(Command::DrillMakeupWells { plant: id })
        .unwrap();
    let card = hud::geothermal::geothermal_model(&game, &l10n, id, &style).unwrap();
    assert!(card.progress.is_some());
    assert!(!card.action.as_ref().unwrap().enabled);
    assert_eq!(card.reason, Some(l10n.text("geothermal-busy")));
    game.execute(Command::BeginOperating).unwrap();
    let card = hud::geothermal::geothermal_model(&game, &l10n, id, &style).unwrap();
    assert!(card.field.starts_with("Field:"));
    assert!(card.progress.is_some());
    assert_eq!((card.quote, card.action, card.reason), (None, None, None));
    assert!(l10n.problems().is_empty());
}

/// Once exploration succeeds the construction card stops quoting the exploration odds and risk.
#[test]
fn geothermal_construction_card_drops_exploration_figures_after_success() {
    let l10n = support::shipped_l10n();
    let style = support::usd_style();
    let (mut game, id) = (1..50)
        .find_map(|seed| {
            let mut game = western_game(seed);
            let cell = western_site(&game);
            game.execute(Command::BuildPlant {
                kind: PlantKind::Geothermal,
                cell,
            })
            .unwrap();
            let id = game
                .session()
                .plant_views()
                .into_iter()
                .find(|p| p.kind == PlantKind::Geothermal)
                .unwrap()
                .id;
            let card = hud::construction_card_model(&game, &l10n, id, &style).unwrap();
            assert!(
                card.stats
                    .iter()
                    .any(|s| s.label == l10n.text("geothermal-odds"))
            );
            close_months(&mut game, 3);
            game.session()
                .power()
                .plant(id)
                .is_some()
                .then_some((game, id))
        })
        .expect("a seed with a productive field");
    let card = hud::construction_card_model(&game, &l10n, id, &style).unwrap();
    for key in ["geothermal-odds", "geothermal-risk"] {
        assert!(
            !card.stats.iter().any(|s| s.label == l10n.text(key)),
            "{key} after success"
        );
    }
    close_months(&mut game, 0);
    assert!(l10n.problems().is_empty());
}

/// A game restored from before geothermal says why the row is disabled, without claiming the
/// region has no resource. (The Western Interconnection restore is pinned in infrastructure.)
#[test]
fn geothermal_build_row_in_a_pre_geothermal_game_gives_the_legacy_reason() {
    use infrastructure::save::{decode_save, replay_compatibility};
    let l10n = support::shipped_l10n();
    let style = support::usd_style();
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../infrastructure/tests/support/pre-geothermal-schema4-rules27/planning.mgsave"),
    )
    .unwrap();
    let (bundle, region) = decode_save(&bytes, &replay_compatibility()).unwrap();
    let game = bundle
        .restore(
            region,
            &replay_compatibility(),
            Box::new(application::InMemoryCommandLog::default()),
            &|| false,
        )
        .unwrap();
    assert!(!game.session().geothermal_available());
    let t = game.session().region().terrain();
    let at = CellCoord {
        x: t.width() / 2,
        y: t.height() / 2,
    };
    let model = hud::build_model(&game, &l10n, Selection::Cell(at), &style).unwrap();
    let hud::PlantSection::Kinds { rows, .. } = model.plant else {
        panic!("kinds")
    };
    let row = rows
        .iter()
        .find(|r| r.kind == PlantKind::Geothermal)
        .unwrap();
    assert!(!row.build.enabled);
    // The fixture region's own city keys are not shipped, so translation problems are expected.
    assert_eq!(row.blocked, Some(l10n.text("geothermal-legacy")));
    // The frozen stub's spec is a placeholder: no capacity, price or lead time is quoted from it.
    assert!(row.stats.is_empty(), "{:?}", row.stats);
}

#[test]
fn geothermal_counts_read_naturally_in_the_singular() {
    let l10n = support::shipped_l10n();
    let field = l10n.args(
        "geothermal-field",
        &[
            ("field", "90%".into()),
            ("decline", "3%".into()),
            ("rounds", 1.into()),
        ],
    );
    assert!(field.ends_with("1 makeup round"), "{field}");
    for key in ["geothermal-exploring", "geothermal-drilling"] {
        let text = l10n.args(
            key,
            &[
                ("months", 1.into()),
                ("odds", "80%".into()),
                ("capital", "$1".into()),
            ],
        );
        assert!(text.contains("1 month left"), "{key}: {text}");
    }
    let staged = l10n.args(
        "geothermal-staged-payment",
        &[
            ("exploration", "$1".into()),
            ("months", 1.into()),
            ("construction", "$2".into()),
        ],
    );
    assert!(staged.contains("for 1 month,"), "{staged}");
    let quote = l10n.args(
        "geothermal-makeup-quote",
        &[
            ("capital", "$1".into()),
            ("months", 1.into()),
            ("from", "1 MW".into()),
            ("to", "2 MW".into()),
        ],
    );
    assert!(quote.contains("over 1 month ·"), "{quote}");
    assert!(l10n.problems().is_empty());
}

/// Selecting a geothermal site shows the odds and expected output beside its potential; a cell
/// that is not a site shows neither.
#[test]
fn geothermal_cell_inspector_shows_site_odds_and_expected_output() {
    let l10n = support::shipped_l10n();
    let style = support::usd_style();
    let game = western_game(7);
    let site = western_site(&game);
    let (odds, field) = game.session().geothermal_site(site).unwrap();
    let readings = |at| match hud::inspector_model(&game, &l10n, Selection::Cell(at), &style) {
        Some(hud::InspectorModel::Cell(cell)) => cell.readings,
        other => panic!("cell inspector, got {other:?}"),
    };
    let value = |rows: &[hud::Stat], key: &str| {
        rows.iter()
            .find(|s| s.label == l10n.text(key))
            .map(|s| s.value.clone())
    };
    let rows = readings(site);
    assert_eq!(
        value(&rows, "geothermal-odds"),
        Some(game::l10n::format::percent(odds))
    );
    let nameplate = game.session().region().spec(PlantKind::Geothermal).capacity;
    assert_eq!(
        value(&rows, "geothermal-expected"),
        Some(game::l10n::format::megawatts(nameplate * field.value()))
    );
    let rows = readings(CellCoord { x: 0, y: 0 });
    assert_eq!(value(&rows, "geothermal-odds"), None);
    assert_eq!(value(&rows, "geothermal-expected"), None);
}

/// The widest geothermal Planning state (field line, makeup quote, enabled Drill button, and the
/// field warning row) is fully inside the minimum window, at both zoom levels.
#[test]
fn geothermal_planning_card_with_quote_and_warning_fit_at_the_minimum_window() {
    use bevy_egui::egui;
    let (dir, mut game, id) = operating_fixture();
    close_months(&mut game, 3);
    assert!(
        game.session()
            .power()
            .plant(id)
            .unwrap()
            .geothermal()
            .unwrap()
            .notice
            .is_some()
    );
    let l10n = support::shipped_l10n();
    let style = support::usd_style();
    let quote = hud::geothermal::geothermal_model(&game, &l10n, id, &style)
        .unwrap()
        .quote
        .expect("a worthwhile round");
    let mut app = support::hud_app(&dir.path().join("logs"));
    app.insert_resource(game::paths::SaveDir(dir.path().join("saves")));
    support::request(&mut app, "western_interconnection");
    app.insert_resource(Session(game));
    dispatch_in(&mut app, vec![UiAction::InspectPlant(id)]);
    support::update_n(&mut app, 2);
    let pass = EguiPass::attach(&mut app);
    let (width, height) = (
        game::window::MIN_WINDOW_SIZE.x,
        game::window::MIN_WINDOW_SIZE.y,
    );
    for zoom in [1.0, 1.25] {
        pass.ctx().set_zoom_factor(zoom);
        let widgets = pass.settled(&mut app, width / zoom, height / zoom);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height) / zoom);
        for (what, wanted) in [
            ("field line", "Field:".to_owned()),
            ("makeup quote", quote.clone()),
            ("drill button", l10n.text("geothermal-drill")),
            ("warning row", "Field output falling at".to_owned()),
        ] {
            let found: Vec<_> = widgets
                .iter()
                .filter(|w| w.text.contains(&wanted))
                .collect();
            assert!(!found.is_empty(), "{what} missing at zoom {zoom}");
            for w in found {
                assert!(
                    screen.contains_rect(w.rect),
                    "{what} at {:?} leaves the {width}x{height} window at zoom {zoom}",
                    w.rect
                );
            }
        }
    }
    pass.ctx().set_zoom_factor(1.0);
}
