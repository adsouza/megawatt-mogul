//! Speculative next-month commitments. Only immutable copies leave the UI thread.
use super::executor;
use application::{AutomaticCommitmentRun, Game, PreparedReviewCommitments, ReviewCommitmentWork};
use bevy::prelude::Resource;
use domain::{
    session::{DayPhase, GameSession, SessionStatus},
    time::SimMonth,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, TryRecvError},
};

type Completion = Result<Option<PreparedReviewCommitments>, ()>;

struct Job {
    receiver: Mutex<Receiver<Completion>>,
    cancel: Arc<AtomicBool>,
}

struct PendingApplication {
    state_hash: u64,
    revision: u32,
}

#[derive(Resource, Default)]
pub struct ReviewCommitmentCache {
    source: Option<(u64, SimMonth)>,
    job: Option<Job>,
    ready: Option<PreparedReviewCommitments>,
    pending: Option<PendingApplication>,
    failed: bool,
}

pub(crate) enum ReviewPlan {
    Ready(AutomaticCommitmentRun),
    Waiting,
    Unavailable,
}

impl ReviewCommitmentCache {
    pub fn is_ready(&self) -> bool {
        self.ready.is_some()
    }
    pub fn is_preparing(&self) -> bool {
        self.job.is_some()
    }
    /// Planning is visible, but the next-month automatic plan has not yet been queued.
    pub fn is_applying(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn clear(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.source = None;
        self.ready = None;
        self.pending = None;
        self.failed = false;
    }

    fn poll(&mut self) {
        let Some(job) = &self.job else {
            return;
        };
        let completion = job
            .receiver
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .try_recv();
        match completion {
            Ok(Ok(Some(ready))) => {
                self.job = None;
                self.ready = Some(ready);
            }
            Err(TryRecvError::Empty) => {}
            _ => {
                self.job = None;
                self.failed = true;
                tracing::warn!(
                    "review commitment precomputation failed; automatic planning will retry"
                );
            }
        }
    }

    fn start(&mut self, review: &GameSession, generation: u64) {
        self.clear();
        self.source = Some((generation, review.clock().month()));
        let snapshot = review.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        match executor::spawn(
            "review-commitments",
            move || prepare(snapshot, worker_cancel),
            || Err(()),
        ) {
            Ok(receiver) => {
                self.job = Some(Job {
                    receiver: Mutex::new(receiver),
                    cancel,
                })
            }
            Err(error) => {
                self.failed = true;
                tracing::warn!(%error, "could not start review commitment precomputation");
            }
        }
    }

    /// One automatic Planning run consumes a ready result or subscribes to the running job.
    pub(crate) fn request(&mut self, game: &Game, generation: u64, revision: u32) -> ReviewPlan {
        if game.session().phase() != DayPhase::Planning {
            return ReviewPlan::Unavailable;
        }
        self.poll();
        let matches = self.source.is_some_and(|(owner, month)| {
            owner == generation
                && month.checked_add_months(1).ok() == Some(game.session().clock().month())
        });
        if !matches {
            self.clear();
            return ReviewPlan::Unavailable;
        }
        if let Some(ready) = self.ready.take() {
            self.clear();
            return ready
                .into_run(game)
                .map_or(ReviewPlan::Unavailable, ReviewPlan::Ready);
        }
        if self.job.is_some() {
            self.pending = Some(PendingApplication {
                state_hash: game.session().state_hash(),
                revision,
            });
            ReviewPlan::Waiting
        } else {
            ReviewPlan::Unavailable
        }
    }

    pub(crate) fn refresh(
        &mut self,
        game: &Game,
        generation: u64,
        revision: u32,
        enabled: bool,
    ) -> Option<AutomaticCommitmentRun> {
        if !enabled || self.source.is_some_and(|(owner, _)| owner != generation) {
            self.clear();
        }
        match game.session().phase() {
            DayPhase::Review if enabled && game.session().status() == SessionStatus::Active => {
                let source = Some((generation, game.session().clock().month()));
                if self.source != source {
                    self.start(game.session(), generation);
                }
                self.poll();
                None
            }
            DayPhase::Planning => {
                self.poll();
                let pending = self.pending.as_mut()?;
                if pending.revision != revision {
                    if pending.state_hash != game.session().state_hash() {
                        self.clear();
                        return None;
                    }
                    pending.revision = revision;
                }
                if let Some(ready) = self.ready.take() {
                    self.clear();
                    ready.into_run(game)
                } else if self.failed {
                    self.clear();
                    application::automatic_commitment_run(game)
                } else {
                    None
                }
            }
            _ => {
                self.clear();
                None
            }
        }
    }
}

impl Drop for ReviewCommitmentCache {
    fn drop(&mut self) {
        self.clear();
    }
}

async fn prepare(snapshot: GameSession, cancel: Arc<AtomicBool>) -> Completion {
    #[cfg(target_arch = "wasm32")]
    yield_to_browser().await;
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let Some(mut work) = ReviewCommitmentWork::new(snapshot) else {
        return Ok(None);
    };
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if work.advance() {
            return Ok(Some(work.finish()));
        }
        #[cfg(target_arch = "wasm32")]
        yield_to_browser().await;
    }
}

#[cfg(target_arch = "wasm32")]
async fn yield_to_browser() {
    // A macrotask lets the browser draw and accept input between candidate evaluations.
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::{Command, InMemoryCommandLog};
    use std::sync::mpsc::{self, Sender};

    fn review() -> Game {
        let mut game =
            crate::hud::test_support::eastern_game(Box::new(InMemoryCommandLog::default()));
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
        game
    }

    fn controlled_job(
        game: &Game,
    ) -> (
        ReviewCommitmentCache,
        Sender<Completion>,
        PreparedReviewCommitments,
    ) {
        let ready = ReviewCommitmentWork::new(game.session().clone())
            .unwrap()
            .finish();
        let (sender, receiver) = mpsc::channel();
        let cache = ReviewCommitmentCache {
            source: Some((7, game.session().clock().month())),
            job: Some(Job {
                receiver: Mutex::new(receiver),
                cancel: Arc::new(AtomicBool::new(false)),
            }),
            ready: None,
            pending: None,
            failed: false,
        };
        (cache, sender, ready)
    }

    #[test]
    fn continue_can_show_forecast_while_precomputation_is_still_running() {
        let mut game = review();
        let (mut cache, sender, ready) = controlled_job(&game);
        game.execute(Command::FinishReview).unwrap();
        assert!(matches!(cache.request(&game, 7, 10), ReviewPlan::Waiting));
        assert_eq!(game.session().phase(), DayPhase::Planning);
        assert!(cache.is_applying());
        assert!(cache.refresh(&game, 7, 10, true).is_none());
        sender
            .send(Ok(Some(ready)))
            .unwrap_or_else(|_| panic!("receiver exists"));
        let expected = application::automatic_commitment_run(&game).unwrap();
        let run = cache.refresh(&game, 7, 10, true).unwrap();
        assert_eq!(run.commands, expected.commands);
        assert_eq!(run.diagnostic, expected.diagnostic);
        assert!(!cache.is_applying());
    }

    #[test]
    fn planning_edits_discard_a_late_result() {
        let mut game = review();
        let (mut cache, sender, ready) = controlled_job(&game);
        game.execute(Command::FinishReview).unwrap();
        assert!(matches!(cache.request(&game, 7, 10), ReviewPlan::Waiting));
        let plant = game
            .session()
            .plant_views()
            .into_iter()
            .find(|p| p.kind == domain::region::PlantKind::Ccgt)
            .unwrap()
            .id;
        game.execute(Command::CommitPlant {
            plant,
            hours: [false; 24],
        })
        .unwrap();
        sender
            .send(Ok(Some(ready)))
            .unwrap_or_else(|_| panic!("receiver exists"));
        assert!(cache.refresh(&game, 7, 11, true).is_none());
        assert!(!cache.is_applying());
        assert!(!cache.is_ready());
    }

    #[test]
    fn a_ready_result_is_reused_only_by_the_same_session() {
        let mut game = review();
        let (mut cache, sender, ready) = controlled_job(&game);
        sender
            .send(Ok(Some(ready)))
            .unwrap_or_else(|_| panic!("receiver exists"));
        game.execute(Command::FinishReview).unwrap();
        assert!(matches!(
            cache.request(&game, 8, 10),
            ReviewPlan::Unavailable
        ));
        assert!(!cache.is_ready());
        assert!(!cache.is_preparing());
    }

    #[test]
    fn disabling_automatic_commitment_cancels_speculation() {
        let game = review();
        let (mut cache, sender, _) = controlled_job(&game);
        let cancel = cache.job.as_ref().unwrap().cancel.clone();
        assert!(cache.refresh(&game, 7, 10, false).is_none());
        assert!(cancel.load(Ordering::Relaxed));
        assert!(sender.send(Ok(None)).is_err());
    }
}
