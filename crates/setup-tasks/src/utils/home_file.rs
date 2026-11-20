//! Writes files under the home directory of the user. A directory that exists
//! keeps its mode.

use std::path::Path;

use crate::context::System;
use crate::error::TaskError;

const HOME_FILE_MODE: u32 = 0o600;
const HOME_DIRECTORY_MODE: u32 = 0o755;

pub fn ensure_directory(system: &dyn System, path: &Path) -> Result<(), TaskError> {
    if system.exists(path) {
        return Ok(());
    }
    system.create_dir_all(path, HOME_DIRECTORY_MODE)
}

/// Fails with [`TaskError::SymbolicLink`] when `path` is a symbolic link. The
/// setup never writes through a link that the user made.
pub fn refuse_symbolic_link(system: &dyn System, path: &Path) -> Result<(), TaskError> {
    match system.read_link(path) {
        Some(_) => Err(TaskError::SymbolicLink(path.to_owned())),
        None => Ok(()),
    }
}

/// Replaces `path` atomically and makes its parent directory when it is
/// missing. A symbolic link stays as it is: see [`refuse_symbolic_link`].
pub fn write(system: &dyn System, path: &Path, contents: &str) -> Result<(), TaskError> {
    refuse_symbolic_link(system, path)?;
    if let Some(parent) = path.parent() {
        ensure_directory(system, parent)?;
    }
    system.write_file(path, contents, HOME_FILE_MODE)
}
