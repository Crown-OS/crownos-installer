//! Reads and writes the JSON configuration files of the coding agents.

use std::path::Path;

use serde_json::{Map, Value};

use crate::context::System;
use crate::error::TaskError;
use crate::utils::home_file;

/// The document in `path`. A missing or empty file gives an empty object.
/// `None` when the file is not plain JSON, for example JSON with comments.
pub fn read(system: &dyn System, path: &Path) -> Result<Option<Value>, TaskError> {
    let text = system.read_file(path)?.unwrap_or_default();
    if text.trim().is_empty() {
        return Ok(Some(Value::Object(Map::new())));
    }
    Ok(serde_json::from_str(&text).ok())
}

/// Writes `document` as indented JSON with a line break at the end.
pub fn write(system: &dyn System, path: &Path, document: &Value) -> Result<(), TaskError> {
    let mut text = serde_json::to_string_pretty(document)?;
    text.push('\n');
    home_file::write(system, path, &text)
}
