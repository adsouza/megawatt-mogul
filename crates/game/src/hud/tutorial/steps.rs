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
use domain::time::SimMonth;

use crate::hud::Selection;

/// A widget a tutorial step points at. The panel that draws it marks its rect each pass
/// ([`super::mark_anchor`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TutorialAnchor {
    Cash,
    Map,
    MapLayers,
    PlantCard,
    /// The current chapter's plant card's *Build plant* button.
    PlantBuildButton,
    LineSection,
    /// The build menu's button that takes a line on from a tutorial plant: the first *Line
    /// from* button that starts at one, or, once a line starts at one, the first enabled *Build
    /// line* button among its targets.
    PlantLineButton,
    StartDay,
    SpeedControls,
    ReviewConstruction,
    Save,
    CrystalBall,
    OutlookCity,
    OutlookGraph,
    PayoffButton,
    PayoffGraph,
    PayoffTable,
    PayoffClose,
}

/// Step identities for the main tutorial and the independent first-use outlook guide.
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
    Supply,
    PickGeneratorSite,
    RevealGenerator,
    GeneratorSpotlight,
    BuildGenerator,
    ConnectGeneratorSpotlight,
    ConnectGenerator,
    SaveSnapshot,
    CrystalBall,
    OpenPayoff,
    PayoffConstruction,
    PayoffOperating,
    ClosePayoff,
    RenewableSiting,
    OutlookCitySpotlight,
    OutlookChooseCity,
    OutlookCushion,
}

impl TutorialStep {
    /// Stable save identifiers; keep these keys when renaming Rust variants.
    fn save_key(self) -> &'static str {
        match self {
            Self::FrameGrid => "frame-grid",
            Self::Crawl => "crawl",
            Self::ShowForecast => "show-forecast",
            Self::Cash => "cash",
            Self::PickSite => "pick-site",
            Self::RevealBattery => "reveal-battery",
            Self::BatterySpotlight => "battery-spotlight",
            Self::BuildBattery => "build-battery",
            Self::ConnectSpotlight => "connect-spotlight",
            Self::ConnectBattery => "connect-battery",
            Self::StartSpotlight => "start-spotlight",
            Self::StartDay => "start-day",
            Self::Speed => "speed",
            Self::Construction => "construction",
            Self::Supply => "supply",
            Self::PickGeneratorSite => "pick-generator-site",
            Self::RevealGenerator => "reveal-generator",
            Self::GeneratorSpotlight => "generator-spotlight",
            Self::BuildGenerator => "build-generator",
            Self::ConnectGeneratorSpotlight => "connect-generator-spotlight",
            Self::ConnectGenerator => "connect-generator",
            Self::SaveSnapshot => "save-snapshot",
            Self::CrystalBall => "crystal-ball",
            Self::OpenPayoff => "open-payoff",
            Self::PayoffConstruction => "payoff-construction",
            Self::PayoffOperating => "payoff-operating",
            Self::ClosePayoff => "close-payoff",
            Self::RenewableSiting => "renewable-siting",
            Self::OutlookCitySpotlight => "outlook-city-spotlight",
            Self::OutlookChooseCity => "outlook-choose-city",
            Self::OutlookCushion => "outlook-cushion",
        }
    }

    /// The main tutorial notes drawn inside the payoff dialog rather than behind it.
    pub(crate) fn is_payoff_guide(self) -> bool {
        matches!(
            self,
            Self::PayoffConstruction | Self::PayoffOperating | Self::ClosePayoff
        )
    }
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
    CellSelectedOrGenerator,
    GeneratorQueued,
    GeneratorLinked,
    /// The save caution waits until the new generator has an ordered grid path.
    GeneratorNotLinked,
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
pub const TUTORIAL_STEPS: [StepDef; 28] = {
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
            Some(A::PlantCard),
            Some("tutorial-battery-spotlight"),
            Done::BatteryQueued,
        ),
        def(
            S::BuildBattery,
            Planning,
            Instruction,
            Some(A::PlantBuildButton),
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
            Some(A::PlantLineButton),
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
        def(
            S::Supply,
            Planning,
            Spotlight,
            Some(A::Map),
            Some("tutorial-supply"),
            Done::GeneratorLinked,
        ),
        def(
            S::PickGeneratorSite,
            Planning,
            Instruction,
            Some(A::Map),
            Some("tutorial-ccgt-pick-site"),
            Done::CellSelectedOrGenerator,
        ),
        def(
            S::RevealGenerator,
            Planning,
            Move,
            None,
            None,
            Done::GeneratorQueued,
        ),
        def(
            S::GeneratorSpotlight,
            Planning,
            Spotlight,
            Some(A::PlantCard),
            Some("tutorial-ccgt-spotlight"),
            Done::GeneratorQueued,
        ),
        def(
            S::BuildGenerator,
            Planning,
            Instruction,
            Some(A::PlantBuildButton),
            Some("tutorial-ccgt-build"),
            Done::GeneratorQueued,
        ),
        def(
            S::ConnectGeneratorSpotlight,
            Planning,
            Spotlight,
            Some(A::LineSection),
            Some("tutorial-connect-spotlight"),
            Done::GeneratorLinked,
        ),
        def(
            S::ConnectGenerator,
            Planning,
            Instruction,
            Some(A::PlantLineButton),
            Some("tutorial-ccgt-connect"),
            Done::GeneratorLinked,
        ),
        def(
            S::SaveSnapshot,
            Planning,
            Spotlight,
            Some(A::Save),
            Some("tutorial-save-snapshot"),
            Done::GeneratorNotLinked,
        ),
        def(
            S::CrystalBall,
            Planning,
            Spotlight,
            Some(A::CrystalBall),
            Some("tutorial-crystal-ball"),
            Done::GeneratorNotLinked,
        ),
        def(
            S::OpenPayoff,
            Planning,
            Instruction,
            Some(A::PayoffButton),
            Some("tutorial-payoff-open"),
            Done::Never,
        ),
        def(
            S::PayoffConstruction,
            Planning,
            Spotlight,
            Some(A::PayoffGraph),
            Some("tutorial-payoff-construction"),
            Done::Never,
        ),
        def(
            S::PayoffOperating,
            Planning,
            Spotlight,
            Some(A::PayoffTable),
            Some("tutorial-payoff-operating"),
            Done::Never,
        ),
        def(
            S::ClosePayoff,
            Planning,
            Instruction,
            Some(A::PayoffClose),
            Some("tutorial-payoff-close"),
            Done::Never,
        ),
        def(
            S::RenewableSiting,
            Planning,
            Spotlight,
            Some(A::MapLayers),
            Some("tutorial-renewable-siting"),
            Done::Never,
        ),
    ]
};

/// First-use tool guidance, independent of the first game's lifecycle and save loading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlookGuideStep {
    CitySpotlight,
    ChooseCity,
    Cushion,
}
impl OutlookGuideStep {
    pub fn definition(self) -> &'static StepDef {
        &OUTLOOK_STEPS[match self {
            Self::CitySpotlight => 0,
            Self::ChooseCity => 1,
            Self::Cushion => 2,
        }]
    }
}
/// Tool definitions run in every in-game phase; their phase field is unused.
pub const OUTLOOK_STEPS: [StepDef; 3] = [
    def(
        TutorialStep::OutlookCitySpotlight,
        DayPhase::Planning,
        StepKind::Spotlight,
        Some(TutorialAnchor::OutlookCity),
        Some("tutorial-outlook-city"),
        Done::Never,
    ),
    def(
        TutorialStep::OutlookChooseCity,
        DayPhase::Planning,
        StepKind::Instruction,
        Some(TutorialAnchor::OutlookCity),
        Some("tutorial-outlook-city"),
        Done::Never,
    ),
    def(
        TutorialStep::OutlookCushion,
        DayPhase::Planning,
        StepKind::Spotlight,
        Some(TutorialAnchor::OutlookGraph),
        Some("tutorial-outlook-cushion"),
        Done::Never,
    ),
];

/// What the game's state can't tell the tutorial: which game it belongs to, the plants the
/// player started with, the one-shot steps that have played and whether the first Review has
/// been seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TutorialState {
    /// The [`SessionGeneration`](crate::sim::SessionGeneration) of the game it was started in.
    pub generation: u64,
    /// The batteries in the starter fleet, which don't count as the player's (Southern China
    /// starts with one).
    pub starter_batteries: BTreeSet<PlantId>,
    /// The region's recommended generator for the second Planning chapter.
    pub supply_kind: PlantKind,
    /// Every starter generator, regardless of kind; none count as the player's new supply.
    pub starter_generators: BTreeSet<PlantId>,
    pub played: BTreeSet<TutorialStep>,
    pub seen_review: bool,
    /// The annual outlook was actually opened during this run.
    pub outlook_used: bool,
    /// First Planning month for the final comparison chapter, after both prerequisites.
    pub payoff_month: Option<SimMonth>,
}

impl TutorialState {
    /// A tutorial for `game`, just started, in session generation `generation`.
    pub fn new(generation: u64, game: &Game) -> Self {
        let supply_kind = match game.session().region().id().as_str() {
            "brazil_sin" | "southern_china" => PlantKind::Solar,
            _ => PlantKind::Ccgt,
        };
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
            supply_kind,
            starter_generators: game
                .session()
                .plant_views()
                .into_iter()
                .filter(|plant| plant.kind != PlantKind::Battery)
                .map(|plant| plant.id)
                .collect(),
            played: BTreeSet::new(),
            seen_review: false,
            outlook_used: false,
            payoff_month: None,
        }
    }

    /// Updates bookkeeping at a simulation boundary. True means the tutorial has finished.
    pub(crate) fn reconcile(&mut self, game: &Game) -> bool {
        let session = game.session();
        let month = session.clock().month();
        if self.payoff_month.is_none() && self.outlook_used && generator_linked(self, game) {
            self.payoff_month = month.checked_add_months(1).ok();
        }
        match session.phase() {
            DayPhase::Review => {
                self.seen_review = true;
                false
            }
            DayPhase::Planning => self.played.contains(&TutorialStep::RenewableSiting),
            DayPhase::Operating => {
                self.seen_review && self.payoff_month.is_none_or(|ready| month >= ready)
            }
        }
    }

    pub(crate) fn saved(&self) -> infrastructure::client_tutorial::SavedTutorial {
        infrastructure::client_tutorial::SavedTutorial {
            version: 1,
            played: self
                .played
                .iter()
                .map(|step| step.save_key().to_owned())
                .collect(),
            starter_batteries: self.starter_batteries.iter().map(|id| id.0).collect(),
            starter_generators: self.starter_generators.iter().map(|id| id.0).collect(),
            supply_kind: match self.supply_kind {
                PlantKind::Ccgt => "ccgt",
                PlantKind::Solar => "solar",
                _ => "unsupported",
            }
            .into(),
            seen_review: self.seen_review,
            outlook_used: self.outlook_used,
            payoff_month: self.payoff_month.map(|month| (month.year(), month.month())),
        }
    }

    pub(crate) fn restore(bytes: &[u8], generation: u64, game: &Game) -> Option<Self> {
        let saved = infrastructure::client_tutorial::SavedTutorial::decode(bytes)?;
        let mut played = saved
            .played
            .iter()
            .map(|key| {
                TUTORIAL_STEPS
                    .iter()
                    .find(|def| def.step.save_key() == key)
                    .map(|def| def.step)
            })
            .collect::<Option<BTreeSet<_>>>()?;
        // The regional recommendation is independent of the kind the player actually built.
        let supply_kind = match saved.supply_kind.as_str() {
            "ccgt" => PlantKind::Ccgt,
            "solar" => PlantKind::Solar,
            _ => return None,
        };
        // Dialogs aren't persisted. Re-offer opening the comparison, preserving its completed
        // notes, rather than waiting for an anchor inside a closed dialog.
        if !played.contains(&TutorialStep::ClosePayoff) {
            played.remove(&TutorialStep::OpenPayoff);
        }
        let payoff_month = match saved.payoff_month {
            Some((year, month)) => Some(SimMonth::new(year, month).ok()?),
            None => None,
        };
        let mut state = Self {
            generation,
            starter_batteries: saved.starter_batteries.into_iter().map(PlantId).collect(),
            starter_generators: saved.starter_generators.into_iter().map(PlantId).collect(),
            supply_kind,
            played,
            seen_review: saved.seen_review,
            outlook_used: saved.outlook_used,
            payoff_month,
        };
        (!state.reconcile(game)).then_some(state)
    }

    /// The plant the current Planning chapter asks the player to build.
    pub fn plant_kind(&self) -> PlantKind {
        if self.seen_review {
            self.supply_kind
        } else {
            PlantKind::Battery
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
    let facts = Facts::of(state, game);
    let payoff_ready = state
        .payoff_month
        .is_some_and(|month| session.clock().month() >= month);
    TUTORIAL_STEPS.iter().find(|def| {
        def.phase == phase
            && (phase != DayPhase::Planning
                || if def.step >= TutorialStep::OpenPayoff {
                    payoff_ready
                } else {
                    !payoff_ready && (def.step >= TutorialStep::Supply) == state.seen_review
                })
            && !state.played.contains(&def.step)
            // A connected alternative generator completes the supply task too, including
            // the earlier site/card/build instructions for the recommended kind.
            && !(facts.generator_linked
                && (TutorialStep::Supply..=TutorialStep::ConnectGenerator).contains(&def.step))
            && !facts.done(def.done, selection)
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

/// The nodes of the tutorial's batteries: those the player ordered, built or not.
pub fn tutorial_battery_nodes(state: &TutorialState, game: &Game) -> BTreeSet<NodeId> {
    tutorial_batteries(state, &game.session().plant_views())
        .map(|battery| battery.node)
        .collect()
}

/// Nodes for the plant the current chapter asks for, including construction orders.
pub fn tutorial_plant_nodes(state: &TutorialState, game: &Game) -> BTreeSet<NodeId> {
    if !state.seen_review {
        return tutorial_battery_nodes(state, game);
    }
    tutorial_generators(state, &game.session().plant_views())
        .map(|plant| plant.node)
        .collect()
}

/// Whether any new generating plant has a grid path, including ordered lines. Storage is excluded.
pub(super) fn generator_linked(state: &TutorialState, game: &Game) -> bool {
    Facts::of(state, game).generator_linked
}

fn tutorial_generators<'a>(
    state: &'a TutorialState,
    plants: &'a [PlantView],
) -> impl Iterator<Item = &'a PlantView> {
    plants.iter().filter(|plant| {
        plant.kind == state.supply_kind && !state.starter_generators.contains(&plant.id)
    })
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
    generator_queued: bool,
    generator_linked: bool,
}

impl Facts {
    fn of(state: &TutorialState, game: &Game) -> Self {
        let session = game.session();
        let plants = session.plant_views();
        let lines = session.line_views();
        let nodes: Vec<NodeId> = tutorial_batteries(state, &plants)
            .map(|battery| battery.node)
            .collect();
        let generators: Vec<NodeId> = tutorial_generators(state, &plants)
            .map(|plant| plant.node)
            .collect();
        Self {
            generator_queued: !generators.is_empty(),
            generator_linked: plants
                .iter()
                .filter(|plant| {
                    plant.kind != PlantKind::Battery
                        && !state.starter_generators.contains(&plant.id)
                })
                .any(|plant| {
                    session
                        .power()
                        .network()
                        .months_until_connected(plant.node)
                        .is_some()
                }),
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
            Done::CellSelectedOrGenerator => {
                matches!(selection, Selection::Cell(_)) || self.generator_queued
            }
            Done::GeneratorQueued => self.generator_queued,
            Done::GeneratorLinked => self.generator_linked,
            Done::GeneratorNotLinked => !self.generator_linked,
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
        plant_site(game, PlantKind::Battery)
    }

    fn plant_site(game: &Game, kind: PlantKind) -> (CellCoord, NodeId) {
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
            .filter(|cell| session.plant_quote(kind, *cell).is_ok())
            .min_by(|a, b| {
                domain::grid::distance_cells(*a, home)
                    .total_cmp(&domain::grid::distance_cells(*b, home))
            })
            .expect("somewhere to build the plant");
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
        assert_eq!(steps.len(), 28);
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
    #[test]
    fn regional_supply_guidance_chooses_solar_only_for_brazil_and_southern_china() {
        for region in [
            "brazil_sin",
            "southern_china",
            "western_interconnection",
            "eastern_interconnection",
            "continental_europe_maghreb",
            "ips_ups",
            "india_nepal_bhutan",
            "sgcc",
        ] {
            let game = game(region);
            let mut state = TutorialState::new(1, &game);
            assert_eq!(state.plant_kind(), PlantKind::Battery, "{region}");
            state.seen_review = true;
            let expected = if matches!(region, "brazil_sin" | "southern_china") {
                PlantKind::Solar
            } else {
                PlantKind::Ccgt
            };
            assert_eq!(state.plant_kind(), expected, "{region}");
            assert!(
                tutorial_plant_nodes(&state, &game).is_empty(),
                "starter plants in {region}"
            );
        }
    }

    #[test]
    fn ccgt_chapter_starts_only_after_review_and_ignores_starter_generators() {
        let mut game = game("western_interconnection");
        let mut state = TutorialState::new(1, &game);
        assert!(!state.starter_generators.is_empty());
        assert_eq!(state.plant_kind(), PlantKind::Battery);
        play_month(&mut game);
        state.seen_review = true;
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::Construction)
        );
        game.execute(Command::FinishReview).unwrap();
        assert_eq!(state.plant_kind(), PlantKind::Ccgt);
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::Supply)
        );
        state.played.insert(TutorialStep::Supply);
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::PickGeneratorSite)
        );
        let (cell, _) = battery_site(&game);
        assert_eq!(
            active(&state, &game, Selection::Cell(cell)),
            Some(TutorialStep::RevealGenerator)
        );
        state.played.insert(TutorialStep::RevealGenerator);
        assert_eq!(
            active(&state, &game, Selection::Cell(cell)),
            Some(TutorialStep::GeneratorSpotlight)
        );
        state.played.insert(TutorialStep::GeneratorSpotlight);
        assert_eq!(
            active(&state, &game, Selection::Cell(cell)),
            Some(TutorialStep::BuildGenerator)
        );
        assert_eq!(
            active(&state, &game, Selection::None),
            Some(TutorialStep::PickGeneratorSite)
        );
    }

    #[test]
    fn another_generator_kind_advances_to_save_only_after_connecting() {
        for (region, other) in [
            ("western_interconnection", PlantKind::Solar),
            ("brazil_sin", PlantKind::Ccgt),
            ("southern_china", PlantKind::Ccgt),
        ] {
            let mut game = game(region);
            let mut state = TutorialState::new(1, &game);
            state.seen_review = true;
            state.played = TUTORIAL_STEPS
                .iter()
                .filter(|def| {
                    def.step >= TutorialStep::Supply && def.step < TutorialStep::BuildGenerator
                })
                .map(|def| def.step)
                .collect();
            let (cell, anchor) = battery_site(&game);
            game.execute(Command::BuildPlant { kind: other, cell })
                .unwrap();
            let node = game
                .session()
                .plant_views()
                .into_iter()
                .find(|plant| plant.kind == other && plant.cell == cell)
                .unwrap()
                .node;
            assert!(!generator_linked(&state, &game), "{region}");
            assert_eq!(
                active(&state, &game, Selection::Cell(cell)),
                Some(TutorialStep::BuildGenerator),
                "an unconnected alternative does not complete the walkthrough: {region}"
            );
            game.execute(Command::BuildLine {
                from: anchor,
                to: node,
            })
            .unwrap();
            assert!(tutorial_plant_nodes(&state, &game).is_empty(), "{region}");
            assert!(generator_linked(&state, &game), "{region}");
            assert_eq!(
                active(&state, &game, Selection::Cell(cell)),
                Some(TutorialStep::SaveSnapshot),
                "{region}"
            );
        }
    }

    #[test]
    fn every_new_generating_plant_can_unlock_save_but_storage_and_starters_cannot() {
        for kind in PlantKind::ALL {
            let mut game = if kind == PlantKind::Geothermal {
                proven_geothermal_game()
            } else {
                game("western_interconnection")
            };
            let mut state = TutorialState::new(1, &game);
            state.seen_review = true;
            assert!(!generator_linked(&state, &game), "starter fleet: {kind:?}");
            let (cell, anchor) = plant_site(&game, kind);
            game.execute(Command::BuildPlant { kind, cell }).unwrap();
            let node = game
                .session()
                .plant_views()
                .into_iter()
                .find(|plant| plant.kind == kind && plant.cell == cell)
                .unwrap()
                .node;
            assert!(!generator_linked(&state, &game), "isolated {kind:?}");
            if kind == PlantKind::Geothermal {
                // Connections are forbidden during exploration; use the guaranteed-success
                // fixture and complete its region-authored exploration period first.
                let months = game
                    .session()
                    .region()
                    .geothermal()
                    .unwrap()
                    .exploration_months;
                for _ in 0..months {
                    play_month(&mut game);
                    game.execute(Command::FinishReview).unwrap();
                }
            }
            let remote = game
                .session()
                .city_views()
                .into_iter()
                .find(|city| !game.session().power().network().is_connected(city.node))
                .unwrap()
                .node;
            game.execute(Command::BuildLine {
                from: node,
                to: remote,
            })
            .unwrap();
            assert!(!generator_linked(&state, &game), "dangling {kind:?}");
            game.execute(Command::BuildLine {
                from: remote,
                to: anchor,
            })
            .unwrap();
            let qualifies = kind != PlantKind::Battery;
            assert_eq!(generator_linked(&state, &game), qualifies, "{kind:?}");
            assert_eq!(
                active(&state, &game, Selection::None),
                Some(if qualifies {
                    TutorialStep::SaveSnapshot
                } else {
                    TutorialStep::Supply
                }),
                "{kind:?}"
            );
        }
    }

    fn proven_geothermal_game() -> Game {
        struct Catalog;
        impl application::RegionCatalog for Catalog {
            fn list(
                &self,
            ) -> Result<Vec<application::RegionListing>, application::RegionLoadError> {
                Ok(Vec::new())
            }
            fn load(
                &self,
                _: &domain::ids::RegionId,
            ) -> Result<application::LoadedRegion, application::RegionLoadError> {
                Ok(application::LoadedRegion {
                    definition: std::sync::Arc::new(
                        domain::region::RegionDefinition::try_from(
                            domain::region::fixtures::geothermal_acceptance_parts(),
                        )
                        .unwrap(),
                    ),
                    source_bytes: Vec::new(),
                    fingerprint: application::RegionFingerprint::new([0; 32]),
                })
            }
        }
        Game::start_new(
            &Catalog,
            &domain::ids::RegionId::new("tiny"),
            7,
            domain::session::GameSettings::default(),
            Box::new(InMemoryCommandLog::default()),
        )
        .unwrap()
    }

    #[test]
    fn early_generator_order_skips_build_steps_and_its_line_completes_connecting() {
        for region in ["western_interconnection", "brazil_sin", "southern_china"] {
            let mut game = game(region);
            let mut state = TutorialState::new(1, &game);
            let kind = state.supply_kind;
            let (cell, anchor) = battery_site(&game);
            game.execute(Command::BuildPlant { kind, cell }).unwrap();
            let node = game
                .session()
                .plant_views()
                .into_iter()
                .find(|plant| plant.kind == kind && plant.cell == cell)
                .unwrap()
                .node;
            play_month(&mut game);
            state.seen_review = true;
            game.execute(Command::FinishReview).unwrap();
            state.played.insert(TutorialStep::Supply);
            assert_eq!(tutorial_plant_nodes(&state, &game), BTreeSet::from([node]));
            assert_eq!(
                active(&state, &game, Selection::None),
                Some(TutorialStep::ConnectGeneratorSpotlight)
            );
            state.played.insert(TutorialStep::ConnectGeneratorSpotlight);
            assert_eq!(
                active(&state, &game, Selection::None),
                Some(TutorialStep::ConnectGenerator)
            );
            assert!(!generator_linked(&state, &game));
            game.execute(Command::BuildLine {
                from: anchor,
                to: node,
            })
            .unwrap();
            assert!(generator_linked(&state, &game));
            assert_eq!(
                active(&state, &game, Selection::None),
                Some(TutorialStep::SaveSnapshot)
            );
            state.played.insert(TutorialStep::SaveSnapshot);
            assert_eq!(
                active(&state, &game, Selection::None),
                Some(TutorialStep::CrystalBall)
            );
            state.played.insert(TutorialStep::CrystalBall);
            assert_eq!(active(&state, &game, Selection::None), None);
        }
    }

    #[test]
    fn a_generator_line_to_an_unconnected_city_still_needs_a_path_to_the_grid() {
        for region in ["western_interconnection", "brazil_sin", "southern_china"] {
            let mut game = game(region);
            let state = TutorialState::new(1, &game);
            let kind = state.supply_kind;
            let (cell, anchor) = battery_site(&game);
            game.execute(Command::BuildPlant { kind, cell }).unwrap();
            let node = game
                .session()
                .plant_views()
                .into_iter()
                .find(|plant| plant.kind == kind && plant.cell == cell)
                .unwrap()
                .node;
            let remote = game
                .session()
                .city_views()
                .into_iter()
                .find(|city| !game.session().power().network().is_connected(city.node))
                .unwrap()
                .node;
            game.execute(Command::BuildLine {
                from: node,
                to: remote,
            })
            .unwrap();
            assert!(
                !generator_linked(&state, &game),
                "a dangling line cannot supply the existing grid"
            );
            game.execute(Command::BuildLine {
                from: remote,
                to: anchor,
            })
            .unwrap();
            assert!(
                generator_linked(&state, &game),
                "a complete ordered path counts before commissioning"
            );
        }
    }

    #[test]
    fn restored_payoff_lesson_reoffers_its_closed_dialog_and_preserves_finished_notes() {
        let game = game("western_interconnection");
        let mut state = TutorialState::new(1, &game);
        state.seen_review = true;
        state.payoff_month = Some(game.session().clock().month());
        state.played = played(&[TutorialStep::OpenPayoff, TutorialStep::PayoffConstruction]);
        let restored = TutorialState::restore(&state.saved().encode().unwrap(), 9, &game).unwrap();
        assert_eq!(restored.generation, 9);
        assert!(restored.played.contains(&TutorialStep::PayoffConstruction));
        assert_eq!(
            active(&restored, &game, Selection::None),
            Some(TutorialStep::OpenPayoff)
        );
        state
            .played
            .extend([TutorialStep::PayoffOperating, TutorialStep::ClosePayoff]);
        let restored = TutorialState::restore(&state.saved().encode().unwrap(), 9, &game).unwrap();
        assert_eq!(
            active(&restored, &game, Selection::None),
            Some(TutorialStep::RenewableSiting)
        );
        state.played.insert(TutorialStep::RenewableSiting);
        assert!(TutorialState::restore(&state.saved().encode().unwrap(), 9, &game).is_none());
    }

    #[test]
    fn unknown_or_invalid_tutorial_fields_do_not_restore_a_run() {
        let game = game("western_interconnection");
        let saved = TutorialState::new(1, &game).saved();
        for invalid in 0..4 {
            let mut altered = saved.clone();
            match invalid {
                0 => altered.version = 99,
                1 => {
                    altered.played.insert("unknown-future-lesson".into());
                }
                2 => altered.supply_kind = "unknown-plant".into(),
                _ => altered.payoff_month = Some((2030, 13)),
            }
            assert!(TutorialState::restore(&altered.encode().unwrap(), 9, &game).is_none());
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(16))]
        #[test]
        fn tutorial_checkpoint_round_trip_preserves_bookkeeping_without_changing_the_game(
            mask in proptest::num::u32::ANY,
            seen_review in proptest::bool::ANY,
            outlook_used in proptest::bool::ANY,
            payoff_scheduled in proptest::bool::ANY,
        ) {
            let game = game("western_interconnection");
            let before = game.session().state_hash();
            let mut state = TutorialState::new(1, &game);
            state.played = TUTORIAL_STEPS.iter().enumerate()
                .filter(|(index, def)| mask & (1 << index) != 0 && !matches!(def.step, TutorialStep::OpenPayoff | TutorialStep::RenewableSiting))
                .map(|(_, def)| def.step).collect();
            state.seen_review = seen_review;
            state.outlook_used = outlook_used;
            state.payoff_month = payoff_scheduled.then(|| game.session().clock().month().checked_add_months(1).unwrap());
            let restored = TutorialState::restore(&state.saved().encode().unwrap(), 9, &game).unwrap();
            state.generation = 9;
            proptest::prop_assert_eq!(restored, state);
            proptest::prop_assert_eq!(game.session().state_hash(), before);
        }
    }

    proptest::proptest! {
        #[test]
        fn planning_never_shows_steps_from_the_other_chapter(seen_review in proptest::bool::ANY, mask in proptest::num::u32::ANY) {
            let game = game("western_interconnection");
            let mut state = TutorialState::new(1, &game);
            state.seen_review = seen_review;
            state.played = TUTORIAL_STEPS.iter().enumerate()
                .filter(|(index, _)| mask & (1 << index) != 0).map(|(_, def)| def.step).collect();
            if let Some(def) = active_step(&state, &game, Selection::None) {
                proptest::prop_assert_eq!(def.step >= TutorialStep::Supply, seen_review);
            }
        }
    }
}
