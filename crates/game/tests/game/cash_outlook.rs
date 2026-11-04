use crate::{
    hud_smoke::{dispatch_in, sent},
    support::{egui_harness::button, egui_pass::EguiPass, hud_app, request},
};
use game::{
    hud::UiAction,
    sim::{PlayerCommand, Session, SimSpeed},
};
use tempfile::TempDir;

#[test]
fn cash_outlook_opens_from_construction_without_ordering_anything() {
    let dir = TempDir::new().unwrap();
    let mut app = hud_app(dir.path());
    {
        let mut progress = app
            .world_mut()
            .resource_mut::<game::progress::ClientProgressStore>();
        progress.loaded = true;
        progress.value.tutorial_pending = false;
    }
    request(&mut app, "western_interconnection");
    let before = app.world().resource::<Session>().0.session().state_hash();
    let pass = EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    let open = button(&widgets, "Cash outlook").rect;
    let payoff = button(&widgets, "Compare plant payoff").rect;
    assert!(open.left() >= payoff.right());
    assert!((open.center().y - payoff.center().y).abs() < 1.0);
    dispatch_in(&mut app, vec![UiAction::SetSpeed(SimSpeed::X8)]);
    pass.click_at(&mut app, 1024.0, 640.0, open.center());
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(
        widgets
            .iter()
            .any(|w| w.text == "Cash outlook — construction and income")
    );
    assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::Paused);
    assert_eq!(
        app.world().resource::<Session>().0.session().state_hash(),
        before
    );
    assert!(sent::<PlayerCommand>(&app).is_empty());
    let close = button(&widgets, "Close cash outlook").rect;
    pass.click_at(&mut app, 1024.0, 640.0, close.center());
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(
        !widgets
            .iter()
            .any(|w| w.text == "Cash outlook — construction and income")
    );
}

fn escape(pass: &EguiPass, app: &mut bevy::prelude::App, size: (f32, f32)) {
    for pressed in [true, false] {
        pass.run_with(
            app,
            size.0,
            size.1,
            vec![bevy_egui::egui::Event::Key {
                key: bevy_egui::egui::Key::Escape,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
    }
}

#[test]
fn cash_preview_chooser_is_read_only_and_popup_takes_escape_first_at_maximum_zoom() {
    use bevy_egui::egui;
    use domain::{ids::CellCoord, region::PlantKind};
    use game::hud::{Selection, UiState};
    let dir = TempDir::new().unwrap();
    let mut app = hud_app(dir.path());
    {
        let mut progress = app
            .world_mut()
            .resource_mut::<game::progress::ClientProgressStore>();
        progress.loaded = true;
        progress.value.tutorial_pending = false;
    }
    request(&mut app, "western_interconnection");
    let session = app.world().resource::<Session>().0.session();
    let terrain = session.region().terrain();
    let cell = (0..terrain.height())
        .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
        .find(|at| session.plant_quote(PlantKind::Battery, *at).is_ok())
        .unwrap();
    let before = session.state_hash();
    dispatch_in(
        &mut app,
        vec![
            UiAction::Select(Selection::Cell(cell)),
            UiAction::OpenCashOutlook,
        ],
    );
    let pass = EguiPass::attach(&mut app);
    let size = (1024.0 / 1.5, 640.0 / 1.5);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0, size.1));
    let widgets = pass.settled(&mut app, size.0, size.1);
    assert!(screen.contains_rect(button(&widgets, "Close cash outlook").rect));
    let picker = widgets
        .iter()
        .find(|w| {
            w.role == egui::accesskit::Role::ComboBox && w.text == "Existing commitments only"
        })
        .unwrap()
        .rect;
    assert!(screen.contains_rect(picker));
    pass.click_at(&mut app, size.0, size.1, picker.center());
    assert!(egui::Popup::is_any_open(pass.ctx()));
    escape(&pass, &mut app, size);
    assert!(app.world().resource::<UiState>().cash_open);
    assert!(!egui::Popup::is_any_open(pass.ctx()));

    let widgets = pass.settled(&mut app, size.0, size.1);
    let picker = widgets
        .iter()
        .find(|w| {
            w.role == egui::accesskit::Role::ComboBox && w.text == "Existing commitments only"
        })
        .unwrap()
        .rect;
    pass.click_at(&mut app, size.0, size.1, picker.center());
    let widgets = pass.run(&mut app, size.0, size.1);
    let battery = widgets
        .iter()
        .find(|w| w.role == egui::accesskit::Role::Button && w.text == "Battery storage")
        .unwrap()
        .rect;
    pass.click_at(&mut app, size.0, size.1, battery.center());
    assert_eq!(
        app.world().resource::<UiState>().cash_plant,
        Some(PlantKind::Battery)
    );
    let l10n = crate::support::shipped_l10n();
    let model = game::hud::cash_outlook_model(
        &app.world().resource::<Session>().0,
        &l10n,
        Some(cell),
        Some(PlantKind::Battery),
        &crate::support::usd_style(),
    )
    .unwrap();
    assert!(
        model
            .forecast
            .months
            .iter()
            .any(|month| month.proposed > domain::units::Money::ZERO)
    );
    assert!(l10n.problems().is_empty());
    pass.settled(&mut app, size.0, size.1);
    escape(&pass, &mut app, size);
    assert!(!app.world().resource::<UiState>().cash_open);
    assert_eq!(
        app.world().resource::<Session>().0.session().state_hash(),
        before
    );
    assert!(sent::<PlayerCommand>(&app).is_empty());
}
