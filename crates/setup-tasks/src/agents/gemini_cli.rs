//! `~/.gemini/settings.json` of Gemini CLI: the `mcpServers` object, as
//! <https://geminicli.com/docs/tools/mcp-server/> tells. Gemini CLI reads user
//! skills from `~/.agents/skills` too, see
//! <https://geminicli.com/docs/cli/skills/>.

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
    json!({ "command": server.command, "args": server.arguments })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::servers::with_every_server_command;
    use crate::context::fake::FakeSystem;

    const PATH: &str = "/home/ada/.gemini/settings.json";

    #[test]
    fn the_servers_join_the_other_settings() {
        let system =
            with_every_server_command(FakeSystem::new()).with_file(PATH, r#"{"hooks": {"x": 1}}"#);
        let merge = merge_servers(&system, Path::new(PATH))
            .expect("merges")
            .expect("plain JSON");
        assert_eq!(
            merge.added,
            ["crownos-computer-use", "playwright", "crownos-secrets"]
        );
        let document: Value =
            serde_json::from_str(&system.file(PATH).expect("written")).expect("JSON");
        assert_eq!(document["hooks"], json!({ "x": 1 }));
        assert_eq!(
            document["mcpServers"]["playwright"],
            json!({ "command": "crownos-setup", "args": ["playwright-mcp"] })
        );
    }

    #[test]
    fn an_empty_file_gets_the_servers() {
        let system = with_every_server_command(FakeSystem::new()).with_file(PATH, "\n");
        let merge = merge_servers(&system, Path::new(PATH))
            .expect("merges")
            .expect("plain JSON");
        assert_eq!(merge.added.len(), 3);
    }

    #[test]
    fn a_server_with_the_same_name_is_kept_and_the_file_stays() {
        let text = r#"{"mcpServers": {"playwright": {"command": "npx"}, "crownos-computer-use": {"command": "crownos-computer-use", "args": ["mcp"]}}}"#;
        let system = FakeSystem::new()
            .with_program("crownos-computer-use")
            .with_program("crownos-setup")
            .with_file(PATH, text);
        let merge = merge_servers(&system, Path::new(PATH))
            .expect("merges")
            .expect("plain JSON");
        assert!(merge.added.is_empty());
        assert_eq!(merge.kept, ["playwright"]);
        assert!(system.journal().is_empty());
    }

    #[test]
    fn a_broken_file_is_skipped() {
        let system = with_every_server_command(FakeSystem::new()).with_file(PATH, "{ not json");
        assert_eq!(
            merge_servers(&system, Path::new(PATH)).expect("reads"),
            None
        );
        assert!(system.journal().is_empty());
    }
}
