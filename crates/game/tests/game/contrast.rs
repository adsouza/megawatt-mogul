//! Contrast checks use an independent WCAG luminance calculation.
use crate::support::usd_style;
use bevy_egui::egui::{self, Color32};
use tempfile::TempDir;

use crate::support::egui_pass::EguiPass;
use crate::support::hud_app;

const MINIMUM: f64 = 4.5;

fn luminance(color: Color32) -> f64 {
    let channel = |value: u8| {
        let s = f64::from(value) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

fn contrast(a: Color32, b: Color32) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn over(foreground: Color32, background: Color32) -> Color32 {
    let alpha = f64::from(foreground.a()) / 255.0;
    let channel = |f: u8, b: u8| (f64::from(f) + f64::from(b) * (1.0 - alpha)).round() as u8;
    Color32::from_rgb(
        channel(foreground.r(), background.r()),
        channel(foreground.g(), background.g()),
        channel(foreground.b(), background.b()),
    )
}

#[test]
fn hud_text_palette_meets_minimum_contrast_even_when_disabled() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    let pass = EguiPass::attach(&mut app);
    pass.run(&mut app, 1280.0, 800.0);
    let style = pass.ctx().global_style();
    let v = &style.visuals;
    let mut surfaces = vec![
        v.panel_fill,
        v.window_fill,
        v.extreme_bg_color,
        v.code_bg_color,
        v.faint_bg_color,
        v.selection.bg_fill,
    ];
    for widget in [
        &v.widgets.noninteractive,
        &v.widgets.inactive,
        &v.widgets.hovered,
        &v.widgets.active,
        &v.widgets.open,
    ] {
        surfaces.extend([widget.bg_fill, widget.weak_bg_fill]);
    }
    for (name, color) in [
        ("normal", v.text_color()),
        ("muted", v.weak_text_color()),
        ("strong", v.strong_text_color()),
        ("warning", v.warn_fg_color),
        ("error", v.error_fg_color),
        ("link", v.hyperlink_color),
        ("selected", v.selection.stroke.color),
    ] {
        for background in &surfaces {
            let background = over(*background, v.panel_fill);
            let foreground = over(color, background);
            assert!(
                contrast(foreground, background) >= MINIMUM,
                "{name}: {foreground:?} on {background:?} has contrast {}",
                contrast(foreground, background)
            );
        }
    }
    // Disabled text is checked as it is really drawn, below: egui's own disabled look is a fade,
    // which would turn this palette into unchecked colours.
    assert_eq!(
        v.disabled_alpha, 1.0,
        "the HUD draws disabled controls itself"
    );
    disabled_buttons_paint_readably_and_unlike_enabled_ones_on(&surfaces, v.panel_fill);
}

/// On each of `surfaces`, a disabled button, a disabled toggle that is on and an enabled button,
/// drawn by the HUD's own button helpers with the HUD theme: every label meets the minimum against
/// what is painted under it, and the disabled button doesn't look like the enabled one.
fn disabled_buttons_paint_readably_and_unlike_enabled_ones_on(
    surfaces: &[Color32],
    panel: Color32,
) {
    use crate::support::egui_harness::{button, widgets_of};
    use game::hud::{ButtonModel, ToggleModel, UiAction};
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ctx.set_visuals(game::hud::theme::visuals());
    let model = |label: &str, enabled| {
        ButtonModel::new(label.to_owned(), UiAction::ToggleMilestones).enabled_if(enabled)
    };
    for surface in surfaces {
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            egui::Frame::new()
                .fill(over(*surface, panel))
                .inner_margin(8.0)
                .show(ui, |ui| {
                    let mut actions = Vec::new();
                    game::hud::button(ui, &model("Offered", true), &mut actions);
                    game::hud::button(ui, &model("Unavailable", false), &mut actions);
                    let toggle = ToggleModel {
                        button: model("Unavailable and on", false),
                        on: true,
                    };
                    game::hud::toggle_button(ui, &toggle, &mut actions);
                });
        });
        let widgets = widgets_of(&output);
        let painted = output.shapes.clone();
        assert_eq!(check_paint(output), 3, "on {surface:?}");
        let enabled = button_look(&painted, button(&widgets, "Offered").rect);
        let disabled = button_look(&painted, button(&widgets, "Unavailable").rect);
        assert!(button(&widgets, "Unavailable").disabled);
        assert_ne!(enabled.text, disabled.text, "on {surface:?}");
        assert_ne!(
            (enabled.fill, enabled.stroke),
            (disabled.fill, disabled.stroke),
            "on {surface:?}"
        );
    }
}

fn flattened<'a>(
    shape: &'a egui::Shape,
    clip: egui::Rect,
    out: &mut Vec<(egui::Rect, &'a egui::Shape)>,
) {
    if let egui::Shape::Vec(children) = shape {
        for child in children {
            flattened(child, clip, out);
        }
    } else {
        out.push((clip, shape));
    }
}

/// One section of painted text: what it says, where it is, its colour and the colour under it.
pub(crate) struct PaintedText {
    pub text: String,
    pub at: egui::Pos2,
    pub color: Color32,
    pub background: Color32,
}

fn shapes_of(painted: &[egui::epaint::ClippedShape]) -> Vec<(egui::Rect, &egui::Shape)> {
    let mut shapes = Vec::new();
    for clipped in painted {
        flattened(&clipped.shape, clipped.clip_rect, &mut shapes);
    }
    shapes
}

/// What a painted text whose whole string is `whole` actually shows: the glyphs egui laid out,
/// so a label cut short to fit reads as its visible part and "…". AccessKit, and the galley's
/// `text()`, keep the whole string. `None` if no such text was painted.
pub(crate) fn painted_glyphs(
    painted: &[egui::epaint::ClippedShape],
    whole: &str,
) -> Option<String> {
    painted_galley(painted, whole).map(|galley| {
        galley
            .rows
            .iter()
            .flat_map(|row| row.glyphs.iter().map(|glyph| glyph.chr))
            .collect()
    })
}

/// The galley of the painted text whose whole string is `whole`: its font and laid-out glyphs.
pub(crate) fn painted_galley(
    painted: &[egui::epaint::ClippedShape],
    whole: &str,
) -> Option<std::sync::Arc<egui::Galley>> {
    shapes_of(painted)
        .into_iter()
        .find_map(|(_, shape)| match shape {
            egui::Shape::Text(text) if text.galley.text() == whole => Some(text.galley.clone()),
            _ => None,
        })
}

/// Every exposed, non-blank text section in `painted`, with the colour of what is under it.
pub(crate) fn painted_texts(painted: &[egui::epaint::ClippedShape]) -> Vec<PaintedText> {
    let shapes = shapes_of(painted);
    let mut texts = Vec::new();
    for (index, (clip, shape)) in shapes.iter().enumerate() {
        let egui::Shape::Text(text) = shape else {
            continue;
        };
        if text.galley.text().trim().is_empty() {
            continue;
        }
        let bounds = text.visual_bounding_rect().intersect(*clip);
        if !bounds.is_positive() {
            continue;
        }
        let at = bounds.center();
        // Text covered by a later popup, modal backdrop or tooltip is not an exposed label.
        if shapes[index + 1..].iter().any(|(clip, shape)| {
            matches!(shape,
            egui::Shape::Rect(rect) if rect.fill.a() > 0 && rect.rect.intersect(*clip).contains(at))
        }) {
            continue;
        }
        let background =
            shapes[..index]
                .iter()
                .fold(
                    Color32::from_gray(27),
                    |background, (clip, shape)| match shape {
                        egui::Shape::Rect(rect) if rect.rect.intersect(*clip).contains(at) => {
                            over(rect.fill, background)
                        }
                        _ => background,
                    },
                );
        for section in &text.galley.job.sections {
            let color = text
                .override_text_color
                .unwrap_or_else(|| {
                    if section.format.color == Color32::PLACEHOLDER {
                        text.fallback_color
                    } else {
                        section.format.color
                    }
                })
                .gamma_multiply(text.opacity_factor);
            if color.a() == 0 {
                continue;
            }
            texts.push(PaintedText {
                text: text.galley.text().to_owned(),
                at,
                color,
                background,
            });
        }
    }
    texts
}

pub(crate) fn check_paint(output: egui::FullOutput) -> usize {
    let painted = output.shapes.clone();
    output.drop_without_applying_deltas();
    let texts = painted_texts(&painted);
    for text in &texts {
        let ratio = contrast(over(text.color, text.background), text.background);
        assert!(
            ratio >= MINIMUM,
            "{:.2}:1 for {:?}: {:?} on {:?} at {:?}",
            ratio,
            text.text,
            text.color,
            text.background,
            text.at
        );
    }
    texts.len()
}

/// How a button at `rect` was painted: its frame's fill and stroke (none if it drew no frame),
/// and its label's colour and the colour under the label.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ButtonLook {
    pub fill: Color32,
    pub stroke: egui::Stroke,
    pub text: Color32,
    pub background: Color32,
}

pub(crate) fn button_look(painted: &[egui::epaint::ClippedShape], rect: egui::Rect) -> ButtonLook {
    // The last frame painted around the button's centre is its own.
    let frame = shapes_of(painted)
        .into_iter()
        .rev()
        .find_map(|(_, shape)| match shape {
            egui::Shape::Rect(frame)
                if rect.expand(1.0).contains_rect(frame.rect)
                    && frame.rect.contains(rect.center()) =>
            {
                Some((frame.fill, frame.stroke))
            }
            _ => None,
        });
    let (fill, stroke) = frame.unwrap_or((Color32::TRANSPARENT, egui::Stroke::NONE));
    let label = painted_texts(painted)
        .into_iter()
        .find(|text| rect.contains(text.at))
        .unwrap_or_else(|| panic!("no label painted in {rect:?}"));
    ButtonLook {
        fill,
        stroke,
        text: label.color,
        background: label.background,
    }
}

#[test]
fn plant_card_colors_distinguish_fast_response_and_costly_generation() {
    use crate::support::{request, shipped_l10n};
    use domain::region::PlantKind;
    use game::hud::{draw_plant_card, plant_card_model};

    let logs = TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, "eastern_interconnection");
    let game = &app.world().resource::<game::sim::Session>().0;
    let l10n = shipped_l10n();
    let mut rendered = Vec::new();
    for kind in [PlantKind::Nuclear, PlantKind::Peaker] {
        let id = game
            .session()
            .plant_views()
            .iter()
            .find(|plant| plant.kind == kind)
            .unwrap()
            .id;
        let card = plant_card_model(game, &l10n, id, &usd_style()).unwrap();
        let ctx = egui::Context::default();
        ctx.set_visuals(game::hud::theme::visuals());
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 1200.0),
                )),
                ..Default::default()
            },
            |ui| {
                draw_plant_card(ui, &card);
            },
        );
        let texts = painted_texts(&output.shapes);
        assert!(check_paint(output) > 0);
        let value_color = |label: &str| {
            let value = &card
                .stats
                .iter()
                .find(|stat| stat.label == label)
                .unwrap()
                .value;
            texts.iter().find(|text| &text.text == value).unwrap().color
        };
        let ramp = value_color("Ramp rate");
        let startup = value_color("Start-up time");
        let startup_cost = value_color("Start-up cost");
        if kind == PlantKind::Nuclear {
            assert!(
                ramp.r() > ramp.g(),
                "slow ramping should use a warning color"
            );
            assert!(
                startup.r() > startup.g(),
                "long starts should use a warning color"
            );
            assert!(
                startup_cost.r() > startup_cost.g(),
                "expensive starts should be red"
            );
        } else {
            assert!(ramp.g() > ramp.r(), "fast ramping should be green");
            assert!(startup.g() > startup.r(), "short starts should be green");
            assert!(
                startup_cost.g() > startup_cost.r(),
                "cheap starts should be green"
            );
        }
        rendered.push(value_color("Marginal cost"));
    }
    assert!(
        rendered[0].g() > rendered[0].r(),
        "cheap generation should be green"
    );
    assert!(
        rendered[1].r() > rendered[1].g(),
        "expensive generation should be red"
    );
    assert!(l10n.problems().is_empty());
}

#[test]
fn planning_and_operating_panels_use_semantic_text_colors() {
    use crate::support::{request, shipped_l10n};
    use domain::{
        session::{Incident, IncidentKind},
        units::Ratio,
    };
    use game::hud::{
        draw_incidents, draw_ramp_warnings, draw_tariff, draw_warnings, incidents_for,
        ramp_warnings_for, tariff_model, warnings_for,
    };
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, "eastern_interconnection");
    let l10n = shipped_l10n();
    let game = &app.world().resource::<game::sim::Session>().0;
    fn render(mut draw: impl FnMut(&mut egui::Ui)) -> Vec<PaintedText> {
        let ctx = egui::Context::default();
        ctx.set_visuals(game::hud::theme::visuals());
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            },
            |ui| draw(ui),
        );
        let texts = painted_texts(&output.shapes);
        assert!(check_paint(output) > 0);
        texts
    }
    let color = |texts: &[PaintedText], label: &str| {
        texts
            .iter()
            .find(|text| text.text == label)
            .unwrap_or_else(|| panic!("missing {label}"))
            .color
    };
    let warnings = warnings_for(None, None, &[], false, &l10n);
    let texts = render(|ui| {
        draw_warnings(ui, &warnings);
    });
    let heading = color(&texts, &warnings.heading);
    assert!(
        heading.b() > heading.r(),
        "Planning headings need the blue accent"
    );
    let healthy = color(&texts, warnings.none.as_ref().unwrap());
    assert!(healthy.g() > healthy.r(), "a sound plan is good news");

    let tariff = tariff_model(game, &l10n, &usd_style());
    let texts = render(|ui| {
        draw_tariff(ui, &tariff);
    });
    let value = color(&texts, &tariff.current[0].value);
    assert!(value.g() > value.r(), "tariff figures need the teal accent");
    assert_ne!(value, color(&texts, &tariff.current[0].label));

    let ramp = ramp_warnings_for(None, 0, &l10n);
    let texts = render(|ui| {
        draw_ramp_warnings(ui, &ramp);
    });
    let healthy = color(&texts, ramp.empty.as_ref().unwrap());
    assert!(healthy.g() > healthy.r(), "no ramp warning is good news");

    let incidents = incidents_for(
        &[
            Incident {
                tick: 1,
                kind: IncidentKind::Blackout,
            },
            Incident {
                tick: 2,
                kind: IncidentKind::RotatingOutage {
                    share: Ratio::new(0.1).unwrap(),
                },
            },
            Incident {
                tick: 3,
                kind: IncidentKind::BlackoutRestored,
            },
        ],
        &game::l10n::Names::empty(&l10n),
        &l10n,
    );
    let texts = render(|ui| {
        draw_incidents(ui, &incidents);
    });
    let restored = color(&texts, &incidents.rows[0]);
    let outage = color(&texts, &incidents.rows[1]);
    let blackout = color(&texts, &incidents.rows[2]);
    assert!(restored.g() > restored.r());
    assert!(outage.r() > outage.b());
    assert!(blackout.r() > blackout.g());
    assert_ne!(outage, blackout);
}

#[test]
fn painted_planning_text_keeps_contrast_including_chart_ticks() {
    use crate::support::request;
    use game::hud::{PlanningTab, UiState};
    for region in ["eastern_interconnection", "western_interconnection"] {
        let logs = TempDir::new().unwrap();
        let mut app = hud_app(logs.path());
        let pass = EguiPass::attach(&mut app);
        request(&mut app, region);
        for tab in [
            PlanningTab::Commitments,
            PlanningTab::Forecast,
            PlanningTab::Tariff,
            PlanningTab::Construction,
        ] {
            app.world_mut().resource_mut::<UiState>().tab = tab;
            for (w, h) in [(1024.0, 640.0), (1280.0, 800.0), (1600.0, 1000.0)] {
                pass.settled(&mut app, w, h);
                let output = pass.paint_with(&mut app, w, h, vec![]);
                assert!(check_paint(output) > 20);
            }
        }
    }
}

proptest::proptest! {
    #[test]
    fn adaptive_text_colors_meet_the_floor_for_any_opaque_pair(
        fg in proptest::prelude::any::<[u8; 3]>(),
        bg in proptest::prelude::any::<[u8; 3]>(),
    ) {
        let fg = Color32::from_rgb(fg[0], fg[1], fg[2]);
        let bg = Color32::from_rgb(bg[0], bg[1], bg[2]);
        let adjusted = game::hud::theme::readable_color(fg, bg);
        proptest::prop_assert!(contrast(adjusted, bg) >= MINIMUM);
        proptest::prop_assert!(contrast(game::hud::theme::contrasting_text(bg), bg) >= MINIMUM);
        if contrast(fg, bg) >= MINIMUM { proptest::prop_assert_eq!(adjusted, fg); }
        let backing = game::hud::theme::LABEL_BACKING;
        proptest::prop_assert_eq!(backing.a(), 255);
        proptest::prop_assert!(contrast(game::hud::theme::LABEL_TEXT, over(backing, bg)) >= MINIMUM);
    }
}

fn audit(app: &mut bevy::prelude::App, pass: &EguiPass) {
    pass.settled(app, 1280.0, 800.0);
    assert!(check_paint(pass.paint_with(app, 1280.0, 800.0, vec![])) > 0);
}

#[test]
fn menus_operating_review_and_prompts_use_readable_text() {
    use crate::support::{request, update_n};
    use application::Command;
    use bevy::prelude::*;
    use game::hud::{MilestonesOpen, OpenPrompt, OperatingTab, Prompt};
    use game::sim::Session;
    use game::state::AppState;
    for region in ["eastern_interconnection", "western_interconnection"] {
        let logs = TempDir::new().unwrap();
        let mut app = hud_app(logs.path());
        let pass = EguiPass::attach(&mut app);
        audit(&mut app, &pass);
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::RegionSelect);
        update_n(&mut app, 1);
        audit(&mut app, &pass);
        request(&mut app, region);
        app.world_mut()
            .resource_mut::<Session>()
            .0
            .execute(Command::BeginOperating)
            .unwrap();
        app.world_mut()
            .resource_mut::<Session>()
            .0
            .execute(Command::AdvanceTicks { n: 180 })
            .unwrap();
        update_n(&mut app, 2);
        for tab in [OperatingTab::Supply, OperatingTab::Hydro] {
            *app.world_mut().resource_mut::<OperatingTab>() = tab;
            audit(&mut app, &pass);
        }
        app.world_mut()
            .resource_mut::<Session>()
            .0
            .execute(Command::AdvanceTicks { n: 1440 })
            .unwrap();
        update_n(&mut app, 2);
        audit(&mut app, &pass);
        app.world_mut().resource_mut::<MilestonesOpen>().0 = true;
        audit(&mut app, &pass);
        app.world_mut().resource_mut::<MilestonesOpen>().0 = false;
        app.world_mut().resource_mut::<OpenPrompt>().0 = Some(Prompt::LeaveGame);
        audit(&mut app, &pass);
    }
}

#[test]
fn semantic_status_text_stays_readable_including_emergencies_and_reached_milestones() {
    use crate::support::{request, shipped_l10n};
    use game::hud::{
        HIGH_COLOR, LOW_COLOR, NORMAL_COLOR, SHEDDING_COLOR, draw_frequency, draw_milestones,
        frequency_model, milestones_model,
    };
    use game::sim::Session;
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, "eastern_interconnection");
    let game = &app.world().resource::<Session>().0;
    let l10n = shipped_l10n();
    let mut frequency = frequency_model(game, &l10n);
    frequency.blackout = true;
    frequency.mode = "Blackout".into();
    frequency.shed = Some("Protection has shed 5% of the load".into());
    let mut milestones = milestones_model(game, &l10n);
    milestones.rows[0] = game::hud::milestone_row(
        application::projections::MILESTONES[0],
        application::projections::MilestoneStatus::Reached {
            month: game.session().clock().month(),
        },
        &l10n,
    );
    let ctx = egui::Context::default();
    ctx.set_visuals(game::hud::theme::visuals());
    for color in [NORMAL_COLOR, LOW_COLOR, HIGH_COLOR, SHEDDING_COLOR] {
        frequency.dial.color = color;
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            draw_frequency(ui, &frequency);
            draw_milestones(ui, &milestones);
            let unavailable = game::hud::ButtonModel::new(
                "Unavailable action".into(),
                game::hud::UiAction::ToggleMilestones,
            )
            .enabled_if(false);
            game::hud::button(ui, &unavailable, &mut Vec::new());
        });
        let texts = painted_texts(&output.shapes);
        assert!(check_paint(output) > 5);
        let color = |label: &str| texts.iter().find(|text| text.text == label).unwrap().color;
        let reached = &milestones.rows[0];
        let green = color(&reached.status);
        assert!(green.g() > green.r(), "reached milestones should be green");
        assert_eq!(color(&reached.name), green);
        assert_eq!(color(&reached.detail), green);
        let progress = &milestones.rows[1];
        let heading = color(&progress.name);
        assert!(
            heading.b() > heading.r(),
            "milestone headings need the blue accent"
        );
        assert_ne!(color(&progress.status), green);
        assert_ne!(color(&progress.detail), green);
    }
}

#[test]
fn map_glyphs_and_construction_labels_have_opaque_backings() {
    use crate::support::request;
    use application::Command;
    use domain::region::PlantKind;
    use game::sim::Session;
    for region in ["eastern_interconnection", "western_interconnection"] {
        let logs = TempDir::new().unwrap();
        let mut app = hud_app(logs.path());
        request(&mut app, region);
        let mut session = app.world_mut().resource_mut::<Session>();
        let definition = session.0.session().region();
        let terrain = definition.terrain();
        let cell = (0..terrain.height())
            .flat_map(|y| (0..terrain.width()).map(move |x| domain::ids::CellCoord { x, y }))
            .find(|cell| {
                definition.site_check(PlantKind::Peaker, *cell).is_ok()
                    && !session
                        .0
                        .session()
                        .plant_views()
                        .iter()
                        .any(|plant| plant.cell == *cell)
            })
            .unwrap();
        session
            .0
            .execute(Command::BuildPlant {
                kind: PlantKind::Peaker,
                cell,
            })
            .unwrap();
        let pass = EguiPass::attach(&mut app);
        pass.settled(&mut app, 3200.0, 1800.0);
        let output = pass.paint_with(&mut app, 3200.0, 1800.0, vec![]);
        let painted = output.shapes.clone();
        output.drop_without_applying_deltas();
        let mut shapes = Vec::new();
        for clipped in &painted {
            flattened(&clipped.shape, clipped.clip_rect, &mut shapes);
        }
        let (mut glyphs, mut projects) = (0, 0);
        for (i, (_, shape)) in shapes.iter().enumerate() {
            let egui::Shape::Text(text) = shape else {
                continue;
            };
            let label = text.galley.text();
            if !["N", "C", "G", "P", "S", "H"].contains(&label) && !label.ends_with(" mo") {
                continue;
            }
            let egui::Shape::Rect(backing) = shapes[i - 1].1 else {
                panic!("{label} has no backing")
            };
            assert_eq!(backing.fill, game::hud::theme::LABEL_BACKING);
            assert!(backing.rect.contains_rect(text.visual_bounding_rect()));
            if label.ends_with(" mo") {
                projects += 1;
            } else {
                glyphs += 1;
            }
        }
        assert!(
            glyphs > 0 && projects > 0,
            "{region}: {glyphs} glyphs, {projects} projects"
        );
    }
}

/// In the real pass, a plant row's *All day* and *Off* are drawn alike, side by side, and only one
/// of them is offered: the other must look unavailable, and its label must stay readable.
#[test]
fn a_disabled_button_paints_differently_from_an_enabled_one() {
    use crate::support::egui_harness::buttons;
    use crate::support::request;
    use game::hud::{PlanningTab, UiState};
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    let pass = EguiPass::attach(&mut app);
    request(&mut app, "eastern_interconnection");
    app.world_mut().resource_mut::<UiState>().tab = PlanningTab::Commitments;
    let (width, height) = (1280.0, 800.0);
    let widgets = pass.settled(&mut app, width, height);
    let (enabled, disabled) = buttons(&widgets, "All day")
        .into_iter()
        .find_map(|all_day| {
            let off = buttons(&widgets, "Off")
                .into_iter()
                .find(|off| (off.rect.top() - all_day.rect.top()).abs() < 1.0)?;
            match (all_day.disabled, off.disabled) {
                (false, true) => Some((all_day.rect, off.rect)),
                (true, false) => Some((off.rect, all_day.rect)),
                _ => None,
            }
        })
        .expect("a plant row offers one of All day and Off");
    assert!(
        (enabled.height() - disabled.height()).abs() < 0.5,
        "laid out alike"
    );
    let output = pass.paint_with(&mut app, width, height, vec![]);
    let painted = output.shapes.clone();
    output.drop_without_applying_deltas();
    let (enabled, disabled) = (
        button_look(&painted, enabled),
        button_look(&painted, disabled),
    );
    assert_ne!(enabled.text, disabled.text, "{enabled:?} / {disabled:?}");
    assert_ne!(
        (enabled.fill, enabled.stroke),
        (disabled.fill, disabled.stroke),
        "{enabled:?} / {disabled:?}"
    );
    let ratio = contrast(
        over(disabled.text, disabled.background),
        disabled.background,
    );
    assert!(ratio >= MINIMUM, "{ratio:.2}:1 for {disabled:?}");
}

/// How a slider at `rect` was painted: its rail's fill, and its handle's fill and outline.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SliderLook {
    rail: Color32,
    handle: Color32,
    handle_stroke: egui::Stroke,
}

fn slider_look(painted: &[egui::epaint::ClippedShape], rect: egui::Rect) -> SliderLook {
    // The rail, then the handle: the theme's handle is a rectangle on the rail.
    let parts: Vec<(Color32, egui::Stroke)> = shapes_of(painted)
        .into_iter()
        .filter_map(|(_, shape)| match shape {
            egui::Shape::Rect(part) if rect.expand(1.0).contains_rect(part.rect) => {
                Some((part.fill, part.stroke))
            }
            _ => None,
        })
        .collect();
    let [(rail, _), (handle, handle_stroke)] = parts[..] else {
        panic!("a rail and a handle in {rect:?}, not {parts:?}");
    };
    SliderLook {
        rail,
        handle,
        handle_stroke,
    }
}

/// In the real pass, a plant's target slider is offered until AGC takes the target over: then it
/// must look unavailable, in its rail or its handle, and its label must stay readable.
#[test]
fn a_disabled_slider_paints_differently_from_an_enabled_one() {
    use crate::hud_smoke::{EASTERN, dispatch_in, operating_hour_app};
    use crate::support::update_n;
    use application::Command;
    use bevy::prelude::*;
    use game::hud::{Selection, UiAction};
    use game::sim::Session;
    let (width, height) = (1280.0, 800.0);
    let logs = TempDir::new().unwrap();
    let mut app = operating_hour_app(&logs, EASTERN);
    let pass = EguiPass::attach(&mut app);
    let plant = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .plant_views()
        .into_iter()
        .find(|plant| {
            !plant.auto_follow
                && matches!(plant.status, domain::generation::PlantStatus::Online { .. })
        })
        .expect("an online plant off AGC")
        .id;
    *app.world_mut().resource_mut::<Selection>() = Selection::Plant(plant);
    let look = |app: &mut App, enabled: bool| {
        let widgets = pass.settled(app, width, height);
        let slider = widgets
            .iter()
            .find(|widget| widget.role == egui::accesskit::Role::Slider)
            .expect("the target slider is drawn");
        assert_eq!(slider.disabled, !enabled);
        let rect = slider.rect;
        let output = pass.paint_with(app, width, height, vec![]);
        let painted = output.shapes.clone();
        output.drop_without_applying_deltas();
        let label = painted_texts(&painted)
            .into_iter()
            .find(|text| text.text == "Set target")
            .expect("the slider's label is painted");
        let ratio = contrast(over(label.color, label.background), label.background);
        assert!(ratio >= MINIMUM, "{ratio:.2}:1 for the slider's label");
        (slider_look(&painted, rect), label.color)
    };
    let enabled = look(&mut app, true);
    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::SetAutoFollow {
            plant,
            on: true,
        })],
    );
    update_n(&mut app, 2);
    let disabled = look(&mut app, false);
    assert_ne!(enabled.1, disabled.1, "the label is muted");
    assert!(
        enabled.0.rail != disabled.0.rail || enabled.0.handle != disabled.0.handle,
        "{enabled:?} / {disabled:?}"
    );
}

/// A control in a `Ui` an ancestor disabled is disabled too, and must look it: like a control
/// disabled itself, not like an enabled one.
#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "disables an ancestor `Ui` through egui on purpose, to see the HUD helper cope"
)]
fn a_control_in_a_disabled_ui_looks_disabled() {
    use crate::support::egui_harness::{button, widgets_of};
    use game::hud::{ButtonModel, UiAction};
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ctx.set_visuals(game::hud::theme::visuals());
    let model = |label: &str, enabled| {
        ButtonModel::new(label.to_owned(), UiAction::ToggleMilestones).enabled_if(enabled)
    };
    let output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let mut actions = Vec::new();
        game::hud::button(ui, &model("Offered", true), &mut actions);
        game::hud::button(ui, &model("Unavailable", false), &mut actions);
        ui.scope_builder(egui::UiBuilder::new().disabled(), |ui| {
            game::hud::button(ui, &model("In a disabled ui", true), &mut actions);
        });
    });
    let widgets = widgets_of(&output);
    let painted = output.shapes.clone();
    assert_eq!(check_paint(output), 3);
    let look = |label| button_look(&painted, button(&widgets, label).rect);
    assert!(button(&widgets, "In a disabled ui").disabled);
    assert_ne!(look("In a disabled ui"), look("Offered"));
    assert_eq!(
        ButtonLook {
            background: look("Unavailable").background,
            ..look("In a disabled ui")
        },
        look("Unavailable")
    );
}

/// A selected toggle that can't be used now must not look like one that can: its selection fill
/// is dimmed, not only its text and outline. It still looks on, unlike a disabled toggle that is
/// off, and its label meets the minimum on the dimmed fill.
#[test]
fn a_disabled_selected_toggle_paints_differently_from_an_enabled_selected_one() {
    use crate::support::egui_harness::{button, widgets_of};
    use game::hud::{ButtonModel, ToggleModel, UiAction};
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ctx.set_visuals(game::hud::theme::visuals());
    let toggle = |label: &str, enabled, on| ToggleModel {
        button: ButtonModel::new(label.to_owned(), UiAction::ToggleMilestones).enabled_if(enabled),
        on,
    };
    let output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let mut actions = Vec::new();
        for model in [
            toggle("Offered and on", true, true),
            toggle("Unavailable and on", false, true),
            toggle("Unavailable and off", false, false),
        ] {
            game::hud::toggle_button(ui, &model, &mut actions);
        }
    });
    let widgets = widgets_of(&output);
    let painted = output.shapes.clone();
    // Every label, the dimmed one included, meets the minimum against what is under it.
    assert_eq!(check_paint(output), 3);
    let look = |label| button_look(&painted, button(&widgets, label).rect);
    let (enabled, disabled, off) = (
        look("Offered and on"),
        look("Unavailable and on"),
        look("Unavailable and off"),
    );
    assert!(button(&widgets, "Unavailable and on").disabled);
    assert!(button(&widgets, "Unavailable and on").selected);
    assert_ne!(enabled.fill, disabled.fill, "{enabled:?} / {disabled:?}");
    assert_ne!(disabled.fill, off.fill, "still on: {disabled:?} / {off:?}");
    assert_eq!(
        disabled.background, disabled.fill,
        "the label sits on the dimmed fill"
    );
}
