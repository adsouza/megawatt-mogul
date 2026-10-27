//! Produce canonical, content-addressed browser assets; never includes local saves/logs.
use application::RegionCatalog;
use infrastructure::{
    region::RonRegionCatalog,
    web_assets::{AssetFile, AssetManifest},
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, error::Error, fs, path::PathBuf};
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or("asset root required")?);
    let output = PathBuf::from(args.next().ok_or("output required")?);
    let build = args
        .next()
        .ok_or("source SHA required")?
        .into_string()
        .map_err(|_| "invalid SHA")?;
    fs::create_dir_all(&output)?;
    let catalog = RonRegionCatalog::new(root.join("regions"));
    let mut files = BTreeMap::new();
    for listing in catalog.list()? {
        let region = catalog.load(&listing.id)?;
        files.insert(
            format!("regions/{}.bundle", listing.id.as_str()),
            region.source_bytes,
        );
    }
    for dir in ["locales/en-US", "audio"] {
        for entry in fs::read_dir(root.join(dir))? {
            let path = entry?.path();
            if path.is_file() {
                files.insert(
                    format!(
                        "{dir}/{}",
                        path.file_name().ok_or("file name")?.to_string_lossy()
                    ),
                    fs::read(&path)?,
                );
            }
        }
    }
    let mut assets = Vec::new();
    for (name, bytes) in files {
        let checksum: [u8; 32] = Sha256::digest(&bytes).into();
        let hash: String = checksum.iter().map(|b| format!("{b:02x}")).collect();
        let url = format!("{hash}.bin");
        fs::write(output.join(&url), &bytes)?;
        assets.push(AssetFile {
            name,
            url,
            length: bytes.len(),
            checksum,
        });
    }
    fs::write(
        output.join("manifest.ron"),
        ron::ser::to_string(&AssetManifest {
            version: 1,
            build,
            assets,
        })?,
    )?;
    Ok(())
}
