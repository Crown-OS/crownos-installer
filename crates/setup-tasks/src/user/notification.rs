//! The notification that tells the user that the setup is complete, one time
//! for each generation of the choices.

use std::path::Path;
use std::time::Duration;

use crownos_config::system::{DeveloperTool, SetupTask};

use super::{SetupSnapshot, UserMarker};
use crate::context::System;
use crate::error::TaskError;
use crate::utils::home_file;
use crate::utils::user_directories::UserDirectories;

const TITLE: &str = "Your setup is complete";
const TITLE_WITH_PROBLEMS: &str = "Some of your setup could not finish";
const RETRY_INTERVAL: Duration = Duration::from_secs(10);
const RETRY_LIMIT: Duration = Duration::from_secs(10 * 60);

/// Posts the summary when the marker has an older generation. The retries can
/// take minutes, so the marker is read again before it is written. The
/// notification daemon can start after the session, so a failed
/// `notify-send` is tried again every 10 seconds for 10 minutes. A missing
/// `notify-send` stops at once. A marker file that is a symbolic link gives
/// [`TaskError::SymbolicLink`] and no notification. True when the
/// notification was posted.
pub fn notify_once(
    system: &dyn System,
    directories: &UserDirectories,
    snapshot: &SetupSnapshot,
) -> Result<bool, TaskError> {
    let marker_file = directories.marker_file();
    if UserMarker::load(system, &marker_file).notified_generation >= snapshot.generation {
        return Ok(false);
    }
    home_file::refuse_symbolic_link(system, &marker_file)?;
    let body = summary(system, snapshot);
    let attempts = RETRY_LIMIT.as_secs() / RETRY_INTERVAL.as_secs();
    for attempt in 1..=attempts {
        let output = system.run(
            "notify-send",
            &[
                "--app-name",
                "CrownOS",
                "--hint",
                "string:desktop-entry:crownos-settings",
                "--",
                title(snapshot),
                &body,
            ],
        );
        match output {
            Ok(output) if output.succeeded() => {
                let mut marker = UserMarker::load(system, &marker_file);
                marker.notified_generation = snapshot.generation;
                marker.store(system, &marker_file)?;
                return Ok(true);
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!("could not start notify-send: {error}");
                return Ok(false);
            }
        }
        if attempt < attempts {
            system.sleep(RETRY_INTERVAL);
        }
    }
    Ok(false)
}

/// One short line for each part of the setup: what is ready and what could
/// not be installed. A developer tool counts as installed when its program
/// exists.
pub fn summary(system: &dyn System, snapshot: &SetupSnapshot) -> String {
    let config = &snapshot.config;
    let browser = config.browser.name();
    let mut lines = vec![if snapshot.has_given_up(SetupTask::Browser) {
        format!("{browser} could not be installed. See Settings > Web Browser.")
    } else {
        format!("{browser} is your browser.")
    }];

    let (installed_tools, missing_tools): (Vec<DeveloperTool>, Vec<DeveloperTool>) =
        config.developer_tools.enabled().partition(|tool| {
            !snapshot.has_given_up(SetupTask::DeveloperTools)
                || system.exists(Path::new(tool.executable()))
        });
    match installed_tools.as_slice() {
        [] => {}
        [tool] => lines.push(format!("{} is installed.", tool.name())),
        tools => lines.push(format!("{} are installed.", join_names(tools))),
    }
    if !missing_tools.is_empty() {
        lines.push(format!(
            "{} could not be installed. See Settings > Developer Tools.",
            join_names(&missing_tools)
        ));
    }

    if config.wants_playwright() {
        lines.push(if snapshot.has_given_up(SetupTask::Playwright) {
            "Playwright could not be set up. See Settings > Developer Tools.".to_owned()
        } else {
            "Playwright is ready for coding agents.".to_owned()
        });
    }
    lines.join("\n")
}

fn title(snapshot: &SetupSnapshot) -> &'static str {
    if SetupTask::ALL
        .into_iter()
        .any(|task| snapshot.has_given_up(task))
    {
        TITLE_WITH_PROBLEMS
    } else {
        TITLE
    }
}

/// `A`, `A and B`, `A, B and C`.
fn join_names(tools: &[DeveloperTool]) -> String {
    let names: Vec<&str> = tools.iter().map(|tool| tool.name()).collect();
    match names.split_last() {
        None => String::new(),
        Some((last, [])) => (*last).to_owned(),
        Some((last, first)) => format!("{} and {last}", first.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use crownos_config::system::{
        Browser, DeveloperTools, MAX_TASK_ATTEMPTS, SetupConfig, TaskState, TaskStatus,
    };

    use super::*;
    use crate::context::fake::FakeSystem;

    const NOTIFY: &str = "notify-send --app-name CrownOS --hint string:desktop-entry:crownos-settings -- Your setup is complete";

    fn directories() -> UserDirectories {
        UserDirectories::new("/home/ada".into(), None, None)
    }

    fn brave_with_tools(developer_tools: DeveloperTools) -> SetupSnapshot {
        SetupSnapshot {
            config: SetupConfig {
                browser: Browser::Brave,
                developer_tools,
            },
            generation: 21,
            states: Vec::new(),
        }
    }

    fn notifications(system: &FakeSystem) -> usize {
        system
            .commands()
            .iter()
            .filter(|command| command.starts_with(NOTIFY))
            .count()
    }

    fn snapshot_of(config: SetupConfig) -> SetupSnapshot {
        SetupSnapshot {
            config,
            generation: 21,
            states: Vec::new(),
        }
    }

    fn gave_up() -> TaskState {
        TaskState {
            status: TaskStatus::Failed {
                message: "target not found".to_owned(),
            },
            generation: 21,
            started_at_unix: None,
            finished_at_unix: None,
            attempts: MAX_TASK_ATTEMPTS,
        }
    }

    #[test]
    fn the_summary_names_the_choices() {
        let system = FakeSystem::new();
        let tools = DeveloperTools {
            claude_code: true,
            codex: true,
            cursor: false,
        };
        assert_eq!(
            summary(&system, &brave_with_tools(tools)),
            "Brave is your browser.\nClaude Code and Codex are installed.\nPlaywright is ready for coding agents."
        );
        let every_tool = DeveloperTools {
            cursor: true,
            ..tools
        };
        assert!(
            summary(&system, &brave_with_tools(every_tool))
                .contains("Claude Code, Codex and Cursor are installed.")
        );
        assert_eq!(
            summary(&system, &snapshot_of(SetupConfig::default())),
            "Chromium is your browser.\nPlaywright is ready for coding agents."
        );
    }

    #[test]
    fn the_summary_names_what_could_not_be_installed() {
        let system = FakeSystem::new().with_file("/usr/bin/claude", "");
        let snapshot = SetupSnapshot {
            states: vec![
                (SetupTask::DeveloperTools, gave_up()),
                (SetupTask::Playwright, gave_up()),
            ],
            ..brave_with_tools(DeveloperTools {
                claude_code: true,
                codex: false,
                cursor: true,
            })
        };
        assert_eq!(
            summary(&system, &snapshot),
            "Brave is your browser.\nClaude Code is installed.\nCursor could not be installed. See Settings > Developer Tools.\nPlaywright could not be set up. See Settings > Developer Tools."
        );
        assert_eq!(title(&snapshot), TITLE_WITH_PROBLEMS);

        let browser_failed = SetupSnapshot {
            states: vec![(SetupTask::Browser, gave_up())],
            ..brave_with_tools(DeveloperTools::default())
        };
        assert!(
            summary(&system, &browser_failed)
                .starts_with("Brave could not be installed. See Settings > Web Browser.")
        );
    }

    #[test]
    fn the_notification_is_posted_one_time_for_each_generation() {
        let system = FakeSystem::new().with_program("notify-send");
        let snapshot = brave_with_tools(DeveloperTools::default());
        assert!(notify_once(&system, &directories(), &snapshot).expect("notifies"));
        assert!(!notify_once(&system, &directories(), &snapshot).expect("notifies"));
        assert_eq!(notifications(&system), 1);

        let newer = SetupSnapshot {
            generation: 22,
            ..snapshot
        };
        assert!(notify_once(&system, &directories(), &newer).expect("notifies"));
        assert_eq!(notifications(&system), 2);
    }

    #[test]
    fn a_failed_notify_send_is_tried_again_for_ten_minutes() {
        let snapshot = brave_with_tools(DeveloperTools::default());
        let command_line = format!("{NOTIFY} {}", summary(&FakeSystem::new(), &snapshot));
        let system = FakeSystem::new()
            .with_program("notify-send")
            .with_failure(&command_line, "no notification daemon");
        assert!(!notify_once(&system, &directories(), &snapshot).expect("gives up"));
        assert_eq!(notifications(&system), 60);
        assert_eq!(
            UserMarker::load(&system, &directories().marker_file()).notified_generation,
            0
        );
    }

    #[test]
    fn a_missing_notify_send_stops_at_once() {
        let system = FakeSystem::new();
        let snapshot = brave_with_tools(DeveloperTools::default());
        assert!(!notify_once(&system, &directories(), &snapshot).expect("gives up"));
        assert_eq!(notifications(&system), 1);
        assert!(
            !system
                .journal()
                .iter()
                .any(|entry| entry.starts_with("sleep"))
        );
    }

    #[test]
    fn a_linked_marker_gets_no_notification() {
        let marker = directories().marker_file();
        let system = FakeSystem::new()
            .with_program("notify-send")
            .with_file("/home/ada/dotfiles/user.ron", "(dock_generation: 1)")
            .with_link(&marker, "/home/ada/dotfiles/user.ron");
        let snapshot = brave_with_tools(DeveloperTools::default());
        assert!(matches!(
            notify_once(&system, &directories(), &snapshot),
            Err(TaskError::SymbolicLink(_))
        ));
        assert_eq!(notifications(&system), 0);
    }
}
