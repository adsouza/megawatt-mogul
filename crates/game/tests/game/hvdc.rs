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

fn prepare_remote_service(game: &mut Game) -> (domain::ids::NodeId, domain::ids::PlantId) {
    use domain::{events::DomainEvent, generation::PlantStatus, ids::CellCoord, region::PlantKind};
    let city = game
        .session()
        .city_views()
        .into_iter()
        .find(|c| c.name_key == "city-shanghai")
        .unwrap();
    let home = game.session().anchor_node();
    let mut months = game.session().link_quote(home, city.node).unwrap().months;
    game.execute(Command::BuildLink {
        from: home,
        to: city.node,
    })
    .unwrap();
    let region = game.session().region();
    let (w, h) = (region.terrain().width(), region.terrain().height());
    let area = region.area_at(city.cell);
    let cell = (0..h)
        .flat_map(|y| (0..w).map(move |x| CellCoord { x, y }))
        .filter(|c| {
            region.area_at(*c) == area && game.session().plant_quote(PlantKind::Peaker, *c).is_ok()
        })
        .min_by(|a, b| {
            domain::grid::distance_cells(*a, city.cell)
                .total_cmp(&domain::grid::distance_cells(*b, city.cell))
        })
        .unwrap();
    let events = game
        .execute(Command::BuildPlant {
            kind: PlantKind::Peaker,
            cell,
        })
        .unwrap();
    let plant = events
        .iter()
        .find_map(|e| {
            if let DomainEvent::PlantConstructionStarted { plant, .. } = e {
                Some(*plant)
            } else {
                None
            }
        })
        .unwrap();
    let p = game.session().power().plant(plant).unwrap();
    let PlantStatus::UnderConstruction { months_left } = p.status() else {
        panic!("new plant");
    };
    let node = p.node();
    months = months
        .max(months_left)
        .max(game.session().line_quote(city.node, node).unwrap().months);
    game.execute(Command::BuildLine {
        from: city.node,
        to: node,
    })
    .unwrap();
    for _ in 0..months {
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
        game.execute(Command::FinishReview).unwrap();
    }
    game.execute(Command::CommitPlant {
        plant,
        hours: [true; 24],
    })
    .unwrap();
    assert!(!game.session().power().island_active(city.node));
    (city.node, plant)
}

#[test]
fn service_activation_cancels_safely_refreshes_stale_plans_and_enrols_once_after_explicit_shortfall_acceptance()
 {
    use super::{
        hud_smoke::dispatch_in,
        support::{hud_app, request, update_n},
    };
    use application::RegionCatalog;
    use game::sim::Session;
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, "sgcc");
    let (island, plant) = prepare_remote_service(&mut app.world_mut().resource_mut::<Session>().0);
    let l10n = shipped_l10n();
    let card = links_model(&app.world().resource::<Session>().0, &l10n, &usd_style())
        .unwrap()
        .activations
        .into_iter()
        .find(|c| c.title.contains("Shanghai"))
        .unwrap();
    assert!(card.review.enabled);
    let before = app.world().resource::<Session>().0.checkpoint();
    dispatch_in(&mut app, vec![card.review.action.clone()]);
    let open = *app.world().resource::<OpenPrompt>();
    let model = prompt_model(open, Some(&app.world().resource::<Session>().0), &l10n).unwrap();
    assert_eq!(
        model.confirm.label,
        l10n.text("activation-confirm-shortfall")
    );
    assert!(model.question.contains("Shanghai") && model.question.contains("short"));
    for (width, zoom) in [(1024.0, 1.0), (1024.0, 1.25)] {
        let harness = Harness::with_screen(width / zoom, 640.0 / zoom);
        let draw = |ui: &mut egui::Ui| {
            egui::Modal::new(egui::Id::new("activation_test"))
                .show(ui.ctx(), |ui| draw_prompt(ui, &model))
                .inner
        };
        let widgets = harness.draw(draw);
        let screen =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width / zoom, 640.0 / zoom));
        for label in [&model.confirm.label, &model.cancel.label] {
            assert!(
                screen.contains_rect(super::support::egui_harness::button(&widgets, label).rect)
            );
        }
        assert_eq!(
            harness.press_key(egui::Key::Enter, draw),
            vec![UiAction::Prompt(None)]
        );
        assert_eq!(
            harness.press_key(egui::Key::Escape, draw),
            vec![UiAction::Prompt(None)]
        );
    }
    dispatch_in(&mut app, vec![UiAction::Prompt(None)]);
    update_n(&mut app, 2);
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), before);
    dispatch_in(&mut app, vec![card.review.action]);
    let old_confirm = model.confirm.action;
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(Command::SetAutoFollow { plant, on: false })
        .unwrap();
    dispatch_in(&mut app, vec![old_confirm]);
    assert!(
        !app.world()
            .resource::<Session>()
            .0
            .session()
            .power()
            .island_active(island)
    );
    assert_ne!(*app.world().resource::<OpenPrompt>(), open);
    let model = prompt_model(
        *app.world().resource::<OpenPrompt>(),
        Some(&app.world().resource::<Session>().0),
        &l10n,
    )
    .unwrap();
    let before = app
        .world()
        .resource::<Session>()
        .0
        .checkpoint()
        .command_count;
    dispatch_in(
        &mut app,
        vec![model.confirm.action.clone(), model.confirm.action],
    );
    update_n(&mut app, 3);
    let game = &app.world().resource::<Session>().0;
    assert!(game.session().power().island_active(island));
    assert_eq!(game.checkpoint().command_count, before + 1);
    let restored = Game::restore(
        RonRegionCatalog::new(shipped_assets().join("regions"))
            .load(&RegionId::new("sgcc"))
            .unwrap(),
        game.checkpoint(),
        Box::new(InMemoryCommandLog::default()),
        &|| false,
    )
    .unwrap();
    assert!(restored.session().power().island_active(island));
    assert_eq!(restored.checkpoint(), game.checkpoint());
    let checkpoint = game.checkpoint();
    let replayed = application::replay(
        RonRegionCatalog::new(shipped_assets().join("regions"))
            .load(&RegionId::new("sgcc"))
            .unwrap(),
        &checkpoint.header,
        &checkpoint.commands,
    )
    .unwrap();
    assert_eq!(replayed.state_hash(), game.session().state_hash());
    assert!(replayed.power().island_active(island));
    assert!(l10n.problems().is_empty());
}

#[test]
fn missing_grid_support_cannot_be_activated_through_a_raw_ui_command_or_confirm() {
    use super::{
        hud_smoke::dispatch_in,
        support::{hud_app, request, update_n},
    };
    use game::sim::Session;
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, "sgcc");
    let (island, plant) = prepare_remote_service(&mut app.world_mut().resource_mut::<Session>().0);
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(Command::CommitPlant {
            plant,
            hours: [false; 24],
        })
        .unwrap();
    let a = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .activation_assessment(island)
        .unwrap();
    let before = app.world().resource::<Session>().0.checkpoint();
    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::ActivateService {
            island,
            accept_shortfall: true,
            expected_revision: a.revision,
        })],
    );
    let model = prompt_model(
        *app.world().resource::<OpenPrompt>(),
        Some(&app.world().resource::<Session>().0),
        &shipped_l10n(),
    )
    .unwrap();
    assert!(!model.confirm.enabled);
    assert!(model.question.contains("committed"));
    dispatch_in(
        &mut app,
        vec![UiAction::ConfirmActivation {
            island,
            revision: a.revision,
        }],
    );
    update_n(&mut app, 3);
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), before);
}

#[test]
fn a_plan_edit_queued_before_activation_cannot_bypass_the_domain_revision_check() {
    use super::{
        hud_smoke::dispatch_in,
        support::{hud_app, request, update_n},
    };
    use game::sim::Session;
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, "sgcc");
    let (island, plant) = prepare_remote_service(&mut app.world_mut().resource_mut::<Session>().0);
    dispatch_in(&mut app, vec![UiAction::RequestActivation { island }]);
    let model = prompt_model(
        *app.world().resource::<OpenPrompt>(),
        Some(&app.world().resource::<Session>().0),
        &shipped_l10n(),
    )
    .unwrap();
    let before = app
        .world()
        .resource::<Session>()
        .0
        .checkpoint()
        .command_count;
    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::SetAutoFollow { plant, on: false }),
            model.confirm.action,
        ],
    );
    update_n(&mut app, 3);
    let game = &app.world().resource::<Session>().0;
    assert!(!game.session().power().island_active(island));
    assert_eq!(
        game.checkpoint().command_count,
        before + 1,
        "only the successful plan edit is recorded"
    );
}

#[test]
fn all_five_starter_grids_are_reachable_in_the_forecast_picker_at_minimum_size() {
    use super::{
        hud_smoke::dispatch_in,
        support::{egui_harness::widget, egui_pass::EguiPass, hud_app, request},
    };
    use game::{
        hud::{self, PlanningTab, UiState},
        sim::Session,
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, "sgcc");
    dispatch_in(
        &mut app,
        vec![UiAction::SetPlanningTab(PlanningTab::Forecast)],
    );
    let l10n = shipped_l10n();
    let model = hud::forecast_model(
        &app.world().resource::<Session>().0,
        &l10n,
        app.world().resource::<UiState>(),
    );
    assert_eq!(model.islands.len(), 5);
    assert!(model.islands.iter().all(|island| island.active));
    let checkpoint = app.world().resource::<Session>().0.checkpoint();
    let pass = EguiPass::attach(&mut app);
    pass.ctx().set_zoom_factor(1.25);
    for island in &model.islands {
        dispatch_in(&mut app, vec![UiAction::SelectIsland(None)]);
        let widgets = pass.settled(&mut app, 1024.0, 640.0);
        let at = widget(&widgets, &model.island_picker_label).rect.center();
        pass.click_at(&mut app, 1024.0, 640.0, at);
        let widgets = pass.settled(&mut app, 1024.0, 640.0);
        let at = widget(&widgets, &island.inspect.label).rect.center();
        pass.click_at(&mut app, 1024.0, 640.0, at);
        assert_eq!(
            app.world().resource::<UiState>().island,
            Some(island.island)
        );
    }
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
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
    dispatch_in(&mut app, vec![UiAction::SelectIsland(None)]);
    let overview = hud::forecast_model(
        &app.world().resource::<Session>().0,
        &l10n,
        app.world().resource::<UiState>(),
    );
    let inspect_label = &overview
        .islands
        .iter()
        .find(|i| i.island == to)
        .unwrap()
        .inspect
        .label;
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    let inspect = button(&widgets, inspect_label);
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
fn equipment_inspector_opens_the_selected_project_without_collapsed_cards_or_unrelated_activation()
{
    use game::hud::{HvdcTab, UiState, links_model_with_ui};
    let mut game = sgcc();
    let home = game.session().anchor_node();
    let cities: Vec<_> = game
        .session()
        .city_views()
        .into_iter()
        .filter(|c| ["city-shanghai", "city-chifeng"].contains(&c.name_key.as_str()))
        .collect();
    assert_eq!(cities.len(), 2);
    for city in &cities {
        game.execute(Command::BuildLink {
            from: home,
            to: city.node,
        })
        .unwrap();
    }
    let links: Vec<_> = game.session().power().network().links().cloned().collect();
    let station = links[1].to;
    let mut state = UiState::default();
    state.apply(&UiAction::InspectHvdc {
        link: links[1].id,
        station: Some(station.id),
    });
    let l10n = shipped_l10n();
    for (width, height) in [(1024.0, 640.0), (819.2, 512.0)] {
        let model = links_model_with_ui(&game, &l10n, &usd_style(), &state).unwrap();
        assert_eq!(model.selected, Some(links[1].id));
        assert!(model.cards[1].terminals[1].focused);
        let harness = Harness::with_screen(width, height);
        let draw = |ui: &mut egui::Ui| {
            let mut actions = Vec::new();
            egui::Window::new(&model.title)
                .default_width((width - 40.0).min(780.0))
                .max_size(egui::vec2(width - 32.0, height - 32.0))
                .show(ui.ctx(), |ui| {
                    actions = draw_links(ui, &model);
                });
            actions
        };
        let widgets = harness.draw(draw);
        assert!(super::support::egui_harness::has(
            &widgets,
            &model.cards[1].terminals[1].name
        ));
        assert!(!super::support::egui_harness::has(
            &widgets,
            &model.cards[0].terminals[1].name
        ));
        assert!(harness.screen_rect().contains_rect(
            super::support::egui_harness::button(&widgets, &model.close.label).rect
        ));
        assert_eq!(
            harness.press_key(egui::Key::Escape, draw),
            vec![UiAction::ShowLinks(false)]
        );
    }
    state.apply(&UiAction::SetHvdcTab(HvdcTab::Costs));
    let model = links_model_with_ui(&game, &l10n, &usd_style(), &state).unwrap();
    let harness = Harness::new();
    let draw = |ui: &mut egui::Ui| central(ui, |ui| draw_links(ui, &model));
    let widgets = harness.draw(draw);
    assert!(super::support::egui_harness::has(
        &widgets,
        &l10n.text("hvdc-metric-capital")
    ));
    assert!(l10n.problems().is_empty());
}

#[test]
fn hvdc_range_enter_uses_current_text_once_and_escape_never_changes_the_plan() {
    use super::{
        hud_smoke::dispatch_in,
        support::{egui_pass::EguiPass, hud_app, request, update_n},
    };
    use game::{
        hud::{HvdcRangeForm, HvdcTab, UiState},
        sim::Session,
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, "sgcc");
    let home = app.world().resource::<Session>().0.session().anchor_node();
    let far = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .city_views()
        .into_iter()
        .find(|c| c.name_key == "city-shanghai")
        .unwrap()
        .node;
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(Command::BuildLink {
            from: home,
            to: far,
        })
        .unwrap();
    let link = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .power()
        .network()
        .links()
        .find(|link| link.to_grid() == far)
        .unwrap()
        .id;
    dispatch_in(
        &mut app,
        vec![
            UiAction::InspectHvdc {
                link,
                station: None,
            },
            UiAction::SetHvdcTab(HvdcTab::Dispatch),
            UiAction::EditHvdcRange(HvdcRangeForm {
                link: Some(link),
                from: 6,
                to: 9,
                mw: String::new(),
            }),
        ],
    );
    let pass = EguiPass::attach(&mut app);
    pass.ctx().set_zoom_factor(1.25);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    let field = widgets
        .iter()
        .find(|w| w.role == egui::accesskit::Role::TextInput && w.text.is_empty())
        .unwrap()
        .rect
        .center();
    pass.click_at(&mut app, 1024.0, 640.0, field);
    let before = app
        .world()
        .resource::<Session>()
        .0
        .checkpoint()
        .command_count;
    pass.run_with(
        &mut app,
        1024.0,
        640.0,
        vec![
            egui::Event::Text("-50".into()),
            egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    update_n(&mut app, 2);
    let game = &app.world().resource::<Session>().0;
    assert_eq!(game.checkpoint().command_count, before + 1);
    let schedule = game
        .session()
        .power()
        .network()
        .link(link)
        .unwrap()
        .schedule;
    for (h, mw) in schedule.iter().enumerate() {
        assert_eq!(*mw, if (6..9).contains(&h) { -50.0 } else { 0.0 });
    }
    dispatch_in(
        &mut app,
        vec![UiAction::EditHvdcRange(HvdcRangeForm {
            link: Some(link),
            from: 0,
            to: 24,
            mw: "NaN".into(),
        })],
    );
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    let apply =
        super::support::egui_harness::button(&widgets, &shipped_l10n().text("hvdc-range-apply"));
    assert!(apply.disabled);
    let field = widgets
        .iter()
        .find(|w| w.role == egui::accesskit::Role::TextInput && w.text == "NaN")
        .unwrap()
        .rect
        .center();
    pass.click_at(&mut app, 1024.0, 640.0, field);
    let before = app.world().resource::<Session>().0.checkpoint();
    pass.run_with(
        &mut app,
        1024.0,
        640.0,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    update_n(&mut app, 2);
    assert!(!app.world().resource::<UiState>().links_open);
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), before);
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(Command::BeginOperating)
        .unwrap();
    let before = app.world().resource::<Session>().0.checkpoint();
    dispatch_in(
        &mut app,
        vec![
            UiAction::EditHvdcRange(HvdcRangeForm {
                link: Some(link),
                from: 0,
                to: 24,
                mw: "100".into(),
            }),
            UiAction::ApplyHvdcRange(link),
        ],
    );
    update_n(&mut app, 2);
    assert_eq!(
        app.world().resource::<Session>().0.checkpoint(),
        before,
        "range edits cannot change the operating schedule"
    );
}

#[test]
fn equipment_inspection_selects_the_clicked_project_and_station_and_keeps_it_across_planning_reset()
{
    use domain::{
        grid::{ConverterEnd, ConverterStationId},
        ids::LinkId,
    };
    use game::hud::{HvdcTab, UiState};
    let mut ui = UiState::default();
    let station = ConverterStationId {
        link: LinkId(97),
        end: ConverterEnd::From,
    };
    ui.apply(&UiAction::InspectHvdc {
        link: station.link,
        station: Some(station),
    });
    assert!(ui.links_open);
    assert_eq!(ui.hvdc_selected, Some(station.link));
    assert_eq!(ui.hvdc_terminal, Some(station));
    ui.apply(&UiAction::SetHvdcTab(HvdcTab::Dispatch));
    ui.apply(&UiAction::ShowLinks(false));
    ui.apply(&UiAction::ShowLinks(true));
    assert_eq!(ui.hvdc_tab, HvdcTab::Dispatch);
    assert_eq!(ui.hvdc_selected, Some(station.link));
}

#[test]
fn selecting_a_converter_marker_opens_hvdc_details_without_selecting_or_reconnecting_the_city() {
    use super::support::{egui_pass::EguiPass, hud_app, request, update_n};
    use game::{
        hud::UiState,
        sim::Session,
        world::{GridTransform, MapSurface, OrbitCamera, WorldControls, WorldViewPlugin},
    };
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    app.add_plugins(WorldViewPlugin);
    request(&mut app, "sgcc");
    let home = app.world().resource::<Session>().0.session().anchor_node();
    let far = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .city_views()
        .into_iter()
        .find(|c| c.name_key == "city-shanghai")
        .unwrap()
        .node;
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(Command::BuildLink {
            from: home,
            to: far,
        })
        .unwrap();
    update_n(&mut app, 2);
    let session = app.world().resource::<Session>().0.session();
    let terrain = session.region().terrain();
    let bus = GridTransform::new(terrain.width(), terrain.height())
        .cell_center(terrain, session.power().network().node(home).unwrap().cell)
        .unwrap();
    app.world_mut().resource_mut::<WorldControls>().orbit = OrbitCamera {
        target: bevy::prelude::Vec2::new(bus.x, bus.z),
        zoom: 0.3,
        elevation: 1.42,
        ..Default::default()
    };
    let pass = EguiPass::attach(&mut app);
    pass.settled(&mut app, 1024.0, 640.0);
    let surface = app.world().resource::<MapSurface>();
    let rect = surface.rect.unwrap();
    let projected = surface
        .pose
        .unwrap()
        .project(bus + bevy::prelude::Vec3::Y * 0.6)
        .unwrap();
    let at = egui::pos2(
        rect.left() + (projected.x + 1.0) * 0.5 * rect.width(),
        rect.top() + (1.0 - projected.y) * 0.5 * rect.height(),
    ) + egui::vec2(12.0, -12.0);
    assert!(rect.contains(at));
    let before = app.world().resource::<Session>().0.checkpoint();
    let selection = *app.world().resource::<Selection>();
    pass.click_at(&mut app, 1024.0, 640.0, at);
    assert!(app.world().resource::<UiState>().links_open);
    assert_eq!(*app.world().resource::<Selection>(), selection);
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), before);
}

#[test]
fn selecting_a_local_bus_on_the_map_keeps_the_explicit_hvdc_tool_active() {
    use super::{
        hud_smoke::dispatch_in,
        support::{egui_harness::has, egui_pass::EguiPass, hud_app, request},
    };
    use game::{hud::UiState, sim::Session};
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, "sgcc");
    let city = app.world().resource::<Session>().0.session().city_views()[0].id;
    dispatch_in(&mut app, vec![UiAction::Select(Selection::HvdcStart)]);
    dispatch_in(&mut app, vec![UiAction::Select(Selection::City(city))]);
    assert!(app.world().resource::<UiState>().hvdc_construction);
    let pass = EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    let l10n = shipped_l10n();
    assert!(has(&widgets, &l10n.text("build-hvdc-hint-from")));
    assert!(!has(&widgets, &l10n.text("build-line-hint-from")));
    dispatch_in(&mut app, vec![UiAction::Select(Selection::None)]);
    assert!(!app.world().resource::<UiState>().hvdc_construction);
}

#[test]
fn ac_and_hvdc_construction_are_separate_and_cannot_silently_switch_types() {
    use game::hud::draw_build;
    let game = sgcc();
    let l10n = shipped_l10n();
    let home = game.session().anchor_node();
    let model = build_model(&game, &l10n, Selection::LineFrom(home), &usd_style()).unwrap();
    assert!(!model.hvdc_active);
    let LineSection::End { targets, .. } = &model.line else {
        panic!("AC targets");
    };
    for target in targets {
        let cell = game
            .session()
            .power()
            .network()
            .node(target.node)
            .unwrap()
            .cell;
        let home_cell = game.session().power().network().node(home).unwrap().cell;
        assert_eq!(
            game.session().region().area_at(cell),
            game.session().region().area_at(home_cell)
        );
        assert!(matches!(
            target.build.action,
            UiAction::Command(Command::BuildLine { .. })
        ));
    }
    assert_eq!(
        model.transport_modes[1].button.action,
        UiAction::Select(Selection::HvdcStart)
    );
    for zoom in [1.0, 1.25] {
        let harness = Harness::with_screen(1024.0 / zoom, 640.0 / zoom);
        let draw = |ui: &mut egui::Ui| central(ui, |ui| draw_build(ui, &model));
        let actions = harness.click(&l10n.text("build-hvdc-tool"), draw);
        assert_eq!(actions, vec![UiAction::Select(Selection::HvdcStart)]);
    }
    let model = build_model(&game, &l10n, Selection::HvdcFrom(home), &usd_style()).unwrap();
    assert!(model.hvdc_active);
    let LineSection::End { targets, hint, .. } = &model.hvdc.unwrap() else {
        panic!("HVDC targets");
    };
    assert!(hint.contains("station") && hint.contains("another zone"));
    assert!(!targets.is_empty());
    for target in targets {
        let cell = game
            .session()
            .power()
            .network()
            .node(target.node)
            .unwrap()
            .cell;
        let home_cell = game.session().power().network().node(home).unwrap().cell;
        assert_ne!(
            game.session().region().area_at(cell),
            game.session().region().area_at(home_cell)
        );
        assert!(matches!(
            target.build.action,
            UiAction::Command(Command::BuildLink { .. })
        ));
        assert!(target.name.contains("converter"));
    }
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
    let model = build_model(&game, &l10n, Selection::HvdcFrom(from), &style).unwrap();
    let LineSection::End { targets, .. } = model.hvdc.unwrap() else {
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
    assert_eq!(model.cards.len(), 6);
    let added = model.cards.last().unwrap();
    assert!(added.planning);
    assert_eq!(added.schedule, [0.0; 24]);
    assert!(added.cancel.is_some());
    for inherited in &model.cards[..5] {
        assert!(inherited.planning);
        assert_eq!(inherited.rating, 1000.0);
        assert_eq!(inherited.schedule, [0.0; 24]);
        assert!(inherited.cancel.is_none());
    }
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
    let link = game
        .session()
        .power()
        .network()
        .links()
        .find(|link| link.to_grid() == to)
        .unwrap()
        .id;
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
        let proceed_label = if kind == ConnectionKind::Dc {
            "Build inactive connection"
        } else {
            "Proceed"
        };
        assert_eq!(model.confirm.label, proceed_label);
        assert_eq!(model.cancel.label, "Cancel");
        assert!(model.question.contains("one new city"));
        let widgets = pass.settled(&mut app, 1024.0, 640.0);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0));
        for label in [proceed_label, "Cancel"] {
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
        let proceed = harness.click(proceed_label, draw);
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
        5
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
