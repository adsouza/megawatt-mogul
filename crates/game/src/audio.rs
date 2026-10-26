//! Presentation-only sunrise sound, following the regional solar calendar at 1×.
use bevy::{audio::Volume, prelude::*, transform::TransformSystems};
use bevy_egui::EguiPostUpdateSet;
use domain::{
    session::{DayPhase, SessionStatus},
    time::{SimMonth, TICKS_PER_HOUR},
};

use crate::{
    paths::AssetRoot,
    sim::{Persistence, Session, SessionGeneration, SimSpeed},
};

/// Client audio only: no simulation state, randomness, or checkpoint data.
pub struct SunriseAudioPlugin;

impl Plugin for SunriseAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Assets<AudioSource>>()
            .init_resource::<SunriseAudio>()
            .add_systems(Startup, load_rooster)
            .add_systems(
                PostUpdate,
                update_sunrise
                    .after(EguiPostUpdateSet::EndPass)
                    .before(TransformSystems::Propagate),
            );
    }
}

#[derive(Resource, Default)]
struct SunriseAudio {
    source: Option<Handle<AudioSource>>,
    cue: SunriseCue,
}

#[derive(Component)]
struct SunrisePlayback(u64);

fn load_rooster(
    root: Res<AssetRoot>,
    mut sources: ResMut<Assets<AudioSource>>,
    mut audio: ResMut<SunriseAudio>,
) {
    // Like locales, this small asset is read once from the CLI's root, including packaged
    // installs and --assets overrides, rather than Bevy's separate default asset directory.
    let path = root.0.join("audio/rooster.ogg");
    match std::fs::read(&path) {
        Ok(bytes) => {
            audio.source = Some(sources.add(AudioSource {
                bytes: bytes.into(),
            }))
        }
        Err(error) => warn!("could not load sunrise audio {}: {error}", path.display()),
    }
}

fn update_sunrise(
    mut commands: Commands,
    session: Option<Res<Session>>,
    speed: Res<SimSpeed>,
    generation: Res<SessionGeneration>,
    persistence: Res<Persistence>,
    mut audio: ResMut<SunriseAudio>,
    playing: Query<(Entity, &SunrisePlayback)>,
) {
    let session = session.as_ref().map(|session| session.0.session());
    let audible = *speed == SimSpeed::X1
        && !persistence.frozen()
        && session.is_some_and(|session| {
            session.phase() == DayPhase::Operating && session.status() == SessionStatus::Active
        });
    for (entity, owner) in &playing {
        if session.is_none() || owner.0 != generation.0 {
            commands.entity(entity).despawn();
        }
    }
    let Some(session) = session else {
        audio.cue = SunriseCue::default();
        return;
    };
    let clock = session.clock();
    let sunrise = session.region().solar().daylight[clock.month().index0()].0;
    if audio
        .cue
        .observe(generation.0, clock.month(), clock.tick(), sunrise, audible)
        && let Some(source) = &audio.source
    {
        commands.spawn((
            SunrisePlayback(generation.0),
            AudioPlayer::new(source.clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.35)),
        ));
    }
}

#[derive(Default)]
struct SunriseCue {
    previous: Option<(u64, SimMonth, u32)>,
}

impl SunriseCue {
    fn observe(
        &mut self,
        generation: u64,
        month: SimMonth,
        tick: u32,
        sunrise_hour: f64,
        audible: bool,
    ) -> bool {
        let previous = self.previous.replace((generation, month, tick));
        // Update even while silent: a sunrise skipped at 2×+ must never be replayed later.
        // New/load generations establish a baseline instead of catching up past sounds.
        audible
            && previous.is_some_and(|(owner, old_month, old_tick)| {
                owner == generation
                    && old_month == month
                    && f64::from(old_tick) / f64::from(TICKS_PER_HOUR) < sunrise_hour
                    && f64::from(tick) / f64::from(TICKS_PER_HOUR) >= sunrise_hour
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_assets};
    use application::{Command, InMemoryCommandLog};
    use domain::time::SimMonth;
    use proptest::prelude::*;

    fn april() -> SimMonth {
        SimMonth::new(2026, 4).unwrap()
    }

    fn audio_app(root: std::path::PathBuf) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(AssetRoot(root))
            .init_resource::<SessionGeneration>()
            .init_resource::<Persistence>()
            .insert_resource(SimSpeed::X1)
            .add_plugins(SunriseAudioPlugin);
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        game.execute(Command::BeginOperating).unwrap();
        app.insert_resource(Session(game));
        app.update();
        app
    }

    fn advance(app: &mut App, ticks: u32) {
        app.world_mut()
            .resource_mut::<Session>()
            .0
            .execute(Command::AdvanceTicks { n: ticks })
            .unwrap();
        app.update();
    }

    fn before_sunrise(app: &mut App) {
        let session = app.world().resource::<Session>().0.session();
        let rise = session.region().solar().daylight[session.clock().month().index0()].0;
        advance(app, (rise * f64::from(TICKS_PER_HOUR)).ceil() as u32 - 1);
    }

    fn crow_count(app: &mut App) -> usize {
        app.world_mut()
            .query_filtered::<Entity, With<SunrisePlayback>>()
            .iter(app.world())
            .count()
    }

    #[test]
    fn audio_plugin_plays_once_with_the_custom_asset_root_and_natural_playback_speed() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(root.path().join("audio")).unwrap();
        let bytes = std::fs::read(shipped_assets().join("audio/rooster.ogg")).unwrap();
        std::fs::write(root.path().join("audio/rooster.ogg"), &bytes).unwrap();
        let mut app = audio_app(root.path().to_path_buf());
        let audio = app.world().resource::<SunriseAudio>();
        let source = app
            .world()
            .resource::<Assets<AudioSource>>()
            .get(audio.source.as_ref().unwrap())
            .unwrap();
        assert_eq!(source.bytes.as_ref(), bytes);
        before_sunrise(&mut app);
        assert_eq!(crow_count(&mut app), 0);
        advance(&mut app, 1);
        assert_eq!(crow_count(&mut app), 1);
        // Once started, the natural recording can finish even if the player changes speed.
        for speed in [SimSpeed::Paused, SimSpeed::X2, SimSpeed::X1] {
            app.insert_resource(speed);
            app.update();
            assert_eq!(crow_count(&mut app), 1);
        }
        let (_, settings) = app
            .world_mut()
            .query::<(&AudioPlayer, &PlaybackSettings)>()
            .single(app.world())
            .unwrap();
        assert_eq!(settings.speed, 1.0);
        assert!(matches!(settings.mode, bevy::audio::PlaybackMode::Despawn));
        assert!(!settings.spatial);
        app.update();
        advance(&mut app, 1);
        assert_eq!(crow_count(&mut app), 1);
    }

    #[test]
    fn audio_plugin_skips_sunrise_at_every_other_speed_without_late_playback() {
        for speed in [
            SimSpeed::Paused,
            SimSpeed::X2,
            SimSpeed::X4,
            SimSpeed::X8,
            SimSpeed::X16,
            SimSpeed::X32,
        ] {
            let mut app = audio_app(shipped_assets());
            before_sunrise(&mut app);
            app.insert_resource(speed);
            advance(&mut app, speed.ticks().max(1));
            assert_eq!(crow_count(&mut app), 0, "{speed:?}");
            app.insert_resource(SimSpeed::X1);
            app.update();
            advance(&mut app, 1);
            assert_eq!(crow_count(&mut app), 0, "no delayed {speed:?} cue");
        }
    }

    #[test]
    fn audio_plugin_cleans_up_when_the_session_is_replaced_or_removed() {
        for replace in [false, true] {
            let mut app = audio_app(shipped_assets());
            before_sunrise(&mut app);
            advance(&mut app, 1);
            assert_eq!(crow_count(&mut app), 1);
            if replace {
                app.world_mut().resource_mut::<SessionGeneration>().0 += 1;
            } else {
                app.world_mut().remove_resource::<Session>();
            }
            app.update();
            assert_eq!(crow_count(&mut app), 0);
        }
    }

    #[test]
    fn shipped_rooster_decodes_with_the_games_enabled_codec() {
        use bevy::audio::{AudioSource, Decodable};
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/audio/rooster.ogg");
        let source = AudioSource {
            bytes: std::fs::read(path).unwrap().into(),
        };
        let samples: Vec<_> = source.decoder().collect();
        assert!(samples.len() > 44_100, "a real recording, not an empty cue");
        assert!(samples.iter().all(|sample| sample.is_finite()));
        assert!(samples.iter().any(|sample| sample.abs() > 0.1));
    }

    #[test]
    fn sunrise_crow_is_once_at_the_first_minute_at_or_after_regional_sunrise() {
        let mut cue = SunriseCue::default();
        assert!(!cue.observe(0, april(), 0, 6.31, true));
        assert!(!cue.observe(0, april(), 378, 6.31, true));
        assert!(cue.observe(0, april(), 379, 6.31, true));
        assert!(!cue.observe(0, april(), 379, 6.31, true));
        assert!(!cue.observe(0, april(), 380, 6.31, true));
    }

    #[test]
    fn skipped_sunrise_does_not_crow_when_switching_back_to_1x() {
        let mut cue = SunriseCue::default();
        cue.observe(0, april(), 377, 6.3, true);
        assert!(!cue.observe(0, april(), 381, 6.3, false));
        assert!(!cue.observe(0, april(), 381, 6.3, true));
        assert!(!cue.observe(0, april(), 382, 6.3, true));
    }

    #[test]
    fn loading_at_or_after_sunrise_does_not_crow_but_loading_before_it_can() {
        let mut cue = SunriseCue::default();
        assert!(!cue.observe(0, april(), 378, 6.3, true));
        cue.observe(0, april(), 0, 6.3, true);
        assert!(!cue.observe(1, april(), 378, 6.3, true));
        assert!(!cue.observe(2, april(), 377, 6.3, true));
        assert!(cue.observe(2, april(), 378, 6.3, true));
    }

    #[test]
    fn a_new_month_uses_its_own_sunrise_and_can_crow_again() {
        let mut cue = SunriseCue::default();
        cue.observe(0, april(), 377, 6.3, true);
        assert!(cue.observe(0, april(), 378, 6.3, true));
        let may = april().next();
        assert!(!cue.observe(0, may, 0, 5.75, true));
        assert!(!cue.observe(0, may, 344, 5.75, true));
        assert!(cue.observe(0, may, 345, 5.75, true));
    }

    proptest! {
        #[test]
        fn any_sunrise_is_crossed_exactly_once_at_1x(rise in 1_u32..1439, fraction in 0_u32..100) {
            let hour = (f64::from(rise) + f64::from(fraction) / 100.0) / 60.0;
            let expected = if fraction == 0 { rise } else { rise + 1 };
            let mut cue = SunriseCue::default();
            for tick in 0..=1440 {
                prop_assert_eq!(cue.observe(0, april(), tick, hour, true), tick == expected);
            }
        }
    }
}
