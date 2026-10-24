use domain::{
    region::fixtures::tiny_region,
    session::{GameSession, GameSettings},
};
use std::sync::Arc;

#[test]
fn portable_state_preserves_the_complete_session_without_debug_or_replay() {
    let region = Arc::new(tiny_region());
    let mut game = GameSession::new(region.clone(), 42, GameSettings::default());
    game.begin_operating().unwrap();
    for _ in 0..500 {
        game.tick().unwrap();
    }
    let bytes = game.snapshot_state();
    let restored = GameSession::from_snapshot_state(&bytes, region.clone()).unwrap();
    assert_eq!(restored.state_hash(), game.state_hash());
    let mut a = game;
    let mut b = restored;
    for _ in 0..30 {
        assert_eq!(a.tick(), b.tick());
    }
    assert_eq!(a.state_hash(), b.state_hash());
    assert!(GameSession::from_snapshot_state(&bytes[..bytes.len() - 1], region).is_err());
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(24))]
    #[test]
    fn arbitrary_tick_boundaries_preserve_state_and_rng(seed in proptest::prelude::any::<u64>(), ticks in 0u32..1440) {
        let region=Arc::new(tiny_region());
        let mut live=GameSession::new(region.clone(),seed,GameSettings::default());live.begin_operating().unwrap();
        for _ in 0..ticks {live.tick().unwrap();}
        let bytes=live.snapshot_state();
        let mut restored=GameSession::from_snapshot_state(&bytes,region).unwrap();
        proptest::prop_assert_eq!(live.state_hash(),restored.state_hash());
        for _ in ticks..(ticks+5).min(1440) {proptest::prop_assert_eq!(live.tick(),restored.tick());}
        proptest::prop_assert_eq!(live.state_hash(),restored.state_hash());
    }
    #[test]
    fn hostile_state_bytes_return_an_error_without_panicking(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(),0..2048)) {
        let _=GameSession::from_snapshot_state(&bytes,Arc::new(tiny_region()));
    }
}
