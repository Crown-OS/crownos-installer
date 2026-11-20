use std::io;
use std::path::PathBuf;

use crownos_config::system::{SetupConfigError, SetupStateError};

#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("`{program}` failed with {}: {}", describe_status(*status), stderr.trim())]
    Command {
        program: String,
        status: Option<i32>,
        stderr: String,
    },
    #[error("another program holds the pacman database lock")]
    PacmanLocked,
    #[error("ran, but the result is not complete")]
    Incomplete,
    #[error("the system user `{0}` does not exist")]
    MissingUser(&'static str),
    #[error("{} is a symbolic link", .0.display())]
    SymbolicLink(PathBuf),
    #[error("could not parse {}: {message}", path.display())]
    Parse { path: PathBuf, message: String },
    #[error("could not render JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    State(#[from] SetupStateError),
    #[error(transparent)]
    Config(#[from] SetupConfigError),
}

impl TaskError {
    /// True for an I/O error because the path exists already.
    pub fn is_already_exists(&self) -> bool {
        matches!(self, Self::Io { source, .. } if source.kind() == io::ErrorKind::AlreadyExists)
    }

    pub fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

fn describe_status(status: Option<i32>) -> String {
    status.map_or_else(|| "a signal".to_owned(), |code| format!("status {code}"))
}
