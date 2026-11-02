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
    progress.value.outlook_tutorial_pending = false;
    request(&mut app, "western_interconnection");
    let pass = EguiPass::attach(&mut app);
    (app, pass)
}
fn settled(
    pass: &EguiPass,
    app: &mut App,
    width: f32,
    height: f32,
) -> Vec<crate::support::egui_harness::Widget> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let widgets = pass.settled(app, width, height);
        if !widgets
            .iter()
            .any(|w| w.text == "Calculating monthly supply forecasts…")
        {
            pass.settled(app, width, height);
            return pass.settled(app, width, height);
        }
        assert!(
            std::time::Instant::now() < deadline,
            "outlook worker did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
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
    let widgets = settled(&pass, &mut app, 1024.0, 640.0);
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
    let widgets = settled(&pass, &mut app, 1024.0, 640.0);
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
fn outlook_month_opens_daily_curves_and_escape_returns_then_closes() {
    let logs = TempDir::new().unwrap();
    let (mut app, pass) = app(&logs);
    let before = app.world().resource::<Session>().0.session().state_hash();
    let month = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .clock()
        .month();
    dispatch_in(&mut app, vec![UiAction::ShowOutlook(true)]);
    let widgets = settled(&pass, &mut app, 1200.0, 900.0);
    let label = game::l10n::format::month_year_short(month);
    pass.click_at(
        &mut app,
        1200.0,
        900.0,
        button(&widgets, &label).rect.center(),
    );
    assert_eq!(app.world().resource::<UiState>().outlook_month, Some(month));
    let widgets = settled(&pass, &mut app, 1200.0, 900.0);
    assert!(widgets.iter().any(|w| w.text == "Available net supply"));
    assert!(
        widgets
            .iter()
            .any(|w| w.text.starts_with("Daily outlook —"))
    );
    assert!(widgets.iter().any(|w| w.text == "Back to 12 months"));
    escape(&pass, &mut app, (1200.0, 900.0));
    assert!(app.world().resource::<UiState>().outlook_open);
    assert_eq!(app.world().resource::<UiState>().outlook_month, None);
    escape(&pass, &mut app, (1200.0, 900.0));
    assert!(!app.world().resource::<UiState>().outlook_open);
    assert_eq!(
        app.world().resource::<Session>().0.session().state_hash(),
        before
    );
    assert!(sent::<PlayerCommand>(&app).is_empty());
}

#[test]
fn outlook_loading_is_visible_and_can_be_closed_with_escape() {
    let logs = TempDir::new().unwrap();
    let (mut app, pass) = app(&logs);
    let before = app.world().resource::<Session>().0.session().state_hash();
    dispatch_in(&mut app, vec![UiAction::ShowOutlook(true)]);
    let widgets = pass.run(&mut app, 1024.0, 640.0);
    assert!(
        widgets
            .iter()
            .any(|w| w.text == "Calculating monthly supply forecasts…")
    );
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(!button(&widgets, "Close outlook").disabled);
    escape(&pass, &mut app, (1024.0, 640.0));
    assert!(!app.world().resource::<UiState>().outlook_open);
    assert_eq!(
        app.world().resource::<Session>().0.session().state_hash(),
        before
    );
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
    let widgets = settled(&pass, &mut app, size.0, size.1);
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
    let widgets = settled(&pass, &mut app, size.0, size.1);
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
    let widgets = settled(&pass, &mut app, size.0, size.1);
    let current = button(&widgets, "Current grid and scheduled orders").rect;
    pass.click_at(&mut app, size.0, size.1, current.center());
    assert_eq!(app.world().resource::<UiState>().outlook_city, None);
    settled(&pass, &mut app, size.0, size.1);
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
    settled(&pass, &mut app, 1024.0, 640.0);
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
    let widgets = settled(&pass, &mut app, 1024.0, 640.0);
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
        let operational = game.session().operational_outlook(None).unwrap();
        assert_eq!(
            operational.months[0].baseline.forecast,
            game.session().supply_forecast(),
            "{id}"
        );
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
    let widgets = settled(&pass, &mut app, 1024.0, 640.0);
    assert!(
        widgets
            .iter()
            .any(|w| w.role == egui::accesskit::Role::ComboBox
                && w.text == "Current grid and scheduled orders")
    );
}

const CITY_GUIDANCE: &str = "Choose a new city here to preview its demand before you connect it.";
const CUSHION_GUIDANCE: &str = "The graph checks supply throughout each month's representative day. Green means no expected shortage; bright red means a local grid falls short. Click a month to inspect generation, battery charging and demand before expanding.";

#[test]
fn first_outlook_use_guides_the_city_chooser() {
    use game::hud::tutorial::OutlookGuideStep;
    use game::progress::ClientProgressStore;
    let logs = TempDir::new().unwrap();
    let (mut app, pass) = app(&logs);
    app.world_mut()
        .resource_mut::<ClientProgressStore>()
        .value
        .outlook_tutorial_pending = true;
    let model =
        hud::outlook_model(&app.world().resource::<Session>().0, &shipped_l10n(), None).unwrap();
    let (city, city_name) = model.cities[1].clone();
    dispatch_in(
        &mut app,
        vec![
            UiAction::Select(Selection::City(city.unwrap())),
            UiAction::ShowOutlook(true),
        ],
    );
    let hash = app.world().resource::<Session>().0.session().state_hash();
    let widgets = settled(&pass, &mut app, 1024.0, 640.0);
    assert!(widgets.iter().any(|w| w.text == CITY_GUIDANCE));
    assert_eq!(
        app.world().resource::<UiState>().outlook_guide,
        Some(OutlookGuideStep::CitySpotlight)
    );
    escape(&pass, &mut app, (1024.0, 640.0));
    assert!(app.world().resource::<UiState>().outlook_open);
    assert_eq!(
        app.world().resource::<UiState>().outlook_guide,
        Some(OutlookGuideStep::ChooseCity)
    );
    let widgets = settled(&pass, &mut app, 1024.0, 640.0);
    assert!(widgets.iter().any(|w| w.text == CITY_GUIDANCE));
    let picker = widgets
        .iter()
        .find(|w| w.role == egui::accesskit::Role::ComboBox && w.text == city_name)
        .unwrap()
        .rect;
    pass.click_at(&mut app, 1024.0, 640.0, picker.center());
    let widgets = settled(&pass, &mut app, 1024.0, 640.0);
    assert!(
        !widgets.iter().any(|w| w.text == CITY_GUIDANCE),
        "the instruction steps aside for the popup"
    );
    pass.click_at(
        &mut app,
        1024.0,
        640.0,
        button(&widgets, &city_name).rect.center(),
    );
    assert_eq!(
        app.world().resource::<UiState>().outlook_guide,
        Some(OutlookGuideStep::Cushion)
    );
    for _ in 0..30 {
        pass.run(&mut app, 1024.0, 640.0);
    }
    let widgets = settled(&pass, &mut app, 1024.0, 640.0);
    assert!(widgets.iter().any(|w| w.text == CUSHION_GUIDANCE));
    assert!(
        app.world()
            .resource::<ClientProgressStore>()
            .value
            .outlook_tutorial_pending
    );
    escape(&pass, &mut app, (1024.0, 640.0));
    assert!(
        !app.world()
            .resource::<ClientProgressStore>()
            .value
            .outlook_tutorial_pending
    );
    assert!(
        app.world().resource::<UiState>().outlook_open,
        "dismissing guidance does not close the preview"
    );
    assert_eq!(app.world().resource::<UiState>().outlook_guide, None);
    let saved = app.world().resource::<ClientProgressStore>().value.clone();
    assert!(
        !infrastructure::client_progress::decode_progress(
            &infrastructure::client_progress::encode_progress(&saved).unwrap()
        )
        .unwrap()
        .outlook_tutorial_pending
    );
    settled(&pass, &mut app, 1024.0, 640.0);
    escape(&pass, &mut app, (1024.0, 640.0));
    dispatch_in(&mut app, vec![UiAction::ShowOutlook(true)]);
    let widgets = settled(&pass, &mut app, 1024.0, 640.0);
    assert_eq!(app.world().resource::<UiState>().outlook_guide, None);
    assert!(
        !widgets
            .iter()
            .any(|w| w.text == CITY_GUIDANCE || w.text == CUSHION_GUIDANCE)
    );
    assert_eq!(
        app.world().resource::<Session>().0.session().state_hash(),
        hash
    );
    assert!(sent::<PlayerCommand>(&app).is_empty());
}

#[test]
fn closing_the_first_use_guide_keeps_it_pending_and_small_window_guidance_fits() {
    use game::progress::ClientProgressStore;
    let logs = TempDir::new().unwrap();
    let (mut app, pass) = app(&logs);
    app.world_mut()
        .resource_mut::<ClientProgressStore>()
        .value
        .outlook_tutorial_pending = true;
    dispatch_in(&mut app, vec![UiAction::ShowOutlook(true)]);
    let size = (1024.0 / 1.5, 640.0 / 1.5);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0, size.1));
    let widgets = settled(&pass, &mut app, size.0, size.1);
    assert!(
        widgets
            .iter()
            .filter(|w| w.text == CITY_GUIDANCE)
            .all(|w| screen.contains_rect(w.rect))
    );
    escape(&pass, &mut app, size);
    settled(&pass, &mut app, size.0, size.1);
    escape(&pass, &mut app, size);
    assert!(!app.world().resource::<UiState>().outlook_open);
    assert!(
        app.world()
            .resource::<ClientProgressStore>()
            .value
            .outlook_tutorial_pending
    );
    dispatch_in(&mut app, vec![UiAction::ShowOutlook(true)]);
    settled(&pass, &mut app, size.0, size.1);
    escape(&pass, &mut app, size);
    let city = hud::outlook_model(&app.world().resource::<Session>().0, &shipped_l10n(), None)
        .unwrap()
        .cities[1]
        .0;
    dispatch_in(&mut app, vec![UiAction::PreviewOutlookCity(city)]);
    for _ in 0..30 {
        pass.run(&mut app, size.0, size.1);
    }
    let widgets = settled(&pass, &mut app, size.0, size.1);
    let notes: Vec<_> = widgets
        .iter()
        .filter(|w| w.text == CUSHION_GUIDANCE)
        .collect();
    assert!(!notes.is_empty());
    assert!(notes.iter().all(|w| screen.contains_rect(w.rect)));
}

#[test]
fn first_use_spotlights_expire_but_city_selection_still_requires_a_real_choice() {
    use game::hud::tutorial::OutlookGuideStep;
    use game::progress::ClientProgressStore;
    let logs = TempDir::new().unwrap();
    let (mut app, pass) = app(&logs);
    app.world_mut()
        .resource_mut::<ClientProgressStore>()
        .value
        .outlook_tutorial_pending = true;
    dispatch_in(&mut app, vec![UiAction::ShowOutlook(true)]);
    settled(&pass, &mut app, 1024.0, 640.0);
    for _ in 0..12 {
        crate::tutorial::advance_real(&mut app, 1.0);
        pass.run(&mut app, 1024.0, 640.0);
    }
    assert_eq!(
        app.world().resource::<UiState>().outlook_guide,
        Some(OutlookGuideStep::ChooseCity)
    );
    dispatch_in(&mut app, vec![UiAction::PreviewOutlookCity(None)]);
    assert_eq!(
        app.world().resource::<UiState>().outlook_guide,
        Some(OutlookGuideStep::ChooseCity)
    );
    let city = hud::outlook_model(&app.world().resource::<Session>().0, &shipped_l10n(), None)
        .unwrap()
        .cities[1]
        .0;
    dispatch_in(&mut app, vec![UiAction::PreviewOutlookCity(city)]);
    settled(&pass, &mut app, 1024.0, 640.0);
    for _ in 0..12 {
        crate::tutorial::advance_real(&mut app, 1.0);
        pass.run(&mut app, 1024.0, 640.0);
    }
    assert_eq!(app.world().resource::<UiState>().outlook_guide, None);
    assert!(
        !app.world()
            .resource::<ClientProgressStore>()
            .value
            .outlook_tutorial_pending
    );
}

#[test]
fn first_use_with_no_new_cities_highlights_the_existing_capacity_cushion() {
    use game::hud::tutorial::OutlookGuideStep;
    use game::progress::ClientProgressStore;
    let logs = TempDir::new().unwrap();
    let (mut app, pass) = app(&logs);
    {
        let mut session = app.world_mut().resource_mut::<Session>();
        let from = session.0.session().anchor_node();
        let cities = session.0.session().city_views();
        for city in cities {
            if session
                .0
                .session()
                .city_connection_months(city.id)
                .is_none()
            {
                session
                    .0
                    .execute(application::Command::BuildLine {
                        from,
                        to: city.node,
                    })
                    .unwrap();
            }
        }
    }
    app.world_mut()
        .resource_mut::<ClientProgressStore>()
        .value
        .outlook_tutorial_pending = true;
    dispatch_in(&mut app, vec![UiAction::ShowOutlook(true)]);
    assert_eq!(
        app.world().resource::<UiState>().outlook_guide,
        Some(OutlookGuideStep::Cushion)
    );
    for _ in 0..30 {
        pass.run(&mut app, 1024.0, 640.0);
    }
    let widgets = settled(&pass, &mut app, 1024.0, 640.0);
    assert!(widgets.iter().any(|w| w.text == CUSHION_GUIDANCE));
    escape(&pass, &mut app, (1024.0, 640.0));
    assert!(
        !app.world()
            .resource::<ClientProgressStore>()
            .value
            .outlook_tutorial_pending
    );
}
