//! Robustness: any sequence of commands, valid or not, leaves the session
//! consistent. A reference model tracks the day phase; the real `Game` runs
//! through its full pipeline, and in debug builds that includes the
//! `InvariantCheckLayer`, which panics on any broken invariant.
//!
//! The model cannot foresee the one outcome that depends on the economics of
//! the played month: the game ending (bankruptcy or a revoked licence) when a
//! month closes. The system-under-test wrapper therefore latches that fact
//! when it first sees it, and from then on holds the session to the
//! absorbing-state rules (every command refused, nothing changes) instead of
//! to the model's phase.

#[allow(dead_code)]
mod support;

use application::{Command, CommandError, Game};
use domain::error::DomainError;
use domain::events::DomainEvent;
use domain::ids::{CellCoord, NodeId, PlantId};
use domain::region::PlantKind;
use domain::region::fixtures::tiny_region_parts;
use domain::session::{DayPhase, SessionStatus};
use domain::time::TICKS_PER_DAY;
use domain::units::Megawatts;
use proptest::array::uniform24;
use proptest::prelude::*;
use proptest::sample::select;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest, prop_state_machine};
use support::game_on;

/// What the model knows about a game: its seed, the day phase and the tick
/// within the day, and the ids the start state hands out.
#[derive(Debug, Clone)]
struct Model {
    seed: u64,
    phase: DayPhase,
    tick: u32,
    plants: Vec<PlantId>,
    nodes: Vec<NodeId>,
}

impl Model {
    fn new(seed: u64) -> Self {
        let (game, _) = game_on(tiny_region_parts(), seed);
        let session = game.session();
        let mut nodes = vec![session.substation()];
        nodes.extend(session.city_views().iter().map(|city| city.node));
        nodes.extend(session.plant_views().iter().map(|plant| plant.node));
        Model {
            seed,
            phase: session.phase(),
            tick: session.clock().tick(),
            plants: session.plant_views().iter().map(|plant| plant.id).collect(),
            nodes,
        }
    }
}

/// The phases in which the domain accepts `command`.
fn allowed_phases(command: &Command) -> &'static [DayPhase] {
    use DayPhase::{Operating, Planning, Review};
    match command {
        Command::BuildPlant { .. }
        | Command::BuildLine { .. }
        | Command::CommitPlant { .. }
        | Command::BeginOperating => &[Planning],
        Command::SetDispatchTarget { .. }
        | Command::SetAutoFollow { .. }
        | Command::StartPlant { .. }
        | Command::StopPlant { .. } => &[Planning, Operating],
        Command::AdvanceTicks { .. } => &[Operating],
        Command::FinishReview => &[Review],
    }
}

/// Commands that always succeed in their phase while the game is active.
fn always_succeeds_in_phase(command: &Command) -> bool {
    matches!(
        command,
        Command::BeginOperating | Command::AdvanceTicks { .. } | Command::FinishReview
    )
}

fn plant_id(model: &Model) -> BoxedStrategy<PlantId> {
    // Mostly ids from the views; occasionally any small id, which may name a
    // node, a line, a not-yet-built plant or nothing at all.
    prop_oneof![
        8 => select(model.plants.clone()),
        1 => (0u32..64).prop_map(PlantId),
    ]
    .boxed()
}

fn node_id(model: &Model) -> BoxedStrategy<NodeId> {
    prop_oneof![
        8 => select(model.nodes.clone()),
        1 => (0u32..64).prop_map(NodeId),
    ]
    .boxed()
}

fn cell() -> impl Strategy<Value = CellCoord> {
    // The tiny grid is 12 x 8; the margin exercises out-of-bounds cells.
    (0u16..14, 0u16..10).prop_map(|(x, y)| CellCoord { x, y })
}

fn hours() -> impl Strategy<Value = [bool; 24]> {
    prop_oneof![
        Just([true; 24]),
        Just([false; 24]),
        uniform24(any::<bool>()),
    ]
}

fn advance() -> impl Strategy<Value = Command> {
    select(vec![0u32, 1, 60, 1440]).prop_map(|n| Command::AdvanceTicks { n })
}

struct SessionMachine;

impl ReferenceStateMachine for SessionMachine {
    type State = Model;
    type Transition = Command;

    fn init_state() -> BoxedStrategy<Model> {
        any::<u64>().prop_map(Model::new).boxed()
    }

    fn transitions(model: &Model) -> BoxedStrategy<Command> {
        // Lean towards the command that moves the current phase along, so a
        // 40-command run closes a few months, without ever excluding the
        // wrong-phase commands.
        let (begin, step, finish) = match model.phase {
            DayPhase::Planning => (6, 1, 1),
            DayPhase::Operating => (1, 8, 1),
            DayPhase::Review => (1, 1, 6),
        };
        prop_oneof![
            begin => Just(Command::BeginOperating),
            step => advance(),
            finish => Just(Command::FinishReview),
            2 => (select(PlantKind::ALL.to_vec()), cell())
                .prop_map(|(kind, cell)| Command::BuildPlant { kind, cell }),
            2 => (node_id(model), node_id(model))
                .prop_map(|(from, to)| Command::BuildLine { from, to }),
            3 => (plant_id(model), hours())
                .prop_map(|(plant, hours)| Command::CommitPlant { plant, hours }),
            3 => (plant_id(model), -50.0..1_300.0f64).prop_map(|(plant, mw)| {
                Command::SetDispatchTarget { plant, mw: Megawatts::new(mw) }
            }),
            3 => (plant_id(model), any::<bool>())
                .prop_map(|(plant, on)| Command::SetAutoFollow { plant, on }),
            2 => plant_id(model).prop_map(|plant| Command::StartPlant { plant }),
            2 => plant_id(model).prop_map(|plant| Command::StopPlant { plant }),
        ]
        .boxed()
    }

    fn apply(mut model: Model, command: &Command) -> Model {
        if !allowed_phases(command).contains(&model.phase) {
            return model;
        }
        match command {
            Command::BeginOperating => {
                model.phase = DayPhase::Operating;
                model.tick = 0;
            }
            Command::AdvanceTicks { n } => {
                if model.tick + n >= TICKS_PER_DAY {
                    model.phase = DayPhase::Review;
                    model.tick = 0;
                } else {
                    model.tick += n;
                }
            }
            Command::FinishReview => model.phase = DayPhase::Planning,
            _ => {}
        }
        model
    }
}

/// The real game, plus whether it has been seen to end.
struct Sut {
    game: Game,
    over: bool,
}

struct SessionTest;

impl StateMachineTest for SessionTest {
    type SystemUnderTest = Sut;
    type Reference = SessionMachine;

    fn init_test(model: &Model) -> Sut {
        let (game, _) = game_on(tiny_region_parts(), model.seed);
        Sut { game, over: false }
    }

    fn apply(mut sut: Sut, _model: &Model, command: Command) -> Sut {
        let phase = sut.game.session().phase();
        let hash = sut.game.session().state_hash();
        let result = sut.game.execute(command.clone());

        if sut.over {
            assert!(
                matches!(
                    result,
                    Err(CommandError::Domain(DomainError::GameOver { .. }))
                ),
                "a finished game refuses {command:?}, got {result:?}"
            );
        } else if !allowed_phases(&command).contains(&phase) {
            assert_eq!(
                result.as_ref().err(),
                Some(&CommandError::Domain(DomainError::WrongPhase {
                    actual: phase
                })),
                "{command:?} is wrong in {phase:?}"
            );
        } else if always_succeeds_in_phase(&command) {
            assert!(
                result.is_ok(),
                "{command:?} must succeed in {phase:?}: {result:?}"
            );
        }

        match &result {
            Err(_) => assert_eq!(
                sut.game.session().state_hash(),
                hash,
                "a refused {command:?} changed the session"
            ),
            Ok(events) => {
                if let SessionStatus::GameOver(reason) = sut.game.session().status() {
                    assert!(!sut.over, "a finished game accepted {command:?}");
                    assert!(
                        matches!(command, Command::AdvanceTicks { .. })
                            && sut.game.session().phase() == DayPhase::Review
                            && events.iter().any(|event| matches!(
                                event,
                                DomainEvent::Bankrupt | DomainEvent::LicenseRevoked
                            )),
                        "the game ended ({reason:?}) other than by a month close"
                    );
                    sut.over = true;
                }
            }
        }
        sut
    }

    fn check_invariants(sut: &Sut, model: &Model) {
        let session = sut.game.session();
        assert_eq!(session.check_invariants(), Ok(()));
        if sut.over {
            // Game over is absorbing: the final month stays in review.
            assert_eq!(session.phase(), DayPhase::Review);
        } else {
            assert_eq!(session.phase(), model.phase);
            assert_eq!(session.status(), SessionStatus::Active);
            if model.phase == DayPhase::Operating {
                assert_eq!(session.clock().tick(), model.tick, "tick within the day");
            }
        }
    }
}

prop_state_machine! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn any_command_sequence_keeps_the_session_consistent(sequential 1..=40 => SessionTest);
}
