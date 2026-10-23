//! Compare an existing save with current encoding in memory; never rewrites source snapshots.
use application::{InMemoryCommandLog, SaveKind, SaveStore};
use infrastructure::save::{FileSaveStore, decode_save, encode_save};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args_os()
        .nth(1)
        .ok_or("usage: snapshot_rle_size <save-directory>")?;
    let root = std::path::PathBuf::from(root);
    let store = FileSaveStore::new(root.clone());
    let entry = store
        .list()?
        .into_iter()
        .filter_map(|r| r.entry.ok())
        .find(|e| e.metadata.kind == SaveKind::Automatic)
        .ok_or("no automatic save")?;
    let path = root.join(format!("{}.mgsave", entry.metadata.slot.as_str()));
    if path.is_dir() {
        let total: u64 = std::fs::read_dir(&path)?
            .map(|e| e.and_then(|e| e.metadata()).map(|m| m.len()))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .sum();
        let (bundle, region) = store.read(&entry.metadata.slot)?;
        let game = bundle.clone().restore(
            region,
            &store.compatibility(),
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )?;
        assert_eq!(game.checkpoint(), bundle.checkpoint);
        println!(
            "{}: directory snapshot={} bytes; commands={}",
            entry.metadata.slot.as_str(),
            total,
            bundle.checkpoint.command_count
        );
        return Ok(());
    }
    #[derive(serde::Deserialize)]
    struct Envelope {
        payload: String,
    }
    let journal_bytes = |bytes: &[u8]| -> Result<usize, Box<dyn std::error::Error>> {
        let envelope: Envelope = ron::de::from_bytes(bytes)?;
        let tail = envelope
            .payload
            .split_once("commands:")
            .ok_or("missing commands")?
            .1;
        let journal = tail.split_once(",count:").ok_or("missing count")?.0;
        Ok(journal.len())
    };
    let original = std::fs::read(&path)?;
    let (bundle, _) = store.read(&entry.metadata.slot)?;
    let encoded = encode_save(&bundle)?;
    let (decoded, region) = decode_save(&encoded, &store.compatibility())?;
    assert_eq!(decoded, bundle);
    let game = decoded.restore(
        region,
        &store.compatibility(),
        Box::new(InMemoryCommandLog::default()),
        &|| false,
    )?;
    assert_eq!(game.checkpoint(), bundle.checkpoint);
    let ticks = bundle
        .checkpoint
        .commands
        .iter()
        .filter(|c| matches!(c, application::Command::AdvanceTicks { .. }))
        .count();
    println!(
        "{}: original={} bytes; current={} bytes; saved={} bytes; original_commands={}; tick_commands={ticks}; current_tick_runs={}",
        entry.metadata.slot.as_str(),
        original.len(),
        encoded.len(),
        original.len() as i64 - encoded.len() as i64,
        bundle.checkpoint.commands.len(),
        String::from_utf8_lossy(&encoded)
            .matches("TickRun(")
            .count()
    );
    println!(
        "journal: original={} bytes; current={} bytes",
        journal_bytes(&original)?,
        journal_bytes(&encoded)?
    );
    assert_eq!(std::fs::read(&path)?, original);
    Ok(())
}
