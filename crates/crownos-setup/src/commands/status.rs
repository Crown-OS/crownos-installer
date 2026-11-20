//! `crownos-setup status`: the choices and the state of each task.

use anyhow::Result;
use crownos_config::system::{DeveloperTool, SetupConfig, SetupTask, TaskState};
use serde_json::json;

use crate::utils::task_status;

pub fn print_status(as_json: bool) -> Result<()> {
    let config = SetupConfig::load()?;
    let states = TaskState::load_all_installed()?;
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&status_json(config.as_ref(), &states))?
        );
    } else {
        print!("{}", status_text(config.as_ref(), &states));
    }
    Ok(())
}

fn status_json(
    config: Option<&SetupConfig>,
    states: &[(SetupTask, TaskState)],
) -> serde_json::Value {
    json!({
        "config": config.map(|config| json!({
            "browser": config.browser.id(),
            "developer_tools": config.developer_tools,
        })),
        "tasks": states.iter().map(|(task, state)| json!({
            "task": task.id(),
            "status": task_status::label(&state.status),
            "message": task_status::message(&state.status),
            "generation": state.generation,
        })).collect::<Vec<_>>(),
    })
}

fn status_text(config: Option<&SetupConfig>, states: &[(SetupTask, TaskState)]) -> String {
    let mut text = match config {
        Some(config) => {
            let tools: Vec<&str> = config
                .developer_tools
                .enabled()
                .map(DeveloperTool::name)
                .collect();
            let tools = if tools.is_empty() {
                "none".to_owned()
            } else {
                tools.join(", ")
            };
            format!(
                "Web browser: {}\nDeveloper tools: {tools}\n",
                config.browser.name()
            )
        }
        None => "no choices recorded\n".to_owned(),
    };
    for (task, state) in states {
        let label = task_status::label(&state.status);
        match task_status::message(&state.status) {
            Some(message) => text.push_str(&format!("{task}: {label}: {message}\n")),
            None => text.push_str(&format!("{task}: {label}\n")),
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use crownos_config::system::{Browser, DeveloperTools, TaskStatus};

    use super::*;

    fn state(status: TaskStatus) -> TaskState {
        TaskState {
            status,
            generation: 5,
            started_at_unix: None,
            finished_at_unix: None,
            attempts: 0,
        }
    }

    fn example() -> (SetupConfig, Vec<(SetupTask, TaskState)>) {
        let config = SetupConfig {
            browser: Browser::Firefox,
            developer_tools: DeveloperTools {
                codex: true,
                ..DeveloperTools::default()
            },
        };
        let states = vec![
            (SetupTask::Browser, state(TaskStatus::Done)),
            (
                SetupTask::Playwright,
                state(TaskStatus::Failed {
                    message: "npm failed".to_owned(),
                }),
            ),
        ];
        (config, states)
    }

    #[test]
    fn nothing_recorded_prints_one_line() {
        assert_eq!(status_text(None, &[]), "no choices recorded\n");
        assert_eq!(
            status_json(None, &[]),
            json!({ "config": null, "tasks": [] })
        );
    }

    #[test]
    fn the_text_has_the_choices_and_one_line_for_each_task() {
        let (config, states) = example();
        assert_eq!(
            status_text(Some(&config), &states),
            "Web browser: Firefox\nDeveloper tools: Codex\nbrowser: done\nplaywright: failed: npm failed\n"
        );
    }

    #[test]
    fn the_json_uses_the_ids() {
        let (config, states) = example();
        assert_eq!(
            status_json(Some(&config), &states),
            json!({
                "config": {
                    "browser": "firefox",
                    "developer_tools": { "claude_code": false, "codex": true, "cursor": false },
                },
                "tasks": [
                    { "task": "browser", "status": "done", "message": null, "generation": 5 },
                    { "task": "playwright", "status": "failed", "message": "npm failed", "generation": 5 },
                ],
            })
        );
    }
}
