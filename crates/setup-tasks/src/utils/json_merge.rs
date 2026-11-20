//! Adds MCP servers to the server object of a JSON configuration document.

use serde_json::{Map, Value};

/// The servers that a merge added, and the servers that it did not change
/// because the document has a server with the same name and another command
/// or other arguments.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Merge<'a> {
    pub added: Vec<&'a str>,
    pub kept: Vec<&'a str>,
}

/// Adds each `(name, entry)` in `entries` to the object at `path`. Makes the
/// missing objects on `path`. Never changes a server that exists. `None`
/// when the document or a value on `path` is not an object.
pub fn merge_servers<'a>(
    document: &mut Value,
    path: &[&str],
    entries: impl IntoIterator<Item = (&'a str, Value)>,
) -> Option<Merge<'a>> {
    let mut object = document.as_object_mut()?;
    for key in path {
        object = object
            .entry(*key)
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()?;
    }
    let mut merge = Merge::default();
    for (name, wanted_entry) in entries {
        match object.get(name) {
            None => {
                object.insert(name.to_owned(), wanted_entry);
                merge.added.push(name);
            }
            Some(existing_entry) if !starts_the_same_server(existing_entry, &wanted_entry) => {
                merge.kept.push(name);
            }
            Some(_) => {}
        }
    }
    Some(merge)
}

fn starts_the_same_server(existing_entry: &Value, wanted_entry: &Value) -> bool {
    ["command", "args"]
        .into_iter()
        .all(|key| existing_entry.get(key) == wanted_entry.get(key))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn entries() -> [(&'static str, Value); 2] {
        [
            (
                "crownos-computer-use",
                json!({ "type": "stdio", "command": "crownos-computer-use", "args": ["mcp"] }),
            ),
            (
                "playwright",
                json!({ "type": "stdio", "command": "crownos-setup", "args": ["playwright-mcp"] }),
            ),
        ]
    }

    #[test]
    fn an_empty_document_gets_the_path_and_each_server() {
        let mut document = json!({});
        let merge = merge_servers(&mut document, &["a", "b"], entries()).expect("merges");
        assert_eq!(merge.added, ["crownos-computer-use", "playwright"]);
        assert!(merge.kept.is_empty());
        assert_eq!(
            document["a"]["b"]["playwright"],
            json!({ "type": "stdio", "command": "crownos-setup", "args": ["playwright-mcp"] })
        );
    }

    #[test]
    fn only_the_command_and_the_arguments_are_compared() {
        let mut document = json!({ "servers": {
            "playwright": { "command": "npx", "args": ["@playwright/mcp"] },
            "crownos-computer-use": { "command": "crownos-computer-use", "args": ["mcp"], "env": { "A": "1" } },
        }});
        let before = document.clone();
        let merge = merge_servers(&mut document, &["servers"], entries()).expect("merges");
        assert_eq!(
            merge,
            Merge {
                added: vec![],
                kept: vec!["playwright"]
            }
        );
        assert_eq!(document, before);
    }

    #[test]
    fn a_value_that_is_not_an_object_stops_the_merge() {
        assert_eq!(
            merge_servers(&mut json!({ "servers": [] }), &["servers"], entries()),
            None
        );
        assert_eq!(merge_servers(&mut json!([]), &["servers"], entries()), None);
    }
}
