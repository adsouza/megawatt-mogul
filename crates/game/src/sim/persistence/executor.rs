//! Platform job execution. Only ordinary Rust completion data leaves the local future/thread.
use std::{
    future::Future,
    sync::mpsc::{self, Receiver},
};
#[cfg(not(target_arch = "wasm32"))]
pub(super) trait PlatformSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send> PlatformSend for T {}
#[cfg(target_arch = "wasm32")]
pub(super) trait PlatformSend {}
#[cfg(target_arch = "wasm32")]
impl<T> PlatformSend for T {}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn spawn<T: Send + 'static, F, Fut>(
    name: &str,
    work: F,
    failed: impl FnOnce() -> T + Send + 'static,
) -> std::io::Result<Receiver<T>>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = T> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                bevy::tasks::block_on(work())
            }))
            .unwrap_or_else(|_| failed());
            let _ = tx.send(result);
        })?;
    Ok(rx)
}
#[cfg(target_arch = "wasm32")]
pub(super) fn spawn<T: 'static, F, Fut>(
    _: &str,
    work: F,
    _failed: impl FnOnce() -> T + 'static,
) -> std::io::Result<Receiver<T>>
where
    F: FnOnce() -> Fut + 'static,
    Fut: Future<Output = T> + 'static,
{
    let (tx, rx) = mpsc::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = tx.send(work().await);
    });
    Ok(rx)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_returns_its_typed_result() {
        assert_eq!(
            spawn("test", || async { Ok::<_, &str>(42) }, || Err("panic"))
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
                || async { panic!("injected worker panic") },
                || Err::<u32, _>("panic")
            )
            .unwrap()
            .recv()
            .unwrap(),
            Err("panic")
        );
    }
}
