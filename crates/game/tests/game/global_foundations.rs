//! Schema-4 presentation through the real HUD and world reconciliation.
use crate::support::egui_pass::EguiPass;
use crate::support::{hud_app, request, shipped_assets, update_n};
use application::{Command, Game, InMemoryCommandLog};
use bevy::prelude::*;
use bevy_egui::egui;
use domain::{
    ids::{CityId, RegionId},
    session::GameSettings,
};
use game::{
    hud::{Selection, UiAction},
    l10n::Localizer,
    sim::Session,
};
use infrastructure::region::RonRegionCatalog;

fn strait_game() -> Game {
    let regions = tempfile::TempDir::new().unwrap();
    std::fs::write(
        regions.path().join("minimal.ron"),
        include_str!("../fixtures/strait-v4.ron"),
    )
    .unwrap();
    Game::start_new(
        &RonRegionCatalog::new(regions.path().to_owned()),
        &RegionId::new("minimal"),
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap()
}
fn strait_l10n() -> Localizer {
    let mut paths: Vec<_> = std::fs::read_dir(shipped_assets().join("locales/en-US"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "ftl"))
        .collect();
    paths.sort();
    let mut sources: Vec<_> = paths
        .into_iter()
        .map(|path| {
            (
                path.display().to_string(),
                std::fs::read_to_string(path).unwrap(),
            )
        })
        .collect();
    sources.push(("strait.ftl".into(),"line-strait = Strait of Gibraltar\ncity-tiny-1 = West shore\ncity-tiny-2 = East shore\ncity-tiny-3 = Inland\n".into()));
    Localizer::from_sources(sources).unwrap()
}
fn app(logs: &std::path::Path, world: bool) -> App {
    let mut app = hud_app(logs);
    if world {
        app.add_plugins(game::world::WorldViewPlugin);
    }
    request(&mut app, "eastern_interconnection");
    app.world_mut().insert_resource(Session(strait_game()));
    app.world_mut().insert_resource(strait_l10n());
    update_n(&mut app, 2);
    app
}
fn painted(shape: &egui::Shape, label: &str) -> bool {
    match shape {
        egui::Shape::Text(text) => text.galley.text() == label,
        egui::Shape::Vec(shapes) => shapes.iter().any(|shape| painted(shape, label)),
        _ => false,
    }
}

#[test]
fn schema4_clocks_calendar_and_city_card_fit_the_minimum_window_and_zoom() {
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = app(logs.path(), false);
    let pass = EguiPass::attach(&mut app);
    crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::Select(Selection::City(CityId(2)))]);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    for text in [
        "00:00 MSK",
        "Autumn",
        "System time (MSK)",
        "Local time: 02:30 IST",
    ] {
        assert!(widgets.iter().any(|w| w.text == text), "missing {text}");
    }
    crate::hud_smoke::dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::BeginOperating),
            UiAction::Command(Command::AdvanceTicks { n: 860 }),
        ],
    );
    update_n(&mut app, 2);
    for zoom in [1.0, 1.25] {
        pass.ctx().set_zoom_factor(zoom);
        let widgets = pass.settled(&mut app, 1024.0 / zoom, 640.0 / zoom);
        assert_eq!(
            widgets
                .iter()
                .filter(|w| w.text == "14:20 MSK" && w.role == egui::accesskit::Role::Label)
                .count(),
            1,
            "one system clock at zoom {zoom}"
        );
        let details = crate::operating_side::region(&widgets, "East shore");
        let local = crate::operating_side::label(&widgets, "Local time: 16:50 IST");
        assert!(
            details.contains_rect(local),
            "local time fits fixed card at zoom {zoom}"
        );
        let list = crate::operating_side::region(&widgets, "Plants");
        crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::Select(Selection::None)]);
        let cleared = pass.settled(&mut app, 1024.0 / zoom, 640.0 / zoom);
        assert_eq!(
            crate::operating_side::region(&cleared, "Plants"),
            list,
            "selection keeps list in place"
        );
        crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::Select(Selection::City(CityId(2)))]);
    }
    assert!(app.world().resource::<Localizer>().problems().is_empty());
}

#[test]
fn the_gibraltar_style_starter_line_paints_its_name_and_reuses_cable_meshes_and_pulses() {
    use bevy::mesh::VertexAttributeValues;
    use game::world::{FlowPulse, ViewSync, WorldAction, WorldAssets, WorldScene};
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = app(logs.path(), true);
    let pass = EguiPass::attach(&mut app);
    let named = app
        .world()
        .resource::<WorldScene>()
        .model
        .as_ref()
        .unwrap()
        .lines
        .values()
        .find(|line| line.name_key.as_deref() == Some("line-strait"))
        .unwrap()
        .id;
    pass.settled(&mut app, 1280.0, 800.0);
    let output = pass.paint_with(&mut app, 1280.0, 800.0, Vec::new());
    assert!(
        output
            .shapes
            .iter()
            .any(|shape| painted(&shape.shape, "Strait of Gibraltar")),
        "world map names the starter cable"
    );
    output.drop_without_applying_deltas();
    // The retained flat-map adapter consumes the same name and water segmentation.
    let game = &app.world().resource::<Session>().0;
    let model = game::hud::map_model(
        game,
        app.world().resource::<Localizer>(),
        game::hud::MapLayer::Irradiance,
        game::hud::LegendChoice(Some(false)),
        Selection::None,
    );
    let ctx = egui::Context::default();
    let mut cache = game::hud::MapCache::default();
    let flat = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                game::hud::draw_map(ui, &model, &mut cache);
            });
        },
    );
    assert!(
        flat.shapes
            .iter()
            .any(|shape| painted(&shape.shape, "Strait of Gibraltar")),
        "flat map names the starter cable"
    );
    assert!(flat.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::LineSegment{stroke,..} if stroke.color==egui::Color32::from_rgb(75,100,125))),"flat map draws dark water dashes");
    flat.drop_without_applying_deltas();
    crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::Select(Selection::Line(named))]);
    let card = pass.settled(&mut app, 1024.0, 640.0);
    for text in [
        "Strait of Gibraltar",
        "Submarine cable · 3 cells",
        "Transfer limits are not simulated (copper plate)",
    ] {
        assert!(
            card.iter().any(|widget| widget.text == text),
            "line card paints {text}"
        );
    }
    let entity = app.world().resource::<ViewSync>().lines[&named];
    let handle = app.world().get::<Mesh3d>(entity).unwrap().0.clone();
    let mesh = app.world().resource::<Assets<Mesh>>().get(&handle).unwrap();
    let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        panic!("line colors");
    };
    assert!(
        colors.iter().any(|color| color[0] < 0.5),
        "cable has dark water segments"
    );
    let cable_paths: std::collections::BTreeSet<_> = app
        .world()
        .resource::<WorldScene>()
        .model
        .as_ref()
        .unwrap()
        .flow_paths
        .iter()
        .filter(|(_, path)| path.line == named)
        .map(|(key, _)| *key)
        .collect();
    assert!(!cable_paths.is_empty());
    let pulses: Vec<_> = app
        .world_mut()
        .query::<(Entity, &FlowPulse)>()
        .iter(app.world())
        .filter(|(_, pulse)| cable_paths.contains(&pulse.path))
        .map(|(entity, _)| entity)
        .collect();
    assert!(!pulses.is_empty());
    crate::hud_smoke::dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::BeginOperating),
            UiAction::Command(Command::AdvanceTicks { n: 60 }),
            UiAction::World(WorldAction::ControlRoom),
        ],
    );
    app.update();
    assert!(
        pulses
            .iter()
            .any(|entity| app.world().get::<Visibility>(*entity) == Some(&Visibility::Visible)),
        "energized cable retains moving-flow indicators"
    );
    let material = app
        .world()
        .get::<MeshMaterial3d<StandardMaterial>>(entity)
        .unwrap();
    let cable_material = app
        .world()
        .resource::<Assets<StandardMaterial>>()
        .get(&material.0)
        .unwrap();
    assert_eq!(
        cable_material.emissive,
        bevy::color::LinearRgba::BLACK,
        "hologram emission must not wash out the darker submarine segments; pulses indicate flow"
    );
    let count = app.world().resource::<WorldAssets>().counts();
    let checkpoint = app.world().resource::<Session>().0.checkpoint();
    for zoom in [0.0, 100.0, -100.0] {
        crate::hud_smoke::dispatch_in(&mut app, vec![UiAction::World(WorldAction::Zoom(zoom))]);
        pass.settled(&mut app, 1024.0, 640.0);
        update_n(&mut app, 5);
        assert_eq!(app.world().resource::<ViewSync>().lines[&named], entity);
        assert_eq!(app.world().get::<Mesh3d>(entity).unwrap().0, handle);
        assert_eq!(app.world().resource::<WorldAssets>().counts(), count);
        assert!(
            pulses
                .iter()
                .all(|entity| app.world().get::<FlowPulse>(*entity).is_some())
        );
        assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
    }
}
