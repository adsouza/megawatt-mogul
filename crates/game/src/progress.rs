//! Player progress (`progress.ron`): whether the first-game tutorial is owed and which regions the
//! player has played (first-game tutorial spec §5, §6). Kept apart from preferences because it
//! must never be silently reset: a file that exists but can't be read is never overwritten.
use crate::{paths::SaveDir, sim::Session};
use bevy::prelude::*;
use infrastructure::client_progress::ClientProgress;
#[cfg(not(target_arch = "wasm32"))]
use infrastructure::client_progress::{read_progress, write_progress};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

#[cfg(target_arch = "wasm32")]
type Reply<T> = std::sync::Mutex<std::sync::mpsc::Receiver<Result<T, application::SaveError>>>;

/// The player's progress in memory. `HudPlugin` initialises it, so headless tests use it without
/// any I/O; only [`ProgressPlugin`] loads and writes it.
#[derive(Resource, Default)]
pub struct ClientProgressStore {
    pub value: ClientProgress,
    /// The load has finished, successfully or not.
    pub loaded: bool,
    /// The file was read (or is missing), so writing it can't destroy anything unread.
    writable: bool,
    /// The value last loaded or written; a write happens when `value` differs from it.
    written: Option<ClientProgress>,
    #[cfg(not(target_arch = "wasm32"))]
    root: Option<PathBuf>,
    #[cfg(target_arch = "wasm32")]
    reading: Option<Reply<ClientProgress>>,
    #[cfg(target_arch = "wasm32")]
    pending: Option<(ClientProgress, Reply<()>)>,
}

impl ClientProgressStore {
    fn finish_load(&mut self, result: Result<ClientProgress, application::SaveError>) {
        match result {
            Ok(value) => {
                self.written = Some(value.clone());
                self.value = value;
                self.writable = true;
            }
            Err(error) => {
                warn!("cannot read player progress; it will not be saved this run: {error}");
                self.writable = false;
            }
        }
        self.loaded = true;
    }

    fn wants_write(&self) -> bool {
        self.writable && self.written.as_ref() != Some(&self.value)
    }
}

/// Starts every region picker card unlocked for this run (`--unlock-all-regions`). It never
/// writes `played_regions`.
#[derive(Resource)]
pub struct UnlockAllRegions;

/// Loads `progress.ron` from [`SaveDir`] once and writes it in `Last` whenever it changes.
pub struct ProgressPlugin;

impl Plugin for ProgressPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClientProgressStore>()
            .add_systems(PreUpdate, load)
            .add_systems(Last, write_changed);
    }
}

/// Adds the session's region to `played_regions` once its game counts as played (spec §6).
pub fn record_played_region(
    session: Option<Res<Session>>,
    mut progress: ResMut<ClientProgressStore>,
) {
    let Some(session) = session else {
        return;
    };
    if !session.0.projections().expansion.played() {
        return;
    }
    let id = session.0.session().region().id().as_str();
    if !progress.value.played_regions.contains(id) {
        progress.value.played_regions.insert(id.to_owned());
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn load(dir: Option<Res<SaveDir>>, mut store: ResMut<ClientProgressStore>) {
    if store.loaded {
        return;
    }
    let Some(dir) = dir else {
        return;
    };
    store.root = Some(dir.0.clone());
    store.finish_load(read_progress(&dir.0));
}

#[cfg(target_arch = "wasm32")]
fn load(dir: Option<Res<SaveDir>>, mut store: ResMut<ClientProgressStore>) {
    let _ = dir;
    if store.loaded {
        return;
    }
    if store.reading.is_none() {
        let (tx, rx) = std::sync::mpsc::channel();
        wasm_bindgen_futures::spawn_local(async move {
            let _ = tx.send(infrastructure::browser::read_progress().await);
        });
        store.reading = Some(std::sync::Mutex::new(rx));
        return;
    }
    let result = store
        .reading
        .as_ref()
        .and_then(|rx| rx.lock().ok()?.try_recv().ok());
    if let Some(result) = result {
        store.reading = None;
        store.finish_load(result);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write_changed(mut store: ResMut<ClientProgressStore>) {
    if !store.wants_write() {
        return;
    }
    let Some(root) = store.root.clone() else {
        return;
    };
    let value = store.value.clone();
    if let Err(error) = write_progress(&root, &value) {
        warn!("cannot write player progress; retrying on the next change: {error}");
    }
    // A failed write is not retried every frame; the next change tries again.
    store.written = Some(value);
}

#[cfg(target_arch = "wasm32")]
fn write_changed(mut store: ResMut<ClientProgressStore>) {
    let result = store
        .pending
        .as_ref()
        .and_then(|(_, rx)| rx.lock().ok()?.try_recv().ok());
    if let Some(result) = result {
        let (value, _) = store.pending.take().expect("completed progress write");
        if let Err(error) = result {
            warn!("cannot write player progress; retrying on the next change: {error}");
        }
        store.written = Some(value);
    }
    if store.pending.is_some() || !store.wants_write() {
        return;
    }
    let value = store.value.clone();
    let captured = value.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = tx.send(infrastructure::browser::write_progress(&captured).await);
    });
    store.pending = Some((value, std::sync::Mutex::new(rx)));
}
