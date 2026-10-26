use application::{
    Command, Game, InMemoryCommandLog, ReplayCompatibility, SaveBundle, SaveId, SaveKind,
    SaveMetadata,
};
use domain::{
    company::AssetRef,
    generation::PlantStatus,
    ids::{CellCoord, RegionId},
    region::PlantKind,
    session::GameSettings,
};
use infrastructure::{
    region::RonRegionCatalog,
    save::{decode_save, encode_save},
};

fn game() -> (tempfile::TempDir, Game) {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("minimal.ron"),
        include_bytes!("support/economic_region_v4.ron"),
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
    (dir, game)
}
fn close(game: &mut Game) {
    game.execute(Command::BeginOperating).unwrap();
    game.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
}

fn zoned_hvdc_game(mask: u8) -> (tempfile::TempDir, Game) {
    use infrastructure::region::{AreaDto, HvdcConstructionDto, HvdcDto, RegionFileV4};
    let dir = tempfile::TempDir::new().unwrap();
    let mut file: RegionFileV4 =
        ron::from_str(include_str!("support/economic_region_v4.ron")).unwrap();
    // This fixture's flat demand keeps the longer construction cases about
    // pricing and persistence; stochastic protection failures are tested elsewhere.
    file.demand_noise.forecast_error = 0.0;
    file.areas = vec![
        AreaDto {
            id: 1,
            name_key: "area-east".into(),
            polygons: vec![vec![(6.0, 0.0), (12.0, 0.0), (12.0, 8.0), (6.0, 8.0)]],
            geography: Vec::new(),
        },
        AreaDto {
            id: 2,
            name_key: "area-west".into(),
            polygons: vec![vec![(1.0, 0.0), (6.0, 0.0), (6.0, 8.0), (1.0, 8.0)]],
            geography: Vec::new(),
        },
    ];
    file.hvdc = Some(HvdcDto {
        rating_mw: 200.0,
        converter_cost: 101,
        cost_per_cell: 7,
        converter_build_months: 2,
        ramp_mw_per_min: 20.0,
        converter_loss: 0.01,
        loss_per_cell: 0.001,
        book_life_months: 240,
        designed_hours_per_year: 4000.0,
    });
    let zones = &mut file.economic_zones.as_mut().unwrap().zones;
    if mask & 1 != 0 {
        zones[0].hvdc = HvdcConstructionDto {
            converter_cost_per_end: Some(41),
            converter_build_months: Some(3),
            cost_per_cell: Some(11),
            cells_per_month: Some(1),
        };
    }
    if mask & 2 != 0 {
        zones[1].hvdc = HvdcConstructionDto {
            converter_cost_per_end: Some(59),
            converter_build_months: Some(5),
            cost_per_cell: Some(13),
            cells_per_month: Some(3),
        };
    }
    std::fs::write(
        dir.path().join("minimal.ron"),
        ron::to_string(&file).unwrap(),
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
    (dir, game)
}

fn economic_round_trip(game: &Game) -> Game {
    let compatibility = ReplayCompatibility {
        build: "test".into(),
        platform: "test".into(),
    };
    let bundle = SaveBundle {
        compatibility: compatibility.clone(),
        metadata: SaveMetadata {
            slot: SaveId::new("a".repeat(32)).unwrap(),
            run: SaveId::new("b".repeat(32)).unwrap(),
            name: "HVDC markets".into(),
            kind: SaveKind::Manual,
            saved_at: 0,
        },
        checkpoint: game.checkpoint(),
    };
    let (decoded, region) = decode_save(&encode_save(&bundle).unwrap(), &compatibility).unwrap();
    let restored = decoded
        .restore(
            region,
            &compatibility,
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )
        .unwrap();
    assert_eq!(restored.checkpoint(), game.checkpoint());
    restored
}

#[test]
fn zoned_hvdc_partial_overrides_survive_payments_cancellation_commissioning_and_resume() {
    use domain::{company::LedgerCategory, grid::LineStatus, units::Money};
    // For home (6,3) to remote (3,5), four samples cross west three times and east once.
    // Base converters cost EUR 50.50 per end. Route terms and terminal durations
    // deliberately dominate in different cases; west-only instalments have a remainder.
    for (mask, expected_cents, expected_months) in [
        (0, 12_900, 2),
        (1, 13_150, 4),
        (2, 14_350, 5),
        (3, 14_600, 5),
    ] {
        for reverse in [false, true] {
            let (dir, mut live) = zoned_hvdc_game(mask);
            let home = live.session().anchor_node();
            let far = live
                .session()
                .city_views()
                .into_iter()
                .find(|c| c.id.0 == 3)
                .unwrap()
                .node;
            let (from, to) = if reverse { (far, home) } else { (home, far) };
            let before = live.checkpoint();
            let quote = live.session().link_quote(from, to).unwrap();
            assert_eq!(
                (quote.capital.cents(), quote.months),
                (expected_cents, expected_months)
            );
            assert_eq!(quote, live.session().link_quote(to, from).unwrap());
            assert_eq!(live.checkpoint(), before, "quotes must be pure");
            let started = live.session().clock().month();
            live.execute(Command::BuildLink { from, to }).unwrap();
            live.execute(Command::BuildLink { from: to, to: from })
                .unwrap();
            let links: Vec<_> = live
                .session()
                .power()
                .network()
                .links()
                .map(|link| link.id)
                .collect();
            let (kept, cancelled) = (links[0], links[1]);
            assert_eq!(live.session().link_construction(kept), Some(quote.clone()));
            let link = live.session().power().network().link(kept).unwrap();
            assert_eq!(
                (link.from, link.to),
                (from, to),
                "pricing must not reverse flow orientation"
            );
            live.execute(Command::BeginOperating).unwrap();
            live.execute(Command::AdvanceTicks { n: 100 }).unwrap();
            // The standalone save must retain the authored partial overrides, not re-read
            // changed installed data. This capture still has every instalment pending.
            std::fs::write(dir.path().join("minimal.ron"), b"changed installed source").unwrap();
            let mut resumed = economic_round_trip(&live);
            let base = expected_cents / i64::from(expected_months);
            let mut expected_paid = Vec::new();
            for month in 1..=expected_months {
                for game in [&mut live, &mut resumed] {
                    if month > 1 {
                        game.execute(Command::BeginOperating).unwrap();
                    }
                    game.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
                }
                expected_paid.push(-if month == expected_months {
                    expected_cents - base * i64::from(expected_months - 1)
                } else {
                    base
                });
                assert_eq!(
                    live.session().status(),
                    domain::session::SessionStatus::Active,
                    "mask={mask}, reverse={reverse}, month={month}"
                );
                let payments = |game: &Game, id| {
                    game.session()
                        .company()
                        .ledger()
                        .iter()
                        .filter(|entry| {
                            entry.category == LedgerCategory::Construction
                                && entry.subject == Some(AssetRef::Link(id))
                        })
                        .map(|entry| entry.amount.cents())
                        .collect::<Vec<_>>()
                };
                assert_eq!(payments(&live, kept), expected_paid);
                assert_eq!(live.checkpoint(), resumed.checkpoint());
                assert_eq!(live.session().link_construction(kept), Some(quote.clone()));
                let status = if month == expected_months {
                    LineStatus::InService
                } else {
                    LineStatus::UnderConstruction {
                        months_left: expected_months - month,
                    }
                };
                assert_eq!(
                    live.session().power().network().link(kept).unwrap().status,
                    status
                );
                let asset = live
                    .session()
                    .company()
                    .rate_base()
                    .asset(AssetRef::Link(kept));
                if month < expected_months {
                    assert!(asset.is_none());
                } else {
                    let asset = asset.unwrap();
                    assert_eq!(asset.original, Money::from_cents(expected_cents));
                    assert_eq!(
                        asset.added_on,
                        started.checked_add_months(expected_months - 1).unwrap()
                    );
                }
                for game in [&mut live, &mut resumed] {
                    game.execute(Command::FinishReview).unwrap();
                    if month == 1 {
                        game.execute(Command::CancelLinkConstruction { link: cancelled })
                            .unwrap();
                    }
                    assert!(game.session().power().network().link(cancelled).is_none());
                    assert!(
                        game.session()
                            .company()
                            .rate_base()
                            .asset(AssetRef::Link(cancelled))
                            .is_none()
                    );
                    assert_eq!(
                        payments(game, cancelled),
                        [-base],
                        "cancellation keeps only the paid instalment"
                    );
                    game.session().check_invariants().unwrap();
                }
                // Re-save after every close/cancel, including the commissioning boundary.
                resumed = economic_round_trip(&resumed);
                assert_eq!(live.checkpoint(), resumed.checkpoint());
            }
            for game in [&mut live, &mut resumed] {
                game.execute(Command::SetLinkSchedule {
                    link: kept,
                    mw: Box::new([-50.0; 24]),
                })
                .unwrap();
                game.execute(Command::BeginOperating).unwrap();
                game.execute(Command::AdvanceTicks { n: 10 }).unwrap();
            }
            assert_eq!(live.checkpoint(), resumed.checkpoint());
            let restored = economic_round_trip(&resumed);
            let link = restored.session().power().network().link(kept).unwrap();
            assert_eq!((link.from, link.to, link.schedule), (from, to, [-50.0; 24]));
        }
    }
}

#[test]
fn local_orders_quotes_payments_progress_and_rate_base_agree() {
    let (_dir, mut game) = game();
    let west = CellCoord { x: 4, y: 4 };
    let east = CellCoord { x: 8, y: 5 };
    let before = game.checkpoint();
    let a = game.session().plant_quote(PlantKind::Wind, west).unwrap();
    let b = game.session().plant_quote(PlantKind::Wind, east).unwrap();
    assert_eq!(game.checkpoint(), before);
    assert_eq!(
        (a.capital.cents(), a.months, b.capital.cents(), b.months),
        (100_000_000, 2, 200_000_000, 3)
    );
    for cell in [west, east] {
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
        quote,
        game.session()
            .line_quote(plants[1].node, plants[0].node)
            .unwrap()
    );
    game.execute(Command::BuildLine {
        from: plants[0].node,
        to: plants[1].node,
    })
    .unwrap();
    let line = game.session().line_views().into_iter().last().unwrap().id;
    assert_eq!(game.session().line_build_months(line), Some(quote.months));
    let mut paid = 0;
    for month in 1..=3 {
        close(&mut game);
        paid -= game
            .projections()
            .monthly_reports
            .history()
            .last()
            .unwrap()
            .construction
            .cents();
        for p in &plants {
            let expected = if p.cell == west { a } else { b };
            assert_eq!(game.session().plant_construction(p.id).unwrap(), expected);
            let current = game
                .session()
                .plant_views()
                .into_iter()
                .find(|v| v.id == p.id)
                .unwrap();
            assert_eq!(
                matches!(current.status, PlantStatus::UnderConstruction { .. }),
                month < expected.months
            );
            if month >= expected.months {
                let asset = game
                    .session()
                    .company()
                    .rate_base()
                    .assets()
                    .find(|v| v.asset == AssetRef::Plant(p.id))
                    .unwrap();
                assert_eq!(asset.original, expected.capital);
            }
        }
        if month < 3 {
            game.execute(Command::FinishReview).unwrap();
        }
    }
    assert_eq!(
        paid,
        a.capital.cents() + b.capital.cents() + quote.capital.cents()
    );
    let asset = game
        .session()
        .company()
        .rate_base()
        .assets()
        .find(|v| v.asset == AssetRef::Line(line))
        .unwrap();
    assert_eq!(asset.original, quote.capital);
}
#[test]
fn local_inflight_snapshot_restores_with_archived_prices() {
    let (dir, mut game) = game();
    game.execute(Command::BuildPlant {
        kind: PlantKind::Wind,
        cell: CellCoord { x: 4, y: 4 },
    })
    .unwrap();
    game.execute(Command::BuildPlant {
        kind: PlantKind::Wind,
        cell: CellCoord { x: 8, y: 5 },
    })
    .unwrap();
    let plants: Vec<_> = game
        .session()
        .plant_views()
        .into_iter()
        .filter(|p| p.kind == PlantKind::Wind)
        .collect();
    game.execute(Command::BuildLine {
        from: plants[0].node,
        to: plants[1].node,
    })
    .unwrap();
    game.execute(Command::BeginOperating).unwrap();
    game.execute(Command::AdvanceTicks { n: 100 }).unwrap();
    let compatibility = ReplayCompatibility {
        build: "test".into(),
        platform: "test".into(),
    };
    let bundle = SaveBundle {
        compatibility: compatibility.clone(),
        metadata: SaveMetadata {
            slot: SaveId::new("a".repeat(32)).unwrap(),
            run: SaveId::new("b".repeat(32)).unwrap(),
            name: "local".into(),
            kind: SaveKind::Manual,
            saved_at: 0,
        },
        checkpoint: game.checkpoint(),
    };
    let bytes = encode_save(&bundle).unwrap();
    std::fs::write(dir.path().join("minimal.ron"), b"changed installed source").unwrap();
    let (decoded, region) = decode_save(&bytes, &compatibility).unwrap();
    let mut resumed = decoded
        .restore(
            region,
            &compatibility,
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )
        .unwrap();
    assert_eq!(resumed.checkpoint(), game.checkpoint());
    game.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
    resumed.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
    assert_eq!(resumed.checkpoint(), game.checkpoint());
    for _ in 0..2 {
        for candidate in [&mut game, &mut resumed] {
            candidate.execute(Command::FinishReview).unwrap();
            close(candidate);
        }
        assert_eq!(resumed.checkpoint(), game.checkpoint());
    }
}

#[test]
fn rules16_construction_capture_keeps_original_bytes_and_continuation() {
    use sha2::{Digest, Sha256};
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/economic-baseline");
    let sums = std::fs::read_to_string(root.join("SHA256SUMS")).unwrap();
    for row in sums.lines() {
        let (expected, name) = row.split_once("  ").unwrap();
        let hash = Sha256::digest(std::fs::read(root.join(name)).unwrap());
        assert_eq!(
            hash.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            expected
        );
    }
    let expected = std::fs::read_to_string(root.join("expected.txt")).unwrap();
    let value = |key: &str| {
        expected
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
            .unwrap()
            .parse::<u64>()
            .unwrap()
    };
    let identity = ReplayCompatibility {
        build: "test".into(),
        platform: "test".into(),
    };
    let bytes = std::fs::read(root.join("in-flight-rules16.mgsave")).unwrap();
    let (bundle, region) = decode_save(&bytes, &identity).unwrap();
    assert_eq!(bundle.checkpoint.header.sim_rules_version, 16);
    let snapshot = bundle.checkpoint.snapshot.as_ref().unwrap();
    assert_eq!(snapshot.schema, 3);
    assert_eq!(bundle.checkpoint.state_hash, value("boundary_hash"));
    assert_eq!(
        snapshot.session,
        std::fs::read(root.join("session-schema3.bin")).unwrap()
    );
    assert_eq!(
        snapshot.projections,
        std::fs::read(root.join("projections-schema3.bin")).unwrap()
    );
    assert_eq!(
        region.source_bytes,
        std::fs::read(root.join("minimal.ron")).unwrap()
    );
    let original = bundle.checkpoint.clone();
    let mut game = bundle
        .restore(
            region,
            &identity,
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )
        .unwrap();
    // New captures use schema 4; compare the frozen schema-3 representation without
    // rewriting or regenerating any historical bytes or expected outcomes.
    use domain::state::State;
    let historical_snapshot = |game: &Game| application::PortableState {
        journal_hash: game.checkpoint().snapshot.unwrap().journal_hash,
        origin: None,
        schema: 3,
        session: game.session().snapshot_state_schema(3),
        projections: game.projections().state_bytes_schema(3),
    };
    assert_eq!(
        historical_snapshot(&game),
        original.snapshot.clone().unwrap()
    );
    for index in 1..=3 {
        if index > 1 {
            game.execute(Command::FinishReview).unwrap();
            game.execute(Command::BeginOperating).unwrap();
        }
        game.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(
                historical_snapshot(&game).state_hash(),
                value(&format!("close_{index}_hash"))
            );
        }
        let report = game.projections().monthly_reports.history().last().unwrap();
        let construction = expected
            .lines()
            .find_map(|line| line.strip_prefix(&format!("close_{index}_construction=")))
            .unwrap()
            .parse::<i64>()
            .unwrap();
        assert_eq!(report.construction.cents(), construction);
        let paid_plant = game
            .session()
            .company()
            .rate_base()
            .assets()
            .find(|a| a.asset == AssetRef::Plant(domain::ids::PlantId(13)));
        assert_eq!(paid_plant.is_some(), index == 3);
        if let Some(plant) = paid_plant {
            assert_eq!(plant.original.cents(), 30_000_000_000);
        }
    }
}

#[test]
fn forged_effective_specs_and_changed_archived_maps_are_rejected() {
    use domain::state::State;
    let (_dir, mut game) = game();
    let cell = CellCoord { x: 4, y: 4 };
    game.execute(Command::BuildPlant {
        kind: PlantKind::Wind,
        cell,
    })
    .unwrap();
    let checkpoint = game.checkpoint();
    let region = infrastructure::region::load_region_bytes(
        checkpoint.region_source.clone(),
        &RegionId::new("minimal"),
    )
    .unwrap();
    let spec = region
        .definition
        .plant_spec_at(PlantKind::Wind, cell)
        .unwrap();
    let bytes = spec.state_bytes();
    for price in [true, false] {
        let mut candidate = checkpoint.clone();
        let snapshot = candidate.snapshot.as_mut().unwrap();
        let offset = snapshot
            .session
            .windows(bytes.len())
            .position(|p| p == bytes)
            .unwrap();
        let mut forged = spec.clone();
        if price {
            forged.capital_cost = domain::units::Money::from_cents(1);
        } else {
            forged.build_months += 1;
        }
        snapshot
            .session
            .splice(offset..offset + bytes.len(), forged.state_bytes());
        candidate.state_hash = snapshot.state_hash();
        assert!(matches!(
            Game::restore(
                region.clone(),
                candidate,
                Box::new(InMemoryCommandLog::default()),
                &|| false
            ),
            Err(application::RestoreError::InvalidSnapshot)
        ));
    }
    let changed = String::from_utf8(checkpoint.region_source.clone())
        .unwrap()
        .replace("1000000, build_months: 2", "9000000, build_months: 2");
    let changed =
        infrastructure::region::load_region_bytes(changed.into_bytes(), &RegionId::new("minimal"))
            .unwrap();
    assert!(
        Game::restore(
            changed,
            checkpoint,
            Box::new(InMemoryCommandLog::default()),
            &|| false
        )
        .is_err()
    );
}

#[test]
fn cancelling_a_local_line_keeps_paid_capital_and_releases_only_future_payments() {
    let (_dir, mut game) = game();
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
    assert!(quote.months > 1);
    game.execute(Command::BuildLine {
        from: plants[0].node,
        to: plants[1].node,
    })
    .unwrap();
    let id = game.session().line_views().into_iter().last().unwrap().id;
    close(&mut game);
    let paid = game
        .session()
        .company()
        .ledger()
        .iter()
        .filter(|e| e.subject == Some(AssetRef::Line(id)))
        .map(|e| -e.amount.cents())
        .sum::<i64>();
    assert_eq!(paid, quote.capital.cents() / i64::from(quote.months));
    game.execute(Command::FinishReview).unwrap();
    let cash = game.session().company_view().cash;
    game.execute(Command::CancelLineConstruction { line: id })
        .unwrap();
    assert_eq!(game.session().company_view().cash, cash);
    assert_eq!(
        game.session()
            .line_quote(plants[0].node, plants[1].node)
            .unwrap(),
        quote
    );
    close(&mut game);
    assert_eq!(
        game.session()
            .company()
            .ledger()
            .iter()
            .filter(|e| e.subject == Some(AssetRef::Line(id)))
            .map(|e| -e.amount.cents())
            .sum::<i64>(),
        paid
    );
    assert!(
        !game
            .session()
            .company()
            .rate_base()
            .assets()
            .any(|a| a.asset == AssetRef::Line(id))
    );
}

#[test]
fn starter_assets_use_local_original_values_without_construction_delay() {
    let dir = tempfile::TempDir::new().unwrap();
    let source=include_str!("support/economic_region_v4.ron").replace(
        "plants: [(kind: Wind, capital_cost: 2000000, build_months: 3)]",
        "plants: [(kind: Wind, capital_cost: 2000000, build_months: 3), (kind: Peaker, capital_cost: 500000, build_months: 6)]");
    assert!(source.contains("capital_cost: 500000, build_months: 6"));
    std::fs::write(dir.path().join("minimal.ron"), source).unwrap();
    let game = Game::start_new(
        &RonRegionCatalog::new(dir.path()),
        &RegionId::new("minimal"),
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    let peaker = game
        .session()
        .plant_views()
        .into_iter()
        .find(|p| p.kind == PlantKind::Peaker)
        .unwrap();
    assert!(!matches!(
        peaker.status,
        PlantStatus::UnderConstruction { .. }
    ));
    let original = game
        .session()
        .company()
        .rate_base()
        .assets()
        .find(|a| a.asset == AssetRef::Plant(peaker.id))
        .unwrap();
    assert_eq!(original.original.cents(), 50_000_000);
    assert!(original.legacy);
    let line = game
        .session()
        .line_views()
        .into_iter()
        .find(|l| l.from == peaker.node || l.to == peaker.node)
        .unwrap();
    let original = game
        .session()
        .company()
        .rate_base()
        .assets()
        .find(|a| a.asset == AssetRef::Line(line.id))
        .unwrap();
    assert_eq!(original.original.cents(), 70_000_000);
    assert!(original.legacy);
}

/// Plan 7 Task 9: every economic zone the shipped Continental Europe-Maghreb region authors,
/// with each plant kind it overrides, built at the buildable cell in that zone nearest the
/// zone's largest city. The quote, the instalments paid and the commissioning month all equal
/// the zone's resolved terms, and the original rate base is the quoted capital.
#[test]
fn shipped_europe_maghreb_profiles_quote_pay_and_commission_by_their_terms() {
    use domain::region::EconomicZoneId;
    const ID: &str = "continental_europe_maghreb";
    let catalog =
        RonRegionCatalog::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/regions"));
    let mut game = Game::start_new(
        &catalog,
        &RegionId::new(ID),
        3,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    let region = std::sync::Arc::clone(game.session().region());
    assert!(region.has_economic_zones());
    let terrain = region.terrain();
    let cells: Vec<CellCoord> = (0..terrain.height())
        .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
        .collect();
    let mut occupied: Vec<CellCoord> = game
        .session()
        .plant_views()
        .iter()
        .map(|p| p.cell)
        .chain(game.session().city_views().iter().map(|c| c.cell))
        .collect();
    // (plant, expected spec) for every authored zone and overridden kind.
    let mut orders = Vec::new();
    let mut id = 1;
    while let Some(zone) = region.economic_zone(EconomicZoneId(id)) {
        let largest = region
            .cities()
            .iter()
            .filter(|c| region.economic_zone_at(c.cell).unwrap() == zone.id)
            .max_by(|a, b| a.base_load.value().total_cmp(&b.base_load.value()))
            .unwrap_or_else(|| panic!("{} holds a city", zone.name_key));
        for over in &zone.plants {
            let cell = *cells
                .iter()
                .filter(|c| {
                    region.economic_zone_at(**c).unwrap() == zone.id
                        && !occupied.contains(c)
                        && region.site_check(over.kind, **c).is_ok()
                })
                .min_by_key(|c| {
                    (
                        u32::from(c.x.abs_diff(largest.cell.x)).pow(2)
                            + u32::from(c.y.abs_diff(largest.cell.y)).pow(2),
                        **c,
                    )
                })
                .unwrap_or_else(|| panic!("{} has a {:?} site", zone.name_key, over.kind));
            occupied.push(cell);
            let spec = region.plant_spec_at(over.kind, cell).unwrap();
            if let Some(cost) = over.capital_cost {
                assert_eq!(spec.capital_cost, cost, "{} {:?}", zone.name_key, over.kind);
            }
            if let Some(months) = over.build_months {
                assert_eq!(
                    spec.build_months, months,
                    "{} {:?}",
                    zone.name_key, over.kind
                );
            }
            let quote = game.session().plant_quote(over.kind, cell).unwrap();
            assert_eq!(
                (quote.capital, quote.months),
                (spec.capital_cost, spec.build_months),
                "{} {:?}",
                zone.name_key,
                over.kind
            );
            let events = game
                .execute(Command::BuildPlant {
                    kind: over.kind,
                    cell,
                })
                .unwrap();
            let plant = events
                .iter()
                .find_map(|e| match e {
                    domain::events::DomainEvent::PlantConstructionStarted { plant, .. } => {
                        Some(*plant)
                    }
                    _ => None,
                })
                .unwrap();
            assert_eq!(game.session().plant_construction(plant).unwrap(), quote);
            orders.push((zone.name_key.clone(), plant, quote));
        }
        id += 1;
    }
    let mut covered: Vec<&str> = orders.iter().map(|(zone, _, _)| zone.as_str()).collect();
    covered.dedup();
    assert_eq!(
        covered,
        [
            "economic-zone-spain",
            "economic-zone-morocco",
            "economic-zone-algeria",
            "economic-zone-tunisia",
        ],
        "every authored zone is built in"
    );
    let longest = orders.iter().map(|(_, _, q)| q.months).max().unwrap();
    for month in 1..=longest {
        close(&mut game);
        for (label, plant, quote) in &orders {
            let view = game
                .session()
                .plant_views()
                .into_iter()
                .find(|v| v.id == *plant)
                .unwrap();
            assert_eq!(
                matches!(view.status, PlantStatus::UnderConstruction { .. }),
                month < quote.months,
                "{label}: month {month} of {}",
                quote.months
            );
            let paid: i64 = game
                .session()
                .company()
                .ledger()
                .iter()
                .filter(|e| {
                    e.subject == Some(AssetRef::Plant(*plant))
                        && e.category == domain::company::LedgerCategory::Construction
                })
                .map(|e| -e.amount.cents())
                .sum();
            if month >= quote.months {
                assert_eq!(paid, quote.capital.cents(), "{label}: instalments");
                let asset = game
                    .session()
                    .company()
                    .rate_base()
                    .assets()
                    .find(|a| a.asset == AssetRef::Plant(*plant))
                    .unwrap();
                assert_eq!(asset.original, quote.capital, "{label}: rate base");
            } else {
                assert!(paid < quote.capital.cents(), "{label}: paid early");
            }
        }
        game.execute(Command::FinishReview).unwrap();
    }
}
