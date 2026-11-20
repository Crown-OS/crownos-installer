//! Runs commands under coreutils `timeout`, so that a command that hangs ends
//! and its task fails and counts as an attempt.

use std::path::Path;
use std::time::Duration;

use crate::context::{Output, System};
use crate::error::TaskError;

/// The limit of a pacman or npm call and of a browser download.
pub const INSTALL_TIME_LIMIT: Duration = Duration::from_secs(2 * 60 * 60);
/// The limit of a makepkg build.
pub const BUILD_TIME_LIMIT: Duration = Duration::from_secs(3 * 60 * 60);
/// The limit of a git clone or pull.
pub const GIT_TIME_LIMIT: Duration = Duration::from_secs(30 * 60);
/// The exit status of `timeout` when the limit stopped the command.
pub const TIMED_OUT_STATUS: i32 = 124;

const TIMEOUT_OPTIONS: [&str; 2] = ["--signal=INT", "--kill-after=1m"];

pub fn run(
    system: &dyn System,
    limit: Duration,
    program: &str,
    args: &[&str],
) -> Result<Output, TaskError> {
    let seconds = limit.as_secs().to_string();
    system
        .run("timeout", &timeout_arguments(&seconds, program, args))
        .map(|output| output.with_time_limit(limit))
}

pub fn run_as(
    system: &dyn System,
    user: &str,
    working_directory: &Path,
    environment: &[(&str, &str)],
    limit: Duration,
    program: &str,
    args: &[&str],
) -> Result<Output, TaskError> {
    let seconds = limit.as_secs().to_string();
    system
        .run_as(
            user,
            working_directory,
            environment,
            "timeout",
            &timeout_arguments(&seconds, program, args),
        )
        .map(|output| output.with_time_limit(limit))
}

fn timeout_arguments<'a>(seconds: &'a str, program: &'a str, args: &[&'a str]) -> Vec<&'a str> {
    [&TIMEOUT_OPTIONS[..], &[seconds, program], args].concat()
}

/// `2 hours` or `30 minutes`.
pub fn describe(limit: Duration) -> String {
    let minutes = limit.as_secs() / 60;
    match (minutes / 60, minutes % 60) {
        (1, 0) => "1 hour".to_owned(),
        (hours, 0) if hours > 0 => format!("{hours} hours"),
        _ => format!("{minutes} minutes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::fake::FakeSystem;

    #[test]
    fn the_command_runs_under_timeout() {
        assert_eq!(
            timeout_arguments("1800", "git", &["pull", "--ff-only"]),
            [
                "--signal=INT",
                "--kill-after=1m",
                "1800",
                "git",
                "pull",
                "--ff-only"
            ]
        );
    }

    #[test]
    fn the_limits_read_as_hours_or_minutes() {
        assert_eq!(describe(INSTALL_TIME_LIMIT), "2 hours");
        assert_eq!(describe(BUILD_TIME_LIMIT), "3 hours");
        assert_eq!(describe(GIT_TIME_LIMIT), "30 minutes");
        assert_eq!(describe(Duration::from_secs(3600)), "1 hour");
    }

    #[test]
    fn a_stopped_command_is_reported_as_timed_out() {
        let system = FakeSystem::new().with_result(
            "npm install",
            Output {
                status: Some(TIMED_OUT_STATUS),
                ..Output::default()
            },
        );
        let error = run(&system, INSTALL_TIME_LIMIT, "npm", &["install"])
            .expect("runs")
            .check("npm install")
            .expect_err("timed out");
        assert_eq!(error.to_string(), "`npm install` timed out after 2 hours");
        assert_eq!(system.commands(), ["npm install"]);
    }
}
