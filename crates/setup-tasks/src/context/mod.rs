//! The side effects of the setup tasks. [`RealSystem`] changes the machine.
//! The `fake` module records the calls and changes only memory.

mod real;

#[cfg(any(test, feature = "fake"))]
pub mod fake;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub use real::RealSystem;

use crate::error::TaskError;
use crate::utils::text::last_lines;
use crate::utils::time_limit::TIMED_OUT_STATUS;

/// The number of stderr lines that a [`TaskError::Command`] keeps.
const KEPT_STDERR_LINES: usize = 20;

/// The result of a program that ran to its end.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output {
    /// The exit code, or `None` when a signal stopped the program.
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// The limit of a command that ran under `timeout`.
    pub time_limit: Option<Duration>,
}

impl Output {
    pub fn succeeded(&self) -> bool {
        self.status == Some(0)
    }

    pub fn with_time_limit(self, limit: Duration) -> Self {
        Self {
            time_limit: Some(limit),
            ..self
        }
    }

    /// Changes a failed run into [`TaskError::Command`], or into
    /// [`TaskError::TimedOut`] when its time limit stopped it.
    pub fn check(self, program: &str) -> Result<Self, TaskError> {
        if self.succeeded() {
            return Ok(self);
        }
        if let Some(limit) = self.time_limit
            && self.status == Some(TIMED_OUT_STATUS)
        {
            return Err(TaskError::TimedOut {
                program: program.to_owned(),
                limit,
            });
        }
        Err(TaskError::Command {
            program: program.to_owned(),
            status: self.status,
            stderr: last_lines(&self.stderr, KEPT_STDERR_LINES).to_owned(),
        })
    }
}

pub trait System {
    /// Runs `program` and returns its output. A non-zero exit is not an error
    /// here: use [`Output::check`] when the caller needs success.
    fn run(&self, program: &str, args: &[&str]) -> Result<Output, TaskError>;

    /// Runs `program` as `user` in `working_directory`, with the additional
    /// variables `environment`.
    fn run_as(
        &self,
        user: &str,
        working_directory: &Path,
        environment: &[(&str, &str)],
        program: &str,
        args: &[&str],
    ) -> Result<Output, TaskError>;

    /// The text of `path`, or `None` when the file does not exist.
    fn read_file(&self, path: &Path) -> Result<Option<String>, TaskError>;

    /// Replaces `path` atomically. Readers see the old or the new file, never
    /// a part of it.
    fn write_file(&self, path: &Path, contents: &str, mode: u32) -> Result<(), TaskError>;

    fn exists(&self, path: &Path) -> bool;

    /// True when `path` is a directory or a symbolic link to a directory.
    fn is_directory(&self, path: &Path) -> bool;

    /// The executable file `name` in a directory of `PATH`.
    fn find_program(&self, name: &str) -> Option<PathBuf>;

    /// The entries of `path`. A missing directory has no entries.
    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>, TaskError>;

    /// Removes `path`. A missing file is not an error.
    fn remove_file(&self, path: &Path) -> Result<(), TaskError>;

    /// Removes the directory `path` and its contents. A missing directory is
    /// not an error.
    fn remove_dir_all(&self, path: &Path) -> Result<(), TaskError>;

    fn create_dir_all(&self, path: &Path, mode: u32) -> Result<(), TaskError>;

    /// Gives `path` to `user` and to the login group of `user`.
    fn chown(&self, path: &Path, user: &str) -> Result<(), TaskError>;

    fn modified(&self, path: &Path) -> Option<SystemTime>;

    fn symlink(&self, target: &Path, link: &Path) -> Result<(), TaskError>;

    /// The target of the symbolic link `path`, or `None` when `path` is not a
    /// symbolic link.
    fn read_link(&self, path: &Path) -> Option<PathBuf>;

    fn sleep(&self, duration: Duration);

    /// True when a TCP connection to `address` (`host:port`) opens before
    /// `timeout`.
    fn can_connect(&self, address: &str, timeout: Duration) -> bool;
}
