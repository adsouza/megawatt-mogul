//! Automatically retained client layout. Preference writes are separate from game saves.
use crate::{
    hud::{HudSet, LastSpeed},
    paths::SaveDir,
    sim::SimSpeed,
};
use bevy::{
    prelude::*,
    window::{PrimaryWindow, WindowPosition},
};
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use infrastructure::client_preferences::{
    ClientPreferences, PreferenceWindow, read_preferences, write_preferences,
};
use std::{path::PathBuf, thread::JoinHandle};

const PANELS: [&str; 6] = [
    "inspector",
    "build",
    "planning",
    "operating",
    "operating_side",
    "operating_log",
];
struct PendingWrite {
    value: ClientPreferences,
    handle: JoinHandle<Result<(), application::SaveError>>,
}
#[derive(Resource, Default)]
pub struct ClientSettings {
    pub value: ClientPreferences,
    pub loaded: bool,
    pub(crate) egui_applied: bool,
    root: Option<PathBuf>,
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
) {
    if settings.loaded {
        return;
    }
    let Some(dir) = dir else {
        return;
    };
    match read_preferences(&dir.0) {
        Ok(value) => settings.value = value,
        Err(error) => warn!("cannot read client preferences: {error}"),
    }
    settings.root = Some(dir.0.clone());
    settings.written = Some(settings.value.clone());
    settings.loaded = true;
    // The menu already owns LastSpeed before its first egui pass. Seed it before the observer
    // can mistake its default 1x for a player choice and overwrite the restored preference.
    if let Some(mut last) = last {
        last.0 = settings.speed();
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
) -> Result {
    if !settings.loaded || !settings.egui_applied {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    settings.value.zoom = ctx.zoom_factor().clamp(0.2, 5.0);
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
    let available = pixels.as_vec2() / scale;
    let size = Vec2::new(saved.width, saved.height)
        .max(crate::window::MIN_WINDOW_SIZE)
        .min(available);
    let room = (pixels.as_vec2() - size * scale).max(Vec2::ZERO);
    PreferenceWindow {
        width: size.x,
        height: size.y,
        x: (saved.x as f32)
            .clamp(origin.x as f32, origin.x as f32 + room.x)
            .round() as i32,
        y: (saved.y as f32)
            .clamp(origin.y as f32, origin.y as f32 + room.y)
            .round() as i32,
    }
}
