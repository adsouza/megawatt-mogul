use infrastructure::client_preferences::{
    ClientPreferences, PreferenceWindow, read_preferences, write_preferences,
    write_preferences_with,
};
use tempfile::TempDir;
#[test]
fn client_preferences_restart_retains_window_zoom_speed_and_panel_geometry() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("settings");
    assert_eq!(
        read_preferences(&root).unwrap(),
        ClientPreferences::default()
    );
    let mut p = ClientPreferences {
        window: Some(PreferenceWindow {
            width: 1320.0,
            height: 880.0,
            x: -1200,
            y: 80,
        }),
        zoom: 1.25,
        speed: 8,
        ..Default::default()
    };
    p.panels.insert("planning".into(), [1320.0, 320.0]);
    p.panels.insert("operating_log".into(), [520.0, 220.0]);
    write_preferences(&root, &p).unwrap();
    assert_eq!(read_preferences(&root).unwrap(), p);
    p.speed = 4;
    write_preferences(&root, &p).unwrap();
    assert_eq!(read_preferences(&root).unwrap(), p);
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
}
#[test]
fn client_preferences_retains_accelerated_day_speeds() {
    let dir = TempDir::new().unwrap();
    for speed in [16, 32] {
        let p = ClientPreferences {
            speed,
            ..Default::default()
        };
        write_preferences(dir.path(), &p).unwrap();
        assert_eq!(read_preferences(dir.path()).unwrap().speed, speed);
    }
}

#[test]
fn client_preferences_invalid_values_do_not_replace_the_previous_file() {
    let dir = TempDir::new().unwrap();
    let p = ClientPreferences::default();
    write_preferences(dir.path(), &p).unwrap();
    let mut bad = p.clone();
    bad.zoom = f32::NAN;
    assert!(write_preferences(dir.path(), &bad).is_err());
    assert_eq!(read_preferences(dir.path()).unwrap(), p);
    std::fs::write(dir.path().join("preferences.ron"), "broken").unwrap();
    assert!(read_preferences(dir.path()).is_err());
}

#[test]
fn client_preferences_retains_the_full_egui_zoom_range() {
    let dir = TempDir::new().unwrap();
    for zoom in [0.2, 0.4, 3.5, 5.0] {
        let p = ClientPreferences {
            zoom,
            ..Default::default()
        };
        write_preferences(dir.path(), &p).unwrap();
        assert_eq!(read_preferences(dir.path()).unwrap().zoom, zoom);
    }
}

#[test]
fn client_preferences_defaults_legacy_effects_to_on_and_roundtrips_both_choices() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("preferences.ron"),
        "(version:1,window:None,zoom:1.25,speed:4,panels:{},tools:{})",
    )
    .unwrap();
    let mut preferences = read_preferences(dir.path()).unwrap();
    assert!(preferences.effects);
    assert_eq!(preferences.zoom, 1.25);
    assert_eq!(preferences.speed, 4);
    for effects in [false, true] {
        preferences.effects = effects;
        write_preferences(dir.path(), &preferences).unwrap();
        assert_eq!(read_preferences(dir.path()).unwrap(), preferences);
    }
}

#[test]
fn client_preferences_writer_syncs_only_its_own_directory_and_the_ones_it_created() {
    use std::{
        cell::RefCell,
        collections::BTreeSet,
        path::{Path, PathBuf},
    };
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("settings/client");
    let synced = RefCell::new(BTreeSet::new());
    let sync = |d: &Path| {
        synced.borrow_mut().insert(d.to_path_buf());
        Ok(())
    };
    let p = ClientPreferences::default();
    write_preferences_with(&root, &p, &sync).unwrap();
    let created: BTreeSet<PathBuf> =
        [root.clone(), dir.path().join("settings"), dir.path().into()].into();
    assert_eq!(synced.take(), created);
    write_preferences_with(&root, &p, &sync).unwrap();
    assert_eq!(synced.take(), [root.clone()].into());
    assert_eq!(read_preferences(&root).unwrap(), p);
}
