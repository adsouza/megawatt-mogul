//! Crystal-ball UI through actual widgets, including the chosen-city preview and modal keyboard.
use crate::{
    hud_smoke::{dispatch_in, sent},
    support::{
        egui_harness::button, egui_pass::EguiPass, hud_app, request, shipped_l10n, update_n,
    },
};
use bevy::prelude::*;
use bevy_egui::egui;
use game::{
    hud::{self, Selection, UiAction, UiState},
    sim::{PlayerCommand, Session, SimSpeed},
};
use tempfile::TempDir;

fn app(logs: &TempDir) -> (App, EguiPass) {
    let mut app = hud_app(logs.path());
    let mut progress = app
        .world_mut()
        .resource_mut::<game::progress::ClientProgressStore>();
    progress.loaded = true;
    progress.value.tutorial_pending = false;
    request(&mut app, "western_interconnection");
    let pass = EguiPass::attach(&mut app);
    (app, pass)
}
fn nearby_city(app: &App) -> domain::ids::CityId {
    let session = app.world().resource::<Session>().0.session();
    session
        .city_views()
        .into_iter()
        .filter(|c| !c.connected)
        .min_by(|a, b| {
            let distance = |cell| {
                session
                    .power()
                    .network()
                    .nodes()
                    .filter(|(id, _)| session.power().node_active(*id))
                    .map(|(_, node)| domain::grid::distance_cells(cell, node.cell))
                    .fold(f64::INFINITY, f64::min)
            };
            distance(a.cell).total_cmp(&distance(b.cell))
        })
        .unwrap()
        .id
}
fn escape(pass: &EguiPass, app: &mut App, size: (f32, f32)) {
    for pressed in [true, false] {
        pass.run_with(
            app,
            size.0,
            size.1,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
}
#[test]
fn outlook_icon_opens_selected_city_preview_and_close_leaves_simulation_unchanged() {
    let logs = TempDir::new().unwrap();
    let (mut app, pass) = app(&logs);
    let city = nearby_city(&app);
    dispatch_in(&mut app, vec![UiAction::Select(Selection::City(city))]);
    let before = app.world().resource::<Session>().0.session().state_hash();
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    let icon = button(&widgets, "Crystal ball: 12-month outlook").rect;
    assert!(icon.width() <= 40.0, "a compact icon");
    let milestones = button(&widgets, "Milestones").rect;
    let save = button(&widgets, "Save game").rect;
    let menu = button(&widgets, "Menu").rect;
    assert!(
        icon.right() < milestones.left(),
        "crystal ball belongs left of Milestones"
    );
    assert!(milestones.right() < save.left() && save.right() < menu.left());
    for rect in [icon, milestones, save] {
        assert!(
            (rect.center().y - menu.center().y).abs() < 0.5,
            "top-bar buttons align: {rect:?} / {menu:?}"
        );
    }

    pass.click_at(&mut app, 1024.0, 640.0, icon.center());
    assert!(app.world().resource::<UiState>().outlook_open);
    assert_eq!(app.world().resource::<UiState>().outlook_city, Some(city));
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(
        widgets
            .iter()
            .any(|w| w.text == "Crystal ball — the next 12 months")
    );
    let model = hud::outlook_model(
        &app.world().resource::<Session>().0,
        &shipped_l10n(),
        Some(city),
    )
    .unwrap();
    assert_eq!(model.rows.len(), 12);
    assert!(model.route.contains("to build, ready"), "{}", model.route);
    assert!(model.rows.iter().any(|row| row.peak > row.baseline_peak));
    let close = button(&widgets, "Close outlook").rect;
    pass.click_at(&mut app, 1024.0, 640.0, close.center());
    assert!(!app.world().resource::<UiState>().outlook_open);
    assert_eq!(
        app.world().resource::<Session>().0.session().state_hash(),
        before
    );
    assert!(sent::<PlayerCommand>(&app).is_empty());
}
#[test]
fn outlook_city_chooser_and_escape_respect_popup_precedence_at_maximum_zoom() {
    let logs = TempDir::new().unwrap();
    let (mut app, pass) = app(&logs);
    let city = nearby_city(&app);
    dispatch_in(
        &mut app,
        vec![
            UiAction::Select(Selection::City(city)),
            UiAction::ShowOutlook(true),
        ],
    );
    let size = (1024.0 / 1.5, 640.0 / 1.5);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0, size.1));
    let widgets = pass.settled(&mut app, size.0, size.1);
    assert!(screen.contains_rect(button(&widgets, "Close outlook").rect));
    let picker = widgets
        .iter()
        .find(|w| {
            w.role == egui::accesskit::Role::ComboBox
                && w.text
                    == game::l10n::Names::new(
                        &shipped_l10n(),
                        app.world().resource::<Session>().0.session(),
                    )
                    .city(city)
        })
        .unwrap()
        .rect;
    assert!(screen.contains_rect(picker));
    pass.click_at(&mut app, size.0, size.1, picker.center());
    assert!(egui::Popup::is_any_open(pass.ctx()), "city chooser opened");
    escape(&pass, &mut app, size);
    assert!(
        app.world().resource::<UiState>().outlook_open,
        "the popup receives Escape first"
    );
    let widgets = pass.settled(&mut app, size.0, size.1);
    let picker = widgets
        .iter()
        .find(|w| {
            w.role == egui::accesskit::Role::ComboBox
                && w.text
                    == game::l10n::Names::new(
                        &shipped_l10n(),
                        app.world().resource::<Session>().0.session(),
                    )
                    .city(city)
        })
        .unwrap()
        .rect;
    pass.click_at(&mut app, size.0, size.1, picker.center());
    let widgets = pass.settled(&mut app, size.0, size.1);
    let current = button(&widgets, "Current grid and scheduled orders").rect;
    pass.click_at(&mut app, size.0, size.1, current.center());
    assert_eq!(app.world().resource::<UiState>().outlook_city, None);
    pass.settled(&mut app, size.0, size.1);
    escape(&pass, &mut app, size);
    assert!(!app.world().resource::<UiState>().outlook_open);
}
#[test]
fn outlook_pauses_operating_and_survives_review_with_the_next_month_first() {
    let logs = TempDir::new().unwrap();
    let (mut app, pass) = app(&logs);
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(application::Command::BeginOperating)
        .unwrap();
    update_n(&mut app, 2);
    dispatch_in(
        &mut app,
        vec![
            UiAction::SetSpeed(SimSpeed::X1),
            UiAction::ShowOutlook(true),
        ],
    );
    assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::Paused);
    pass.settled(&mut app, 1024.0, 640.0);
    escape(&pass, &mut app, (1024.0, 640.0));
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(application::Command::AdvanceTicks {
            n: domain::time::TICKS_PER_DAY,
        })
        .unwrap();
    update_n(&mut app, 2);
    dispatch_in(&mut app, vec![UiAction::ShowOutlook(true)]);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(
        widgets
            .iter()
            .any(|w| w.text == "Crystal ball — the next 12 months")
    );
    let model =
        hud::outlook_model(&app.world().resource::<Session>().0, &shipped_l10n(), None).unwrap();
    assert_eq!(
        model.rows[0].month,
        game::l10n::format::month_year_short(
            app.world()
                .resource::<Session>()
                .0
                .session()
                .clock()
                .month()
                .next()
        )
    );
}

#[test]
fn outlook_forecasts_every_shipped_region_without_changing_state() {
    use application::{Game, InMemoryCommandLog};
    use domain::{ids::RegionId, session::GameSettings};
    use infrastructure::region::RonRegionCatalog;
    let catalog = RonRegionCatalog::new(crate::support::shipped_assets().join("regions"));
    for id in crate::support::SHIPPED_REGIONS {
        let game = Game::start_new(
            &catalog,
            &RegionId::new(id),
            7,
            GameSettings::default(),
            Box::new(InMemoryCommandLog::default()),
        )
        .unwrap();
        let hash = game.session().state_hash();
        let outlook = game.session().year_ahead_outlook(None).unwrap();
        assert_eq!(outlook.months.len(), 12, "{id}");
        let peak = (0..domain::time::TICKS_PER_DAY)
            .map(|tick| {
                game.session().power().connected_demand_forecast(
                    game.session().region(),
                    game.session().clock().month(),
                    tick,
                )
            })
            .fold(
                domain::units::Megawatts::ZERO,
                domain::units::Megawatts::max,
            );
        assert_eq!(outlook.months[0].baseline.peak_demand, peak, "{id}");
        assert_eq!(game.session().state_hash(), hash, "{id}");
    }
}

#[test]
fn outlook_preview_excludes_connected_and_scheduled_cities_and_clears_ineligible_selection() {
    let logs = TempDir::new().unwrap();
    let (mut app, pass) = app(&logs);
    let city = nearby_city(&app);
    let l10n = shipped_l10n();
    {
        let game = &app.world().resource::<Session>().0;
        let model = hud::outlook_model(game, &l10n, None).unwrap();
        for connected in game
            .session()
            .city_views()
            .iter()
            .filter(|city| city.connected)
        {
            assert!(
                !model.cities.iter().any(|(id, _)| *id == Some(connected.id)),
                "connected city is not a preview option"
            );
        }
        assert!(model.cities.iter().any(|(id, _)| *id == Some(city)));
    }
    let (from, to) = {
        let session = app.world().resource::<Session>().0.session();
        let preview = session.year_ahead_outlook(Some(city)).unwrap();
        (
            preview.connection.unwrap().from.unwrap(),
            session.power().cities()[&city].node(),
        )
    };
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(application::Command::BuildLine { from, to })
        .unwrap();
    update_n(&mut app, 2);
    let model =
        hud::outlook_model(&app.world().resource::<Session>().0, &l10n, Some(city)).unwrap();
    assert!(
        !model.cities.iter().any(|(id, _)| *id == Some(city)),
        "ordered city is already included in the baseline"
    );
    assert_eq!(model.city, None);
    assert!(model.rows.iter().all(|row| row.peak == row.baseline_peak));
    dispatch_in(
        &mut app,
        vec![
            UiAction::Select(Selection::City(city)),
            UiAction::ShowOutlook(true),
        ],
    );
    assert_eq!(app.world().resource::<UiState>().outlook_city, None);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(
        widgets
            .iter()
            .any(|w| w.role == egui::accesskit::Role::ComboBox
                && w.text == "Current grid and scheduled orders")
    );
}
