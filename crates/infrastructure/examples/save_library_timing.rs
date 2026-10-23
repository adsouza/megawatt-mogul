//! Cold/warm/restart library measurement (writes only disposable metadata); full selected-save validation is a separate operation.
use application::SaveStore;
use infrastructure::save::FileSaveStore;
use std::time::Instant;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args_os()
        .nth(1)
        .ok_or("usage: save_library_timing <save-directory>")?;
    let store = FileSaveStore::new(root.clone().into());
    let start = Instant::now();
    let cold = store.list()?;
    let cold_time = start.elapsed();
    let start = Instant::now();
    let warm = store.list()?;
    println!(
        "{} entries; cold={cold_time:?}; warm={:?}",
        cold.len(),
        start.elapsed()
    );
    assert_eq!(cold, warm);
    let start = Instant::now();
    let restarted = FileSaveStore::new(root.into()).list()?;
    println!("restart={:?}", start.elapsed());
    assert_eq!(cold, restarted);
    Ok(())
}
