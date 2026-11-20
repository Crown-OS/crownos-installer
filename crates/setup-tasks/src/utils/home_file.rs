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

/// Replaces `path` atomically and makes its parent directory when it is missing.
pub fn write(system: &dyn System, path: &Path, contents: &str) -> Result<(), TaskError> {
    if let Some(parent) = path.parent() {
        ensure_directory(system, parent)?;
    }
    system.write_file(path, contents, HOME_FILE_MODE)
}
