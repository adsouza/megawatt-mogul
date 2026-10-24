//! Construction progress (spec §16.9): what is being built, how far along it is, and the month it
//! enters service.
//!
//! - **The Construction tab**, the fourth of Planning's bottom panel: one row per plant or line
//!   under construction, soonest in service first. Its label counts the projects, so the player
//!   sees them without opening it.
//! - **The construction card**, for a selected plant under construction: in Planning's inspector
//!   and in Operating's selected-object block, where plant details and the city card show.
//!
//! Everything is read from the public views: the months left from each plant's and line's
//! status, the total from the kind's `build_months` or the line's build time, and the in-service
//! month from the current month by the month close's own rule ([`in_service_month`]).

use application::Game;
use bevy_egui::egui;
use domain::generation::PlantStatus;
use domain::grid::LineStatus;
use domain::ids::{LineId, PlantId};
use domain::session::DayPhase;
use domain::time::SimMonth;

use super::theme::{TextTone, heading_color};
use super::top_bar::Stat;
use crate::l10n::{Localizer, Names, format};

/// A project under construction. Plants sort before lines, each by id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Project {
    Plant(PlantId),
    Line(LineId),
}

/// One project on the Construction tab.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstructionRow {
    pub project: Project,
    /// `Gas peaker 12`, or `Line 9: Pittsburgh to Columbus`.
    pub name: String,
    pub months_left: u32,
    /// The whole build time, in months.
    pub total: u32,
    /// `3 of 6 months left`.
    pub months: String,
    /// The share built: `(total − left) / total`, for the progress bar.
    pub progress: f32,
    /// The first month the project is in service.
    pub in_service: SimMonth,
    /// `In service from June 2027`.
    pub in_service_text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstructionModel {
    /// Soonest in service first, then plants before lines, then by id.
    pub rows: Vec<ConstructionRow>,
    /// The one line the tab shows when nothing is under construction.
    pub empty: Option<String>,
}

/// The first month a project with `months_left` is in service, seen in `phase` of `month`.
///
/// The month close counts construction down by one ([`domain::session`]'s
/// `advance_construction`) and the project commissions at the close that reaches zero; it is in
/// service from the month after. Until the close of `month` has run (Planning, Operating) that is
/// `months_left` closes from now, so `month + months_left`. In Review the close of `month` has
/// already counted its month, so one more.
pub fn in_service_month(phase: DayPhase, month: SimMonth, months_left: u32) -> SimMonth {
    let closes = if phase == DayPhase::Review {
        months_left + 1
    } else {
        months_left
    };
    (0..closes).fold(month, |month, _| month.next())
}

/// How far along one project is, read from the views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Progress {
    project: Project,
    months_left: u32,
    total: u32,
    in_service: SimMonth,
}

impl Progress {
    /// The share built, `(total − left) / total`, in `0..=1`.
    fn share(&self) -> f32 {
        if self.total == 0 {
            return 1.0;
        }
        let built = self.total.saturating_sub(self.months_left);
        (built as f32 / self.total as f32).clamp(0.0, 1.0)
    }

    fn months(&self, l10n: &Localizer) -> String {
        l10n.args(
            "construction-months-left",
            &[
                ("left", self.months_left.into()),
                ("total", self.total.into()),
            ],
        )
    }

    fn in_service(&self, l10n: &Localizer) -> String {
        l10n.args(
            "construction-in-service",
            &[("month", format::month_year(self.in_service).into())],
        )
    }
}

/// Every plant and line under construction, in tab order: soonest in service, then plants before
/// lines, then by id.
fn projects(game: &Game) -> Vec<Progress> {
    let session = game.session();
    let (phase, month) = (session.phase(), session.clock().month());
    let progress = |project, months_left, total| Progress {
        project,
        months_left,
        total,
        in_service: in_service_month(phase, month, months_left),
    };
    let plants = session.plant_views().into_iter().filter_map(|plant| {
        let PlantStatus::UnderConstruction { months_left } = plant.status else {
            return None;
        };
        let total = session.region().spec(plant.kind).build_months;
        Some(progress(Project::Plant(plant.id), months_left, total))
    });
    let lines = session.line_views().into_iter().filter_map(|line| {
        let LineStatus::UnderConstruction { months_left } = line.status else {
            return None;
        };
        let total = session.line_build_months(line.id).unwrap_or(months_left);
        Some(progress(Project::Line(line.id), months_left, total))
    });
    let mut all: Vec<Progress> = plants.chain(lines).collect();
    all.sort_by_key(|p| (p.in_service, p.project));
    all
}

/// Every project under construction, in tab order.
pub fn construction_model(game: &Game, l10n: &Localizer) -> ConstructionModel {
    let session = game.session();
    let names = Names::new(l10n, session);
    let lines = session.line_views();
    let name = |project| match project {
        Project::Plant(plant) => names.plant(plant),
        Project::Line(line) => match lines.iter().find(|l| l.id == line) {
            Some(view) => l10n.args(
                "construction-line-name",
                &[
                    ("line", names.line(line).into()),
                    ("from", names.node(view.from).into()),
                    ("to", names.node(view.to).into()),
                ],
            ),
            None => names.line(line),
        },
    };
    let rows: Vec<ConstructionRow> = projects(game)
        .into_iter()
        .map(|p| ConstructionRow {
            project: p.project,
            name: name(p.project),
            months_left: p.months_left,
            total: p.total,
            months: p.months(l10n),
            progress: p.share(),
            in_service: p.in_service,
            in_service_text: p.in_service(l10n),
        })
        .collect();
    ConstructionModel {
        empty: rows.is_empty().then(|| l10n.text("construction-empty")),
        rows,
    }
}

/// The tab's label: `Construction (2)`, or `Construction` with nothing under construction.
pub fn construction_tab_label(game: &Game, l10n: &Localizer) -> String {
    match projects(game).len() {
        0 => l10n.text("construction-title"),
        count => l10n.args(
            "construction-title-count",
            &[("count", count.to_string().into())],
        ),
    }
}

/// The card of a selected plant under construction.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstructionCardModel {
    pub plant: PlantId,
    /// `Gas peaker 12`: the kind's name and the plant's number.
    pub name: String,
    /// `Under construction`.
    pub status: String,
    /// Capacity, actual grid connection, capital cost and approximate monthly payment.
    pub stats: Vec<Stat>,
    /// `3 of 12 months left`.
    pub months: String,
    /// The share built, for the progress bar.
    pub progress: f32,
    /// `In service from June 2027`.
    pub in_service: String,
}

/// The card for `plant`; `None` for a plant that isn't under construction, or isn't known.
pub fn construction_card_model(
    game: &Game,
    l10n: &Localizer,
    plant: PlantId,
) -> Option<ConstructionCardModel> {
    let session = game.session();
    let view = session.plant_views().into_iter().find(|p| p.id == plant)?;
    let progress = projects(game)
        .into_iter()
        .find(|p| p.project == Project::Plant(plant))?;
    let mut model = ConstructionCardModel {
        plant,
        name: Names::new(l10n, session).plant(plant),
        status: l10n.text("construction-card-status"),
        stats: vec![
            Stat {
                label: l10n.text("build-capacity"),
                value: format::megawatts(session.region().spec(view.kind).capacity),
            },
            super::plant_panel::grid_connection_stat(l10n, view.connected),
            Stat {
                label: l10n.text("build-capital"),
                value: format::money(session.region().spec(view.kind).capital_cost),
            },
            Stat {
                label: l10n.text("build-monthly-payment"),
                value: super::build::monthly_payment(
                    session.region().spec(view.kind).capital_cost,
                    session.region().spec(view.kind).build_months,
                ),
            },
        ],
        months: progress.months(l10n),
        progress: progress.share(),
        in_service: progress.in_service(l10n),
    };
    if let Some(b) = session.region().spec(view.kind).battery {
        model.stats.extend([
            Stat {
                label: l10n.text("battery-charge-rating"),
                value: format::megawatts(b.charge_power),
            },
            Stat {
                label: l10n.text("battery-energy-rating"),
                value: format::megawatt_hours(b.energy_capacity),
            },
        ]);
    }
    Some(model)
}

/// The tab's body: the rows in a grid that scrolls when there are more than fit, or the one line
/// that says nothing is being built. It has no controls.
pub fn draw_construction(ui: &mut egui::Ui, model: &ConstructionModel) {
    if let Some(empty) = &model.empty {
        ui.weak(empty.as_str());
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("construction_rows")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("construction_grid")
                .num_columns(4)
                .spacing(egui::vec2(16.0, 4.0))
                .show(ui, |ui| {
                    for row in &model.rows {
                        ui.colored_label(heading_color(), &row.name);
                        progress_bar(ui, row.progress);
                        ui.colored_label(TextTone::Information.color(), &row.months);
                        ui.colored_label(TextTone::Caution.color(), &row.in_service_text);
                        ui.end_row();
                    }
                });
        });
}

/// The width of a progress bar, on the tab and on the card.
const PROGRESS_WIDTH: f32 = 120.0;

fn progress_bar(ui: &mut egui::Ui, share: f32) {
    ui.add(egui::ProgressBar::new(share).desired_width(PROGRESS_WIDTH));
}

/// The card: the name as a heading, its status, the capacity, then the progress. No controls.
pub fn draw_construction_card(ui: &mut egui::Ui, model: &ConstructionCardModel) {
    ui.heading(egui::RichText::new(&model.name).color(heading_color()));
    ui.colored_label(TextTone::Information.color(), &model.status);
    egui::Grid::new("construction_card_stats")
        .num_columns(2)
        .spacing(egui::vec2(8.0, 1.0))
        .min_row_height(0.0)
        .show(ui, |ui| {
            for stat in &model.stats {
                ui.colored_label(TextTone::Neutral.color(), &stat.label);
                ui.colored_label(TextTone::Information.color(), &stat.value);
                ui.end_row();
            }
        });
    ui.colored_label(TextTone::Information.color(), &model.months);
    progress_bar(ui, model.progress);
    ui.colored_label(TextTone::Caution.color(), &model.in_service);
}

#[cfg(test)]
mod tests {
    use application::{Command, InMemoryCommandLog};
    use domain::events::DomainEvent;
    use domain::generation::PlantStatus;
    use domain::grid::LineStatus;
    use domain::region::PlantKind;
    use domain::time::TICKS_PER_DAY;

    use super::*;
    use crate::hud::test_support::{
        eastern_game, order_line, order_plant, order_short_line, shipped_l10n,
    };
    use crate::l10n::{Names, format};

    fn month(year: i32, month: u8) -> SimMonth {
        SimMonth::new(year, month).unwrap()
    }

    #[test]
    fn the_in_service_month_counts_the_closes_still_to_run() {
        let june = month(2027, 6);
        for phase in [DayPhase::Planning, DayPhase::Operating] {
            assert_eq!(in_service_month(phase, june, 1), month(2027, 7));
            assert_eq!(in_service_month(phase, june, 6), month(2027, 12));
            assert_eq!(in_service_month(phase, june, 7), month(2028, 1));
            assert_eq!(in_service_month(phase, june, 84), month(2034, 6));
        }
        // In Review June's close has already counted June.
        assert_eq!(in_service_month(DayPhase::Review, june, 1), month(2027, 8));
        assert_eq!(in_service_month(DayPhase::Review, june, 6), month(2028, 1));
    }

    #[test]
    fn plants_sort_before_lines_each_by_id() {
        assert!(Project::Plant(PlantId(90)) < Project::Line(LineId(1)));
        assert!(Project::Plant(PlantId(3)) < Project::Plant(PlantId(4)));
        assert!(Project::Line(LineId(3)) < Project::Line(LineId(4)));
    }

    fn row(model: &ConstructionModel, project: Project) -> &ConstructionRow {
        model
            .rows
            .iter()
            .find(|row| row.project == project)
            .unwrap_or_else(|| panic!("no row for {project:?} in {model:?}"))
    }

    #[test]
    fn construction_rows_sort_soonest_first_then_by_id() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let ccgt = order_plant(&mut game, PlantKind::Ccgt, 0);
        let peaker = order_plant(&mut game, PlantKind::Peaker, 0);
        let second_peaker = order_plant(&mut game, PlantKind::Peaker, 1);
        let (line, line_months) = order_short_line(&mut game);
        let session = game.session();
        let now = session.clock().month();
        let names = Names::new(&l10n, session);
        let build = |kind| session.region().spec(kind).build_months;
        assert!(line_months < build(PlantKind::Peaker));
        assert!(build(PlantKind::Peaker) < build(PlantKind::Ccgt));

        let model = construction_model(&game, &l10n);

        let order: Vec<Project> = model.rows.iter().map(|row| row.project).collect();
        assert_eq!(
            order,
            [
                Project::Line(line),
                Project::Plant(peaker),
                Project::Plant(second_peaker),
                Project::Plant(ccgt),
            ]
        );
        assert_eq!(model.empty, None);
        for (project, kind) in [
            (peaker, PlantKind::Peaker),
            (second_peaker, PlantKind::Peaker),
            (ccgt, PlantKind::Ccgt),
        ] {
            let row = row(&model, Project::Plant(project));
            let total = build(kind);
            assert_eq!(row.name, names.plant(project));
            assert_eq!((row.months_left, row.total), (total, total), "{kind:?}");
            assert_eq!(row.progress, 0.0);
            assert_eq!(row.months, format!("{total} of {total} months left"));
            let due = in_service_month(DayPhase::Planning, now, total);
            assert_eq!(row.in_service, due);
            assert_eq!(
                row.in_service_text,
                format!("In service from {}", format::month_year(due))
            );
        }
        let line_row = row(&model, Project::Line(line));
        let view = session
            .line_views()
            .into_iter()
            .find(|l| l.id == line)
            .unwrap();
        assert_eq!(
            line_row.name,
            format!(
                "{}: {} to {}",
                names.line(line),
                names.node(view.from),
                names.node(view.to)
            )
        );
        assert_eq!(
            (line_row.months_left, line_row.total),
            (line_months, line_months)
        );
        assert_eq!(
            line_row.in_service,
            in_service_month(DayPhase::Planning, now, line_months)
        );
        let total = build(PlantKind::Peaker);

        // A month on, every project is a month along: the bar moves, the date doesn't.
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
            .unwrap();
        game.execute(Command::FinishReview).unwrap();
        let later = construction_model(&game, &l10n);
        let row = row(&later, Project::Plant(peaker));
        assert_eq!((row.months_left, row.total), (total - 1, total));
        assert_eq!(row.months, format!("{} of {total} months left", total - 1));
        assert_eq!(row.progress, 1.0 / total as f32);
        assert_eq!(row.in_service, model.rows[1].in_service);
        assert_eq!(l10n.problems(), vec![]);
    }

    /// A line's total is its whole build time, not what is left of it.
    #[test]
    fn a_line_keeps_its_total_as_it_is_built() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let (line, total) = order_line(&mut game, 2);
        assert!(total >= 2);
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
            .unwrap();
        game.execute(Command::FinishReview).unwrap();

        let model = construction_model(&game, &l10n);
        let row = row(&model, Project::Line(line));
        assert_eq!((row.months_left, row.total), (total - 1, total));
        assert_eq!(
            row.months,
            l10n.args(
                "construction-months-left",
                &[("left", (total - 1).into()), ("total", total.into())]
            )
        );
        assert_eq!(row.progress, 1.0 / total as f32);
        assert_eq!(l10n.problems(), vec![]);
    }

    /// The month each row predicts is the month the project enters service: its commissioning
    /// event fires at the close of the month before, and from the next month it is no longer
    /// under construction. The prediction holds in every phase of every month until then.
    #[test]
    fn the_in_service_month_matches_commissioning() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let plant = order_plant(&mut game, PlantKind::Peaker, 0);
        let (line, line_months) = order_short_line(&mut game);
        let model = construction_model(&game, &l10n);
        let predicted_plant = row(&model, Project::Plant(plant)).in_service;
        let predicted_line = row(&model, Project::Line(line)).in_service;
        let start = game.session().clock().month();
        assert_eq!(
            predicted_plant.months_since(start),
            i32::try_from(game.session().region().spec(PlantKind::Peaker).build_months).unwrap()
        );
        assert_eq!(
            predicted_line.months_since(start),
            i32::try_from(line_months).unwrap()
        );

        let predictions = |game: &Game| {
            let model = construction_model(game, &l10n);
            let find = |project| {
                model
                    .rows
                    .iter()
                    .find(|row| row.project == project)
                    .map(|row| row.in_service)
            };
            (find(Project::Plant(plant)), find(Project::Line(line)))
        };
        let (mut plant_in_service, mut line_in_service) = (None, None);
        while plant_in_service.is_none() || line_in_service.is_none() {
            let now = game.session().clock().month();
            assert!(now.months_since(start) < 24, "both commissioned by now");
            let expect = |done: Option<SimMonth>, predicted| done.is_none().then_some(predicted);
            let expected = (
                expect(plant_in_service, predicted_plant),
                expect(line_in_service, predicted_line),
            );
            assert_eq!(predictions(&game), expected, "Planning of {now:?}");
            game.execute(Command::BeginOperating).unwrap();
            assert_eq!(predictions(&game), expected, "Operating of {now:?}");
            let events = game
                .execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
                .unwrap();
            assert_eq!(game.session().phase(), DayPhase::Review);
            for event in &events {
                match event {
                    DomainEvent::PlantCommissioned { plant: id, .. } if *id == plant => {
                        plant_in_service = Some(now.next());
                    }
                    DomainEvent::LineCommissioned { line: id } if *id == line => {
                        line_in_service = Some(now.next());
                    }
                    _ => {}
                }
            }
            let expected = (
                expect(plant_in_service, predicted_plant),
                expect(line_in_service, predicted_line),
            );
            assert_eq!(predictions(&game), expected, "Review of {now:?}");
            game.execute(Command::FinishReview).unwrap();
        }
        assert_eq!(plant_in_service, Some(predicted_plant));
        assert_eq!(line_in_service, Some(predicted_line));
        // In the predicted month the project is built: the plant is off, ready to start.
        let session = game.session();
        assert!(session.clock().month() >= predicted_plant.max(predicted_line));
        let status = session
            .plant_views()
            .into_iter()
            .find(|p| p.id == plant)
            .unwrap()
            .status;
        assert!(!matches!(status, PlantStatus::UnderConstruction { .. }));
        assert!(
            session
                .line_views()
                .iter()
                .all(|l| l.id != line || l.status == LineStatus::InService)
        );
        assert_eq!(construction_model(&game, &l10n).rows, vec![]);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_tab_label_counts_the_projects_and_the_empty_tab_says_so() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        assert_eq!(construction_tab_label(&game, &l10n), "Construction");
        assert_eq!(
            construction_model(&game, &l10n),
            ConstructionModel {
                rows: Vec::new(),
                empty: Some("Nothing under construction.".to_owned()),
            }
        );

        order_plant(&mut game, PlantKind::Peaker, 0);
        assert_eq!(construction_tab_label(&game, &l10n), "Construction (1)");
        order_short_line(&mut game);
        assert_eq!(construction_tab_label(&game, &l10n), "Construction (2)");
        assert_eq!(construction_model(&game, &l10n).empty, None);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_card_shows_kind_capacity_months_and_the_in_service_month() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let plant = order_plant(&mut game, PlantKind::Peaker, 0);
        let session = game.session();
        let spec = session.region().spec(PlantKind::Peaker);
        let total = spec.build_months;
        let due = in_service_month(DayPhase::Planning, session.clock().month(), total);

        let card = construction_card_model(&game, &l10n, plant).expect("a card");

        assert_eq!(card.plant, plant);
        assert_eq!(card.name, Names::new(&l10n, session).plant(plant));
        assert!(card.name.starts_with("Gas peaker"), "{}", card.name);
        assert_eq!(card.status, "Under construction");
        assert_eq!(
            card.stats,
            [
                Stat {
                    label: "Capacity".to_owned(),
                    value: format::megawatts(spec.capacity),
                },
                Stat {
                    label: "Grid connection".to_owned(),
                    value: "Not connected".to_owned(),
                },
                Stat {
                    label: "Capital cost".to_owned(),
                    value: format::money(spec.capital_cost),
                },
                Stat {
                    label: "Per month (approx.)".to_owned(),
                    value: super::super::build::monthly_payment(
                        spec.capital_cost,
                        spec.build_months
                    ),
                }
            ]
        );
        assert_eq!(card.months, format!("{total} of {total} months left"));
        assert_eq!(card.progress, 0.0);
        assert_eq!(
            card.in_service,
            format!("In service from {}", format::month_year(due))
        );

        // A built plant, or one the session doesn't know, has no card.
        let built = session
            .plant_views()
            .into_iter()
            .find(|p| !matches!(p.status, PlantStatus::UnderConstruction { .. }))
            .unwrap()
            .id;
        assert_eq!(construction_card_model(&game, &l10n, built), None);
        assert_eq!(construction_card_model(&game, &l10n, PlantId(9_999)), None);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn a_one_month_build_reads_in_the_singular() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let (line, months) = order_short_line(&mut game);
        let model = construction_model(&game, &l10n);
        let row = row(&model, Project::Line(line));
        if months == 1 {
            assert_eq!(row.months, "1 of 1 month left");
        } else {
            assert_eq!(row.months, format!("{months} of {months} months left"));
        }
        assert_eq!(
            l10n.args(
                "construction-months-left",
                &[("left", 1u32.into()), ("total", 1u32.into())]
            ),
            "1 of 1 month left"
        );
        assert_eq!(l10n.problems(), vec![]);
    }
}
