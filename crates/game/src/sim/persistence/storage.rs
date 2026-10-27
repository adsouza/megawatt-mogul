//! Async-shaped native adapter; browser builds select the infrastructure browser adapter.
use application::*;
use std::sync::Arc;
pub(super) struct Storage(Arc<dyn SaveStore>);
impl Storage {
    pub fn new(store: Arc<dyn SaveStore>) -> Self {
        Self(store)
    }
    pub fn compatibility(&self) -> ReplayCompatibility {
        self.0.compatibility()
    }
    pub fn allocate_id(&self) -> SaveId {
        self.0.allocate_id()
    }
    pub fn now(&self) -> u64 {
        self.0.now()
    }
    pub async fn list(&self) -> Result<Vec<SaveListing>, SaveError> {
        self.0.list()
    }
    pub async fn revision(&self, id: &SaveId) -> Result<SaveRevision, SaveError> {
        self.0.revision(id)
    }
    pub async fn read_cancellable(
        &self,
        id: &SaveId,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<(SaveBundle, LoadedRegion), SaveError> {
        self.0.read_cancellable(id, cancelled)
    }
    pub async fn write(
        &self,
        bundle: &SaveBundle,
        target: &SaveTarget,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<SaveReceipt, SaveError> {
        self.0.write(bundle, target, cancelled)
    }
    pub async fn rename(
        &self,
        id: &SaveId,
        revision: SaveRevision,
        name: &str,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<SaveReceipt, SaveError> {
        self.0.rename(id, revision, name, cancelled)
    }
    pub async fn delete(
        &self,
        id: &SaveId,
        revision: SaveRevision,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<(), SaveError> {
        self.0.delete(id, revision, cancelled)
    }
}
