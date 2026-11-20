//! `~/.cursor/mcp.json`: the `mcpServers` object, as
//! <https://cursor.com/docs/context/mcp> tells.

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
    json!({ "type": "stdio", "command": server.command, "args": server.arguments })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::fake::FakeSystem;

    const PATH: &str = "/home/ada/.cursor/mcp.json";

    #[test]
    fn a_missing_file_gets_both_servers() {
        let system = FakeSystem::new();
        let merge = merge_servers(&system, Path::new(PATH))
            .expect("merges")
            .expect("plain JSON");
        assert_eq!(merge.added.len(), 2);
        assert_eq!(
            system.file(PATH).as_deref(),
            Some(
                "{\n  \"mcpServers\": {\n    \"crownos-computer-use\": {\n      \"type\": \"stdio\",\n      \"command\": \"crownos-computer-use\",\n      \"args\": [\n        \"mcp\"\n      ]\n    },\n    \"playwright\": {\n      \"type\": \"stdio\",\n      \"command\": \"crownos-setup\",\n      \"args\": [\n        \"playwright-mcp\"\n      ]\n    }\n  }\n}\n"
            )
        );
        assert!(
            system
                .journal()
                .contains(&"mkdir /home/ada/.cursor".to_owned())
        );
    }

    #[test]
    fn other_keys_stay_in_their_order() {
        let system = FakeSystem::new()
            .with_directory("/home/ada/.cursor")
            .with_file(
                PATH,
                r#"{"z": 1, "mcpServers": {"github": {"url": "https://x"}}, "a": 2}"#,
            );
        merge_servers(&system, Path::new(PATH)).expect("merges");
        let document: Value =
            serde_json::from_str(&system.file(PATH).expect("written")).expect("JSON");
        let keys: Vec<&String> = document.as_object().expect("object").keys().collect();
        assert_eq!(keys, ["z", "mcpServers", "a"]);
        let server_names: Vec<&String> = document["mcpServers"]
            .as_object()
            .expect("object")
            .keys()
            .collect();
        assert_eq!(
            server_names,
            ["github", "crownos-computer-use", "playwright"]
        );
        assert!(
            !system
                .journal()
                .iter()
                .any(|entry| entry.starts_with("mkdir"))
        );
    }

    #[test]
    fn a_server_with_the_same_name_is_kept_and_the_file_stays() {
        let text = r#"{"mcpServers": {"playwright": {"command": "npx"}, "crownos-computer-use": {"type": "stdio", "command": "crownos-computer-use", "args": ["mcp"]}}}"#;
        let system = FakeSystem::new().with_file(PATH, text);
        let merge = merge_servers(&system, Path::new(PATH))
            .expect("merges")
            .expect("plain JSON");
        assert_eq!(merge.added, Vec::<&str>::new());
        assert_eq!(merge.kept, ["playwright"]);
        assert_eq!(system.file(PATH).as_deref(), Some(text));
        assert!(system.journal().is_empty());
    }
}
