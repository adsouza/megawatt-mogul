//! Presentation-only daily time signals at 1×, including regional sunrise.
use std::collections::BTreeMap;

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
pub struct DailyAudioPlugin;

impl Plugin for DailyAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Assets<AudioSource>>()
            .init_resource::<DailyAudio>()
            .add_systems(Startup, load_sounds)
            .add_systems(
                PostUpdate,
                update_daily_audio
                    .after(EguiPostUpdateSet::EndPass)
                    .before(TransformSystems::Propagate),
            );
    }
}

#[derive(Resource, Default)]
struct DailyAudio {
    sources: BTreeMap<DailySound, Handle<AudioSource>>,
    cues: [TimeCue; 5],
}

#[derive(Component)]
struct DailyPlayback(u64);

fn load_sounds(
    root: Res<AssetRoot>,
    mut sources: ResMut<Assets<AudioSource>>,
    mut audio: ResMut<DailyAudio>,
) {
    // Read once from the CLI root, including packaged installs and --assets overrides.
    for sound in DailySound::ALL {
        let path = root.0.join("audio").join(sound.filename());
        match std::fs::read(&path) {
            Ok(bytes) => {
                audio.sources.insert(
                    sound,
                    sources.add(AudioSource {
                        bytes: bytes.into(),
                    }),
                );
            }
            Err(error) => warn!("could not load daily audio {}: {error}", path.display()),
        }
    }
}

fn update_daily_audio(
    mut commands: Commands,
    session: Option<Res<Session>>,
    speed: Res<SimSpeed>,
    generation: Res<SessionGeneration>,
    persistence: Res<Persistence>,
    mut audio: ResMut<DailyAudio>,
    playing: Query<(Entity, &DailyPlayback)>,
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
        audio.cues = Default::default();
        return;
    };
    let clock = session.clock();
    let daylight = session.region().solar().daylight[clock.month().index0()];
    for (index, (sound, hour)) in daily_schedule(daylight).into_iter().enumerate() {
        if audio.cues[index].observe(generation.0, clock.month(), clock.tick(), hour, audible)
            && let Some(source) = audio.sources.get(&sound)
        {
            commands.spawn((
                DailyPlayback(generation.0),
                AudioPlayer::new(source.clone()),
                PlaybackSettings::DESPAWN.with_volume(Volume::Linear(sound.volume())),
            ));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DailySound {
    Rooster,
    MetroChime,
    Noon,
    Doorbell,
}

impl DailySound {
    const ALL: [Self; 4] = [Self::Rooster, Self::MetroChime, Self::Noon, Self::Doorbell];

    fn filename(self) -> &'static str {
        match self {
            Self::Rooster => "rooster.ogg",
            Self::MetroChime => "metro-chime.ogg",
            Self::Noon => "noon.ogg",
            Self::Doorbell => "doorbell.ogg",
        }
    }

    fn volume(self) -> f32 {
        match self {
            Self::Rooster => 0.35,
            Self::MetroChime | Self::Doorbell => 0.3,
            // The gong/laughter sequence has its individual gains baked into the asset.
            Self::Noon => 1.0,
        }
    }
}

fn daily_schedule((sunrise, _): (f64, f64)) -> [(DailySound, f64); 5] {
    [
        (DailySound::Rooster, sunrise),
        (DailySound::MetroChime, 8.5),
        (DailySound::Noon, 12.0),
        (DailySound::MetroChime, 17.0),
        (DailySound::Doorbell, 17.5),
    ]
}

#[derive(Default)]
struct TimeCue {
    previous: Option<(u64, SimMonth, u32)>,
}

impl TimeCue {
    fn observe(
        &mut self,
        generation: u64,
        month: SimMonth,
        tick: u32,
        hour: f64,
        audible: bool,
    ) -> bool {
        let previous = self.previous.replace((generation, month, tick));
        // Observe silent fast-forward too, so missed time signals are not replayed later.
        // New/load generations establish a baseline instead of catching up past sounds.
        audible
            && previous.is_some_and(|(owner, old_month, old_tick)| {
                owner == generation
                    && old_month == month
                    && f64::from(old_tick) / f64::from(TICKS_PER_HOUR) < hour
                    && f64::from(tick) / f64::from(TICKS_PER_HOUR) >= hour
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
            .add_plugins(DailyAudioPlugin);
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

    fn cue_count(app: &mut App) -> usize {
        app.world_mut()
            .query_filtered::<Entity, With<DailyPlayback>>()
            .iter(app.world())
            .count()
    }

    #[test]
    fn audio_plugin_plays_once_with_the_custom_asset_root_and_natural_playback_speed() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(root.path().join("audio")).unwrap();
        for sound in DailySound::ALL {
            std::fs::copy(
                shipped_assets().join("audio").join(sound.filename()),
                root.path().join("audio").join(sound.filename()),
            )
            .unwrap();
        }
        let mut app = audio_app(root.path().to_path_buf());
        let audio = app.world().resource::<DailyAudio>();
        assert_eq!(audio.sources.len(), DailySound::ALL.len());
        for sound in DailySound::ALL {
            let source = app
                .world()
                .resource::<Assets<AudioSource>>()
                .get(&audio.sources[&sound])
                .unwrap();
            let bytes = std::fs::read(root.path().join("audio").join(sound.filename())).unwrap();
            assert_eq!(source.bytes.as_ref(), bytes);
        }
        before_sunrise(&mut app);
        assert_eq!(cue_count(&mut app), 0);
        advance(&mut app, 1);
        assert_eq!(cue_count(&mut app), 1);
        // Once started, the natural recording can finish even if the player changes speed.
        for speed in [SimSpeed::Paused, SimSpeed::X2, SimSpeed::X1] {
            app.insert_resource(speed);
            app.update();
            assert_eq!(cue_count(&mut app), 1);
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
        assert_eq!(cue_count(&mut app), 1);
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
            assert_eq!(cue_count(&mut app), 0, "{speed:?}");
            app.insert_resource(SimSpeed::X1);
            app.update();
            advance(&mut app, 1);
            assert_eq!(cue_count(&mut app), 0, "no delayed {speed:?} cue");
        }
    }

    #[test]
    fn laughter_and_sunset_are_no_longer_separate_time_signals() {
        let hours: Vec<_> = daily_schedule((6.3, 19.1))
            .into_iter()
            .map(|(_, hour)| hour)
            .collect();
        assert_eq!(hours, [6.3, 8.5, 12.0, 17.0, 17.5]);
    }

    #[test]
    fn daily_audio_plays_the_correct_sounds_at_each_scheduled_time() {
        let mut app = audio_app(shipped_assets());
        let session = app.world().resource::<Session>().0.session();
        let rise = session.region().solar().daylight[session.clock().month().index0()].0;
        let mut schedule = [
            ((rise * 60.0).ceil() as u32, DailySound::Rooster),
            (510, DailySound::MetroChime),
            (720, DailySound::Noon),
            (1020, DailySound::MetroChime),
            (1050, DailySound::Doorbell),
        ];
        schedule.sort_by_key(|(tick, _)| *tick);
        let mut counts = BTreeMap::<DailySound, usize>::new();
        for (index, (tick, sound)) in schedule.into_iter().enumerate() {
            let current = app.world().resource::<Session>().0.session().clock().tick();
            advance(&mut app, tick - current - 1);
            assert_eq!(cue_count(&mut app), index);
            advance(&mut app, 1);
            assert_eq!(cue_count(&mut app), index + 1, "cue at minute {tick}");
            *counts.entry(sound).or_default() += 1;
            let handle = app.world().resource::<DailyAudio>().sources[&sound].clone();
            let volumes: Vec<_> = app
                .world_mut()
                .query::<(&AudioPlayer, &PlaybackSettings)>()
                .iter(app.world())
                .filter(|(player, _)| player.0 == handle)
                .map(|(_, settings)| settings.volume.to_linear())
                .collect();
            assert_eq!(
                volumes.len(),
                counts[&sound],
                "correct sound at minute {tick}"
            );
            app.update();
            assert_eq!(cue_count(&mut app), index + 1, "once per crossing");
        }
    }

    #[test]
    fn audio_plugin_cleans_up_when_the_session_is_replaced_or_removed() {
        for replace in [false, true] {
            let mut app = audio_app(shipped_assets());
            before_sunrise(&mut app);
            advance(&mut app, 1);
            assert_eq!(cue_count(&mut app), 1);
            if replace {
                app.world_mut().resource_mut::<SessionGeneration>().0 += 1;
            } else {
                app.world_mut().remove_resource::<Session>();
            }
            app.update();
            assert_eq!(cue_count(&mut app), 0);
        }
    }

    #[test]
    fn every_shipped_daily_sound_decodes_with_the_games_enabled_codec() {
        use bevy::audio::Decodable;
        for sound in DailySound::ALL {
            let source = AudioSource {
                bytes: std::fs::read(shipped_assets().join("audio").join(sound.filename()))
                    .unwrap()
                    .into(),
            };
            let samples: Vec<_> = source.decoder().collect();
            assert!(samples.len() > 44_100, "a real clip for {sound:?}");
            assert!(samples.iter().all(|sample| sample.is_finite()), "{sound:?}");
            assert!(
                samples.iter().any(|sample| sample.abs() > 0.1),
                "audible {sound:?}"
            );
        }
    }

    #[test]
    fn sunrise_crow_is_once_at_the_first_minute_at_or_after_regional_sunrise() {
        let mut cue = TimeCue::default();
        assert!(!cue.observe(0, april(), 0, 6.31, true));
        assert!(!cue.observe(0, april(), 378, 6.31, true));
        assert!(cue.observe(0, april(), 379, 6.31, true));
        assert!(!cue.observe(0, april(), 379, 6.31, true));
        assert!(!cue.observe(0, april(), 380, 6.31, true));
    }

    #[test]
    fn skipped_sunrise_does_not_crow_when_switching_back_to_1x() {
        let mut cue = TimeCue::default();
        cue.observe(0, april(), 377, 6.3, true);
        assert!(!cue.observe(0, april(), 381, 6.3, false));
        assert!(!cue.observe(0, april(), 381, 6.3, true));
        assert!(!cue.observe(0, april(), 382, 6.3, true));
    }

    #[test]
    fn loading_at_or_after_sunrise_does_not_crow_but_loading_before_it_can() {
        let mut cue = TimeCue::default();
        assert!(!cue.observe(0, april(), 378, 6.3, true));
        cue.observe(0, april(), 0, 6.3, true);
        assert!(!cue.observe(1, april(), 378, 6.3, true));
        assert!(!cue.observe(2, april(), 377, 6.3, true));
        assert!(cue.observe(2, april(), 378, 6.3, true));
    }

    #[test]
    fn a_new_month_uses_its_own_sunrise_and_can_crow_again() {
        let mut cue = TimeCue::default();
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
            let mut cue = TimeCue::default();
            for tick in 0..=1440 {
                prop_assert_eq!(cue.observe(0, april(), tick, hour, true), tick == expected);
            }
        }
    }
}
