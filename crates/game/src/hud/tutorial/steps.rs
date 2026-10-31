//! The tutorial's steps (first-game tutorial spec §4.2, §4.3) and the pure model that picks the
//! active one.
//!
//! The model re-evaluates from the game's state every time it is asked: the active step is the
//! first step of the current phase that has neither played (one-shot steps) nor been done (its
//! predicate over the game and the selection). So a player who goes off-script, say by queueing a
//! battery early or starting the day early, simply skips the steps that no longer apply.

use std::collections::BTreeSet;

use application::Game;
use domain::generation::PlantStatus;
use domain::grid::LineStatus;
use domain::ids::{NodeId, PlantId};
use domain::region::PlantKind;
use domain::session::{DayPhase, LineView, PlantView};

use crate::hud::Selection;

/// A widget a tutorial step points at. The panel that draws it marks its rect each pass
/// ([`super::mark_anchor`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TutorialAnchor {
    Cash,
    Map,
    BatteryCard,
    LineSection,
    StartDay,
    SpeedControls,
    ReviewConstruction,
}

/// One step of the tutorial, in the order they play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TutorialStep {
    FrameGrid,
    Crawl,
    ShowForecast,
    Cash,
    PickSite,
    RevealBattery,
    BatterySpotlight,
    BuildBattery,
    ConnectSpotlight,
    ConnectBattery,
    StartSpotlight,
    StartDay,
    Speed,
    Construction,
}

/// How a step presents itself. Moves, the crawl and spotlights are one-shot: they join
/// [`TutorialState::played`] when they finish. Instructions stay until their step is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepKind {
    /// An automatic UI move: framing the camera, switching a tab, scrolling to a card.
    Move,
    Crawl,
    Spotlight,
    Instruction,
}

/// When a step no longer applies, whatever has played.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Done {
    /// Only playing (or the phase passing) ends it.
    Never,
    /// A map cell is selected, or a tutorial battery has been queued.
    CellSelectedOrBattery,
    /// A battery that wasn't in the starter fleet exists, built or under construction.
    BatteryQueued,
    /// Some tutorial battery's node is an end of a line, built or under construction.
    BatteryLinked,
}

/// One row of [`TUTORIAL_STEPS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepDef {
    pub step: TutorialStep,
    pub phase: DayPhase,
    pub kind: StepKind,
    pub anchor: Option<TutorialAnchor>,
    /// The note's `ui.ftl` key. The Construction step's key is chosen by [`construction_key`].
    pub text_key: Option<&'static str>,
    pub done: Done,
}

const fn def(
    step: TutorialStep,
    phase: DayPhase,
    kind: StepKind,
    anchor: Option<TutorialAnchor>,
    text_key: Option<&'static str>,
    done: Done,
) -> StepDef {
    StepDef {
        step,
        phase,
        kind,
        anchor,
        text_key,
        done,
    }
}

/// The tutorial, in order (spec §4.3).
pub const TUTORIAL_STEPS: [StepDef; 14] = {
    use DayPhase::{Operating, Planning, Review};
    use StepKind::{Crawl, Instruction, Move, Spotlight};
    use TutorialAnchor as A;
    use TutorialStep as S;
    [
        def(S::FrameGrid, Planning, Move, None, None, Done::Never),
        def(
            S::Crawl,
            Planning,
            Crawl,
            None,
            Some("tutorial-crawl"),
            Done::Never,
        ),
        def(S::ShowForecast, Planning, Move, None, None, Done::Never),
        def(
            S::Cash,
            Planning,
            Spotlight,
            Some(A::Cash),
            Some("tutorial-cash"),
            Done::Never,
        ),
        def(
            S::PickSite,
            Planning,
            Instruction,
            Some(A::Map),
            Some("tutorial-pick-site"),
            Done::CellSelectedOrBattery,
        ),
        def(
            S::RevealBattery,
            Planning,
            Move,
            None,
            None,
            Done::BatteryQueued,
        ),
        def(
            S::BatterySpotlight,
            Planning,
            Spotlight,
            Some(A::BatteryCard),
            Some("tutorial-battery-spotlight"),
            Done::BatteryQueued,
        ),
        def(
            S::BuildBattery,
            Planning,
            Instruction,
            Some(A::BatteryCard),
            Some("tutorial-battery-build"),
            Done::BatteryQueued,
        ),
        def(
            S::ConnectSpotlight,
            Planning,
            Spotlight,
            Some(A::LineSection),
            Some("tutorial-connect-spotlight"),
            Done::BatteryLinked,
        ),
        def(
            S::ConnectBattery,
            Planning,
            Instruction,
            Some(A::LineSection),
            Some("tutorial-connect-build"),
            Done::BatteryLinked,
        ),
        def(
            S::StartSpotlight,
            Planning,
            Spotlight,
            Some(A::StartDay),
            Some("tutorial-start"),
            Done::Never,
        ),
        def(
            S::StartDay,
            Planning,
            Instruction,
            Some(A::StartDay),
            Some("tutorial-start"),
            Done::Never,
        ),
        def(
            S::Speed,
            Operating,
            Spotlight,
            Some(A::SpeedControls),
            Some("tutorial-speed"),
            Done::Never,
        ),
        def(
            S::Construction,
            Review,
            Spotlight,
            Some(A::ReviewConstruction),
            None,
            Done::Never,
        ),
    ]
};

/// What the game's state can't tell the tutorial: which game it belongs to, the batteries the
/// player started with, the one-shot steps that have played and whether the first Review has
/// been seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TutorialState {
    /// The [`SessionGeneration`](crate::sim::SessionGeneration) of the game it was started in.
    pub generation: u64,
    /// The batteries in the starter fleet, which don't count as the player's (Southern China
    /// starts with one).
    pub starter_batteries: BTreeSet<PlantId>,
    pub played: BTreeSet<TutorialStep>,
    pub seen_review: bool,
}

impl TutorialState {
    /// A tutorial for `game`, just started, in session generation `generation`.
    pub fn new(generation: u64, game: &Game) -> Self {
        let starter_batteries = game
            .session()
            .plant_views()
            .into_iter()
            .filter(|plant| plant.kind == PlantKind::Battery)
            .map(|plant| plant.id)
            .collect();
        Self {
            generation,
            starter_batteries,
            played: BTreeSet::new(),
            seen_review: false,
        }
    }
}

/// The step to show now: the first of the current phase that has not played and is not done.
/// `None` once the phase has none left.
pub fn active_step(
    state: &TutorialState,
    game: &Game,
    selection: Selection,
) -> Option<&'static StepDef> {
    let session = game.session();
    let phase = session.phase();
    let facts = Facts::of(state, &session.plant_views(), &session.line_views());
    TUTORIAL_STEPS.iter().find(|def| {
        def.phase == phase && !state.played.contains(&def.step) && !facts.done(def.done, selection)
    })
}

/// The Review Construction note's key, from what the player actually ordered (spec §4.3, step 10).
pub fn construction_key(state: &TutorialState, game: &Game) -> &'static str {
    let session = game.session();
    let (plants, lines) = (session.plant_views(), session.line_views());
    let batteries: Vec<&PlantView> = tutorial_batteries(state, &plants).collect();
    if batteries.is_empty() {
        // Nothing the tutorial asked for was ordered, so neither was a line to it.
        return "tutorial-construction-general";
    }
    let linked = batteries
        .iter()
        .any(|battery| lines.iter().any(|line| touches(line, battery.node)));
    if !linked {
        // No line to mention, and the battery can't help until one is built.
        let building = batteries
            .iter()
            .any(|battery| matches!(battery.status, PlantStatus::UnderConstruction { .. }));
        return if building {
            "tutorial-construction-battery-paying"
        } else {
            "tutorial-construction-battery-paid"
        };
    }
    let paying = batteries.iter().any(|battery| {
        matches!(battery.status, PlantStatus::UnderConstruction { .. })
            || lines.iter().any(|line| {
                touches(line, battery.node)
                    && matches!(line.status, LineStatus::UnderConstruction { .. })
            })
    });
    if paying {
        "tutorial-construction-paying"
    } else {
        "tutorial-construction-paid"
    }
}

/// The batteries in `plants` that weren't in the starter fleet: the player's, built or not.
fn tutorial_batteries<'a>(
    state: &'a TutorialState,
    plants: &'a [PlantView],
) -> impl Iterator<Item = &'a PlantView> {
    plants.iter().filter(|plant| {
        plant.kind == PlantKind::Battery && !state.starter_batteries.contains(&plant.id)
    })
}

fn touches(line: &LineView, node: NodeId) -> bool {
    line.from == node || line.to == node
}

/// What the [`Done`] predicates read from the game, worked out once per evaluation.
struct Facts {
    battery_queued: bool,
    battery_linked: bool,
}

impl Facts {
    fn of(state: &TutorialState, plants: &[PlantView], lines: &[LineView]) -> Self {
        let nodes: Vec<NodeId> = tutorial_batteries(state, plants)
            .map(|battery| battery.node)
            .collect();
        Self {
            battery_queued: !nodes.is_empty(),
            battery_linked: nodes
                .iter()
                .any(|node| lines.iter().any(|line| touches(line, *node))),
        }
    }

    fn done(&self, done: Done, selection: Selection) -> bool {
        match done {
            Done::Never => false,
            Done::CellSelectedOrBattery => {
                matches!(selection, Selection::Cell(_)) || self.battery_queued
            }
            Done::BatteryQueued => self.battery_queued,
            Done::BatteryLinked => self.battery_linked,
        }
    }
}

#[cfg(test)]
mod tests {
    use application::{Command, InMemoryCommandLog};
    use domain::events::DomainEvent;
    use domain::ids::CellCoord;
    use domain::time::TICKS_PER_DAY;

    use super::*;
    use crate::hud::test_support::shipped_game;

    fn game(region: &str) -> Game {
        shipped_game(region, Box::new(InMemoryCommandLog::default()))
    }

    fn played(steps: &[TutorialStep]) -> BTreeSet<TutorialStep> {
        steps.iter().copied().collect()
    }

    fn active(state: &TutorialState, game: &Game, selection: Selection) -> Option<TutorialStep> {
        active_step(state, game, selection).map(|def| def.step)
    }

    /// FrameGrid, Crawl, ShowForecast and Cash: the one-shot steps before PickSite.
    const OPENING: [TutorialStep; 4] = [
        TutorialStep::FrameGrid,
        TutorialStep::Crawl,
        TutorialStep::ShowForecast,
        TutorialStep::Cash,
    ];

    /// The cell a battery can be built on nearest the anchor city, and the anchor's node.
    fn battery_site(game: &Game) -> (CellCoord, NodeId) {
        let session = game.session();
        let anchor = session.anchor_node();
        let home = session
            .city_views()
            .into_iter()
            .find(|city| city.node == anchor)
            .expect("the anchor is a city")
            .cell;
        let (w, h) = (
            session.region().terrain().width(),
            session.region().terrain().height(),
        );
        let cell = (0..h)
            .flat_map(|y| (0..w).map(move |x| CellCoord { x, y }))
            .filter(|cell| session.plant_quote(PlantKind::Battery, *cell).is_ok())
            .min_by(|a, b| {
                domain::grid::distance_cells(*a, home)
                    .total_cmp(&domain::grid::distance_cells(*b, home))
            })
            .expect("somewhere to build a battery");
        (cell, anchor)
    }

    /// Queues a battery at `cell` and returns its id.
    fn queue_battery(game: &mut Game, cell: CellCoord) -> PlantId {
        game.execute(Command::BuildPlant {
            kind: PlantKind::Battery,
            cell,
        })
        .expect("the battery order")
        .iter()
        .find_map(|event| match event {
            DomainEvent::PlantConstructionStarted { plant, .. } => Some(*plant),
            _ => None,
        })
        .expect("the battery's construction started")
    }

    fn node_of(game: &Game, plant: PlantId) -> NodeId {
        game.session()
            .plant_views()
            .into_iter()
            .find(|view| view.id == plant)
            .expect("the plant")
            .node
    }

    fn play_month(game: &mut Game) {
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
            .unwrap();
        assert_eq!(game.session().phase(), DayPhase::Review);
    }

    #[test]
    fn the_table_is_in_step_order_and_one_row_per_step() {
        let steps: Vec<TutorialStep> = TUTORIAL_STEPS.iter().map(|def| def.step).collect();
        let mut sorted = steps.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(steps, sorted);
        assert_eq!(steps.len(), 14);
    }

    #[test]
    fn first_step_is_frame_grid() {
        let game = game("western_interconnection");
        let state = TutorialState::new(1, &game);
        assert_eq!(state.generation, 1);
        assert!(state.played.is_empty());
        assert!(!state.seen_review);
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::FrameGrid)
        );
    }

    #[test]
    fn played_one_shots_are_skipped() {
        let game = game("western_interconnection");
        let mut state = TutorialState::new(1, &game);
        state.played = played(&[TutorialStep::FrameGrid]);
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::Crawl)
        );
        state.played = played(&[TutorialStep::FrameGrid, TutorialStep::Crawl]);
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::ShowForecast)
        );
        state.played = played(&OPENING);
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::PickSite)
        );
    }

    #[test]
    fn pick_site_done_by_cell_selection() {
        let game = game("western_interconnection");
        let mut state = TutorialState::new(1, &game);
        state.played = played(&OPENING);
        let (cell, _) = battery_site(&game);
        assert_eq!(
            active(&state, &game, Selection::Cell(cell)),
            Some(TutorialStep::RevealBattery)
        );
        state.played.insert(TutorialStep::RevealBattery);
        assert_eq!(
            active(&state, &game, Selection::Cell(cell)),
            Some(TutorialStep::BatterySpotlight)
        );
        state.played.insert(TutorialStep::BatterySpotlight);
        assert_eq!(
            active(&state, &game, Selection::Cell(cell)),
            Some(TutorialStep::BuildBattery)
        );
    }

    #[test]
    fn battery_queued_before_asked_skips_battery_steps() {
        let mut game = game("western_interconnection");
        let mut state = TutorialState::new(1, &game);
        let (cell, anchor) = battery_site(&game);
        let battery = queue_battery(&mut game, cell);
        state.played = played(&OPENING);
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::ConnectSpotlight)
        );
        state.played.insert(TutorialStep::ConnectSpotlight);
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::ConnectBattery)
        );
        // A line under construction to the battery's node is enough.
        game.execute(Command::BuildLine {
            from: anchor,
            to: node_of(&game, battery),
        })
        .expect("the line order");
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::StartSpotlight)
        );
        state.played.insert(TutorialStep::StartSpotlight);
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::StartDay)
        );
    }

    #[test]
    fn deselecting_returns_to_pick_site() {
        let game = game("western_interconnection");
        let mut state = TutorialState::new(1, &game);
        let (cell, _) = battery_site(&game);
        state.played = played(&OPENING);
        state.played.insert(TutorialStep::RevealBattery);
        state.played.insert(TutorialStep::BatterySpotlight);
        assert_eq!(
            active(&state, &game, Selection::Cell(cell)),
            Some(TutorialStep::BuildBattery)
        );
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::PickSite)
        );
    }

    #[test]
    fn starting_the_day_drops_planning_steps() {
        let mut game = game("western_interconnection");
        let mut state = TutorialState::new(1, &game);
        state.played = played(&OPENING);
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::PickSite)
        );
        game.execute(Command::BeginOperating).unwrap();
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::Speed)
        );
        state.played.insert(TutorialStep::Speed);
        assert_eq!(active(&state, &game, Selection::None), None);
        game.execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
            .unwrap();
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::Construction)
        );
    }

    #[test]
    fn southern_china_starter_battery_does_not_count() {
        let game = game("southern_china");
        let state = TutorialState::new(1, &game);
        assert!(!state.starter_batteries.is_empty());
        let mut state = state;
        state.played = played(&OPENING);
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::PickSite)
        );
        state.played.insert(TutorialStep::RevealBattery);
        state.played.insert(TutorialStep::BatterySpotlight);
        let (cell, _) = battery_site(&game);
        assert_eq!(
            active(&state, &game, Selection::Cell(cell)),
            Some(TutorialStep::BuildBattery)
        );
    }

    /// Whether the battery `plant`, or a line touching its node, is still under construction.
    fn building(game: &Game, plant: PlantId) -> bool {
        let node = node_of(game, plant);
        let plant_building =
            game.session().plant_views().iter().any(|p| {
                p.id == plant && matches!(p.status, PlantStatus::UnderConstruction { .. })
            });
        let line_building = game.session().line_views().iter().any(|l| {
            (l.from == node || l.to == node)
                && matches!(l.status, LineStatus::UnderConstruction { .. })
        });
        plant_building || line_building
    }

    #[test]
    fn construction_key_is_general_without_a_tutorial_battery() {
        // Southern China's starter battery is built, and it is not the player's order.
        let mut game = game("southern_china");
        let state = TutorialState::new(1, &game);
        play_month(&mut game);
        assert_eq!(
            construction_key(&state, &game),
            "tutorial-construction-general"
        );
    }

    /// With no line to it, the note doesn't mention one, and says the battery can't help yet:
    /// while it is building, and once it is built.
    #[test]
    fn construction_key_without_a_line_is_about_the_battery_alone() {
        let mut game = game("western_interconnection");
        let state = TutorialState::new(1, &game);
        let (cell, anchor) = battery_site(&game);
        let battery = queue_battery(&mut game, cell);
        // The battery alone is building.
        assert_eq!(
            construction_key(&state, &game),
            "tutorial-construction-battery-paying"
        );
        play_month(&mut game);
        let mut months = 1;
        while building(&game, battery) {
            assert!(months < 60, "the battery never finished");
            game.execute(Command::FinishReview).unwrap();
            play_month(&mut game);
            months += 1;
        }
        assert_eq!(
            construction_key(&state, &game),
            "tutorial-construction-battery-paid"
        );
        // Its line alone is building: now there is one to mention.
        game.execute(Command::FinishReview).unwrap();
        game.execute(Command::BuildLine {
            from: anchor,
            to: node_of(&game, battery),
        })
        .unwrap();
        assert!(building(&game, battery));
        assert_eq!(
            construction_key(&state, &game),
            "tutorial-construction-paying"
        );
    }

    #[test]
    fn construction_key_is_paid_once_the_battery_and_its_line_are_built() {
        let mut game = game("western_interconnection");
        let state = TutorialState::new(1, &game);
        let (cell, anchor) = battery_site(&game);
        let battery = queue_battery(&mut game, cell);
        game.execute(Command::BuildLine {
            from: anchor,
            to: node_of(&game, battery),
        })
        .unwrap();
        play_month(&mut game);
        let mut months = 1;
        // Whichever of the two finishes first, the note says "paying" until both are built.
        while building(&game, battery) {
            assert_eq!(
                construction_key(&state, &game),
                "tutorial-construction-paying"
            );
            assert!(months < 60, "the battery and its line never finished");
            game.execute(Command::FinishReview).unwrap();
            play_month(&mut game);
            months += 1;
        }
        assert_eq!(
            construction_key(&state, &game),
            "tutorial-construction-paid"
        );
    }
}
