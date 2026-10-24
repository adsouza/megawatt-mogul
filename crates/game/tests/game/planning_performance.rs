//! Repeated real HUD passes share analysis; commands refresh it before the next frame is drawn.
use crate::support::{egui_harness::button, egui_pass::EguiPass};
use crate::{
    hud_smoke::{EASTERN, dispatch_in},
    support::{empty_start_in, hud_app, request},
};
use bevy::prelude::*;
use game::hud::{OpenPrompt, PlanningForecastCache, PlanningTab, Prompt, UiAction};
use game::sim::Session;

fn revision(app: &App) -> u32 {
    app.world()
        .resource_ref::<PlanningForecastCache>()
        .last_changed()
        .get()
}

#[test]
fn idle_hud_frames_and_tab_changes_reuse_forecast_but_start_uses_the_latest_plan() {
    let logs = tempfile::TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, EASTERN);
    let pass = EguiPass::attach(&mut app);
    pass.settled(&mut app, 1280.0, 720.0);
    let first = revision(&app);
    let hash = app.world().resource::<Session>().0.session().state_hash();
    for _ in 0..20 {
        pass.run(&mut app, 1280.0, 720.0);
    }
    dispatch_in(
        &mut app,
        vec![
            UiAction::SetPlanningTab(PlanningTab::Forecast),
            UiAction::ShowHour(Some(18)),
        ],
    );
    pass.settled(&mut app, 1280.0, 720.0);
    assert_eq!(
        revision(&app),
        first,
        "view-only actions do not rebuild the forecast"
    );
    assert_eq!(
        app.world().resource::<Session>().0.session().state_hash(),
        hash
    );
    empty_start_in(&mut app);
    let widgets = pass.settled(&mut app, 1280.0, 720.0);
    assert_ne!(revision(&app), first);
    let worst = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .capacity_shortfall_check()
        .unwrap();
    pass.click_at(
        &mut app,
        1280.0,
        720.0,
        button(&widgets, "Start the day").rect.center(),
    );
    assert_eq!(
        *app.world().resource::<OpenPrompt>(),
        OpenPrompt(Some(Prompt::ShortfallStart {
            hour: worst.hour,
            shortfall: worst.shortfall
        }))
    );
}
