//! Command-line flags (`--assets`, `--log-dir`, `--exit-after-frames`), parsed from an argument
//! list, and the launch smoke's frame limit.

use std::ffi::OsString;
use std::num::NonZeroU32;
use std::path::PathBuf;

use bevy::prelude::*;
use game::paths::{CliArgs, CliError, ExitAfterFrames};

fn parse(args: &[&str]) -> Result<CliArgs, CliError> {
    CliArgs::parse(args.iter().map(OsString::from))
}

#[test]
fn no_flags_leave_both_paths_unset() {
    assert_eq!(parse(&[]).unwrap(), CliArgs::default());
}

#[test]
fn version_flag_needs_no_value_and_preserves_other_flags() {
    let args = parse(&["--version", "--assets", "/a"]).unwrap();
    assert!(args.version);
    assert_eq!(args.assets, Some(PathBuf::from("/a")));
    assert!(!CliArgs::default().version);
}

#[test]
fn both_flags_are_read() {
    let args = parse(&["--assets", "/a", "--log-dir", "/l"]).unwrap();

    assert_eq!(args.assets, Some(PathBuf::from("/a")));
    assert_eq!(args.log_dir, Some(PathBuf::from("/l")));
}

#[test]
fn the_default_log_dir_is_logs_under_the_working_directory() {
    assert_eq!(CliArgs::default().log_dir(), PathBuf::from("logs"));
}

#[test]
fn a_flag_without_a_value_is_an_error() {
    assert_eq!(
        parse(&["--assets"]),
        Err(CliError::MissingValue("--assets"))
    );
}

#[test]
fn an_unknown_argument_is_an_error() {
    assert_eq!(
        parse(&["--speed", "8"]),
        Err(CliError::Unknown(OsString::from("--speed")))
    );
}

#[test]
fn exit_after_frames_reads_a_positive_count() {
    let args = parse(&["--exit-after-frames", "120"]).unwrap();

    assert_eq!(args.exit_after_frames, NonZeroU32::new(120));
    assert_eq!(args.assets, None);
}

#[test]
fn exit_after_frames_refuses_zero_and_non_numbers() {
    for value in ["0", "-3", "soon"] {
        assert_eq!(
            parse(&["--exit-after-frames", value]),
            Err(CliError::NotACount {
                flag: "--exit-after-frames",
                value: OsString::from(value),
            }),
            "{value}"
        );
    }
    assert_eq!(
        parse(&["--exit-after-frames"]),
        Err(CliError::MissingValue("--exit-after-frames"))
    );
}

#[test]
fn the_frame_limit_exits_cleanly_on_its_last_frame() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, ExitAfterFrames(NonZeroU32::new(3).unwrap())));

    for _ in 0..2 {
        app.update();
        assert_eq!(app.should_exit(), None);
    }
    app.update();

    assert_eq!(app.should_exit(), Some(AppExit::Success));
}
