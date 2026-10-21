//! Contrast checks use an independent WCAG luminance calculation.
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
            for alpha in [1.0, v.disabled_alpha] {
                let foreground = over(color.gamma_multiply(alpha), background);
                assert!(
                    contrast(foreground, background) >= MINIMUM,
                    "{name}, alpha {alpha}: {foreground:?} on {background:?} has contrast {}",
                    contrast(foreground, background)
                );
            }
        }
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

fn check_paint(output: egui::FullOutput) -> usize {
    let painted = output.shapes.clone();
    output.drop_without_applying_deltas();
    let mut shapes = Vec::new();
    for clipped in &painted {
        flattened(&clipped.shape, clipped.clip_rect, &mut shapes);
    }
    let mut checked = 0;
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
            let ratio = contrast(over(color, background), background);
            assert!(
                ratio >= MINIMUM,
                "{:.2}:1 for {:?}: {color:?} on {background:?} at {at:?}",
                ratio,
                text.galley.text()
            );
            checked += 1;
        }
    }
    checked
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
    milestones.rows[0].reached = true;
    let ctx = egui::Context::default();
    ctx.set_visuals(game::hud::theme::visuals());
    for color in [NORMAL_COLOR, LOW_COLOR, HIGH_COLOR, SHEDDING_COLOR] {
        frequency.dial.color = color;
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            draw_frequency(ui, &frequency);
            draw_milestones(ui, &milestones);
            ui.add_enabled(false, egui::Button::new("Unavailable action"));
        });
        assert!(check_paint(output) > 5);
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
