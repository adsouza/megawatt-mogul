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
