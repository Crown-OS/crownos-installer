//! The notification that tells the user that the setup is complete, one time
//! for each generation of the choices.

use std::time::Duration;

use crownos_config::system::{DeveloperTool, SetupConfig};

use super::{SetupSnapshot, UserMarker};
use crate::context::System;
use crate::error::TaskError;
use crate::utils::home_file;
use crate::utils::user_directories::UserDirectories;

const TITLE: &str = "Your setup is complete";
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
    let body = summary(&snapshot.config);
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
                TITLE,
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

/// One short line for the browser, the developer tools and Playwright.
pub fn summary(config: &SetupConfig) -> String {
    let mut lines = vec![format!("{} is your browser.", config.browser.name())];
    let tools: Vec<&str> = config
        .developer_tools
        .enabled()
        .map(DeveloperTool::name)
        .collect();
    match tools.as_slice() {
        [] => {}
        [tool] => lines.push(format!("{tool} is installed.")),
        [first @ .., last] => lines.push(format!("{} and {last} are installed.", first.join(", "))),
    }
    if config.wants_playwright() {
        lines.push("Playwright is ready for coding agents.".to_owned());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use crownos_config::system::{Browser, DeveloperTools};

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

    #[test]
    fn the_summary_names_the_choices() {
        let tools = DeveloperTools {
            claude_code: true,
            codex: true,
            cursor: false,
        };
        assert_eq!(
            summary(&brave_with_tools(tools).config),
            "Brave is your browser.\nClaude Code and Codex are installed.\nPlaywright is ready for coding agents."
        );
        let every_tool = DeveloperTools {
            cursor: true,
            ..tools
        };
        assert!(
            summary(&brave_with_tools(every_tool).config)
                .contains("Claude Code, Codex and Cursor are installed.")
        );
        assert_eq!(
            summary(&SetupConfig::default()),
            "Chromium is your browser.\nPlaywright is ready for coding agents."
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
        let command_line = format!("{NOTIFY} {}", summary(&snapshot.config));
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
