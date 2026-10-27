//! Real SGCC HUD actions and keyboard cancellation, within the existing game binary.
use super::support::{
    egui_harness::{Harness, central},
    shipped_assets, shipped_l10n, usd_style,
};
use application::{Command, Game, InMemoryCommandLog};
use bevy_egui::egui;
use domain::{ids::RegionId, session::GameSettings};
use game::hud::{
    LinePreview, LineSection, OpenPrompt, Prompt, Selection, UiAction, build_model, draw_links,
    draw_prompt, links_model, prompt_model,
};
use infrastructure::region::RonRegionCatalog;
fn sgcc() -> Game {
    Game::start_new(
        &RonRegionCatalog::new(shipped_assets().join("regions")),
        &RegionId::new("sgcc"),
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap()
}

#[test]
fn island_forecast_buttons_and_warnings_navigate_the_real_hud_at_minimum_size() {
    use super::{
        hud_smoke::dispatch_in,
        support::{
            egui_harness::{button, has},
            egui_pass::EguiPass,
            hud_app, request,
        },
    };
    use game::{
        hud::{self, PlanningTab, UiState},
        sim::Session,
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, "sgcc");
    let from = app.world().resource::<Session>().0.session().anchor_node();
    let to = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .city_views()
        .into_iter()
        .find(|c| c.name_key == "city-shanghai")
        .unwrap()
        .node;
    {
        let mut session = app.world_mut().resource_mut::<Session>();
        let months = session.0.session().link_quote(from, to).unwrap().months;
        session.0.execute(Command::BuildLink { from, to }).unwrap();
        for _ in 0..months {
            session.0.execute(Command::BeginOperating).unwrap();
            session
                .0
                .execute(Command::AdvanceTicks { n: 1440 })
                .unwrap();
            session.0.execute(Command::FinishReview).unwrap();
        }
    }
    let pass = EguiPass::attach(&mut app);
    let l10n = shipped_l10n();
    pass.ctx().set_zoom_factor(1.25);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    let inspect = button(&widgets, &l10n.text("forecast-inspect-island"));
    pass.click_at(&mut app, 1024.0, 640.0, inspect.rect.center());
    assert_eq!(app.world().resource::<UiState>().island, Some(to));
    assert_eq!(app.world().resource::<UiState>().tab, PlanningTab::Forecast);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(has(&widgets, &l10n.text("forecast-minimum")));
    let chart = hud::forecast_model(
        &app.world().resource::<Session>().0,
        &l10n,
        app.world().resource::<UiState>(),
    );
    assert!(chart.scope_label.as_ref().unwrap().contains("Shanghai"));
    assert!(chart.service_note.is_some());
    assert!(chart.demand_minutes.iter().all(|p| p[1] == 0.0));
    let city = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .city_views()
        .into_iter()
        .find(|c| c.node == to)
        .unwrap();
    let card = hud::city_card_model(&app.world().resource::<Session>().0, &l10n, city.id).unwrap();
    assert_eq!(card.connection, l10n.text("city-inactive"));
    assert!(!card.cutting_load);
    assert!(
        chart
            .link_issues
            .iter()
            .any(|s| s.contains("Shanghai") && s.contains("synchronous"))
    );
    assert!(
        !chart
            .link_issues
            .iter()
            .any(|s| s.contains("no synchronous generator committed in North"))
    );
    dispatch_in(&mut app, vec![UiAction::SelectIsland(None)]);
    let chart = hud::forecast_model(
        &app.world().resource::<Session>().0,
        &l10n,
        app.world().resource::<UiState>(),
    );
    let label = &chart
        .islands
        .iter()
        .find(|i| i.island == to)
        .unwrap()
        .inspect
        .label;
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    let view = button(&widgets, label);
    assert!(view.rect.bottom() < 640.0 / 1.25 && view.rect.right() < 1024.0 / 1.25);
    pass.click_at(&mut app, 1024.0, 640.0, view.rect.center());
    assert_eq!(app.world().resource::<UiState>().island, Some(to));
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    pass.click_at(
        &mut app,
        1024.0,
        640.0,
        super::support::egui_harness::buttons(&widgets, &l10n.text("hvdc-title"))
            .into_iter()
            .max_by(|a, b| a.rect.top().total_cmp(&b.rect.top()))
            .unwrap()
            .rect
            .center(),
    );
    assert!(app.world().resource::<UiState>().links_open);
    assert!(l10n.problems().is_empty());
}
#[test]
fn hvdc_cross_area_picker_quotes_a_link_and_card_reports_its_original_terms() {
    let mut game = sgcc();
    let l10n = shipped_l10n();
    let style = usd_style();
    let from = game.session().anchor_node();
    let to = game
        .session()
        .city_views()
        .into_iter()
        .find(|c| c.name_key == "city-shanghai")
        .unwrap()
        .node;
    let model = build_model(&game, &l10n, Selection::LineFrom(from), &style).unwrap();
    let LineSection::End { targets, .. } = model.line else {
        panic!("endpoint picker")
    };
    let target = targets.iter().find(|t| t.node == to).unwrap();
    assert!(matches!(target.preview, LinePreview::Quote(_)));
    assert_eq!(
        target.build.action,
        UiAction::Command(Command::BuildLink { from, to })
    );
    game.execute(Command::BuildLink { from, to }).unwrap();
    let model = links_model(&game, &l10n, &style).unwrap();
    assert_eq!(model.cards.len(), 1);
    assert!(model.cards[0].planning);
    assert_eq!(model.cards[0].schedule, [0.0; 24]);
    assert!(model.cards[0].cancel.is_some());
    assert!(l10n.problems().is_empty());
    let harness = Harness::with_screen(1024.0, 640.0);
    let draw = |ui: &mut egui::Ui| central(ui, |ui| draw_links(ui, &model));
    harness.draw(draw);
    assert_eq!(
        harness.press_key(egui::Key::Escape, draw),
        vec![UiAction::ShowLinks(false)]
    );
}
#[test]
fn hvdc_cancellation_defaults_to_keep_and_escape_cancels_the_dialog() {
    let mut game = sgcc();
    let from = game.session().anchor_node();
    let to = game
        .session()
        .city_views()
        .into_iter()
        .find(|c| c.name_key == "city-shanghai")
        .unwrap()
        .node;
    game.execute(Command::BuildLink { from, to }).unwrap();
    let link = game.session().power().network().links().next().unwrap().id;
    let model = prompt_model(
        OpenPrompt(Some(Prompt::CancelLinkConstruction { link })),
        Some(&game),
        &shipped_l10n(),
    )
    .unwrap();
    let draw = |ui: &mut egui::Ui| {
        egui::Modal::new(egui::Id::new("hvdc_cancel"))
            .show(ui.ctx(), |ui| draw_prompt(ui, &model))
            .inner
    };
    let harness = Harness::with_screen(1024.0, 640.0);
    harness.draw(draw);
    assert_eq!(
        harness.press_key(egui::Key::Enter, draw),
        vec![UiAction::Prompt(None)]
    );
    assert_eq!(
        harness.press_key(egui::Key::Escape, draw),
        vec![UiAction::Prompt(None)]
    );
}

#[test]
fn sgcc_real_hud_scopes_frequency_and_keeps_selection_when_operating_begins() {
    use super::{
        hud_smoke::dispatch_in,
        support::{egui_harness::has, egui_pass::EguiPass, hud_app, request, update_n},
    };
    use game::{hud::UiState, sim::Session};
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, "sgcc");
    let pass = EguiPass::attach(&mut app);
    let anchor = app.world().resource::<Session>().0.session().anchor_node();
    dispatch_in(
        &mut app,
        vec![
            UiAction::SelectIsland(Some(anchor)),
            UiAction::ShowLinks(true),
            UiAction::Command(Command::BeginOperating),
        ],
    );
    update_n(&mut app, 3);
    assert_eq!(app.world().resource::<UiState>().island, Some(anchor));
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(has(&widgets, &shipped_l10n().text("frequency-title")));
    dispatch_in(
        &mut app,
        vec![UiAction::SelectIsland(None), UiAction::ShowLinks(false)],
    );
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(!has(&widgets, &shipped_l10n().text("frequency-title")));
}

#[test]
fn connection_warning_cancel_is_safe_and_proceed_orders_exactly_once_for_ac_and_dc() {
    use super::{
        hud_smoke::dispatch_in,
        support::{hud_app, request, update_n},
    };
    use domain::session::ConnectionKind;
    use game::sim::Session;
    for (kind, key) in [
        (ConnectionKind::Ac, "city-shijiazhuang"),
        (ConnectionKind::Dc, "city-shanghai"),
    ] {
        let logs = tempfile::TempDir::new().unwrap();
        let mut app = hud_app(logs.path());
        request(&mut app, "sgcc");
        let pass = super::support::egui_pass::EguiPass::attach(&mut app);
        let s = &app.world().resource::<Session>().0;
        let from = s.session().anchor_node();
        let to = s
            .session()
            .city_views()
            .into_iter()
            .find(|c| c.name_key == key)
            .unwrap()
            .node;
        let command = if kind == ConnectionKind::Ac {
            Command::BuildLine { from, to }
        } else {
            Command::BuildLink { from, to }
        };
        let before = s.checkpoint();
        dispatch_in(&mut app, vec![UiAction::Command(command.clone())]);
        let open = *app.world().resource::<OpenPrompt>();
        assert!(
            matches!(open.0, Some(Prompt::Connection { .. })),
            "unsafe expansion must ask before ordering"
        );
        assert_eq!(app.world().resource::<Session>().0.checkpoint(), before);
        let model = prompt_model(
            open,
            Some(&app.world().resource::<Session>().0),
            &shipped_l10n(),
        )
        .unwrap();
        assert_eq!(model.confirm.label, "Proceed");
        assert_eq!(model.cancel.label, "Cancel");
        assert!(model.question.contains("one new city"));
        let widgets = pass.settled(&mut app, 1024.0, 640.0);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0));
        for label in ["Proceed", "Cancel"] {
            assert!(
                screen.contains_rect(super::support::egui_harness::widget(&widgets, label).rect)
            );
        }
        let draw = |ui: &mut egui::Ui| {
            egui::Modal::new(egui::Id::new("connection_warning"))
                .show(ui.ctx(), |ui| draw_prompt(ui, &model))
                .inner
        };
        let harness = Harness::with_screen(1024.0, 640.0);
        harness.draw(draw);
        let cancel = harness.press_key(egui::Key::Enter, draw);
        assert_eq!(cancel, vec![UiAction::Prompt(None)]);
        assert_eq!(
            harness.press_key(egui::Key::Escape, draw),
            vec![UiAction::Prompt(None)]
        );
        dispatch_in(&mut app, cancel);
        update_n(&mut app, 2);
        assert_eq!(app.world().resource::<Session>().0.checkpoint(), before);
        dispatch_in(&mut app, vec![UiAction::Command(command.clone())]);
        let proceed = harness.click("Proceed", draw);
        assert_eq!(
            proceed,
            vec![UiAction::ConfirmConnection { kind, from, to }]
        );
        dispatch_in(&mut app, proceed.clone());
        update_n(&mut app, 2);
        let after = app.world().resource::<Session>().0.checkpoint();
        assert_eq!(after.command_count, before.command_count + 1);
        assert_eq!(after.commands.last(), Some(&command));
        assert!(app.world().resource::<OpenPrompt>().0.is_none());
        dispatch_in(&mut app, proceed);
        update_n(&mut app, 2);
        assert_eq!(app.world().resource::<Session>().0.checkpoint(), after);
    }
}

#[test]
fn isolated_connection_orders_do_not_warn_and_stale_confirms_cannot_order() {
    use super::{
        hud_smoke::dispatch_in,
        support::{hud_app, request, update_n},
    };
    use game::sim::Session;
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, "sgcc");
    let node = |app: &bevy::prelude::App, key: &str| {
        app.world()
            .resource::<Session>()
            .0
            .session()
            .city_views()
            .into_iter()
            .find(|c| c.name_key == key)
            .unwrap()
            .node
    };
    let (from, to) = (node(&app, "city-shanghai"), node(&app, "city-nanjing"));
    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::BuildLine { from, to })],
    );
    assert!(app.world().resource::<OpenPrompt>().0.is_none());
    update_n(&mut app, 2);
    assert_eq!(
        app.world()
            .resource::<Session>()
            .0
            .checkpoint()
            .command_count,
        1
    );
    let home = app.world().resource::<Session>().0.session().anchor_node();
    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::BuildLink {
            from: home,
            to: from,
        })],
    );
    let model = prompt_model(
        *app.world().resource::<OpenPrompt>(),
        Some(&app.world().resource::<Session>().0),
        &shipped_l10n(),
    )
    .unwrap();
    dispatch_in(&mut app, vec![UiAction::Command(Command::BeginOperating)]);
    update_n(&mut app, 2);
    assert!(app.world().resource::<OpenPrompt>().0.is_none());
    let before = app.world().resource::<Session>().0.checkpoint();
    dispatch_in(&mut app, vec![model.confirm.action]);
    update_n(&mut app, 2);
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), before);
}

#[test]
fn connection_proceed_refreshes_changed_capacity_before_ordering() {
    use super::{
        hud_smoke::dispatch_in,
        support::{hud_app, request, update_n},
    };
    use domain::{ids::CellCoord, region::PlantKind};
    use game::sim::Session;
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, "sgcc");
    let s = app.world().resource::<Session>().0.session();
    let home = s.anchor_node();
    let city = s
        .city_views()
        .into_iter()
        .find(|c| c.name_key == "city-shanghai")
        .unwrap();
    let terrain = s.region().terrain();
    let cell = (0..terrain.height())
        .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
        .filter(|&c| {
            s.region().area_at(c) == s.region().area_at(city.cell)
                && s.plant_quote(PlantKind::Peaker, c).is_ok()
        })
        .min_by_key(|c| c.x.abs_diff(city.cell.x) + c.y.abs_diff(city.cell.y))
        .unwrap();
    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::BuildLink {
            from: home,
            to: city.node,
        })],
    );
    let old = *app.world().resource::<OpenPrompt>();
    let action = prompt_model(
        old,
        Some(&app.world().resource::<Session>().0),
        &shipped_l10n(),
    )
    .unwrap()
    .confirm
    .action;
    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::BuildPlant {
            kind: PlantKind::Peaker,
            cell,
        })],
    );
    update_n(&mut app, 2);
    let node = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .plant_views()
        .into_iter()
        .find(|p| p.cell == cell)
        .unwrap()
        .node;
    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::BuildLine {
            from: node,
            to: city.node,
        })],
    );
    update_n(&mut app, 2);
    let count = app
        .world()
        .resource::<Session>()
        .0
        .checkpoint()
        .command_count;
    dispatch_in(&mut app, vec![action.clone()]);
    update_n(&mut app, 2);
    assert_ne!(*app.world().resource::<OpenPrompt>(), old);
    assert_eq!(
        app.world()
            .resource::<Session>()
            .0
            .checkpoint()
            .command_count,
        count
    );
    assert_eq!(
        app.world()
            .resource::<Session>()
            .0
            .session()
            .power()
            .network()
            .links()
            .count(),
        0
    );
    dispatch_in(&mut app, vec![action]);
    update_n(&mut app, 2);
    assert!(app.world().resource::<OpenPrompt>().0.is_none());
    assert_eq!(
        app.world()
            .resource::<Session>()
            .0
            .checkpoint()
            .command_count,
        count + 1
    );
}
