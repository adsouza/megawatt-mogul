//! Wind through real construction widgets, dispatch, commissioning and world/HUD views.
use crate::hud_smoke::dispatch_in;
use crate::support::egui_harness::{Harness, buttons};
use crate::support::egui_pass::EguiPass;
use crate::support::{headless_app, request, shipped_assets, shipped_l10n, update_n};
use application::Command;
use domain::{ids::CellCoord, region::PlantKind};
use game::{
    hud::{self, Selection, UiAction},
    paths::SaveDir,
    sim::Session,
    world::WorldModel,
};
use tempfile::TempDir;

#[test]
fn wind_build_connect_commission_and_real_card_have_resource_forecast_without_thermal_controls() {
    let dir = TempDir::new().unwrap();
    let mut app = headless_app(&shipped_assets(), &dir.path().join("logs"));
    app.insert_resource(SaveDir(dir.path().join("saves")))
        .add_plugins((hud::HudPlugin, game::world::WorldViewPlugin));
    update_n(&mut app, 3);
    request(&mut app, "western_interconnection");
    let l10n = shipped_l10n();
    let (cell, anchor) = {
        let game = &app.world().resource::<Session>().0;
        let s = game.session();
        let home = s.power().network().node(s.anchor_node()).unwrap().cell;
        let grid = s.region().terrain();
        let cell = (0..grid.height())
            .flat_map(|y| (0..grid.width()).map(move |x| CellCoord { x, y }))
            .filter(|c| s.plant_site_check(PlantKind::Wind, *c).is_ok())
            .min_by_key(|c| c.x.abs_diff(home.x) + c.y.abs_diff(home.y))
            .unwrap();
        (cell, s.anchor_node())
    };
    let mut model = hud::build_model(
        &app.world().resource::<Session>().0,
        &l10n,
        Selection::Cell(cell),
    )
    .unwrap();
    if let hud::PlantSection::Kinds { rows, .. } = &mut model.plant {
        rows.retain(|row| row.kind == PlantKind::Wind);
        assert_eq!(rows.len(), 1);
        assert!(
            rows[0]
                .stats
                .iter()
                .any(|s| s.label == l10n.text("wind-potential"))
        );
    } else {
        panic!("selected build cell");
    }
    let harness = Harness::with_screen(
        game::window::MIN_WINDOW_SIZE.x,
        game::window::MIN_WINDOW_SIZE.y,
    );
    let actions = harness.click(&l10n.text("build-plant"), |ui| hud::draw_build(ui, &model));
    dispatch_in(&mut app, actions);
    update_n(&mut app, 2);
    let (id, node) = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .plant_views()
        .iter()
        .find(|p| p.kind == PlantKind::Wind)
        .map(|p| (p.id, p.node))
        .unwrap();
    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::BuildLine {
            from: anchor,
            to: node,
        })],
    );
    update_n(&mut app, 2);
    for month in 0..3 {
        dispatch_in(
            &mut app,
            vec![
                UiAction::Command(Command::BeginOperating),
                UiAction::Command(Command::AdvanceTicks { n: 1440 }),
            ],
        );
        update_n(&mut app, 2);
        if month < 2 {
            dispatch_in(&mut app, vec![UiAction::Command(Command::FinishReview)]);
            update_n(&mut app, 2);
        }
    }
    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::FinishReview),
            UiAction::Select(Selection::Plant(id)),
        ],
    );
    update_n(&mut app, 2);
    let game = &app.world().resource::<Session>().0;
    let view = game
        .session()
        .plant_views()
        .into_iter()
        .find(|p| p.id == id)
        .unwrap();
    assert!(view.connected && view.wind.is_some());
    let model = hud::forecast_model(game, &l10n, app.world().resource::<hud::UiState>());
    assert!(
        model
            .stacks
            .iter()
            .any(|s| s.kind == PlantKind::Wind && s.mw.iter().any(|mw| *mw > 0.0))
    );
    assert!(model.wind.is_some());
    let pass = EguiPass::attach(&mut app);
    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::BeginOperating),
            UiAction::Command(Command::AdvanceTicks { n: 1 }),
        ],
    );
    update_n(&mut app, 2);
    let widgets = pass.settled(
        &mut app,
        game::window::MIN_WINDOW_SIZE.x,
        game::window::MIN_WINDOW_SIZE.y,
    );
    for key in [
        "plant-start",
        "plant-stop",
        "battery-enable",
        "battery-disable",
    ] {
        assert!(buttons(&widgets, &l10n.text(key)).is_empty(), "{key}");
    }
    let before = app.world().resource::<Session>().0.session().state_hash();
    let model = WorldModel::of(&app.world().resource::<Session>().0);
    assert!(model.plants.contains_key(&id));
    assert_eq!(
        before,
        app.world().resource::<Session>().0.session().state_hash()
    );
}
