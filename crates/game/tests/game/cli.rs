//! Command-line flags (`--assets`, `--log-dir`), parsed from an argument list.

use std::ffi::OsString;
use std::path::PathBuf;

use game::paths::{CliArgs, CliError};

fn parse(args: &[&str]) -> Result<CliArgs, CliError> {
    CliArgs::parse(args.iter().map(OsString::from))
}

#[test]
fn no_flags_leave_both_paths_unset() {
    assert_eq!(parse(&[]).unwrap(), CliArgs::default());
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
