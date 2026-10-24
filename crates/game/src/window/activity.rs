//! Keep operating play smooth, but let an unchanged planning screen sleep between events.
use crate::sim::{Persistence, Session, SimSet, SimSpeed};
use bevy::{
    prelude::*,
    winit::{UpdateMode, WinitSettings},
};
use domain::session::DayPhase;
use std::time::Duration;

pub struct IdleRedrawPlugin;
impl Plugin for IdleRedrawPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WinitSettings>()
            .add_systems(Update, update_redraw_mode.after(SimSet::Apply));
    }
}

fn redraw_settings(phase: Option<DayPhase>, running: bool, busy: bool) -> WinitSettings {
    if phase == Some(DayPhase::Operating) && running {
        return WinitSettings::game();
    }
    // Menus animate, and background I/O needs prompt polling. Static game screens can sleep.
    let wait = if phase.is_none() || busy {
        Duration::from_millis(33)
    } else {
        Duration::from_millis(250)
    };
    WinitSettings {
        focused_mode: UpdateMode::reactive_low_power(wait),
        unfocused_mode: UpdateMode::reactive_low_power(Duration::from_millis(500)),
    }
}

fn update_redraw_mode(
    session: Option<Res<Session>>,
    speed: Option<Res<SimSpeed>>,
    persistence: Option<Res<Persistence>>,
    mut settings: ResMut<WinitSettings>,
) {
    let phase = session.as_ref().map(|s| s.0.session().phase());
    let running = speed.is_some_and(|s| *s != SimSpeed::Paused)
        && !persistence.as_ref().is_some_and(|p| p.frozen());
    let next = redraw_settings(phase, running, persistence.is_some_and(|p| p.busy()));
    if settings.focused_mode != next.focused_mode || settings.unfocused_mode != next.unfocused_mode
    {
        *settings = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn planning_sleeps_but_window_input_and_async_jobs_still_wake_it() {
        let idle = redraw_settings(Some(DayPhase::Planning), false, false);
        assert!(matches!(idle.focused_mode, UpdateMode::Reactive {
            wait, react_to_window_events: true, react_to_user_events: true,
            react_to_device_events: false,
        } if wait >= Duration::from_millis(200)));
        let busy = redraw_settings(Some(DayPhase::Planning), false, true);
        assert!(
            matches!(busy.focused_mode, UpdateMode::Reactive { wait, .. } if wait <= Duration::from_millis(34))
        );
    }
    #[test]
    fn operating_keeps_continuous_frames_and_pause_returns_to_idle() {
        assert_eq!(
            redraw_settings(Some(DayPhase::Operating), true, false).focused_mode,
            UpdateMode::Continuous
        );
        assert_eq!(
            redraw_settings(Some(DayPhase::Operating), false, false).focused_mode,
            redraw_settings(Some(DayPhase::Planning), false, false).focused_mode
        );
    }
}
