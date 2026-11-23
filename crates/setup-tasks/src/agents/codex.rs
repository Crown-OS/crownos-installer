//! `~/.codex/config.toml`: one `[mcp_servers.<name>]` table for each server.
//! The edit keeps the comments and the formatting of the file.

use std::path::Path;

use toml_edit::{Array, DocumentMut, Item, Table, value};

use super::servers::{McpServer, installed_servers};
use crate::context::System;
use crate::error::TaskError;
use crate::utils::home_file;
use crate::utils::json_merge::Merge;

const SERVERS_KEY: &str = "mcp_servers";

pub fn merge_servers(
    system: &dyn System,
    path: &Path,
) -> Result<Option<Merge<'static>>, TaskError> {
    let text = system.read_file(path)?.unwrap_or_default();
    let Ok(mut document) = text.parse::<DocumentMut>() else {
        return Ok(None);
    };
    let Some(merge) = merge_into_document(system, &mut document) else {
        return Ok(None);
    };
    if !merge.added.is_empty() {
        home_file::write(system, path, &document.to_string())?;
    }
    Ok(Some(merge))
}

fn merge_into_document(system: &dyn System, document: &mut DocumentMut) -> Option<Merge<'static>> {
    let servers_table = document
        .entry(SERVERS_KEY)
        .or_insert_with(|| {
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        })
        .as_table_mut()?;
    let mut merge = Merge::default();
    for server in installed_servers(system) {
        match servers_table.get(server.name) {
            None => {
                servers_table.insert(server.name, Item::Table(server_table(server)));
                merge.added.push(server.name);
            }
            Some(existing) if !is_same_server(existing, server) => merge.kept.push(server.name),
            Some(_) => {}
        }
    }
    Some(merge)
}

fn server_table(server: &McpServer) -> Table {
    let mut table = Table::new();
    table.insert("command", value(server.command));
    table.insert(
        "args",
        value(server.arguments.iter().copied().collect::<Array>()),
    );
    table
}

fn is_same_server(existing: &Item, server: &McpServer) -> bool {
    let arguments = existing.get("args").and_then(Item::as_array).map(|array| {
        array
            .iter()
            .map(|argument| argument.as_str())
            .collect::<Vec<_>>()
    });
    existing.get("command").and_then(Item::as_str) == Some(server.command)
        && arguments
            == Some(
                server
                    .arguments
                    .iter()
                    .map(|&argument| Some(argument))
                    .collect(),
            )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::servers::with_every_server_command;
    use crate::context::fake::FakeSystem;

    const PATH: &str = "/home/ada/.codex/config.toml";

    #[test]
    fn a_missing_file_gets_a_table_for_each_server() {
        let system = with_every_server_command(FakeSystem::new());
        merge_servers(&system, Path::new(PATH))
            .expect("merges")
            .expect("TOML");
        assert_eq!(
            system.file(PATH).as_deref(),
            Some(
                "[mcp_servers.crownos-computer-use]\ncommand = \"crownos-computer-use\"\nargs = [\"mcp\"]\n\n[mcp_servers.playwright]\ncommand = \"crownos-setup\"\nargs = [\"playwright-mcp\"]\n\n[mcp_servers.crownos-secrets]\ncommand = \"crownos-secrets-mcp\"\nargs = []\n"
            )
        );
    }

    #[test]
    fn comments_and_other_tables_stay() {
        let text = "# My Codex settings\nmodel = \"gpt-5\"  # the default\n\n[projects.\"/home/ada/code\"]\ntrust_level = \"trusted\"\n";
        let system = with_every_server_command(FakeSystem::new()).with_file(PATH, text);
        let merge = merge_servers(&system, Path::new(PATH))
            .expect("merges")
            .expect("TOML");
        assert_eq!(merge.added.len(), 3);
        let written = system.file(PATH).expect("written");
        assert!(written.starts_with(text), "{written}");
        assert!(written.contains("[mcp_servers.playwright]\ncommand = \"crownos-setup\"\n"));
    }

    #[test]
    fn a_server_with_the_same_name_is_kept() {
        let text = "[mcp_servers.playwright]\ncommand = \"npx\"\nargs = [\"@playwright/mcp@latest\"]\n\n[mcp_servers.crownos-computer-use]\ncommand = \"crownos-computer-use\"\nargs = [\"mcp\"]\n";
        let system = FakeSystem::new()
            .with_program("crownos-computer-use")
            .with_program("crownos-setup")
            .with_file(PATH, text);
        let merge = merge_servers(&system, Path::new(PATH))
            .expect("merges")
            .expect("TOML");
        assert_eq!(
            merge,
            Merge {
                added: vec![],
                kept: vec!["playwright"]
            }
        );
        assert_eq!(system.file(PATH).as_deref(), Some(text));
    }

    #[test]
    fn a_file_that_is_not_toml_is_skipped() {
        let system = with_every_server_command(FakeSystem::new()).with_file(PATH, "model = \n");
        assert_eq!(
            merge_servers(&system, Path::new(PATH)).expect("reads"),
            None
        );
        assert!(system.journal().is_empty());
    }
}
