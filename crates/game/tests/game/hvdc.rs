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
