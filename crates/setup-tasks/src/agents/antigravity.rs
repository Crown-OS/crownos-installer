//! `~/.gemini/config/mcp_config.json` of Antigravity: the `mcpServers`
//! object, as <https://antigravity.google/docs/mcp> tells. Antigravity reads
//! global skills from `~/.gemini/config/skills`, see
//! <https://antigravity.google/docs/skills>.

use std::path::Path;

use serde_json::{Value, json};

use super::json_config::merge_servers_into_file;
use super::servers::McpServer;
use crate::context::System;
use crate::error::TaskError;
use crate::utils::json_merge::Merge;

pub fn merge_servers(
    system: &dyn System,
    path: &Path,
) -> Result<Option<Merge<'static>>, TaskError> {
    merge_servers_into_file(system, path, &["mcpServers"], entry)
}

fn entry(server: &McpServer) -> Value {
    json!({ "command": server.command, "args": server.arguments, "env": {} })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::fake::FakeSystem;

    const PATH: &str = "/home/ada/.gemini/config/mcp_config.json";

    #[test]
    fn a_missing_file_gets_the_servers_with_an_empty_environment() {
        let system = FakeSystem::new();
        let merge = merge_servers(&system, Path::new(PATH))
            .expect("merges")
            .expect("plain JSON");
        assert_eq!(merge.added, ["crownos-computer-use", "playwright"]);
        let document: Value =
            serde_json::from_str(&system.file(PATH).expect("written")).expect("JSON");
        assert_eq!(
            document["mcpServers"]["playwright"],
            json!({ "command": "crownos-setup", "args": ["playwright-mcp"], "env": {} })
        );
    }

    #[test]
    fn a_server_with_the_same_command_and_arguments_is_silent() {
        let text = r#"{"mcpServers": {"playwright": {"command": "crownos-setup", "args": ["playwright-mcp"]}, "crownos-computer-use": {"command": "crownos-computer-use", "args": ["mcp", "--debug"]}}}"#;
        let system = FakeSystem::new().with_file(PATH, text);
        let merge = merge_servers(&system, Path::new(PATH))
            .expect("merges")
            .expect("plain JSON");
        assert!(merge.added.is_empty());
        assert_eq!(merge.kept, ["crownos-computer-use"]);
        assert!(system.journal().is_empty());
    }
}
