//! Native backup transport: export <slot.mgsave> <backup.mgsave.zip>, import <backup> <save-dir>, validate <backup>.
use application::*;
use infrastructure::{
    backup::{MAX_BACKUP_BYTES, decode_backup, encode_backup},
    save::*,
};
use std::{
    error::Error,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
fn read(path: &Path, limit: usize) -> Result<Vec<u8>, Box<dyn Error>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.is_symlink() || metadata.len() > limit as u64 {
        return Err("invalid or oversized input".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err("oversized input".into());
    }
    Ok(bytes)
}
fn validate(archive: &EncodedArchive) -> Result<(SaveBundle, Game), Box<dyn Error>> {
    let (bundle, region) = decode_archive(archive, &replay_compatibility(), &|| false)?;
    let game = bundle.clone().restore(
        region,
        &replay_compatibility(),
        Box::<InMemoryCommandLog>::default(),
        &|| false,
    )?;
    Ok((bundle, game))
}
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let operation = args.next().ok_or("operation required")?;
    let input = PathBuf::from(args.next().ok_or("input required")?);
    if operation == "export" {
        let output = PathBuf::from(args.next().ok_or("output required")?);
        let archive = if input.is_dir() {
            read_archive_directory(&input)?
        } else {
            let (mut bundle, region) =
                decode_save(&read(&input, MAX_SAVE_BYTES)?, &replay_compatibility())?;
            let game = bundle.clone().restore(
                region,
                &replay_compatibility(),
                Box::<InMemoryCommandLog>::default(),
                &|| false,
            )?;
            if bundle.checkpoint.snapshot.is_none() {
                bundle.checkpoint = game.checkpoint();
                bundle.compatibility = replay_compatibility();
            }
            encode_archive(&bundle, &bundle.compatibility)?
        };
        validate(&archive)?;
        let bytes = encode_backup(&archive)?;
        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&bytes)?;
        file.flush()?;
        file.as_file().sync_all()?;
        file.persist_noclobber(&output)?;
        println!("{}", output.display());
    } else {
        let archive = decode_backup(&read(&input, MAX_BACKUP_BYTES)?)?;
        let (mut bundle, game) = validate(&archive)?;
        if operation == "import" {
            let root = PathBuf::from(args.next().ok_or("save directory required")?);
            let store = FileSaveStore::new(root).with_monthly_charts();
            bundle.metadata.slot = store.allocate_id();
            bundle.metadata.kind = SaveKind::Manual;
            bundle.checkpoint = game.checkpoint();
            bundle.compatibility = store.compatibility();
            let receipt = store.write(&bundle, &SaveTarget::Create, &|| false)?;
            println!("{}", receipt.metadata.slot.as_str());
        } else if operation == "validate" {
            let checkpoint = game.checkpoint();
            let snapshot = checkpoint.snapshot.ok_or("missing portable state")?;
            println!(
                "region={}\nstate_hash={:016x}\nsession_hash={:016x}\nprojection_hash={:016x}\ncommands={}",
                checkpoint.header.region.as_str(),
                checkpoint.state_hash,
                portable_hash(&snapshot.session),
                portable_hash(&snapshot.projections),
                checkpoint.command_count
            );
        } else {
            return Err("unknown operation".into());
        }
    }
    Ok(())
}
