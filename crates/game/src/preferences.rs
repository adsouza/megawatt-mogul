//! Automatically retained client layout. Preference writes are separate from game saves.
use crate::{
    hud::{HudSet, LastSpeed, SupplyChartLayout, UiState},
    paths::SaveDir,
    sim::SimSpeed,
};
use bevy::{
    prelude::*,
    window::{PrimaryWindow, WindowPosition},
};
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use infrastructure::client_preferences::{ClientPreferences, PreferenceWindow};
#[cfg(not(target_arch = "wasm32"))]
use infrastructure::client_preferences::{read_preferences, write_preferences};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::thread::JoinHandle;

const PANELS: [&str; 6] = [
    "inspector",
    "build",
    "planning",
    "operating",
    "operating_side",
    "operating_log",
];
#[cfg(not(target_arch = "wasm32"))]
struct PendingWrite {
    value: ClientPreferences,
    handle: JoinHandle<Result<(), application::SaveError>>,
}
#[cfg(target_arch = "wasm32")]
struct PendingWrite {
    value: ClientPreferences,
    receiver: std::sync::Mutex<std::sync::mpsc::Receiver<Result<(), application::SaveError>>>,
}
#[derive(Resource, Default)]
pub struct ClientSettings {
    pub value: ClientPreferences,
    pub loaded: bool,
    pub(crate) egui_applied: bool,
    #[cfg(not(target_arch = "wasm32"))]
    root: Option<PathBuf>,
    #[cfg(target_arch = "wasm32")]
    reading: Option<
        std::sync::Mutex<
            std::sync::mpsc::Receiver<Result<ClientPreferences, application::SaveError>>,
        >,
    >,
    written: Option<ClientPreferences>,
    pending: Option<PendingWrite>,
    dirty_at: Option<f64>,
}
impl ClientSettings {
    pub fn speed(&self) -> SimSpeed {
        match self.value.speed {
            2 => SimSpeed::X2,
            4 => SimSpeed::X4,
            8 => SimSpeed::X8,
            16 => SimSpeed::X16,
            32 => SimSpeed::X32,
            _ => SimSpeed::X1,
        }
    }
}
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreferenceSet {
    Load,
}
pub struct PreferencesPlugin;
impl Plugin for PreferencesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClientSettings>()
            .add_systems(Update, load.in_set(PreferenceSet::Load))
            .add_systems(EguiPrimaryContextPass, apply_egui.before(HudSet::Theme))
            .add_systems(EguiPrimaryContextPass, observe_egui.after(HudSet::Overlays))
            // Capture native move/resize events before close_when_requested can remove the window.
            .add_systems(PreUpdate, observe_window)
            .add_systems(Last, (observe_window, write_changed, flush_exit).chain());
    }
}
fn load(
    dir: Option<Res<SaveDir>>,
    mut settings: ResMut<ClientSettings>,
    last: Option<ResMut<LastSpeed>>,
    controls: Option<ResMut<crate::world::WorldControls>>,
    ui: Option<ResMut<UiState>>,
) {
    if settings.loaded {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(dir) = dir else {
            return;
        };
        match read_preferences(&dir.0) {
            Ok(value) => settings.value = value,
            Err(error) => warn!("cannot read client preferences: {error}"),
        }
        settings.root = Some(dir.0.clone());
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = dir;
        if settings.reading.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = tx.send(infrastructure::browser::read_preferences().await);
            });
            settings.reading = Some(std::sync::Mutex::new(rx));
            return;
        }
        let result = settings
            .reading
            .as_ref()
            .and_then(|rx| rx.lock().ok()?.try_recv().ok());
        let Some(result) = result else {
            return;
        };
        settings.reading = None;
        match result {
            Ok(value) => settings.value = value,
            Err(error) => warn!("cannot read browser preferences: {error}"),
        }
    }
    settings.written = Some(settings.value.clone());
    settings.loaded = true;
    // The menu already owns LastSpeed before its first egui pass. Seed it before the observer
    // can mistake its default 1x for a player choice and overwrite the restored preference.
    if let Some(mut last) = last {
        last.0 = settings.speed();
    }
    if let Some(mut ui) = ui {
        ui.supply_chart_layout = if settings.value.combined_supply_chart {
            SupplyChartLayout::Combined
        } else {
            SupplyChartLayout::Separate
        };
    }
    if let Some(mut controls) = controls {
        controls.effects = settings.value.effects;
    }
}
fn apply_egui(mut contexts: EguiContexts, mut settings: ResMut<ClientSettings>) -> Result {
    if !settings.loaded || settings.egui_applied {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    ctx.set_zoom_factor(settings.value.zoom);
    for id in PANELS {
        if let Some(size) = settings.value.panels.get(id) {
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
            ctx.data_mut(|data| {
                data.insert_persisted(egui::Id::new(id), egui::PanelState { outer_rect: rect })
            });
        }
    }
    settings.egui_applied = true;
    Ok(())
}
fn observe_egui(
    mut contexts: EguiContexts,
    mut settings: ResMut<ClientSettings>,
    last: Option<Res<LastSpeed>>,
    controls: Option<Res<crate::world::WorldControls>>,
    ui: Option<Res<UiState>>,
) -> Result {
    if !settings.loaded || !settings.egui_applied {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    settings.value.zoom = ctx.zoom_factor().clamp(0.2, 5.0);
    if let Some(ui) = ui {
        settings.value.combined_supply_chart =
            ui.supply_chart_layout == SupplyChartLayout::Combined;
    }
    if let Some(controls) = controls {
        settings.value.effects = controls.effects;
    }
    for id in PANELS {
        if let Some(panel) = egui::PanelState::load(ctx, egui::Id::new(id)) {
            let size = panel.size();
            if size.is_finite() && size.x > 0.0 && size.y > 0.0 {
                settings.value.panels.insert(id.into(), [size.x, size.y]);
            }
        }
    }
    if let Some(tool) = egui::AreaState::load(ctx, egui::Id::new("milestones"))
        && let Some(size) = tool.size
    {
        let pos = tool.left_top_pos();
        if size.x > 0.0 && size.y > 0.0 {
            settings
                .value
                .tools
                .insert("milestones".into(), [pos.x, pos.y, size.x, size.y]);
        }
    }
    if let Some(last) = last
        && last.0 != SimSpeed::Paused
    {
        settings.value.speed = last.0.ticks();
    }
    Ok(())
}
fn observe_window(
    windows: Query<&Window, With<PrimaryWindow>>,
    ready: Option<Res<crate::window::OpeningSizeApplied>>,
    mut settings: ResMut<ClientSettings>,
) {
    if !settings.loaded || ready.is_none() {
        return;
    }
    if let Ok(window) = windows.single()
        && let WindowPosition::At(at) = window.position
        && window.width() >= 64.0
        && window.height() >= 64.0
    {
        settings.value.window = Some(PreferenceWindow {
            width: window.width(),
            height: window.height(),
            x: at.x,
            y: at.y,
        });
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn write_changed(time: Res<Time<Real>>, mut settings: ResMut<ClientSettings>) {
    if settings
        .pending
        .as_ref()
        .is_some_and(|p| p.handle.is_finished())
    {
        let pending = settings.pending.take().expect("completed preference job");
        match pending.handle.join() {
            Ok(Ok(())) => settings.written = Some(pending.value),
            result => {
                warn!("cannot write client preferences: {result:?}");
                settings.dirty_at = Some(time.elapsed_secs_f64());
            }
        }
    }
    if !settings.loaded
        || settings.pending.is_some()
        || settings.written.as_ref() == Some(&settings.value)
    {
        return;
    }
    let now = time.elapsed_secs_f64();
    let since = *settings.dirty_at.get_or_insert(now);
    if now - since < 0.3 {
        return;
    }
    let Some(root) = settings.root.clone() else {
        return;
    };
    let value = settings.value.clone();
    let captured = value.clone();
    match std::thread::Builder::new()
        .name("client-preferences".into())
        .spawn(move || write_preferences(&root, &captured))
    {
        Ok(handle) => settings.pending = Some(PendingWrite { value, handle }),
        Err(error) => warn!("cannot start preference writer: {error}"),
    }
    settings.dirty_at = None;
}
#[cfg(not(target_arch = "wasm32"))]
fn flush_exit(mut exit: MessageReader<AppExit>, mut settings: ResMut<ClientSettings>) {
    if exit.read().next().is_none() || !settings.loaded {
        return;
    }
    // An older worker must finish before the last, newest preference snapshot is published.
    if let Some(pending) = settings.pending.take() {
        let _ = pending.handle.join();
    }
    if let Some(root) = &settings.root
        && let Err(error) = write_preferences(root, &settings.value)
    {
        warn!("cannot flush client preferences: {error}");
    }
}

/// Restore a logical window size on one physical monitor; all of its geometry remains reachable.
pub fn fit_window(
    saved: &PreferenceWindow,
    origin: IVec2,
    pixels: UVec2,
    scale: f64,
) -> PreferenceWindow {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale as f32
    } else {
        1.0
    };
    let (inset, available) = crate::window::screen_work_area(pixels.as_vec2() / scale);
    let top_left = origin.as_vec2() + inset * scale;
    let size = Vec2::new(saved.width, saved.height)
        .max(crate::window::MIN_WINDOW_SIZE)
        .min(available);
    let room = (available - size).max(Vec2::ZERO) * scale;
    PreferenceWindow {
        width: size.x,
        height: size.y,
        x: (saved.x as f32)
            .clamp(top_left.x, top_left.x + room.x)
            .round() as i32,
        y: (saved.y as f32)
            .clamp(top_left.y, top_left.y + room.y)
            .round() as i32,
    }
}

#[cfg(target_arch = "wasm32")]
fn write_changed(time: Res<Time<Real>>, mut settings: ResMut<ClientSettings>) {
    let result = settings
        .pending
        .as_ref()
        .and_then(|p| p.receiver.lock().ok()?.try_recv().ok());
    if let Some(result) = result {
        let pending = settings.pending.take().expect("completed preference job");
        match result {
            Ok(()) => settings.written = Some(pending.value),
            Err(error) => {
                warn!("cannot write browser preferences: {error}");
                settings.dirty_at = Some(time.elapsed_secs_f64());
            }
        }
    }
    if !settings.loaded
        || settings.pending.is_some()
        || settings.written.as_ref() == Some(&settings.value)
    {
        return;
    }
    let now = time.elapsed_secs_f64();
    let since = *settings.dirty_at.get_or_insert(now);
    if now - since < 0.3 {
        return;
    }
    let value = settings.value.clone();
    let captured = value.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = tx.send(infrastructure::browser::write_preferences(&captured).await);
    });
    settings.pending = Some(PendingWrite {
        value,
        receiver: std::sync::Mutex::new(rx),
    });
    settings.dirty_at = None;
}
#[cfg(target_arch = "wasm32")]
fn flush_exit() {}
