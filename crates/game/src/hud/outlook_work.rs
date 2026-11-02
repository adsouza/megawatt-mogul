//! Bounded, cancellable speculative queries. Only snapshots leave the UI thread.
use crate::sim::executor;
use domain::{
    ids::CityId,
    session::{GameSession, OperationalOutlook, OperationalOutlookWork},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, TryRecvError},
};

type Completion = Result<Option<OperationalOutlook>, ()>;
struct Job {
    city: Option<CityId>,
    receiver: Mutex<Receiver<Completion>>,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub(super) struct OutlookCache {
    source: Option<(u32, u64)>,
    baseline: Option<Arc<OperationalOutlook>>,
    scenario: Option<(CityId, Arc<OperationalOutlook>)>,
    job: Option<Job>,
    failed: Option<Option<CityId>>,
}
impl OutlookCache {
    pub fn cancel(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
    fn ready(&self, city: Option<CityId>) -> Option<&OperationalOutlook> {
        match city {
            None => self.baseline.as_deref(),
            Some(city) => self
                .scenario
                .as_ref()
                .filter(|(id, _)| *id == city)
                .map(|(_, r)| r.as_ref()),
        }
    }
    pub fn failed(&self, city: Option<CityId>) -> bool {
        self.failed == Some(city)
    }
    fn poll(&mut self) {
        let Some(job) = &self.job else {
            return;
        };
        let result = job
            .receiver
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .try_recv();
        match result {
            Ok(Ok(Some(result))) => {
                let city = job.city;
                self.job = None;
                if self.baseline.is_none() {
                    self.baseline = Some(Arc::new(result.baseline_only()));
                }
                if let Some(city) = city {
                    self.scenario = Some((city, Arc::new(result)));
                }
            }
            Err(TryRecvError::Empty) => {}
            _ => {
                self.failed = Some(job.city);
                self.job = None;
            }
        }
    }
    pub fn refresh(
        &mut self,
        session: &GameSession,
        source: (u32, u64),
        city: Option<CityId>,
    ) -> Option<&OperationalOutlook> {
        if self.source != Some(source) {
            self.cancel();
            self.source = Some(source);
            self.baseline = None;
            self.scenario = None;
            self.failed = None;
        }
        if self.job.as_ref().is_some_and(|job| job.city != city) {
            self.cancel();
        }
        self.poll();
        if self.ready(city).is_none() && self.job.is_none() && !self.failed(city) {
            let snapshot = session.clone();
            let baseline = self.baseline.clone();
            let cancel = Arc::new(AtomicBool::new(false));
            let worker_cancel = cancel.clone();
            match executor::spawn(
                "operational-outlook",
                move || prepare(snapshot, city, baseline, worker_cancel),
                || Err(()),
            ) {
                Ok(receiver) => {
                    self.job = Some(Job {
                        city,
                        receiver: Mutex::new(receiver),
                        cancel,
                    })
                }
                Err(error) => {
                    tracing::warn!(%error, "could not start outlook calculation");
                    self.failed = Some(city);
                }
            }
        }
        self.ready(city)
    }
}
impl Drop for OutlookCache {
    fn drop(&mut self) {
        self.cancel();
    }
}

async fn prepare(
    session: GameSession,
    city: Option<CityId>,
    baseline: Option<Arc<OperationalOutlook>>,
    cancel: Arc<AtomicBool>,
) -> Completion {
    #[cfg(target_arch = "wasm32")]
    yield_to_browser().await;
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let mut work = OperationalOutlookWork::new(session, city, baseline).map_err(|_| ())?;
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
    use domain::{region::fixtures, session::GameSettings};
    use std::sync::mpsc;

    fn session() -> GameSession {
        GameSession::new(
            Arc::new(fixtures::tiny_region()),
            7,
            GameSettings::default(),
        )
    }
    fn job(city: Option<CityId>) -> (Job, mpsc::Sender<Completion>, Arc<AtomicBool>) {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        (
            Job {
                city,
                receiver: Mutex::new(rx),
                cancel: cancel.clone(),
            },
            tx,
            cancel,
        )
    }

    #[test]
    fn cached_baseline_survives_city_changes_and_obsolete_jobs_cannot_publish() {
        let session = session();
        let baseline = Arc::new(session.operational_outlook(None).unwrap());
        let (job, tx, cancel) = job(Some(CityId(3)));
        let mut cache = OutlookCache {
            source: Some((1, 1)),
            baseline: Some(baseline.clone()),
            job: Some(job),
            scenario: None,
            failed: None,
        };
        assert!(std::ptr::eq(
            cache.refresh(&session, (1, 1), None).unwrap(),
            baseline.as_ref()
        ));
        assert!(cancel.load(Ordering::Relaxed));
        assert!(tx.send(Ok(Some((*baseline).clone()))).is_err());
        assert!(cache.job.is_none());
        cache.cancel();
        assert!(
            cache.ready(None).is_some(),
            "closing/reopening retains a valid result"
        );
    }

    #[test]
    fn changed_revision_or_generation_never_displays_previous_results() {
        for source in [(2, 1), (1, 2)] {
            let session = session();
            let baseline = Arc::new(session.operational_outlook(None).unwrap());
            let (job, tx, cancel) = job(None);
            let mut cache = OutlookCache {
                source: Some((1, 1)),
                baseline: Some(baseline.clone()),
                job: Some(job),
                scenario: None,
                failed: None,
            };
            tx.send(Ok(Some((*baseline).clone()))).unwrap();
            assert!(cache.refresh(&session, source, None).is_none());
            assert!(cancel.load(Ordering::Relaxed));
            assert!(cache.baseline.is_none());
            cache.cancel();
        }
    }

    #[test]
    fn failed_worker_is_reported_without_repeated_spawning() {
        let session = session();
        let (job, tx, _) = job(None);
        tx.send(Err(())).unwrap();
        let mut cache = OutlookCache {
            source: Some((1, 1)),
            baseline: None,
            job: Some(job),
            scenario: None,
            failed: None,
        };
        assert!(cache.refresh(&session, (1, 1), None).is_none());
        assert!(cache.failed(None));
        assert!(cache.job.is_none());
    }

    #[test]
    fn background_calculation_finishes_and_reuses_the_baseline_for_city_preview() {
        let session = session();
        let hash = session.state_hash();
        let mut cache = OutlookCache::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        for city in [None, Some(CityId(3)), None] {
            while cache.refresh(&session, (1, 1), city).is_none() {
                assert!(!cache.failed(city));
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        let baseline = cache.baseline.as_ref().unwrap();
        let scenario = &cache.scenario.as_ref().unwrap().1;
        for (a, b) in baseline.months.iter().zip(&scenario.months) {
            assert!(Arc::ptr_eq(&a.baseline, &b.baseline));
        }
        assert_eq!(session.state_hash(), hash);
    }
}
