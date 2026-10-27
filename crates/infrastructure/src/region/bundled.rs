//! A read-only catalog of fetched canonical bundles. No filesystem or browser dependency.
use super::{load_region_bytes, parse_region_bundle};
use application::{LoadedRegion, RegionCatalog, RegionListing, RegionLoadError, RegionSummary};
use domain::ids::RegionId;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct BundleRegionCatalog(BTreeMap<RegionId, Vec<u8>>);
impl BundleRegionCatalog {
    pub fn new(bundles: BTreeMap<RegionId, Vec<u8>>) -> Self {
        Self(bundles)
    }
}
impl RegionCatalog for BundleRegionCatalog {
    fn list(&self) -> Result<Vec<RegionListing>, RegionLoadError> {
        Ok(self
            .0
            .iter()
            .map(|(id, bytes)| RegionListing {
                id: id.clone(),
                entry: parse_region_bundle(bytes, id).map(|file| RegionSummary {
                    id: id.clone(),
                    name_key: file.name_key().into(),
                }),
            })
            .collect())
    }
    fn load(&self, id: &RegionId) -> Result<LoadedRegion, RegionLoadError> {
        load_region_bytes(
            self.0
                .get(id)
                .ok_or_else(|| RegionLoadError::NotFound(id.clone()))?
                .clone(),
            id,
        )
    }
}
