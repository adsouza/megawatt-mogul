//! Shared test support: an in-code region catalog, game builders, scripted
//! months and the fixture variants the scenario tests play.
//!
//! Regions are defined in code, so their "source bytes" are the `Debug` text
//! of their definition and their fingerprint is a small checksum of those
//! bytes. Both are deterministic and change whenever the content does, which
//! is all the application layer needs from them; real RON parsing and
//! cryptographic digests belong to the infrastructure tests.

use std::sync::Arc;

use application::{
    Command, Game, InMemoryCommandLog, LoadedRegion, LogRecord, RegionCatalog, RegionFingerprint,
    RegionLoadError, RegionSummary,
};
use domain::events::DomainEvent;
use domain::generation::PlantStatus;
use domain::ids::{CellCoord, CityId, PlantId, RegionId};
use domain::region::fixtures::tiny_region_parts;
use domain::region::{PlantKind, RegionDefinition, RegionDefinitionParts, StartPlantDef};
use domain::session::{DayPhase, GameSettings, SessionStatus};
use domain::time::{SimMonth, TICKS_PER_DAY};

/// A catalog over regions already held in memory.
pub struct StaticCatalog(pub Vec<LoadedRegion>);

impl RegionCatalog for StaticCatalog {
    fn list(&self) -> Result<Vec<RegionSummary>, RegionLoadError> {
        Ok(self
            .0
            .iter()
            .map(|loaded| RegionSummary {
                id: loaded.definition.id().clone(),
                name_key: loaded.definition.name_key().to_owned(),
            })
            .collect())
    }

    fn load(&self, id: &RegionId) -> Result<LoadedRegion, RegionLoadError> {
        self.0
            .iter()
            .find(|loaded| loaded.definition.id() == id)
            .cloned()
            .ok_or_else(|| RegionLoadError::NotFound(id.clone()))
    }
}

/// A deterministic stand-in for a content digest: a multiply-add checksum of
/// the bytes spread over 32 lanes. Distinct fixtures get distinct values.
fn fake_fingerprint(bytes: &[u8]) -> RegionFingerprint {
    let mut lanes = [0x5au8; 32];
    for (index, byte) in bytes.iter().enumerate() {
        let lane = &mut lanes[index % 32];
        *lane = lane.wrapping_mul(31).wrapping_add(*byte);
    }
    RegionFingerprint::new(lanes)
}

/// Wraps `parts` as a loaded region with deterministic source bytes and
/// fingerprint. Equal parts always give an equal `LoadedRegion`.
pub fn loaded_region(parts: RegionDefinitionParts) -> LoadedRegion {
    let source_bytes = format!("in-code region definition\n{parts:#?}").into_bytes();
    let fingerprint = fake_fingerprint(&source_bytes);
    let definition = RegionDefinition::try_from(parts).expect("fixture region is valid");
    LoadedRegion {
        definition: Arc::new(definition),
        source_bytes,
        fingerprint,
    }
}

/// The same fixture serves live play and replay.
pub fn tiny_loaded_region() -> LoadedRegion {
    loaded_region(tiny_region_parts())
}

/// Starts a game in the region `parts` describes, with its log.
pub fn game_on(parts: RegionDefinitionParts, seed: u64) -> (Game, InMemoryCommandLog) {
    let loaded = loaded_region(parts);
    let id = loaded.definition.id().clone();
    let log = InMemoryCommandLog::default();
    let game = Game::start_new(
        &StaticCatalog(vec![loaded]),
        &id,
        seed,
        GameSettings::default(),
        Box::new(log.clone()),
    )
    .expect("fixture game starts");
    (game, log)
}

/// The commands logged after the header.
pub fn logged_commands(log: &InMemoryCommandLog) -> Vec<Command> {
    log.records()
        .into_iter()
        .filter_map(|record| match record {
            LogRecord::Command(command) => Some(command),
            LogRecord::Header(_) => None,
        })
        .collect()
}

/// Runs `plan` in Planning, then `BeginOperating`, one whole day of ticks and
/// `FinishReview`, so the next month is ready for planning. Returns the events
/// of the operated day, ending with the month close.
pub fn play_month(game: &mut Game, plan: impl FnOnce(&mut Game)) -> Vec<DomainEvent> {
    let (events, status) = play_month_to_close(game, plan);
    assert_eq!(
        status,
        SessionStatus::Active,
        "the month close ended the game"
    );
    events
}

/// Like [`play_month`], but a month close that ends the game stops there: the
/// session stays in `Review` and `FinishReview` is not sent, because a game
/// that is over rejects every command. Returns the operated day's events
/// (ending with the month close) and the status after the close.
pub fn play_month_to_close(
    game: &mut Game,
    plan: impl FnOnce(&mut Game),
) -> (Vec<DomainEvent>, SessionStatus) {
    assert_eq!(
        game.session().phase(),
        DayPhase::Planning,
        "plan in Planning"
    );
    plan(game);
    game.execute(Command::BeginOperating)
        .expect("begin operating");
    let events = game
        .execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
        .expect("advance the day");
    let status = game.session().status();
    if status == SessionStatus::Active {
        game.execute(Command::FinishReview).expect("finish review");
    }
    (events, status)
}

/// Commits every dispatchable, commissioned plant for hours 0-23 and turns
/// AGC on for it. Plants still under construction are skipped.
pub fn auto_follow_all(game: &mut Game) {
    let plants: Vec<PlantId> = game
        .session()
        .plant_views()
        .iter()
        .filter(|plant| {
            plant.kind.is_dispatchable()
                && !matches!(plant.status, PlantStatus::UnderConstruction { .. })
        })
        .map(|plant| plant.id)
        .collect();
    for plant in plants {
        game.execute(Command::CommitPlant {
            plant,
            hours: [true; 24],
        })
        .expect("commit plant all day");
        game.execute(Command::SetAutoFollow { plant, on: true })
            .expect("turn AGC on");
    }
}

/// The ids of the plants every game on the tiny region starts with, in id order.
pub fn fleet_plants() -> Vec<PlantId> {
    let (game, _) = game_on(tiny_region_parts(), 0);
    game.session()
        .plant_views()
        .iter()
        .map(|plant| plant.id)
        .collect()
}

fn start_month(parts: &mut RegionDefinitionParts, month: u8) {
    let year = parts.start.month.year();
    parts.start.month = SimMonth::new(year, month).expect("valid month");
}

/// January: the winter shape and the highest monthly demand factor.
pub fn winter_peak() -> RegionDefinitionParts {
    let mut parts = tiny_region_parts();
    start_month(&mut parts, 1);
    parts
}

/// June, with 1,500 MW of solar (five 300 MW plants) in the start fleet.
///
/// The brief called for two plants, but 600 MW of solar never curtails and
/// never triggers a ramp warning on the tiny fleet (checked with two and three
/// plants); the midday surplus first appears at four plants and is material
/// at five.
pub fn duck_curve() -> RegionDefinitionParts {
    let mut parts = tiny_region_parts();
    start_month(&mut parts, 6);
    for (x, y) in [(7, 1), (8, 1), (9, 1), (7, 6), (8, 6)] {
        parts.start.fleet.push(StartPlantDef {
            kind: PlantKind::Solar,
            cell: CellCoord { x, y },
        });
    }
    parts
}

/// City 3, which the start does not connect, carries 2,500 MW of base load:
/// far more than the start fleet can serve once the player wires it in.
pub fn overreach() -> RegionDefinitionParts {
    let mut parts = tiny_region_parts();
    let city = parts
        .cities
        .iter_mut()
        .find(|city| city.id == CityId(3))
        .expect("tiny region has city 3");
    city.base_load = domain::units::Megawatts::new(2_500.0);
    parts
}

/// January start, so the first December review is eleven months in.
pub fn overbuild() -> RegionDefinitionParts {
    winter_peak()
}
