//! Determinism: a live session and its replay from the command log end in the
//! same state, whatever the seed and whatever the player scripted.

#[allow(dead_code)]
mod support;

use application::{Command, LogRecord, replay};
use domain::ids::PlantId;
use domain::region::fixtures::tiny_region_parts;
use domain::units::Megawatts;
use proptest::array::uniform24;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;
use support::{fleet_plants, game_on, tiny_loaded_region};

fn fleet_plant() -> impl Strategy<Value = PlantId> {
    select(fleet_plants())
}

/// A planning-phase command on one of the start fleet's plants. Many are
/// rejected (bad patterns, targets outside the stable range); rejected
/// commands are never logged, so replay must agree about those too.
fn arb_planning_command() -> impl Strategy<Value = Command> {
    prop_oneof![
        (fleet_plant(), uniform24(any::<bool>()))
            .prop_map(|(plant, hours)| Command::CommitPlant { plant, hours }),
        (fleet_plant(), any::<bool>()).prop_map(|(plant, on)| Command::SetAutoFollow { plant, on }),
        (fleet_plant(), 0.0..800.0f64).prop_map(|(plant, mw)| Command::SetDispatchTarget {
            plant,
            mw: Megawatts::new(mw)
        }),
        fleet_plant().prop_map(|plant| Command::StartPlant { plant }),
    ]
}

fn arb_planning_script() -> impl Strategy<Value = Vec<Command>> {
    vec(arb_planning_command(), 0..6)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn live_and_replayed_sessions_hash_equal(seed in any::<u64>(), script in arb_planning_script()) {
        let (mut g, log) = game_on(tiny_region_parts(), seed);
        for c in script {
            let _ = g.execute(c);
        }
        let _ = g.execute(Command::BeginOperating);
        let _ = g.execute(Command::AdvanceTicks { n: 1440 });

        let records = log.records();
        let commands: Vec<Command> = records
            .iter()
            .filter_map(|r| match r {
                LogRecord::Command(c) => Some(c.clone()),
                LogRecord::Header(_) => None,
            })
            .collect();
        let header = records
            .iter()
            .find_map(|r| match r {
                LogRecord::Header(h) => Some(h.clone()),
                LogRecord::Command(_) => None,
            })
            .unwrap();
        let replayed = replay(tiny_loaded_region(), &header, &commands).unwrap();
        prop_assert_eq!(replayed.state_hash(), g.session().state_hash());
    }
}
