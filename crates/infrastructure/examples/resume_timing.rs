//! Measure restore and activation using temporary recordings; source saves are never changed.
use application::{InMemoryCommandLog, SaveKind, SaveStore};
use infrastructure::{
    command_log::{FileCommandLog, PreparedRecording},
    save::FileSaveStore,
};
use std::time::Instant;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args_os()
        .nth(1)
        .ok_or("usage: resume_timing <save-directory>")?;
    let store = FileSaveStore::new(root.into());
    let entry = store
        .list()?
        .into_iter()
        .filter_map(|r| r.entry.ok())
        .find(|e| e.metadata.kind == SaveKind::Automatic)
        .ok_or("no automatic save")?;
    let start = Instant::now();
    let (bundle, region) = store.read(&entry.metadata.slot)?;
    let read = start.elapsed();
    let start = Instant::now();
    let game = bundle.restore(
        region,
        &store.compatibility(),
        Box::new(InMemoryCommandLog::default()),
        &|| false,
    )?;
    let restore = start.elapsed();
    let start = Instant::now();
    let game = game.prepare_recording();
    let prepared = PreparedRecording::new(game.checkpoint().clone(), &|| false)?;
    let prepare = start.elapsed();
    let dir = tempfile::TempDir::new()?;
    let sink = FileCommandLog::create(&dir.path().join("resume.log"))?;
    let start = Instant::now();
    let game = game.activate(Box::new(sink.with_prepared_recording(prepared)), &|| false)?;
    let activate = start.elapsed();
    println!(
        "{} commands; read={read:?}; restore={restore:?}; prepare={prepare:?}; activate={activate:?}",
        game.checkpoint().command_count
    );
    Ok(())
}
