use std::path::Path;

use serde_json::Value;

use super::servers::{McpServer, SERVERS};
use crate::context::System;
use crate::error::TaskError;
use crate::utils::json_file;
use crate::utils::json_merge::{self, Merge};

/// Adds [`SERVERS`], each made by `entry`, to the object at `key_path` in the
/// JSON file `path`. Writes the file only when a server was added.
pub(super) fn merge_servers_into_file(
    system: &dyn System,
    path: &Path,
    key_path: &[&str],
    entry: fn(&McpServer) -> Value,
) -> Result<Option<Merge<'static>>, TaskError> {
    let Some(mut document) = json_file::read(system, path)? else {
        return Ok(None);
    };
    let entries = SERVERS.map(|server| (server.name, entry(&server)));
    let Some(merge) = json_merge::merge_servers(&mut document, key_path, &entries) else {
        return Ok(None);
    };
    if !merge.added.is_empty() {
        json_file::write(system, path, &document)?;
    }
    Ok(Some(merge))
}
