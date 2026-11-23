//! `~/.config/zed/settings.json`: the `context_servers` object, as
//! <https://zed.dev/docs/ai/mcp> tells. Zed allows
//! comments in this file. The merge edits only a file without comments and
//! skips the other files, so that no comment is lost.

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
    merge_servers_into_file(system, path, &["context_servers"], entry)
}

fn entry(server: &McpServer) -> Value {
    json!({ "command": server.command, "args": server.arguments, "env": {} })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::servers::with_every_server_command;
    use crate::context::fake::FakeSystem;

    const PATH: &str = "/home/ada/.config/zed/settings.json";

    #[test]
    fn a_plain_file_gets_the_servers_with_an_empty_environment() {
        let system = with_every_server_command(FakeSystem::new())
            .with_file(PATH, r#"{"theme": "One Dark"}"#);
        merge_servers(&system, Path::new(PATH))
            .expect("merges")
            .expect("plain JSON");
        let document: Value =
            serde_json::from_str(&system.file(PATH).expect("written")).expect("JSON");
        assert_eq!(document["theme"], "One Dark");
        assert_eq!(
            document["context_servers"]["crownos-computer-use"],
            json!({ "command": "crownos-computer-use", "args": ["mcp"], "env": {} })
        );
    }

    #[test]
    fn a_missing_file_gets_the_servers() {
        let system = with_every_server_command(FakeSystem::new());
        let merge = merge_servers(&system, Path::new(PATH))
            .expect("merges")
            .expect("plain JSON");
        assert_eq!(
            merge.added,
            ["crownos-computer-use", "playwright", "crownos-secrets"]
        );
        assert!(system.file(PATH).is_some());
    }

    #[test]
    fn a_server_with_the_same_name_is_kept() {
        let text = r#"{"context_servers": {"crownos-computer-use": {"command": "other"}}}"#;
        let system = with_every_server_command(FakeSystem::new()).with_file(PATH, text);
        let merge = merge_servers(&system, Path::new(PATH))
            .expect("merges")
            .expect("plain JSON");
        assert_eq!(merge.added, ["playwright", "crownos-secrets"]);
        assert_eq!(merge.kept, ["crownos-computer-use"]);
        let document: Value =
            serde_json::from_str(&system.file(PATH).expect("written")).expect("JSON");
        assert_eq!(
            document["context_servers"]["crownos-computer-use"],
            json!({ "command": "other" })
        );
    }

    #[test]
    fn a_file_with_comments_is_skipped() {
        let text = "// Zed settings\n{\n  \"theme\": \"One Dark\",\n}\n";
        let system = with_every_server_command(FakeSystem::new()).with_file(PATH, text);
        assert_eq!(
            merge_servers(&system, Path::new(PATH)).expect("reads"),
            None
        );
        assert_eq!(system.file(PATH).as_deref(), Some(text));
        assert!(system.journal().is_empty());
    }
}
