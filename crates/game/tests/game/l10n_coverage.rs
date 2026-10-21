//! Localisation coverage: every key the code can ask for exists in en-US, every typed value
//! translates, and a broken `.ftl` fails Boot instead of panicking.

use std::collections::BTreeMap;

use application::projections::{Lesson, MilestoneKind, MilestoneProgress, RampWarning};
use application::{CommandError, CommandLogError, RegionCatalog, RegionLoadError};
use bevy::prelude::*;
use domain::company::{AssetRef, LedgerCategory, LedgerEntry};
use domain::error::DomainError;
use domain::events::DomainEvent;
use domain::generation::PlantStatus;
use domain::grid::{FrequencyBand, GridMode, TickSummary};
use domain::ids::{CellCoord, CityId, LineId, NodeId, PlantId, RegionId};
use domain::region::{PlantKind, RegionError, SiteError};
use domain::regulation::StrikeReason;
use domain::session::{
    DayPhase, GameOverReason, GameSession, GameSettings, IncidentKind, MonthlyReport,
};
use domain::time::{Season, SimDuration, SimMonth};
use domain::units::{Hertz, MegawattHours, Megawatts, Money, Price, Ratio};
use game::l10n::keys::ALL_UI_KEYS;
use game::l10n::{Localize, LocalizeWith, Localizer, Names, Toast};
use game::state::{AppState, BootError};
use infrastructure::region::RonRegionCatalog;
use tempfile::TempDir;

use crate::support::{headless_app, shipped_assets, update_n};

fn shipped() -> Localizer {
    Localizer::load(&shipped_assets().join("locales").join("en-US"))
        .unwrap_or_else(|error| panic!("the shipped en-US files must load: {error}"))
}

/// Real copy starts with a capital letter and has spaces, a bare key is lower-case with hyphens.
fn looks_like_a_bare_key(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Asserts that `values` covers every variant. `index` is an exhaustive `match`, so adding a
/// variant breaks the build here until the table is extended as well.
fn assert_covers_every_variant<T>(
    name: &str,
    values: &[T],
    variants: usize,
    index: fn(&T) -> usize,
) {
    let mut seen: Vec<usize> = values.iter().map(index).collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(
        seen,
        (0..variants).collect::<Vec<_>>(),
        "the {name} table must hold a value for every variant"
    );
}

fn assert_translates<T: Localize>(l10n: &Localizer, values: &[T]) {
    assert_texts(values.iter().map(|value| value.localize(l10n)));
}

/// For the types that name plants and cities, which have no bare `Localize`. With no session the
/// ids are shown as numbers, which must still read as copy.
fn assert_translates_with<T: LocalizeWith>(l10n: &Localizer, values: &[T]) {
    let names = Names::empty(l10n);
    assert_texts(values.iter().map(|value| value.localize_with(l10n, &names)));
}

fn assert_texts(texts: impl Iterator<Item = String>) {
    for text in texts {
        assert!(!text.is_empty());
        assert!(
            !looks_like_a_bare_key(&text),
            "fell back to the key: {text}"
        );
        assert!(!text.contains('{'), "unresolved placeholder: {text}");
        assert!(!text.contains("NaN"), "bad number in: {text}");
    }
}

// ---- the variant tables -------------------------------------------------------------------

const PLANT: PlantId = PlantId(7);

fn megawatts(v: f64) -> Megawatts {
    Megawatts::new(v)
}

fn ratio(v: f64) -> Ratio {
    Ratio::new(v).unwrap()
}

fn site_errors() -> Vec<SiteError> {
    vec![
        SiteError::OutOfBounds,
        SiteError::NotLand,
        SiteError::InsufficientCoolingWater {
            have: ratio(0.2),
            need: ratio(0.5),
        },
        SiteError::NoHydroSite,
    ]
}

fn domain_errors() -> Vec<DomainError> {
    vec![
        DomainError::InvalidRatio { value: 1.5 },
        DomainError::InvalidMonth { month: 13 },
        DomainError::PlantNotDispatchable { plant: PLANT },
        DomainError::PlantNotOffline { plant: PLANT },
        DomainError::PlantNotOnline { plant: PLANT },
        DomainError::PlantNotRunning { plant: PLANT },
        DomainError::NonFiniteMegawatts { plant: PLANT },
        DomainError::BelowMinimumStableOutput {
            plant: PLANT,
            min: megawatts(440.0),
        },
        DomainError::AboveCapacity {
            plant: PLANT,
            max: megawatts(600.0),
        },
        DomainError::CommitmentInfeasible {
            gap: SimDuration::from_hours(2),
            shutdown: SimDuration::from_minutes(90),
            startup: SimDuration::from_hours(8),
        },
        DomainError::DuplicateNode { node: NodeId(3) },
        DomainError::UnknownNode { node: NodeId(3) },
        DomainError::LineToSelf { node: NodeId(3) },
        DomainError::DuplicateLine {
            from: NodeId(3),
            to: NodeId(4),
        },
        DomainError::DuplicateLineId { line: LineId(9) },
        DomainError::UnknownPlant { plant: PLANT },
        DomainError::WrongPhase {
            actual: DayPhase::Review,
        },
        DomainError::GameOver {
            reason: GameOverReason::Bankrupt,
        },
        DomainError::InvalidSite {
            kind: PlantKind::Nuclear,
            cell: CellCoord { x: 4, y: 5 },
            reason: SiteError::NotLand,
        },
        DomainError::PlantNotCommissioned { plant: PLANT },
        DomainError::StartUpBeyondDay {
            remaining: SimDuration::from_hours(3),
            startup: SimDuration::from_hours(8),
        },
    ]
}

fn region_errors() -> Vec<RegionError> {
    let city = CityId(2);
    vec![
        RegionError::GridSizeMismatch {
            expected: 6000,
            actual: 5999,
        },
        RegionError::MissingPlantSpec(PlantKind::Coal),
        RegionError::DuplicatePlantSpec(PlantKind::Coal),
        RegionError::CityOutOfBounds(city),
        RegionError::CityNotOnLand(city),
        RegionError::DuplicateCity(city),
        RegionError::UnknownStartCity(city),
        RegionError::InvalidStartSite {
            kind: PlantKind::Hydro,
            cell: CellCoord { x: 1, y: 1 },
            reason: SiteError::NoHydroSite,
        },
        RegionError::InvalidHomeCity(CityId(99)),
        RegionError::InvalidShape { field: "demand" },
        RegionError::InvalidBand { field: "frequency" },
    ]
}

fn region_load_errors() -> Vec<RegionLoadError> {
    let region = RegionId::new("atlantis");
    vec![
        RegionLoadError::NotFound(region.clone()),
        RegionLoadError::Io {
            region: region.clone(),
            message: "permission denied".to_owned(),
        },
        RegionLoadError::Parse {
            region: region.clone(),
            message: "3:7: expected a comma".to_owned(),
        },
        RegionLoadError::Invalid {
            region: region.clone(),
            error: RegionError::InvalidHomeCity(CityId(99)),
        },
        RegionLoadError::IdMismatch {
            requested: region,
            found: RegionId::new("lemuria"),
        },
    ]
}

fn command_errors() -> Vec<CommandError> {
    vec![
        CommandError::Domain(DomainError::PlantNotOnline { plant: PLANT }),
        CommandError::Region(RegionLoadError::NotFound(RegionId::new("atlantis"))),
        CommandError::Log(CommandLogError {
            message: "disk full".to_owned(),
        }),
    ]
}

fn lessons() -> Vec<Lesson> {
    let mwh = MegawattHours::new(12_345.0);
    vec![
        Lesson::ExpensivePeaking {
            energy: mwh,
            cost: Money::from_dollars(1_850_000),
            alternative: Price::per_mwh(45.0),
        },
        Lesson::RotatingOutages {
            energy: mwh,
            peak_share: ratio(0.12),
        },
        Lesson::UnservedEnergy {
            energy: mwh,
            first_incident_tick: 1_060,
        },
        Lesson::Overgeneration {
            energy: mwh,
            trips: 0,
        },
        Lesson::Overgeneration {
            energy: mwh,
            trips: 1,
        },
        Lesson::Overgeneration {
            energy: mwh,
            trips: 3,
        },
        Lesson::FrequencyPerformance {
            out_of_band: ratio(0.07),
            notice: true,
        },
        Lesson::FrequencyPerformance {
            out_of_band: ratio(0.07),
            notice: false,
        },
        Lesson::SolarCurtailed { energy: mwh },
        Lesson::StartUpCosts {
            total: Money::from_dollars(420_000),
            share: ratio(0.15),
        },
        Lesson::RateReview {
            tariff: Price::per_mwh(61.5),
            allowed_return: ratio(0.085),
        },
        Lesson::AllClear,
    ]
}

fn incidents() -> Vec<IncidentKind> {
    vec![
        IncidentKind::LoadShed {
            mw: megawatts(1_800.0),
        },
        IncidentKind::Blackout,
        IncidentKind::BlackoutRestored,
        IncidentKind::OverFrequencyTrip { plant: PLANT },
        IncidentKind::RotatingOutage { share: ratio(0.1) },
    ]
}

fn plant_statuses() -> Vec<PlantStatus> {
    vec![
        PlantStatus::UnderConstruction { months_left: 1 },
        PlantStatus::UnderConstruction { months_left: 9 },
        PlantStatus::Offline,
        PlantStatus::Starting { ticks_left: 90 },
        PlantStatus::Online {
            output: megawatts(300.0),
            target: megawatts(320.0),
        },
        PlantStatus::Stopping {
            output: megawatts(120.0),
        },
    ]
}

fn milestones() -> Vec<MilestoneKind> {
    vec![
        MilestoneKind::ServePeak { gigawatts: 50 },
        MilestoneKind::FullSeasonWithoutStrike,
        MilestoneKind::ConnectCities { count: 10 },
    ]
}

fn milestone_progress() -> Vec<MilestoneProgress> {
    vec![
        MilestoneProgress::Peak {
            best: megawatts(31_200.0),
            goal: megawatts(50_000.0),
        },
        MilestoneProgress::StrikeFreeMonths {
            months: 1,
            needed: 3,
        },
        MilestoneProgress::Cities {
            connected: 4,
            needed: 10,
        },
    ]
}

fn month_report() -> MonthlyReport {
    let zero = MegawattHours::ZERO;
    MonthlyReport {
        month: SimMonth::new(2027, 3).unwrap(),
        days: 31,
        revenue: Money::ZERO,
        fuel: Money::ZERO,
        start_up: Money::ZERO,
        fixed_om: Money::ZERO,
        construction: Money::ZERO,
        net: Money::ZERO,
        energy_by_kind: BTreeMap::new(),
        merit_order: Vec::new(),
        demand: zero,
        served: zero,
        unserved: zero,
        curtailed: zero,
        imbalance: zero,
        overgeneration: zero,
        rotating_outage: zero,
        peak_outage_share: Ratio::ZERO,
        peak_demand: Megawatts::ZERO,
        reliability: Ratio::ONE,
        incidents: Vec::new(),
        over_frequency_trips: 0,
        out_of_band_share: Ratio::ZERO,
        frequency_failed: false,
        frequency_notice: false,
        rate_review: None,
    }
}

fn tick_summary() -> TickSummary {
    TickSummary {
        tick: 0,
        demand: Megawatts::ZERO,
        served: Megawatts::ZERO,
        unserved: Megawatts::ZERO,
        generation: Megawatts::ZERO,
        curtailed: Megawatts::ZERO,
        imbalance: Megawatts::ZERO,
        by_kind: BTreeMap::new(),
        by_plant: BTreeMap::new(),
        frequency: Hertz::new(60.0),
        mode: GridMode::Normal,
        shed: Ratio::ZERO,
        outage: Ratio::ZERO,
    }
}

/// Every `DomainEvent` variant, with whether it is toast-worthy.
fn events() -> Vec<(DomainEvent, bool)> {
    let month = SimMonth::new(2027, 3).unwrap();
    vec![
        (DomainEvent::PlantStarting { plant: PLANT }, false),
        (DomainEvent::PlantOnline { plant: PLANT }, false),
        (DomainEvent::PlantStopped { plant: PLANT }, false),
        (DomainEvent::PlantTripped { plant: PLANT }, true),
        (
            DomainEvent::PlantCommissioned {
                plant: PLANT,
                kind: PlantKind::Ccgt,
            },
            true,
        ),
        (DomainEvent::HydroBudgetExhausted { plant: PLANT }, true),
        (DomainEvent::TickCompleted(tick_summary()), false),
        (
            DomainEvent::FrequencyDeviated {
                band: FrequencyBand::Low,
            },
            false,
        ),
        (
            DomainEvent::LoadShed {
                mw: megawatts(900.0),
            },
            false,
        ),
        (
            DomainEvent::SolarCurtailed {
                mw: megawatts(300.0),
            },
            false,
        ),
        (DomainEvent::OverFrequencyTrip { plant: PLANT }, true),
        (DomainEvent::Blackout, true),
        (DomainEvent::BlackoutRestored, true),
        (
            DomainEvent::RotatingOutageOrdered {
                hour: 18,
                share: ratio(0.08),
            },
            true,
        ),
        (
            DomainEvent::RotatingOutageOrdered {
                hour: 21,
                share: Ratio::ZERO,
            },
            true,
        ),
        (
            DomainEvent::LedgerEntryRecorded(LedgerEntry {
                month,
                category: LedgerCategory::Fuel,
                amount: Money::from_dollars(-500),
                subject: None,
            }),
            false,
        ),
        (
            DomainEvent::CapitalHeld {
                asset: AssetRef::Plant(PLANT),
                amount: Money::from_dollars(1),
            },
            false,
        ),
        (
            DomainEvent::CapitalReleased {
                asset: AssetRef::Plant(PLANT),
                amount: Money::from_dollars(1),
            },
            false,
        ),
        (
            DomainEvent::CapitalDisallowed {
                asset: AssetRef::Line(LineId(1)),
                amount: Money::from_dollars(1),
            },
            false,
        ),
        (
            DomainEvent::StrikeIssued {
                strikes: 2,
                reason: StrikeReason::UnservedEnergy,
            },
            true,
        ),
        (DomainEvent::FrequencyNoticeIssued { month }, true),
        (DomainEvent::StrikeExpired { strikes: 0 }, true),
        (DomainEvent::StrikeExpired { strikes: 1 }, true),
        (DomainEvent::StrikeExpired { strikes: 2 }, true),
        (DomainEvent::LicenseRevoked, true),
        (
            DomainEvent::RateReviewCompleted {
                tariff: Price::per_mwh(61.5),
                allowed_return: ratio(0.085),
            },
            true,
        ),
        (
            DomainEvent::PhaseChanged {
                from: DayPhase::Planning,
                to: DayPhase::Operating,
            },
            false,
        ),
        (
            DomainEvent::PlantConstructionStarted {
                plant: PLANT,
                kind: PlantKind::Solar,
                cell: CellCoord { x: 1, y: 2 },
            },
            false,
        ),
        (
            DomainEvent::LineConstructionStarted { line: LineId(4) },
            false,
        ),
        (DomainEvent::LineCommissioned { line: LineId(4) }, true),
        (DomainEvent::CityConnected { city: CityId(3) }, true),
        (DomainEvent::MonthClosed(Box::new(month_report())), false),
        (DomainEvent::Bankrupt, true),
    ]
}

// ---- the table guards ---------------------------------------------------------------------

#[test]
fn the_variant_tables_cover_every_variant() {
    assert_covers_every_variant("DomainError", &domain_errors(), 21, |e| match e {
        DomainError::InvalidRatio { .. } => 0,
        DomainError::InvalidMonth { .. } => 1,
        DomainError::PlantNotDispatchable { .. } => 2,
        DomainError::PlantNotOffline { .. } => 3,
        DomainError::PlantNotOnline { .. } => 4,
        DomainError::PlantNotRunning { .. } => 5,
        DomainError::NonFiniteMegawatts { .. } => 6,
        DomainError::BelowMinimumStableOutput { .. } => 7,
        DomainError::AboveCapacity { .. } => 8,
        DomainError::CommitmentInfeasible { .. } => 9,
        DomainError::DuplicateNode { .. } => 10,
        DomainError::UnknownNode { .. } => 11,
        DomainError::LineToSelf { .. } => 12,
        DomainError::DuplicateLine { .. } => 13,
        DomainError::DuplicateLineId { .. } => 14,
        DomainError::UnknownPlant { .. } => 15,
        DomainError::WrongPhase { .. } => 16,
        DomainError::GameOver { .. } => 17,
        DomainError::InvalidSite { .. } => 18,
        DomainError::PlantNotCommissioned { .. } => 19,
        DomainError::StartUpBeyondDay { .. } => 20,
    });
    assert_covers_every_variant("RegionError", &region_errors(), 11, |e| match e {
        RegionError::GridSizeMismatch { .. } => 0,
        RegionError::MissingPlantSpec(_) => 1,
        RegionError::DuplicatePlantSpec(_) => 2,
        RegionError::CityOutOfBounds(_) => 3,
        RegionError::CityNotOnLand(_) => 4,
        RegionError::DuplicateCity(_) => 5,
        RegionError::UnknownStartCity(_) => 6,
        RegionError::InvalidStartSite { .. } => 7,
        RegionError::InvalidHomeCity(_) => 8,
        RegionError::InvalidShape { .. } => 9,
        RegionError::InvalidBand { .. } => 10,
    });
    assert_covers_every_variant("RegionLoadError", &region_load_errors(), 5, |e| match e {
        RegionLoadError::NotFound(_) => 0,
        RegionLoadError::Io { .. } => 1,
        RegionLoadError::Parse { .. } => 2,
        RegionLoadError::Invalid { .. } => 3,
        RegionLoadError::IdMismatch { .. } => 4,
    });
    assert_covers_every_variant("CommandError", &command_errors(), 3, |e| match e {
        CommandError::Domain(_) => 0,
        CommandError::Region(_) => 1,
        CommandError::Log(_) => 2,
    });
    assert_covers_every_variant("SiteError", &site_errors(), 4, |e| match e {
        SiteError::OutOfBounds => 0,
        SiteError::NotLand => 1,
        SiteError::InsufficientCoolingWater { .. } => 2,
        SiteError::NoHydroSite => 3,
    });
    assert_covers_every_variant("Lesson", &lessons(), 9, |l| match l {
        Lesson::ExpensivePeaking { .. } => 0,
        Lesson::RotatingOutages { .. } => 1,
        Lesson::UnservedEnergy { .. } => 2,
        Lesson::Overgeneration { .. } => 3,
        Lesson::FrequencyPerformance { .. } => 4,
        Lesson::SolarCurtailed { .. } => 5,
        Lesson::StartUpCosts { .. } => 6,
        Lesson::RateReview { .. } => 7,
        Lesson::AllClear => 8,
    });
    assert_covers_every_variant("IncidentKind", &incidents(), 5, |i| match i {
        IncidentKind::LoadShed { .. } => 0,
        IncidentKind::Blackout => 1,
        IncidentKind::BlackoutRestored => 2,
        IncidentKind::OverFrequencyTrip { .. } => 3,
        IncidentKind::RotatingOutage { .. } => 4,
    });
    assert_covers_every_variant("PlantStatus", &plant_statuses(), 5, |s| match s {
        PlantStatus::UnderConstruction { .. } => 0,
        PlantStatus::Offline => 1,
        PlantStatus::Starting { .. } => 2,
        PlantStatus::Online { .. } => 3,
        PlantStatus::Stopping { .. } => 4,
    });
    assert_covers_every_variant("MilestoneKind", &milestones(), 3, |m| match m {
        MilestoneKind::ServePeak { .. } => 0,
        MilestoneKind::FullSeasonWithoutStrike => 1,
        MilestoneKind::ConnectCities { .. } => 2,
    });
    assert_covers_every_variant("MilestoneProgress", &milestone_progress(), 3, |p| match p {
        MilestoneProgress::Peak { .. } => 0,
        MilestoneProgress::StrikeFreeMonths { .. } => 1,
        MilestoneProgress::Cities { .. } => 2,
    });
    let events: Vec<DomainEvent> = events().into_iter().map(|(event, _)| event).collect();
    assert_covers_every_variant("DomainEvent", &events, 30, |e| match e {
        DomainEvent::PlantStarting { .. } => 0,
        DomainEvent::PlantOnline { .. } => 1,
        DomainEvent::PlantStopped { .. } => 2,
        DomainEvent::PlantTripped { .. } => 3,
        DomainEvent::PlantCommissioned { .. } => 4,
        DomainEvent::HydroBudgetExhausted { .. } => 5,
        DomainEvent::TickCompleted(_) => 6,
        DomainEvent::FrequencyDeviated { .. } => 7,
        DomainEvent::LoadShed { .. } => 8,
        DomainEvent::SolarCurtailed { .. } => 9,
        DomainEvent::OverFrequencyTrip { .. } => 10,
        DomainEvent::Blackout => 11,
        DomainEvent::BlackoutRestored => 12,
        DomainEvent::RotatingOutageOrdered { .. } => 13,
        DomainEvent::LedgerEntryRecorded(_) => 14,
        DomainEvent::CapitalHeld { .. } => 15,
        DomainEvent::CapitalReleased { .. } => 16,
        DomainEvent::CapitalDisallowed { .. } => 17,
        DomainEvent::StrikeIssued { .. } => 18,
        DomainEvent::FrequencyNoticeIssued { .. } => 19,
        DomainEvent::StrikeExpired { .. } => 20,
        DomainEvent::LicenseRevoked => 21,
        DomainEvent::RateReviewCompleted { .. } => 22,
        DomainEvent::PhaseChanged { .. } => 23,
        DomainEvent::PlantConstructionStarted { .. } => 24,
        DomainEvent::LineConstructionStarted { .. } => 25,
        DomainEvent::LineCommissioned { .. } => 26,
        DomainEvent::CityConnected { .. } => 27,
        DomainEvent::MonthClosed(_) => 28,
        DomainEvent::Bankrupt => 29,
    });
}

// ---- coverage -----------------------------------------------------------------------------

#[test]
fn every_ui_key_exists_in_en_us() {
    let l10n = shipped();

    let missing: Vec<&&str> = ALL_UI_KEYS.iter().filter(|key| !l10n.has(key)).collect();

    assert!(missing.is_empty(), "keys missing from en-US: {missing:?}");
}

#[test]
fn ui_keys_have_no_duplicates() {
    let mut keys = ALL_UI_KEYS.to_vec();
    keys.sort_unstable();
    let before = keys.len();
    keys.dedup();
    assert_eq!(keys.len(), before);
}

#[test]
fn every_error_and_lesson_variant_translates_without_falling_back() {
    let l10n = shipped();

    assert_translates_with(&l10n, &domain_errors());
    assert_translates(&l10n, &region_errors());
    assert_translates(&l10n, &region_load_errors());
    assert_translates_with(&l10n, &command_errors());
    assert_translates(&l10n, &site_errors());
    assert_translates(&l10n, &lessons());
    assert_translates(
        &l10n,
        &[CommandLogError {
            message: "disk full".to_owned(),
        }],
    );

    assert_eq!(
        l10n.problems(),
        vec![],
        "a lookup fell back or misformatted"
    );
}

#[test]
fn every_typed_value_translates_without_falling_back() {
    let l10n = shipped();

    assert_translates_with(&l10n, &incidents());
    assert_translates(&l10n, &milestones());
    assert_translates(&l10n, &milestone_progress());
    assert_translates(&l10n, &plant_statuses());
    assert_translates(&l10n, &PlantKind::ALL);
    assert_translates(
        &l10n,
        &[
            StrikeReason::UnservedEnergy,
            StrikeReason::FrequencyPerformance,
        ],
    );
    assert_translates(
        &l10n,
        &[GameOverReason::Bankrupt, GameOverReason::LicenseRevoked],
    );
    assert_translates(
        &l10n,
        &[DayPhase::Planning, DayPhase::Operating, DayPhase::Review],
    );
    assert_translates(
        &l10n,
        &[
            game::state::PhaseState::Planning,
            game::state::PhaseState::Operating,
            game::state::PhaseState::Review,
        ],
    );
    assert_translates(
        &l10n,
        &[
            Season::Winter,
            Season::Spring,
            Season::Summer,
            Season::Autumn,
        ],
    );
    assert_translates(
        &l10n,
        &[RampWarning {
            at_tick: 1_000,
            horizon_minutes: 60,
            net_load_change: megawatts(6_200.0),
            capability: megawatts(4_900.0),
        }],
    );

    assert_eq!(
        l10n.problems(),
        vec![],
        "a lookup fell back or misformatted"
    );
}

#[test]
fn toast_worthy_events_have_text_and_the_rest_have_none() {
    let l10n = shipped();
    let names = Names::empty(&l10n);

    for (event, worthy) in events() {
        match (event.toast(&l10n, &names), worthy) {
            (Some(text), true) => {
                assert!(!looks_like_a_bare_key(&text), "{event:?} fell back: {text}");
                assert!(!text.contains('{'), "{event:?}: {text}");
            }
            (None, false) => {}
            (got, worthy) => panic!("{event:?}: toast-worthy {worthy}, got {got:?}"),
        }
    }

    assert_eq!(
        l10n.problems(),
        vec![],
        "a lookup fell back or misformatted"
    );
}

#[test]
fn rotating_outage_orders_read_differently_when_lifted() {
    let l10n = shipped();
    let names = Names::empty(&l10n);
    let order = |share| DomainEvent::RotatingOutageOrdered { hour: 18, share };

    let ordered = order(ratio(0.08)).toast(&l10n, &names).unwrap();
    let lifted = order(Ratio::ZERO).toast(&l10n, &names).unwrap();

    assert!(
        ordered.contains("18:00") && ordered.contains("8.0%"),
        "{ordered}"
    );
    assert!(lifted.contains("lifted"), "{lifted}");
}

#[test]
fn plant_statuses_read_as_copy() {
    let l10n = shipped();

    let texts: Vec<String> = plant_statuses()
        .iter()
        .map(|status| status.localize(&l10n))
        .collect();

    assert_eq!(
        texts,
        [
            "Under construction: 1 month left",
            "Under construction: 9 months left",
            "Off",
            "Starting: 1 h 30 min left",
            "Online",
            "Stopping",
        ]
    );
    assert_eq!(l10n.problems(), vec![]);
}

#[test]
fn strike_expiry_pluralises() {
    let l10n = shipped();
    let names = Names::empty(&l10n);
    let expired = |strikes| {
        DomainEvent::StrikeExpired { strikes }
            .toast(&l10n, &names)
            .unwrap()
    };

    assert!(expired(0).contains("no strikes"));
    assert!(expired(1).contains("one strike."));
    assert!(expired(2).contains("2 strikes"));
}

#[test]
fn lessons_use_their_parameters() {
    let l10n = shipped();

    let peaking = Lesson::ExpensivePeaking {
        energy: MegawattHours::new(12_345.0),
        cost: Money::from_dollars(1_850_000),
        alternative: Price::per_mwh(45.0),
    }
    .localize(&l10n);

    assert!(peaking.contains("12.3 GWh"), "{peaking}");
    assert!(peaking.contains("$1.85M"), "{peaking}");
    assert!(peaking.contains("$45.00/MWh"), "{peaking}");
}

#[test]
fn the_ramp_warning_reads_like_the_spec() {
    let l10n = shipped();

    let text = RampWarning {
        at_tick: 1_000,
        horizon_minutes: 60,
        net_load_change: megawatts(6_200.0),
        capability: megawatts(4_900.0),
    }
    .localize(&l10n);

    // The same display rule as everywhere: MW below 10,000 MW.
    assert!(text.contains("6,200 MW"), "{text}");
    assert!(text.contains("4,900 MW"), "{text}");
    assert!(text.contains("60 minutes"), "{text}");
}

#[test]
fn every_shipped_region_and_city_name_key_exists() {
    let l10n = shipped();
    let catalog = RonRegionCatalog::new(shipped_assets().join("regions"));

    let listings = catalog.list().unwrap();
    assert!(listings.len() >= 2, "expected both shipped regions");
    for listing in listings {
        let region = catalog.load(&listing.id).unwrap().definition;
        let mut keys = vec![region.name_key().to_owned()];
        keys.extend(region.cities().iter().map(|city| city.name_key.clone()));
        for key in keys {
            assert!(
                l10n.has(&key),
                "region {} refers to {key}, which en-US lacks",
                listing.id.as_str()
            );
        }
    }
}

#[test]
fn ftl_files_parse_without_errors() {
    let dir = shipped_assets().join("locales").join("en-US");

    Localizer::load(&dir).unwrap_or_else(|error| panic!("{error}"));

    for file in ["ui", "errors", "events", "lessons", "regions"] {
        assert!(
            dir.join(format!("{file}.ftl")).is_file(),
            "{file}.ftl is missing"
        );
    }
}

// ---- Names --------------------------------------------------------------------------------

fn shipped_session() -> GameSession {
    let catalog = RonRegionCatalog::new(shipped_assets().join("regions"));
    let region = catalog
        .load(&RegionId::new("eastern_interconnection"))
        .unwrap()
        .definition;
    GameSession::new(region, 1, GameSettings::default())
}

#[test]
fn a_city_toast_uses_the_localised_city_name() {
    let l10n = shipped();
    let session = shipped_session();
    let names = Names::new(&l10n, &session);
    let city = session.city_views().remove(0);

    let toast = DomainEvent::CityConnected { city: city.id }
        .toast(&l10n, &names)
        .unwrap();

    let name = l10n.text(&city.name_key);
    assert_ne!(name, city.name_key, "the city name must be translated");
    assert!(toast.contains(&name), "{toast}");
    assert!(!toast.contains('#'), "{toast}");
}

#[test]
fn an_already_joined_line_names_both_ends_and_falls_back_to_the_grid_point() {
    let l10n = shipped();
    let session = shipped_session();
    let names = Names::new(&l10n, &session);
    let city = session.city_views().remove(0);
    let plant = session.plant_views().remove(0);
    let ghost = NodeId(99_999);

    let named = |from, to| DomainError::DuplicateLine { from, to }.localize_with(&l10n, &names);

    let home = named(session.anchor_node(), city.node);
    assert_eq!(
        home,
        format!(
            "Columbus and {} are already connected.",
            l10n.text(&city.name_key)
        )
    );
    let plant_end = named(plant.node, city.node);
    assert!(
        plant_end.starts_with(&plant.kind.localize(&l10n)),
        "{plant_end}"
    );
    assert!(!plant_end.contains('#'), "{plant_end}");
    assert_eq!(
        named(ghost, session.anchor_node()),
        "Grid point 99999 and Columbus are already connected."
    );
    assert_eq!(l10n.problems(), vec![]);
}

#[test]
fn a_plant_toast_uses_the_plant_kind() {
    let l10n = shipped();
    let session = shipped_session();
    let names = Names::new(&l10n, &session);
    let plants = session.plant_views();
    assert!(!plants.is_empty(), "the shipped region starts with a fleet");

    for plant in plants {
        let kind = plant.kind.localize(&l10n);
        let expected = format!("{kind} {}", plant.id.0);
        let toast = DomainEvent::PlantTripped { plant: plant.id }
            .toast(&l10n, &names)
            .unwrap();
        assert!(toast.starts_with(&expected), "{toast}");
        let error = DomainError::PlantNotOnline { plant: plant.id }.localize_with(&l10n, &names);
        assert!(error.starts_with(&expected), "{error}");
        let incident =
            IncidentKind::OverFrequencyTrip { plant: plant.id }.localize_with(&l10n, &names);
        assert!(incident.contains(&expected), "{incident}");
    }
}

#[test]
fn a_commissioned_plant_is_named_by_its_event_kind_even_when_unknown() {
    let l10n = shipped();
    let names = Names::empty(&l10n);

    let toast = DomainEvent::PlantCommissioned {
        plant: PlantId(9),
        kind: PlantKind::Coal,
    }
    .toast(&l10n, &names)
    .unwrap();

    assert!(toast.starts_with("Coal plant 9"), "{toast}");
}

#[test]
fn an_unknown_id_falls_back_to_its_number_without_panicking() {
    let l10n = shipped();
    let session = shipped_session();
    let names = Names::new(&l10n, &session);

    assert_eq!(names.plant(PlantId(999_999)), "Plant #999999");
    assert_eq!(names.city(CityId(999_999)), "City #999999");
    assert_eq!(names.line(LineId(4)), "Line 4");
    let toast = DomainEvent::PlantTripped {
        plant: PlantId(999_999),
    }
    .toast(&l10n, &names)
    .unwrap();
    assert!(toast.contains("#999999"), "{toast}");
    let empty = Names::empty(&l10n);
    assert!(
        DomainEvent::CityConnected { city: CityId(3) }
            .toast(&l10n, &empty)
            .unwrap()
            .contains("#3")
    );
    assert_eq!(l10n.problems(), vec![]);
}

// ---- Boot ---------------------------------------------------------------------------------

fn app_state(app: &App) -> AppState {
    *app.world().resource::<State<AppState>>().get()
}

#[test]
fn boot_loads_the_localizer_before_the_main_menu() {
    let logs = TempDir::new().unwrap();
    let mut app = headless_app(&shipped_assets(), logs.path());

    update_n(&mut app, 3);

    assert!(app.world().contains_resource::<Localizer>());
    assert_eq!(app_state(&app), AppState::MainMenu);
}

#[test]
fn an_ftl_syntax_error_fails_boot_with_a_visible_error() {
    let assets = TempDir::new().unwrap();
    let locale = assets.path().join("locales").join("en-US");
    std::fs::create_dir_all(&locale).unwrap();
    std::fs::write(locale.join("bad.ftl"), "broken = { \n").unwrap();
    let logs = TempDir::new().unwrap();
    let mut app = headless_app(assets.path(), logs.path());

    update_n(&mut app, 4);

    let error = app.world().resource::<BootError>();
    assert!(error.0.contains("bad.ftl"), "{}", error.0);
    assert!(!app.world().contains_resource::<Localizer>());
    assert_eq!(app_state(&app), AppState::Boot);
}

#[test]
fn a_missing_locales_directory_fails_boot_with_a_visible_error() {
    let assets = TempDir::new().unwrap();
    let logs = TempDir::new().unwrap();
    let mut app = headless_app(assets.path(), logs.path());

    update_n(&mut app, 4);

    assert!(app.world().contains_resource::<BootError>());
    assert_eq!(app_state(&app), AppState::Boot);
}
