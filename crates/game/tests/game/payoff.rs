//! Payoff comparison through the real construction panel and modal.
use crate::{
    hud_smoke::{dispatch_in, sent},
    support::{egui_harness::button, egui_pass::EguiPass, hud_app, request},
};
use bevy_egui::egui;
use domain::{
    ids::{CellCoord, RegionId},
    region::PlantKind,
    session::GameSettings,
};
use game::{
    hud::{OpenPrompt, Prompt, UiAction, UiState},
    sim::{PlayerCommand, Session, SimSpeed},
};
use tempfile::TempDir;

#[test]
fn payoff_includes_geothermal_where_the_region_has_resource_sites() {
    let l10n = crate::support::shipped_l10n();
    let catalog = infrastructure::region::RonRegionCatalog::new(
        crate::support::shipped_assets().join("regions"),
    );
    for &region in crate::support::SHIPPED_REGIONS {
        let game = application::Game::start_new(
            &catalog,
            &RegionId::new(region),
            7,
            GameSettings::default(),
            Box::new(application::InMemoryCommandLog::default()),
        )
        .unwrap();
        let style = game::l10n::format::MoneyStyle::resolve(
            game.session().region().currency(),
            &l10n,
            |_| true,
        );
        let model = game::hud::payoff_model(&game, &l10n, None, &style);
        let geothermal = model
            .rows
            .iter()
            .find(|row| row.kind == PlantKind::Geothermal);
        assert_eq!(
            geothermal.is_some(),
            matches!(
                region,
                "western_interconnection" | "continental_europe_maghreb"
            ),
            "{region}"
        );
        if let Some(row) = geothermal {
            assert_eq!(row.name, "Geothermal plant");
            assert!(!row.points.is_empty());
            assert_eq!(row.values[0], row.name);
        }
    }
}

#[test]
fn ccgt_cards_warn_when_current_full_utilization_income_is_negative_but_allow_building() {
    let l10n = crate::support::shipped_l10n();
    let catalog = infrastructure::region::RonRegionCatalog::new(
        crate::support::shipped_assets().join("regions"),
    );
    for (region, warning_expected) in [
        ("brazil_sin", true),
        ("southern_china", true),
        ("eastern_interconnection", false),
    ] {
        let game = application::Game::start_new(
            &catalog,
            &RegionId::new(region),
            7,
            GameSettings::default(),
            Box::new(application::InMemoryCommandLog::default()),
        )
        .unwrap();
        let terrain = game.session().region().terrain();
        let cell = (0..terrain.height())
            .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
            .find(|c| game.session().plant_quote(PlantKind::Ccgt, *c).is_ok())
            .unwrap();
        let style = game::l10n::format::MoneyStyle::resolve(
            game.session().region().currency(),
            &l10n,
            |_| true,
        );
        let mut model =
            game::hud::build_model(&game, &l10n, game::hud::Selection::Cell(cell), &style).unwrap();
        let game::hud::PlantSection::Kinds { rows, .. } = &mut model.plant else {
            panic!("selected cell");
        };
        rows.retain(|r| r.kind == PlantKind::Ccgt);
        let row = &rows[0];
        assert_eq!(row.warning.is_some(), warning_expected, "{region}");
        assert!(row.build.enabled);
        let cash = game::hud::payoff_model(&game, &l10n, Some(cell), &style)
            .rows
            .into_iter()
            .find(|r| r.kind == PlantKind::Ccgt)
            .unwrap()
            .cash;
        assert_eq!(cash.monthly_net.is_negative(), warning_expected);
        if let Some(warning) = row.warning.clone() {
            assert!(warning.contains("loses money even at full output"));
            let harness = crate::support::egui_harness::Harness::new();
            let widgets = harness.draw(|ui| game::hud::draw_build(ui, &model));
            assert!(widgets.iter().any(|w| w.text == warning));
            assert!(!button(&widgets, "Build plant").disabled);
        }
    }
}

fn app(logs: &TempDir) -> (bevy::prelude::App, EguiPass) {
    let mut app = hud_app(logs.path());
    {
        let mut progress = app
            .world_mut()
            .resource_mut::<game::progress::ClientProgressStore>();
        progress.loaded = true;
        progress.value.tutorial_pending = false;
    }
    request(&mut app, "western_interconnection");
    let pass = EguiPass::attach(&mut app);
    (app, pass)
}

#[test]
fn payoff_build_button_opens_read_only_comparison_and_close_or_escape_dismisses_it() {
    let logs = TempDir::new().unwrap();
    let (mut app, pass) = app(&logs);
    let before = app.world().resource::<Session>().0.session().state_hash();
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    let open = button(&widgets, "Compare plant payoff").rect;
    assert!(
        open.center().x > 700.0,
        "the button belongs in the right panel"
    );
    pass.click_at(&mut app, 1024.0, 640.0, open.center());
    assert!(app.world().resource::<UiState>().payoff_open);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(widgets.iter().any(|w| w.text == "Plant payoff comparison"));
    let geothermal = widgets
        .iter()
        .find(|w| w.text == "Geothermal plant" && w.role == egui::accesskit::Role::CheckBox)
        .expect("the geothermal curve appears in the chart key");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0));
    assert!(screen.contains_rect(geothermal.rect));
    let close = button(&widgets, "Close comparison").rect;
    pass.click_at(&mut app, 1024.0, 640.0, close.center());
    assert!(!app.world().resource::<UiState>().payoff_open);

    dispatch_in(&mut app, vec![UiAction::ShowPayoff(true)]);
    let size = (1024.0 / 1.5, 640.0 / 1.5);
    let widgets = pass.settled(&mut app, size.0, size.1);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0, size.1));
    assert!(screen.contains_rect(button(&widgets, "Close comparison").rect));
    let geothermal = widgets
        .iter()
        .find(|w| w.text == "Geothermal plant" && w.role == egui::accesskit::Role::CheckBox)
        .expect("the geothermal curve remains available at maximum UI zoom");
    assert!(screen.contains_rect(geothermal.rect));
    for pressed in [true, false] {
        pass.run_with(
            &mut app,
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
    assert!(!app.world().resource::<UiState>().payoff_open);
    assert_eq!(
        app.world().resource::<Session>().0.session().state_hash(),
        before
    );
    assert!(sent::<PlayerCommand>(&app).is_empty());
}

#[test]
fn payoff_waits_for_confirmation_and_pauses_playback() {
    let logs = TempDir::new().unwrap();
    let (mut app, pass) = app(&logs);
    dispatch_in(
        &mut app,
        vec![UiAction::SetSpeed(SimSpeed::X8), UiAction::ShowPayoff(true)],
    );
    assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::Paused);
    dispatch_in(&mut app, vec![UiAction::Prompt(Some(Prompt::LeaveGame))]);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(!widgets.iter().any(|w| w.text == "Plant payoff comparison"));
    for pressed in [true, false] {
        pass.run_with(
            &mut app,
            1024.0,
            640.0,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
    assert_eq!(app.world().resource::<OpenPrompt>().0, None);
    assert!(app.world().resource::<UiState>().payoff_open);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(widgets.iter().any(|w| w.text == "Plant payoff comparison"));
}
