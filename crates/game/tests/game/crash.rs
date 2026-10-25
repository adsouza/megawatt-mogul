//! Crash reports: what a panic writes beside the command logs, which game's log a report names,
//! and the installed hook end to end, through the real binary's hidden `--crash` flag.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use bevy::prelude::*;
use game::crash::{CrashContext, CrashReport};
use game::hud::UiAction;
use game::paths::SaveDir;
use game::sim::{PersistAction, Persistence, ReturnToMenu};
use tempfile::TempDir;

use crate::hud_smoke::dispatch_in;
use crate::save_resume::open_browser;
use crate::support::{hud_app, request, update_n};

fn report() -> CrashReport {
    CrashReport {
        version: "1.2.3".into(),
        unix_secs: 1_234_567_890,
        thread: "main".into(),
        message: "money overflowed".into(),
        location: Some("crates/domain/src/money.rs:42:9".into()),
        game_log: Some(PathBuf::from(
            "/data/logs/20090213-232900-western_interconnection-7.log",
        )),
        backtrace: "   0: domain::money::add\n   1: game::main\n".into(),
    }
}

fn reports_in(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut reports: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    reports.sort();
    reports
}

#[test]
fn a_report_names_the_version_time_thread_panic_game_log_and_backtrace() {
    assert_eq!(
        report().render(),
        "Megawatt Mogul 1.2.3 panicked at 20090213-233130 UTC\n\
         thread: main\n\
         message: money overflowed\n\
         location: crates/domain/src/money.rs:42:9\n\
         game log: /data/logs/20090213-232900-western_interconnection-7.log\n\
         \n\
         backtrace:\n   0: domain::money::add\n   1: game::main\n"
    );
}

#[test]
fn a_report_without_a_location_says_so() {
    let report = CrashReport {
        location: None,
        ..report()
    };

    assert!(report.render().contains("\nlocation: unknown\n"));
}

#[test]
fn a_report_from_outside_a_game_says_no_game_was_running() {
    let report = CrashReport {
        game_log: None,
        ..report()
    };

    assert!(
        report
            .render()
            .contains("\ngame log: none (no game running)\n")
    );
}

#[test]
fn clones_of_a_crash_context_share_the_game_log() {
    let context = CrashContext::default();
    let hook_side = context.clone();
    assert_eq!(hook_side.game_log(), None);

    context.set_game_log(Some(PathBuf::from("logs/a.log")));
    assert_eq!(hook_side.game_log(), Some(PathBuf::from("logs/a.log")));

    context.set_game_log(None);
    assert_eq!(hook_side.game_log(), None);
}

fn game_log(app: &App) -> Option<PathBuf> {
    app.world().resource::<CrashContext>().game_log()
}

fn logs_named(dir: &std::path::Path, prefix: &str) -> Vec<PathBuf> {
    reports_in(dir)
        .into_iter()
        .filter(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name.starts_with(prefix) && name.ends_with(".log")
        })
        .collect()
}

#[test]
fn a_new_game_names_its_log_until_the_player_returns_to_the_menu() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    assert_eq!(game_log(&app), None);

    request(&mut app, "western_interconnection");

    let created = logs_named(logs.path(), "");
    assert_eq!(created.len(), 1, "{created:?}");
    assert_eq!(game_log(&app), Some(created[0].clone()));

    app.world_mut().write_message(ReturnToMenu);
    update_n(&mut app, 2);
    assert_eq!(game_log(&app), None);
}

#[test]
fn a_loaded_game_names_its_resume_log() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    app.insert_resource(SaveDir(saves.path().into()));
    request(&mut app, "western_interconnection");
    dispatch_in(
        &mut app,
        vec![UiAction::Command(application::Command::BeginOperating)],
    );
    update_n(&mut app, 3);
    dispatch_in(
        &mut app,
        vec![UiAction::Persistence(PersistAction::OpenSave {
            leave: false,
        })],
    );
    settle(&mut app);
    dispatch_in(
        &mut app,
        vec![UiAction::Persistence(PersistAction::SaveNew(
            "Crash".into(),
        ))],
    );
    settle(&mut app);
    let slot = app
        .world()
        .resource::<Persistence>()
        .last_saved()
        .unwrap()
        .metadata
        .slot
        .clone();
    drop(app);

    let mut app = hud_app(logs.path());
    app.insert_resource(SaveDir(saves.path().into()));
    update_n(&mut app, 2);
    open_browser(&mut app);
    settle(&mut app);
    dispatch_in(
        &mut app,
        vec![UiAction::Persistence(PersistAction::Load(slot))],
    );
    settle(&mut app);
    update_n(&mut app, 2);

    let resumed = logs_named(logs.path(), "resume-");
    assert_eq!(resumed.len(), 1, "{resumed:?}");
    assert_eq!(game_log(&app), Some(resumed[0].clone()));
}

fn settle(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.world().resource::<Persistence>().busy() {
        assert!(
            std::time::Instant::now() < deadline,
            "persistence worker did not complete"
        );
        app.update();
        std::thread::yield_now();
    }
    app.update();
}

#[test]
fn writing_creates_the_directory_and_names_the_file_for_the_time() {
    let temp = tempfile::tempdir().unwrap();
    let logs = temp.path().join("logs");

    let path = report().write_to(&logs).unwrap();

    assert_eq!(path, logs.join("crash-20090213-233130.txt"));
    assert_eq!(fs::read_to_string(&path).unwrap(), report().render());
}

#[test]
fn a_second_crash_in_the_same_second_keeps_the_first_report() {
    let temp = tempfile::tempdir().unwrap();
    let first = report().write_to(temp.path()).unwrap();
    let second = CrashReport {
        message: "again".into(),
        ..report()
    };

    let path = second.write_to(temp.path()).unwrap();

    assert_eq!(path, temp.path().join("crash-20090213-233130-2.txt"));
    assert_eq!(fs::read_to_string(&first).unwrap(), report().render());
    assert_eq!(fs::read_to_string(&path).unwrap(), second.render());
}

#[test]
fn an_unusable_directory_is_an_error_not_a_panic() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("not-a-directory");
    fs::write(&file, "").unwrap();

    assert!(report().write_to(&file).is_err());
}

/// The release smoke repeats this on the installed packages, where it also proves the packaged
/// binary still has the symbols that make the backtrace readable.
#[test]
fn the_crash_flag_panics_and_leaves_a_readable_report_beside_the_logs() {
    let temp = tempfile::tempdir().unwrap();
    let logs = temp.path().join("logs");

    let output = Command::new(env!("CARGO_BIN_EXE_game"))
        .arg("--log-dir")
        .arg(&logs)
        .arg("--crash")
        .current_dir(temp.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(101), "{output:?}");
    let reports = reports_in(&logs);
    assert_eq!(reports.len(), 1, "{reports:?}");
    let name = reports[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        name.starts_with("crash-") && name.ends_with(".txt"),
        "{name}"
    );
    let text = fs::read_to_string(&reports[0]).unwrap();
    assert!(
        text.starts_with(&format!(
            "Megawatt Mogul {} panicked at ",
            env!("CARGO_PKG_VERSION")
        )),
        "{text}"
    );
    assert!(text.contains("\nthread: main\n"), "{text}");
    assert!(
        text.contains("\nmessage: crash requested by --crash\n"),
        "{text}"
    );
    assert!(
        text.contains("\ngame log: none (no game running)\n"),
        "{text}"
    );
    assert!(text.contains("game::crash::requested_crash"), "{text}");
    // The default hook still reports to stderr, followed by where the report went.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("crash requested by --crash"), "{stderr}");
    assert!(
        stderr.contains(&reports[0].display().to_string()),
        "{stderr}"
    );
}
