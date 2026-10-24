use super::*;
use crate::region::{PlantKind, RegionDefinition, StartPlantDef, fixtures};

#[test]
fn wind_storage_resume_preserves_rng_history_and_midnight_at_each_policy_boundary() {
    let mut parts = fixtures::tiny_region_parts();
    parts.start.fleet.extend([
        StartPlantDef {
            kind: PlantKind::Wind,
            cell: CellCoord { x: 5, y: 1 },
        },
        StartPlantDef {
            kind: PlantKind::Battery,
            cell: CellCoord { x: 5, y: 2 },
        },
    ]);
    parts.demand.forecast_error = Ratio::ZERO;
    let region = Arc::new(RegionDefinition::try_from(parts).unwrap());
    let mut live = GameSession::new(region.clone(), 42, GameSettings::default());
    let farm = live
        .plant_views()
        .into_iter()
        .find(|p| p.kind == PlantKind::Wind)
        .unwrap()
        .id;
    for command in 0..5 {
        let before = live.snapshot_state();
        let result = match command {
            0 => live.commit_plant(farm, [true; 24]),
            1 => live.start_plant(farm),
            2 => live.stop_plant(farm),
            3 => live.set_auto_follow(farm, true),
            _ => live.set_dispatch_target(farm, crate::units::Megawatts::new(100.0)),
        };
        assert!(result.is_err());
        assert!(
            live.snapshot_state() == before,
            "refused control changes no state"
        );
    }
    live.begin_operating().unwrap();
    for boundary in [0, 1, 60, 1019, 1020, 1079, 1080, 1439, 1440] {
        while live.clock.tick() < boundary {
            live.tick().unwrap();
        }
        let bytes = live.snapshot_state();
        let mut a = live.clone();
        let mut b = GameSession::from_snapshot_state(&bytes, region.clone()).unwrap();
        assert!(a.snapshot_state() == b.snapshot_state());
        for _ in boundary..(boundary + 2).min(1440) {
            assert_eq!(a.tick().unwrap(), b.tick().unwrap());
            assert!(a.snapshot_state() == b.snapshot_state());
            a.check_invariants().unwrap();
            b.check_invariants().unwrap();
        }
    }
    live.finish_review().unwrap();
    // A new month's Planning state still retains the previous wind sample as history.
    let restored = GameSession::from_snapshot_state(&live.snapshot_state(), region).unwrap();
    assert!(live.snapshot_state() == restored.snapshot_state());
}
