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

/// A19 cross-target acceptance: operate an isolated native checkpoint and return only reservoir
/// state, which uses basic IEEE arithmetic. Broader cross-target physics identity is not assumed.
#[wasm_bindgen]
pub fn browser_geothermal_probe(bytes: &[u8]) -> Result<String, JsValue> {
    geothermal_probe(bytes).map_err(|e| JsValue::from_str(&e.to_string()))
}
fn geothermal_probe(bytes: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let (bundle, region) = decode_save(bytes, &replay_compatibility())?;
    let source = region.clone();
    let mut game = bundle.restore(
        region,
        &replay_compatibility(),
        Box::<InMemoryCommandLog>::default(),
        &|| false,
    )?;
    let mut traces = Vec::new();
    for _ in 0..8 {
        if game.session().phase() == domain::session::DayPhase::Review {
            game.execute(Command::FinishReview)?;
        }
        let drill: Vec<_> = game
            .session()
            .power()
            .plants()
            .values()
            .filter(|p| {
                p.geothermal().is_some_and(|g| g.notice.is_some())
                    && game.session().makeup_quote(p.id()).is_ok()
            })
            .map(|p| p.id())
            .collect();
        for plant in drill {
            game.execute(Command::DrillMakeupWells { plant })?;
        }
        game.execute(Command::BeginOperating)?;
        game.execute(Command::AdvanceTicks { n: 1440 })?;
        let mut rows = vec![format!("dry={}", game.session().dry_cells().len())];
        for p in game.session().power().plants().values() {
            if let Some(g) = p.geothermal() {
                rows.push(format!(
                    "{}:{}:{:016x}:{}:{}:{:?}",
                    p.id().0,
                    g.exploration_left,
                    g.field.value().to_bits(),
                    g.rounds,
                    g.makeup.as_ref().map_or(0, |m| m.months_left),
                    g.notice
                ));
            }
        }
        traces.push(rows.join("|"));
        // Exercise the target's own state codec at every exploration/decline/makeup boundary.
        let checkpoint = game.checkpoint();
        let expected = checkpoint.state_hash;
        game = Game::restore(
            source.clone(),
            checkpoint,
            Box::<InMemoryCommandLog>::default(),
            &|| false,
        )?;
        if game.checkpoint().state_hash != expected {
            return Err("geothermal checkpoint differs".into());
        }
    }
    Ok(traces.join("\n"))
}
