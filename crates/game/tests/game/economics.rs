//! Local quotes through real widgets, dispatch, construction and save/resume.
use crate::{
    hud_smoke::dispatch_in,
    support::{
        self,
        egui_harness::{Harness, widget},
        egui_pass::EguiPass,
    },
};
use application::{Command, Game, InMemoryCommandLog};
use domain::{
    ids::{CellCoord, RegionId},
    region::PlantKind,
    session::GameSettings,
    units::Currency,
};
use game::{
    hud::{self, Selection, UiAction},
    l10n::{
        Localizer,
        format::{MoneyStyle, money},
    },
    sim::Session,
};
use infrastructure::region::RonRegionCatalog;

fn fixture() -> (tempfile::TempDir, Game, Localizer) {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("minimal.ron"),
        include_bytes!("../../../infrastructure/tests/support/economic_region_v4.ron"),
    )
    .unwrap();
    let game = Game::start_new(
        &RonRegionCatalog::new(dir.path()),
        &RegionId::new("minimal"),
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    let mut sources = std::fs::read_dir(support::shipped_assets().join("locales/en-US"))
        .unwrap()
        .map(|p| {
            let p = p.unwrap().path();
            (
                p.file_name().unwrap().to_string_lossy().to_string(),
                std::fs::read_to_string(p).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    sources.push((
        "economic-names.ftl".into(),
        include_str!("../fixtures/economic-names.ftl").into(),
    ));
    (dir, game, Localizer::from_sources(sources).unwrap())
}

#[test]
fn local_build_widgets_and_real_construction_card_show_the_order_terms() {
    let (dir, game, l10n) = fixture();
    let style = MoneyStyle::resolve(Currency::Eur, &l10n, |_| true);
    for (cell, name, cost, months) in [
        (
            CellCoord { x: 4, y: 4 },
            "Western construction market",
            100_000_000,
            2,
        ),
        (
            CellCoord { x: 8, y: 5 },
            "Eastern construction market",
            200_000_000,
            3,
        ),
    ] {
        let quote = game.session().plant_quote(PlantKind::Wind, cell).unwrap();
        let mut model = hud::build_model(&game, &l10n, Selection::Cell(cell), &style).unwrap();
        let hud::PlantSection::Kinds { heading, rows, .. } = &mut model.plant else {
            panic!("site selected")
        };
        assert!(heading.contains(name));
        rows.retain(|r| r.kind == PlantKind::Wind);
        let row = &rows[0];
        assert!(
            row.stats
                .iter()
                .any(|s| s.value == money(domain::units::Money::from_cents(cost), &style))
        );
        assert!(
            row.stats
                .iter()
                .any(|s| s.value == format!("{months} months"))
        );
        assert_eq!(quote.capital.cents(), cost);
        let harness = Harness::with_screen(1024.0, 640.0);
        let widgets = harness.draw(|ui| hud::draw_build(ui, &model));
        assert!(!widget(&widgets, "Build plant").disabled);
        let actions = harness.click("Build plant", |ui| hud::draw_build(ui, &model));
        assert_eq!(
            actions,
            vec![UiAction::Command(Command::BuildPlant {
                kind: PlantKind::Wind,
                cell
            })]
        );
    }
    let mut app = support::hud_app(&dir.path().join("logs"));
    support::request(&mut app, "eastern_interconnection");
    app.world_mut().insert_resource(Session(game));
    app.world_mut().insert_resource(l10n);
    let cell = CellCoord { x: 4, y: 4 };
    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::BuildPlant {
            kind: PlantKind::Wind,
            cell,
        })],
    );
    support::update_n(&mut app, 2);
    let id = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .plant_views()
        .into_iter()
        .find(|p| p.cell == cell)
        .unwrap()
        .id;
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Plant(id))]);
    support::update_n(&mut app, 2);
    let pass = EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    let capital = money(domain::units::Money::from_cents(100_000_000), &style);
    for expected in [
        "Western construction market",
        capital.as_str(),
        "2 of 2 months left",
    ] {
        assert!(
            widgets.iter().any(|w| w.text.contains(expected)),
            "missing {expected}"
        );
    }
    let output = pass.paint_with(&mut app, 1024.0, 640.0, vec![]);
    assert!(crate::contrast::check_paint(output) > 0);
}

#[test]
fn cross_zone_line_preview_and_owned_card_keep_the_exact_breakdown() {
    let (_dir, mut game, l10n) = fixture();
    let style = MoneyStyle::resolve(Currency::Eur, &l10n, |_| true);
    for cell in [CellCoord { x: 4, y: 4 }, CellCoord { x: 8, y: 5 }] {
        game.execute(Command::BuildPlant {
            kind: PlantKind::Wind,
            cell,
        })
        .unwrap();
    }
    let plants: Vec<_> = game
        .session()
        .plant_views()
        .into_iter()
        .filter(|p| p.kind == PlantKind::Wind)
        .collect();
    let quote = game
        .session()
        .line_quote(plants[0].node, plants[1].node)
        .unwrap();
    assert_eq!(
        (quote.cells, quote.capital.cents(), quote.months),
        (5, 270_000_000, 2)
    );
    let model =
        hud::build_model(&game, &l10n, Selection::LineFrom(plants[0].node), &style).unwrap();
    let hud::LineSection::End { targets, .. } = model.line else {
        panic!("line preview")
    };
    let target = targets.iter().find(|t| t.node == plants[1].node).unwrap();
    let hud::LinePreview::Quote(stats) = &target.preview else {
        panic!("valid quote")
    };
    for (name, units, cents) in [
        ("Western construction market", 2, 60_000_000),
        ("Eastern construction market", 3, 210_000_000),
    ] {
        let stat = stats.iter().find(|s| s.label == name).unwrap();
        assert!(stat.value.contains(&format!("{units} cells")));
        assert!(
            stat.value
                .contains(&money(domain::units::Money::from_cents(cents), &style))
        );
        assert!(stat.value.contains("6/6"));
    }
    game.execute(Command::BuildLine {
        from: plants[0].node,
        to: plants[1].node,
    })
    .unwrap();
    let line = game.session().line_views().into_iter().last().unwrap().id;
    let model = hud::inspector_model(&game, &l10n, Selection::Line(line), &style).unwrap();
    let hud::InspectorModel::Line(card) = model else {
        panic!("owned line")
    };
    for stat in stats.iter().skip(1) {
        assert!(card.construction.contains(stat));
    }
}

#[test]
fn economic_market_names_fit_the_minimum_inspector_at_enlarged_zoom() {
    let (dir, game, l10n) = fixture();
    let mut app = support::hud_app(&dir.path().join("logs"));
    support::request(&mut app, "eastern_interconnection");
    app.world_mut().insert_resource(Session(game));
    app.world_mut().insert_resource(l10n);
    let pass = EguiPass::attach(&mut app);
    let cell = CellCoord { x: 4, y: 4 };
    for zoom in [1.0, 1.25] {
        pass.ctx().set_zoom_factor(zoom);
        dispatch_in(&mut app, vec![UiAction::Select(Selection::Cell(cell))]);
        support::update_n(&mut app, 2);
        let widgets = pass.settled(&mut app, 1024.0 / zoom, 640.0 / zoom);
        let markets: Vec<_> = widgets
            .iter()
            .filter(|w| {
                w.text == "Western construction market"
                    && w.rect.left() < game::hud::INSPECTOR_PANEL_WIDTH
            })
            .collect();
        assert!(!markets.is_empty());
        for market in markets {
            assert!(
                market.rect.right() <= game::hud::INSPECTOR_PANEL_WIDTH + 1.0,
                "market clipped: {:?}",
                market.rect
            );
        }
    }
}
