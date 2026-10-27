//! Browser acceptance measurements run an isolated application, never the player's session.
use application::*;
use domain::{ids::RegionId, session::GameSettings};
use infrastructure::{
    backup::{decode_backup, encode_backup},
    region::BundleRegionCatalog,
    save::{decode_archive, decode_save, encode_archive, encode_save, replay_compatibility},
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn browser_probe(id: &str, bytes: &[u8], months: u32) -> Result<js_sys::Array, JsValue> {
    if !(1..=36).contains(&months) {
        return Err(JsValue::from_str("probe month limit"));
    }
    probe(id, bytes, months).map_err(|e| JsValue::from_str(&e.to_string()))
}
fn probe(id: &str, bytes: &[u8], months: u32) -> Result<js_sys::Array, Box<dyn std::error::Error>> {
    let catalog = BundleRegionCatalog::new([(RegionId::new(id), bytes.to_vec())].into());
    let started = js_sys::Date::now();
    let mut game = Game::start_new(
        &catalog,
        &RegionId::new(id),
        7,
        GameSettings::default(),
        Box::<InMemoryCommandLog>::default(),
    )?;
    let loaded = js_sys::Date::now();
    for month in 0..months {
        if month > 0 {
            game.execute(Command::FinishReview)?;
        }
        for plant in game
            .session()
            .plant_views()
            .iter()
            .filter(|p| crate::hud::is_commitable(p))
        {
            game.execute(Command::CommitPlant {
                plant: plant.id,
                hours: [true; 24],
            })?;
        }
        game.execute(Command::BeginOperating)?;
        game.execute(Command::AdvanceTicks { n: 1440 })?;
    }
    let operated = js_sys::Date::now();
    let checkpoint = game.checkpoint();
    let captured = js_sys::Date::now();
    let slot = SaveId::new("00000000000000000000000000000001".into())?;
    let bundle = SaveBundle {
        compatibility: replay_compatibility(),
        metadata: SaveMetadata {
            slot: slot.clone(),
            run: slot,
            kind: SaveKind::Manual,
            name: "Browser probe".into(),
            saved_at: 0,
        },
        checkpoint: checkpoint.clone(),
    };
    let encoded = encode_save(&bundle)?;
    let encoded_at = js_sys::Date::now();
    let (decoded, region) = decode_save(&encoded, &replay_compatibility())?;
    let restored = decoded.restore(
        region,
        &replay_compatibility(),
        Box::<InMemoryCommandLog>::default(),
        &|| false,
    )?;
    if restored.checkpoint() != checkpoint {
        return Err("probe checkpoint differs".into());
    }
    // Compare same-target future evolution after the portable transport boundary.
    let archive = decode_backup(&encode_backup(&encode_archive(
        &bundle,
        &bundle.compatibility,
    )?)?)?;
    let (candidate, region) = decode_archive(&archive, &replay_compatibility(), &|| false)?;
    let mut candidate = candidate.restore(
        region,
        &replay_compatibility(),
        Box::<InMemoryCommandLog>::default(),
        &|| false,
    )?;
    for command in [
        Command::FinishReview,
        Command::BeginOperating,
        Command::AdvanceTicks { n: 10 },
    ] {
        let expected = game.execute(command.clone())?;
        let actual = candidate.execute(command)?;
        if expected != actual || game.checkpoint() != candidate.checkpoint() {
            return Err("backup continuation differs".into());
        }
    }
    let output = js_sys::Array::new();
    for value in [
        loaded - started,
        operated - loaded,
        captured - operated,
        encoded_at - captured,
        js_sys::Date::now() - encoded_at,
        encoded.len() as f64,
    ] {
        output.push(&JsValue::from_f64(value));
    }
    Ok(output)
}
