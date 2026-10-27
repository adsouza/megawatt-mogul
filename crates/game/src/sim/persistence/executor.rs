//! Native job execution. Only typed completions leave the worker; panics become failures.
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::mpsc::{self, Receiver},
};

pub(super) fn spawn<T: Send + 'static>(
    name: &str,
    work: impl FnOnce() -> T + Send + 'static,
    failed: impl FnOnce() -> T + Send + 'static,
) -> std::io::Result<Receiver<T>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|_| failed());
            let _ = tx.send(result);
        })?;
    Ok(rx)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_returns_its_typed_result() {
        assert_eq!(
            spawn("test", || Ok::<_, &str>(42), || Err("panic"))
                .unwrap()
                .recv()
                .unwrap(),
            Ok(42)
        );
    }
    #[test]
    fn worker_panic_is_contained_and_delivered() {
        assert_eq!(
            spawn(
                "test",
                || panic!("injected worker panic"),
                || Err::<u32, _>("panic")
            )
            .unwrap()
            .recv()
            .unwrap(),
            Err("panic")
        );
    }
}
