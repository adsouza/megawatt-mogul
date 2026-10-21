//! Persistence orchestration. Immutable captures leave the main thread; only verified candidates
//! return. Commands and worker results are scoped to a session generation.
use super::{CommandFailed, DomainEventMessage, PlayerCommand, ReturnToMenu, Session, SimSpeed};
use crate::{
    paths::SaveDir,
    state::{AppState, PhaseState},
};
use application::*;
use bevy::prelude::*;
use domain::session::{DayPhase, SessionStatus};
use infrastructure::save::FileSaveStore;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
};

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionGeneration(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub enum PersistAction {
    OpenSave { leave: bool },
    OpenLeave,
    OpenBrowser,
    SaveNew(String),
    ChooseOverwrite { slot: SaveId, name: String },
    ConfirmOverwrite,
    Load(SaveId),
    Retry,
    Refresh,
    Cancel,
    ContinueWithoutSaving,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SaveScreen {
    #[default]
    Closed,
    Save,
    Leave,
    Browser,
    ConfirmOverwrite,
    Automatic,
}
#[derive(Clone)]
struct WriteTask {
    bundle: SaveBundle,
    target: SaveTarget,
    automatic: bool,
    leave: bool,
}
#[derive(Clone, Copy)]
enum ManualIntent {
    Save { leave: bool },
    Leave,
}
impl ManualIntent {
    fn screen(self) -> SaveScreen {
        match self {
            Self::Save { .. } => SaveScreen::Save,
            Self::Leave => SaveScreen::Leave,
        }
    }
}
struct Run {
    generation: u64,
    id: SaveId,
    automatic: SaveId,
    revision: Option<SaveRevision>,
}
enum Completion {
    Listed(Result<Vec<SaveListing>, SaveError>),
    Saved(Result<SaveReceipt, SaveError>),
    Loaded(Result<Box<Game>, SaveError>),
    Failed(SaveError),
}
/// The boundary whose autosave failed. Boundaries of one run are ordered by their accepted-command
/// count, so a save with at least as many commands covers it.
#[derive(Debug, Clone)]
struct FailedAutosave {
    summary: CheckpointSummary,
    command_count: u64,
}
struct Job {
    receiver: Mutex<Receiver<Completion>>,
    cancel: Arc<AtomicBool>,
    generation: u64,
    token: u64,
}
#[derive(Resource, Default)]
pub struct Persistence {
    store: Option<Arc<dyn SaveStore>>,
    pub screen: SaveScreen,
    pub entries: Vec<SaveListing>,
    pub error: Option<SaveError>,
    pub last_summary: Option<CheckpointSummary>,
    last_saved: Option<SaveReceipt>,
    job: Option<Job>,
    token: u64,
    run: Option<Run>,
    pub(super) pending: VecDeque<PlayerCommand>,
    pub(super) draining: bool,
    capture: Option<GameCheckpoint>,
    manual: Option<ManualIntent>,
    previous_speed: SimSpeed,
    dialog_generation: u64,
    task: Option<WriteTask>,
    /// The latest month-close autosave that failed and has not been made good since (spec §17.5).
    /// It outlives *Continue without saving*: only a confirmed save at that boundary or a later
    /// one clears it. A new game or a load starts from a fresh `Persistence`, without it.
    autosave_failed: Option<FailedAutosave>,
    pub(super) resuming: bool,
    pub(super) resume_phase: Option<DayPhase>,
    return_after_save: bool,
}
impl Persistence {
    pub fn with_store(store: Arc<dyn SaveStore>) -> Self {
        Self {
            store: Some(store),
            ..Default::default()
        }
    }
    pub fn enabled(&self) -> bool {
        self.store.is_some()
    }
    pub fn busy(&self) -> bool {
        self.job.is_some() || self.draining
    }
    pub fn frozen(&self) -> bool {
        self.resuming
            || self.return_after_save
            || self.screen != SaveScreen::Closed
            || self.job.is_some()
            || self.manual.is_some()
    }
    pub fn last_saved(&self) -> Option<&SaveReceipt> {
        self.last_saved.as_ref()
    }
    pub fn checkpoint_summary(&self) -> Option<&CheckpointSummary> {
        self.capture.as_ref().map(|c| &c.summary)
    }
    pub fn overwrite_entry(&self) -> Option<&SaveEntry> {
        let slot = &self.task.as_ref()?.bundle.metadata.slot;
        self.entries
            .iter()
            .find(|entry| entry.id.as_ref() == Some(slot))?
            .entry
            .as_ref()
            .ok()
    }
    pub fn can_save(&self) -> bool {
        self.store.is_some() && self.capture.is_some() && !self.busy()
    }
    /// The boundary of the month-close autosave that failed, while no confirmed save covers it.
    pub fn autosave_failure(&self) -> Option<&CheckpointSummary> {
        self.autosave_failed.as_ref().map(|failed| &failed.summary)
    }
    /// A persistence state whose latest autosave failed at `summary`, for presenter tests.
    #[cfg(test)]
    pub(crate) fn with_failed_autosave(summary: CheckpointSummary) -> Self {
        Self {
            autosave_failed: Some(FailedAutosave {
                summary,
                command_count: 0,
            }),
            ..Default::default()
        }
    }
    /// Remembers the boundary of the autosave that just failed, so the warning shows behind the
    /// error dialog and outlives it. Called wherever an automatic write fails: a `SaveError`
    /// receipt, a worker that died (`Completion::Failed`) or one that never started. Nothing
    /// else can fail an automatic task. A newer failure replaces an older one: it is the later
    /// boundary.
    fn autosave_did_fail(&mut self) {
        if let Some(task) = self.task.as_ref().filter(|task| task.automatic) {
            self.autosave_failed = Some(FailedAutosave {
                summary: task.bundle.checkpoint.summary.clone(),
                command_count: task.bundle.checkpoint.command_count,
            });
        }
    }
    /// A confirmed save of `checkpoint` clears the warning when it covers the failed boundary:
    /// the same one (a successful Retry, or a manual save there) or a later one.
    fn save_confirmed(&mut self, checkpoint: &GameCheckpoint) {
        if self
            .autosave_failed
            .as_ref()
            .is_some_and(|failed| checkpoint.command_count >= failed.command_count)
        {
            self.autosave_failed = None;
        }
    }
    pub fn automatic_error(&self) -> bool {
        self.task.as_ref().is_some_and(|t| t.automatic) && self.error.is_some()
    }
    pub(super) fn enter(&mut self, generation: u64) {
        if self
            .run
            .as_ref()
            .is_some_and(|r| r.generation == generation)
        {
            return;
        }
        self.run = self.store.as_ref().map(|store| Run {
            generation,
            id: store.allocate_id(),
            automatic: store.allocate_id(),
            revision: None,
        });
    }
    pub(super) fn abandon(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        let store = self.store.clone();
        *self = Self {
            store,
            ..Default::default()
        };
    }
    fn spawn(
        &mut self,
        generation: u64,
        work: impl FnOnce(Arc<AtomicBool>) -> Completion + Send + 'static,
    ) {
        if self.job.is_some() {
            return;
        }
        self.token = self.token.wrapping_add(1);
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        match thread::Builder::new()
            .name("save-resume".into())
            .spawn(move || {
                let completion =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(worker_cancel)))
                        .unwrap_or_else(|_| {
                            Completion::Failed(SaveError::Io {
                                stage: "worker",
                                message: "persistence worker panicked".into(),
                            })
                        });
                let _ = tx.send(completion);
            }) {
            Ok(_) => {
                self.job = Some(Job {
                    receiver: Mutex::new(rx),
                    cancel,
                    generation,
                    token: self.token,
                })
            }
            Err(error) => {
                self.error = Some(SaveError::Io {
                    stage: "worker",
                    message: error.to_string(),
                });
                // An autosave whose worker never started has failed too.
                self.autosave_did_fail();
            }
        }
    }
    fn list(&mut self, generation: u64) {
        let Some(store) = self.store.clone() else {
            self.unavailable();
            return;
        };
        self.error = None;
        self.spawn(generation, move |_| Completion::Listed(store.list()));
    }
    fn unavailable(&mut self) {
        self.error = Some(SaveError::Io {
            stage: "store",
            message: String::new(),
        });
    }
    fn write(&mut self, generation: u64) {
        let (Some(store), Some(task)) = (self.store.clone(), self.task.clone()) else {
            self.unavailable();
            return;
        };
        self.error = None;
        self.spawn(generation, move |cancel| {
            Completion::Saved(store.write(&task.bundle, &task.target, &|| {
                cancel.load(Ordering::Relaxed)
            }))
        });
    }
    fn prepare_write(&mut self, slot: SaveId, target: SaveTarget, name: String, generation: u64) {
        let (Some(store), Some(capture), Some(run)) = (&self.store, &self.capture, &self.run)
        else {
            self.unavailable();
            return;
        };
        let metadata = SaveMetadata {
            slot,
            run: run.id.clone(),
            kind: SaveKind::Manual,
            name: name.trim().into(),
            saved_at: store.now(),
        };
        if let Err(error) = metadata.validate() {
            self.error = Some(error);
            return;
        }
        self.error = None;
        self.task = Some(WriteTask {
            bundle: SaveBundle {
                compatibility: store.compatibility(),
                metadata,
                checkpoint: capture.clone(),
            },
            target,
            automatic: false,
            leave: matches!(self.manual, Some(ManualIntent::Save { leave: true })),
        });
        if matches!(
            self.task.as_ref().map(|t| &t.target),
            Some(SaveTarget::Replace(_))
        ) {
            self.screen = SaveScreen::ConfirmOverwrite;
        } else {
            self.write(generation);
        }
    }
    /// The dispatcher owns UI intentions. Capture itself waits for the command funnel to drain.
    pub fn request(
        &mut self,
        action: PersistAction,
        generation: u64,
        speed: &mut SimSpeed,
        log_dir: Option<std::path::PathBuf>,
    ) {
        if self.busy() && !matches!(action, PersistAction::Cancel) {
            return;
        }
        match action {
            PersistAction::OpenSave { leave } => {
                if self.screen == SaveScreen::Leave {
                    self.manual = Some(ManualIntent::Save { leave });
                    self.screen = SaveScreen::Save;
                    self.list(generation);
                } else if self.screen == SaveScreen::Closed {
                    self.previous_speed = *speed;
                    *speed = SimSpeed::Paused;
                    self.dialog_generation = generation;
                    self.manual = Some(ManualIntent::Save { leave });
                    self.draining = true;
                    self.screen = SaveScreen::Save;
                }
            }
            PersistAction::OpenLeave if self.screen == SaveScreen::Closed => {
                self.previous_speed = *speed;
                *speed = SimSpeed::Paused;
                self.dialog_generation = generation;
                self.manual = Some(ManualIntent::Leave);
                self.draining = true;
                self.screen = SaveScreen::Leave;
            }
            PersistAction::OpenBrowser if self.screen == SaveScreen::Closed => {
                self.screen = SaveScreen::Browser;
                self.list(generation);
            }
            PersistAction::Refresh
                if matches!(self.screen, SaveScreen::Save | SaveScreen::Browser) =>
            {
                self.list(generation)
            }
            PersistAction::SaveNew(name) if self.screen == SaveScreen::Save && self.can_save() => {
                let slot = self.store.as_ref().expect("can_save").allocate_id();
                self.prepare_write(slot, SaveTarget::Create, name, generation);
            }
            PersistAction::ChooseOverwrite { slot, name }
                if self.screen == SaveScreen::Save && self.can_save() =>
            {
                if let Some(entry) = self
                    .entries
                    .iter()
                    .find(|e| e.id.as_ref() == Some(&slot))
                    .and_then(|e| e.entry.as_ref().ok())
                    .filter(|e| e.metadata.kind == SaveKind::Manual)
                {
                    self.prepare_write(slot, SaveTarget::Replace(entry.revision), name, generation);
                }
            }
            PersistAction::ConfirmOverwrite if self.screen == SaveScreen::ConfirmOverwrite => {
                self.write(generation)
            }
            PersistAction::Load(slot) if self.screen == SaveScreen::Browser => {
                let (Some(store), Some(dir)) = (self.store.clone(), log_dir) else {
                    self.unavailable();
                    return;
                };
                self.error = None;
                self.spawn(generation, move |cancel| {
                    Completion::Loaded((|| {
                        let (bundle, region) = store.read(&slot)?;
                        if cancel.load(Ordering::Relaxed) {
                            return Err(SaveError::Cancelled);
                        }
                        let (_, sink) = super::new_game::create_log(
                            &dir,
                            &format!("resume-{}", store.allocate_id().as_str()),
                        )
                        .map_err(RestoreError::Log)?;
                        Game::restore(region, bundle.checkpoint, Box::new(sink), &|| {
                            cancel.load(Ordering::Relaxed)
                        })
                        .map(Box::new)
                        .map_err(Into::into)
                    })())
                });
            }
            PersistAction::Retry if self.error.is_some() => {
                if self.task.is_some() {
                    self.write(generation);
                } else {
                    self.list(generation);
                }
            }
            PersistAction::ContinueWithoutSaving if self.automatic_error() => {
                // The failure was recorded when it happened (`autosave_did_fail`), so the warning
                // outlives the task cleared here.
                self.task = None;
                self.error = None;
                self.screen = self.manual.map_or(SaveScreen::Closed, ManualIntent::screen);
                if self.screen == SaveScreen::Save {
                    self.list(generation);
                }
            }
            PersistAction::Cancel => {
                if self.screen == SaveScreen::Automatic {
                    return;
                }
                if let Some(job) = self.job.take() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
                self.token = self.token.wrapping_add(1);
                if self.screen == SaveScreen::ConfirmOverwrite {
                    self.screen = SaveScreen::Save;
                    self.task = None;
                    self.error = None;
                    return;
                }
                if self.dialog_generation == generation {
                    *speed = self.previous_speed;
                }
                self.capture = None;
                self.manual = None;
                self.task = None;
                self.error = None;
                self.draining = false;
                self.screen = SaveScreen::Closed;
            }
            _ => (),
        }
    }
    pub(super) fn after_apply(&mut self, game: &Game, generation: u64) {
        self.enter(generation);
        if self.draining {
            self.capture = Some(game.checkpoint());
            self.draining = false;
            if self.screen == SaveScreen::Save && self.job.is_none() {
                self.list(generation);
            }
        }
    }
    pub(super) fn month_closed(&mut self, game: &Game, generation: u64) {
        // A pending dialog can outlive Operating while its admitted final tick is drained.
        // Cancelling it in Review must never restore the preceding day's running speed.
        self.previous_speed = SimSpeed::Paused;
        let Some(store) = self.store.clone() else {
            return;
        };
        self.enter(generation);
        let Some(run) = &self.run else {
            return;
        };
        let checkpoint = game.checkpoint();
        if self.manual.is_some() {
            self.capture = Some(checkpoint.clone());
            self.draining = false;
        }
        self.task = Some(WriteTask {
            bundle: SaveBundle {
                compatibility: store.compatibility(),
                metadata: SaveMetadata {
                    slot: run.automatic.clone(),
                    run: run.id.clone(),
                    kind: SaveKind::Automatic,
                    name: "save-autosave".into(),
                    saved_at: store.now(),
                },
                checkpoint,
            },
            target: run.revision.map_or(SaveTarget::Create, SaveTarget::Replace),
            automatic: true,
            leave: false,
        });
        self.screen = SaveScreen::Automatic;
        self.write(generation);
    }
}

pub(super) fn initialise_store(dir: Option<Res<SaveDir>>, mut persistence: ResMut<Persistence>) {
    if persistence.store.is_none()
        && let Some(dir) = dir
    {
        persistence.store = Some(Arc::new(FileSaveStore::new(dir.0.clone())));
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn poll_persistence(
    mut persistence: ResMut<Persistence>,
    mut generation: ResMut<SessionGeneration>,
    mut commands: Commands,
    mut next: ResMut<NextState<AppState>>,
    state: Res<State<AppState>>,
    phase: Option<Res<State<PhaseState>>>,
    session: Option<Res<Session>>,
    mut speed: ResMut<SimSpeed>,
    mut to_menu: MessageWriter<ReturnToMenu>,
    mut player: ResMut<Messages<PlayerCommand>>,
    mut events: ResMut<Messages<DomainEventMessage>>,
    mut failures: ResMut<Messages<CommandFailed>>,
) {
    if persistence.resuming {
        *speed = SimSpeed::Paused;
        let settled = *state.get() == AppState::InGame
            && session.as_ref().is_some_and(|s| {
                phase
                    .as_ref()
                    .is_some_and(|phase| *phase.get() == PhaseState::from(s.0.session().phase()))
            });
        if settled {
            persistence.resuming = false;
            persistence.resume_phase = None;
        }
    }
    let result = persistence
        .job
        .as_ref()
        .and_then(|job| job.receiver.lock().ok()?.try_recv().ok());
    let Some(result) = result else {
        return;
    };
    let job = persistence.job.take().expect("completion has job");
    if job.token != persistence.token
        || job.generation != generation.0
        || job.cancel.load(Ordering::Relaxed)
    {
        return;
    }
    match result {
        Completion::Failed(error) => {
            warn!("persistence worker failed: {error}");
            persistence.error = Some(error);
            // A worker that died writing an autosave is a failed autosave.
            persistence.autosave_did_fail();
        }
        Completion::Listed(result) => match result {
            Ok(entries) => persistence.entries = entries,
            Err(error) => {
                warn!("persistence operation failed: {error}");
                persistence.error = Some(error);
            }
        },
        Completion::Saved(result) => match result {
            Ok(receipt) => {
                let Some(task) = persistence.task.take() else {
                    return;
                };
                if task.automatic
                    && let Some(run) = &mut persistence.run
                {
                    run.revision = Some(receipt.revision);
                }
                persistence.save_confirmed(&task.bundle.checkpoint);
                persistence.last_summary = Some(task.bundle.checkpoint.summary);
                persistence.last_saved = Some(receipt);
                persistence.error = None;
                *speed = SimSpeed::Paused;
                if task.automatic && persistence.manual.is_some() {
                    persistence.screen = persistence
                        .manual
                        .map_or(SaveScreen::Closed, ManualIntent::screen);
                    if persistence.screen == SaveScreen::Save {
                        persistence.list(generation.0);
                    }
                } else {
                    persistence.screen = SaveScreen::Closed;
                    persistence.capture = None;
                    persistence.manual = None;
                    if task.leave {
                        persistence.return_after_save = true;
                        to_menu.write(ReturnToMenu);
                    }
                }
            }
            Err(error) => {
                warn!("save failed: {error}");
                let automatic = persistence.task.as_ref().is_some_and(|task| task.automatic);
                if automatic {
                    persistence.autosave_did_fail();
                }
                if let SaveError::DurabilityUnconfirmed { revision, .. } = &error
                    && let Some(task) = &mut persistence.task
                {
                    task.target = SaveTarget::ConfirmDurability(*revision);
                }
                // Publication already changed the visible file. Remember its revision for a
                // later autosave without presenting it as a confirmed durable save.
                if automatic
                    && let SaveError::DurabilityUnconfirmed { revision, .. } = &error
                    && let Some(run) = &mut persistence.run
                {
                    run.revision = Some(*revision);
                }
                persistence.error = Some(error);
            }
        },
        Completion::Loaded(result) => match result {
            Ok(game) => {
                generation.0 = generation.0.wrapping_add(1);
                persistence.abandon();
                persistence.enter(generation.0);
                persistence.resuming = true;
                persistence.resume_phase = Some(game.session().phase());
                player.clear();
                events.clear();
                failures.clear();
                *speed = SimSpeed::Paused;
                commands.insert_resource(Session(*game));
                next.set(AppState::InGame);
            }
            Err(error) => {
                warn!("persistence operation failed: {error}");
                persistence.error = Some(error);
            }
        },
    }
}

pub(super) fn resume_cancel_speed(
    persistence: Res<Persistence>,
    session: Option<Res<Session>>,
    mut speed: ResMut<SimSpeed>,
) {
    if persistence.resuming
        || (persistence.screen == SaveScreen::Closed
            && session.is_some_and(|s| {
                s.0.session().phase() != DayPhase::Operating
                    || s.0.session().status() != SessionStatus::Active
            }))
    {
        *speed = SimSpeed::Paused;
    }
}
