//! Smoke tests for the Review modal and the Milestones panel, drawn by the real egui pass on the
//! real plugins: what shows, what fits at the smallest window, and what a click does.

use application::Command;
use bevy::prelude::*;
use bevy_egui::egui;
use game::hud::{
    MILESTONES_WIDTH, MilestonesOpen, REVIEW_MARGIN, REVIEW_WIDTH, Selection, ToastQueue, UiAction,
    review_model, toasts_model,
};
use game::l10n::Localize;
use game::sim::{PlayerCommand, Session, SimSpeed};
use game::state::PhaseState;
use tempfile::TempDir;

use crate::hud_smoke::{EASTERN, WESTERN, WIDE, dispatch_in, operating_hour_app, sent};
use crate::support::egui_harness::{button as find_button, buttons, has, shows, widget};
use crate::support::egui_pass::EguiPass;
use crate::support::{hud_app, request, shipped_l10n, update_n};

fn phase(app: &App) -> PhaseState {
    *app.world().resource::<State<PhaseState>>().get()
}

/// [`operating_hour_app`]'s day played to the month's close: in Review.
fn review_app(logs: &TempDir, region: &str) -> App {
    let mut app = operating_hour_app(logs, region);
    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::AdvanceTicks { n: 1_440 })],
    );
    update_n(&mut app, 3);
    assert_eq!(phase(&app), PhaseState::Review);
    app
}

#[test]
fn the_review_modal_draws_every_section_after_a_full_day() {
    let (width, height) = WIDE;
    for region in [EASTERN, WESTERN] {
        let logs = TempDir::new().unwrap();
        let mut app = review_app(&logs, region);
        let pass = EguiPass::attach(&mut app);

        let widgets = pass.settled(&mut app, width, height);

        for text in [
            "Month in review",
            "Profit and loss",
            "Revenue",
            "Fuel",
            "Start-ups",
            "Fixed operations and maintenance",
            "Construction",
            "Net",
            "Merit order used",
            "Plant",
            "Marginal cost",
            "Reliability",
            "Served",
            "Unserved (planned rotating outages)",
            "Unserved (protection shedding)",
            "Time out of frequency band",
            "Incidents",
            "Lessons",
            "Regulator",
            // Behind the modal: the map and the top bar.
            "Map",
        ] {
            assert!(has(&widgets, text), "{region}: {text} is not shown");
        }
        let game = &app.world().resource::<Session>().0;
        let report = game.projections().monthly_reports.latest().unwrap();
        let l10n = shipped_l10n();
        let first = &report.merit_order[0];
        let names = game::l10n::Names::new(&l10n, game.session());
        assert!(has(&widgets, &names.plant(first.plant)), "{region}");
        assert!(has(&widgets, &game::l10n::format::month_year(report.month)));
        assert!(shows(&widgets, "Each played day stands for"), "{region}");
        let lesson = game.projections().lessons.latest()[0].localize(&l10n);
        assert!(has(&widgets, &lesson), "{region}: {lesson}");
        assert!(!find_button(&widgets, "Continue").disabled);
        assert!(!find_button(&widgets, "Menu").disabled);
        assert!(!find_button(&widgets, "Milestones").disabled);
    }
}

/// The smallest window the game allows is 1024×640. There the modal sits under the top bar,
/// centred at its fixed width, and Continue is on screen without scrolling.
#[test]
fn at_the_smallest_window_the_review_fits_with_continue_in_view() {
    for region in [EASTERN, WESTERN] {
        let logs = TempDir::new().unwrap();
        let mut app = review_app(&logs, region);
        let pass = EguiPass::attach(&mut app);

        let widgets = pass.settled(&mut app, 1024.0, 640.0);

        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0));
        let menu = find_button(&widgets, "Menu").rect;
        let title = widgets
            .iter()
            .find(|w| w.text == "Month in review" && w.role == egui::accesskit::Role::Label)
            .expect("the modal's title")
            .rect;
        let next = find_button(&widgets, "Continue").rect;
        let (left, right) = ((1024.0 - REVIEW_WIDTH) / 2.0, (1024.0 + REVIEW_WIDTH) / 2.0);
        assert!(screen.contains_rect(next), "{region}: Continue {next:?}");
        assert!(
            next.bottom() <= 640.0 - REVIEW_MARGIN + 0.5,
            "{region}: Continue {next:?} is inside the modal"
        );
        assert!(
            title.top() >= menu.bottom() + REVIEW_MARGIN - 0.5,
            "{region}: the modal {title:?} starts under the top bar {menu:?}"
        );
        assert!(next.top() > title.bottom(), "{region}");
        for (name, rect) in [("title", title), ("continue", next)] {
            assert!(
                rect.left() >= left - 0.5 && rect.right() <= right + 0.5,
                "{region}: {name} {rect:?} is inside the modal's width"
            );
        }
        // Every line of the modal is inside its width: nothing scrolls sideways. (The map behind
        // it may run labels off the screen's edge; it is dimmed and takes no clicks.)
        let l10n = shipped_l10n();
        let model = review_model(&app.world().resource::<Session>().0, &l10n).unwrap();
        let mut texts: Vec<&String> = vec![&model.days_note];
        texts.extend(
            model
                .pnl
                .iter()
                .chain(&model.reliability)
                .flat_map(|s| [&s.label, &s.value]),
        );
        texts.extend(
            // Not the kind column: the map legend behind names the kinds too.
            model
                .merit
                .iter()
                .flat_map(|r| [&r.plant, &r.marginal_cost, &r.energy]),
        );
        texts.extend(
            model
                .lessons
                .iter()
                .chain(&model.incidents)
                .chain(&model.regulatory),
        );
        // The month's toasts (a strike, say) can repeat a line of the modal in their own corner.
        let toasts = egui::Id::new("toasts");
        let toast_area = pass
            .ctx()
            .memory(|memory| memory.area_rect(toasts))
            .unwrap_or(egui::Rect::NOTHING);
        for text in texts {
            for w in widgets
                .iter()
                .filter(|w| &w.text == text && !toast_area.contains_rect(w.rect))
            {
                assert!(
                    w.rect.left() >= left - 0.5 && w.rect.right() <= right + 0.5,
                    "{region}: {text:?} {:?} is outside the modal",
                    w.rect
                );
            }
        }
        // The toasts float above the modal, and never over Continue.
        let world = app.world();
        let shown = toasts_model(
            world.resource::<ToastQueue>(),
            &l10n,
            world.resource::<Time<Real>>().elapsed_secs_f64(),
        )
        .toasts;
        if region == EASTERN {
            assert!(!shown.is_empty(), "the eastern month toasts its strike");
        }
        let layer = |id: egui::Id| {
            pass.ctx()
                .memory(|memory| memory.areas().visible_layer_ids())
                .into_iter()
                .find(|layer| layer.id == id)
                .unwrap_or_else(|| panic!("{region}: no layer {id:?}"))
        };
        if !shown.is_empty() {
            assert!(
                layer(toasts).order > layer(egui::Id::new("review")).order,
                "{region}: the toasts are drawn above the modal"
            );
        }
        for text in &shown {
            let toast = widgets
                .iter()
                .find(|w| &w.text == text && toast_area.contains_rect(w.rect))
                .unwrap_or_else(|| panic!("{region}: the toast {text:?} is drawn"))
                .rect;
            assert!(
                !toast.intersects(next),
                "{region}: {text:?} {toast:?} covers Continue"
            );
        }
    }
}

#[test]
fn continue_finishes_the_review_through_the_real_pass() {
    let logs = TempDir::new().unwrap();
    let mut app = review_app(&logs, EASTERN);
    let pass = EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    // With fixed time standing still the message buffers don't turn over: count from here.
    let before = sent::<PlayerCommand>(&app).len();

    pass.click_at(
        &mut app,
        1024.0,
        640.0,
        find_button(&widgets, "Continue").rect.center(),
    );

    assert_eq!(
        sent::<PlayerCommand>(&app)[before..],
        [PlayerCommand(Command::FinishReview)]
    );
    update_n(&mut app, 2);
    assert_eq!(phase(&app), PhaseState::Planning);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(!has(&widgets, "Month in review"));
    assert!(has(&widgets, "Start the day"));
}

/// The backdrop dims the map and takes its clicks, and a click there doesn't leave Review: only
/// Continue does.
#[test]
fn a_click_on_the_dimmed_map_does_nothing() {
    let logs = TempDir::new().unwrap();
    let mut app = review_app(&logs, EASTERN);
    let pass = EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    let map = widget(&widgets, "Map").rect;
    // Left of the modal, on the map.
    let at = egui::pos2(map.left() + 40.0, map.center().y);
    assert!(at.x < (1024.0 - REVIEW_WIDTH) / 2.0 && map.contains(at));
    let before = *app.world().resource::<Selection>();
    let commands = sent::<PlayerCommand>(&app).len();

    pass.click_at(&mut app, 1024.0, 640.0, at);

    assert_eq!(*app.world().resource::<Selection>(), before);
    assert_eq!(sent::<PlayerCommand>(&app)[commands..], []);
    update_n(&mut app, 2);
    assert_eq!(phase(&app), PhaseState::Review);
}

/// The top bar's Milestones button opens the panel in every phase, Review included (the backdrop
/// leaves the top bar uncovered), and the panel's Close or the button again closes it. It doesn't
/// pause the day.
#[test]
fn the_milestones_panel_opens_from_the_top_bar_in_every_phase() {
    let (width, height) = (1024.0, 640.0);
    let open = |app: &App| app.world().resource::<MilestonesOpen>().0;
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, EASTERN);
    let pass = EguiPass::attach(&mut app);
    let click = |pass: &EguiPass, app: &mut App, label: &str| {
        let widgets = pass.settled(app, width, height);
        pass.click_at(
            app,
            width,
            height,
            find_button(&widgets, label).rect.center(),
        );
    };
    let first = "Serve a monthly peak of 50 GW";

    // Planning.
    assert!(!has(&pass.settled(&mut app, width, height), first));
    click(&pass, &mut app, "Milestones");
    assert!(open(&app));
    let widgets = pass.settled(&mut app, width, height);
    for text in [
        first,
        "Get through a full season without a strike",
        "Connect 10 cities",
        "0 of 3 strike-free months this season",
    ] {
        assert!(has(&widgets, text), "Planning: {text}");
    }
    let close = find_button(&widgets, "Close").rect;
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
    assert!(screen.contains_rect(close), "Close {close:?} is on screen");
    let menu = find_button(&widgets, "Menu").rect;
    assert!(close.top() > menu.bottom());
    // The whole window is on screen under the top bar, about as wide as its content.
    let window = pass
        .ctx()
        .memory(|memory| memory.area_rect(egui::Id::new("milestones")))
        .expect("the Milestones window");
    assert!(screen.contains_rect(window), "{window:?} is on screen");
    assert!(
        window.top() > menu.bottom(),
        "{window:?} is under the top bar"
    );
    assert!(
        window.width() >= MILESTONES_WIDTH && window.width() <= MILESTONES_WIDTH + 40.0,
        "{window:?} is the panel's width plus its frame"
    );
    click(&pass, &mut app, "Close");
    assert!(!open(&app));
    assert!(!has(&pass.settled(&mut app, width, height), first));

    // Operating: open and closed by the top bar, and the day runs on.
    dispatch_in(&mut app, vec![UiAction::Command(Command::BeginOperating)]);
    update_n(&mut app, 3);
    assert_eq!(phase(&app), PhaseState::Operating);
    click(&pass, &mut app, "Milestones");
    assert!(open(&app));
    assert!(has(&pass.settled(&mut app, width, height), first));
    assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::X1);
    click(&pass, &mut app, "Milestones");
    assert!(!open(&app));

    // Review: over the modal.
    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::AdvanceTicks { n: 1_440 })],
    );
    update_n(&mut app, 3);
    assert_eq!(phase(&app), PhaseState::Review);
    click(&pass, &mut app, "Milestones");
    assert!(open(&app), "the top bar is clickable in Review");
    let widgets = pass.settled(&mut app, width, height);
    assert!(has(&widgets, first) && has(&widgets, "Month in review"));
    // Where the two overlap, the panel is the one on top, even once the modal has been clicked
    // (egui raises an area of the same order on any press).
    let title = widgets
        .iter()
        .find(|w| w.text == "Month in review" && w.role == egui::accesskit::Role::Label)
        .expect("the modal's title")
        .rect;
    let commands = sent::<PlayerCommand>(&app).len();
    pass.click_at(&mut app, width, height, title.center());
    assert_eq!(sent::<PlayerCommand>(&app)[commands..], []);
    assert!(open(&app), "a click on the modal leaves the panel open");
    let widgets = pass.settled(&mut app, width, height);
    let name = widgets
        .iter()
        .find(|w| w.text == first && w.role == egui::accesskit::Role::Label)
        .expect("the first milestone's name")
        .rect;
    assert!(
        name.center().x < (width + REVIEW_WIDTH) / 2.0,
        "{name:?} is over the modal"
    );
    assert_eq!(
        pass.ctx().layer_id_at(name.center()).map(|layer| layer.id),
        Some(egui::Id::new("milestones"))
    );
    click(&pass, &mut app, "Close");
    assert!(!open(&app), "the panel is above the Review modal");
    assert_eq!(phase(&app), PhaseState::Review);
}

/// A month close that ends the game moves to Review too, but only the game-over modal shows.
#[test]
fn a_game_that_ends_at_the_month_close_shows_game_over_and_no_review() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, EASTERN);
    let over = |app: &App| {
        matches!(
            app.world().resource::<Session>().0.session().status(),
            domain::session::SessionStatus::GameOver(_)
        )
    };
    // Committing nothing leaves the fleet offline: a strike a month revokes the licence by the
    // third close.
    for _ in 0..3 {
        dispatch_in(
            &mut app,
            vec![
                UiAction::Command(Command::BeginOperating),
                UiAction::Command(Command::AdvanceTicks { n: 1_440 }),
            ],
        );
        update_n(&mut app, 3);
        if over(&app) {
            break;
        }
        dispatch_in(&mut app, vec![UiAction::Command(Command::FinishReview)]);
        update_n(&mut app, 3);
    }
    assert!(over(&app));
    assert_eq!(phase(&app), PhaseState::Review);
    let pass = EguiPass::attach(&mut app);

    let widgets = pass.settled(&mut app, 1024.0, 640.0);

    assert!(has(&widgets, "Game over"));
    assert!(!find_button(&widgets, "Main menu").disabled);
    assert!(!has(&widgets, "Month in review"));
    assert!(buttons(&widgets, "Continue").is_empty());
}
