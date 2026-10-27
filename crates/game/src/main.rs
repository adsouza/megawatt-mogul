#![forbid(unsafe_code)]
#[cfg(not(target_arch = "wasm32"))]
fn main() -> bevy::app::AppExit {
    game::desktop::run()
}
#[cfg(target_arch = "wasm32")]
fn main() {
    game::browser::run();
}
