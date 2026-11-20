//! The steps of `crownos-setup user`, in order.

use std::path::{Path, PathBuf};

use super::{SetupSnapshot, dock};
use crate::agents::playwright_cli::{self, ConfigurationChange};
use crate::agents::{self, skills};
use crate::context::System;
use crate::error::TaskError;
use crate::tasks::playwright::{PLAYWRIGHT_CLI_BINARY, prefix_binary};
use crate::utils::browser_fallback::resolve_browser;
use crate::utils::user_directories::UserDirectories;

/// The result of one run. `kept` and `skipped` stay the same from run to
/// run. The other fields are changes or failures.
#[derive(Debug, Default)]
pub struct UserSetupReport {
    /// The skill links that the run made.
    pub linked: Vec<PathBuf>,
    /// Each MCP server that the run added, with its file.
    pub merged: Vec<(PathBuf, &'static str)>,
    /// Each MCP server that a file has with another command or other
    /// arguments. The run did not change it.
    pub kept: Vec<(PathBuf, &'static str)>,
    /// The files that the run did not edit: symbolic links, and files with
    /// comments or an unexpected format.
    pub skipped: Vec<PathBuf>,
    /// Each step that failed, with the file or folder of the step. The steps
    /// after it ran.
    pub failed: Vec<(PathBuf, TaskError)>,
    pub playwright_configured: bool,
    pub dock_changed: bool,
}

impl UserSetupReport {
    fn record_failure<T>(&mut self, path: &Path, result: Result<T, TaskError>) -> Option<T> {
        result
            .map_err(|error| self.failed.push((path.to_owned(), error)))
            .ok()
    }
}

/// Links the skills and adds the MCP servers for each agent on the account,
/// sets the browser of `playwright-cli` when it is installed, and points the
/// dock at the browser one time for each generation. Each step adds only, so
/// a second run changes nothing. A step that fails does not stop the steps
/// after it.
pub fn wire(
    system: &dyn System,
    directories: &UserDirectories,
    snapshot: &SetupSnapshot,
) -> UserSetupReport {
    let mut report = UserSetupReport::default();
    let agents = agents::present(system, directories);

    let skill_folders = skills::folders(&agents, directories);
    if !skill_folders.is_empty() {
        let system_skills = report
            .record_failure(
                Path::new(skills::SKILLS_DIRECTORY),
                skills::system_skills(system),
            )
            .unwrap_or_default();
        for folder in &skill_folders {
            if let Some(links) =
                report.record_failure(folder, skills::link(system, &system_skills, folder))
            {
                report.linked.extend(links);
            }
        }
    }

    for agent in agents {
        let path = agent.configuration_file(directories);
        match report.record_failure(&path, agent.merge_servers(system, &path)) {
            Some(Some(merge)) => {
                report
                    .merged
                    .extend(merge.added.into_iter().map(|name| (path.clone(), name)));
                report
                    .kept
                    .extend(merge.kept.into_iter().map(|name| (path.clone(), name)));
            }
            Some(None) => report.skipped.push(path),
            None => {}
        }
    }

    if snapshot.config.wants_playwright() && system.exists(&prefix_binary(PLAYWRIGHT_CLI_BINARY)) {
        let path = playwright_cli::configuration_file(&directories.home);
        let browser = resolve_browser(snapshot.config.browser, |executable| {
            system.exists(executable)
        });
        match report.record_failure(&path, playwright_cli::set_browser(system, &path, browser)) {
            Some(ConfigurationChange::Written) => report.playwright_configured = true,
            Some(ConfigurationChange::Skipped) => report.skipped.push(path),
            Some(ConfigurationChange::Unchanged) | None => {}
        }
    }

    report.dock_changed = report
        .record_failure(
            &directories.dock_file(),
            dock::update_once(system, directories, snapshot),
        )
        .unwrap_or(false);
    report
}

#[cfg(test)]
mod tests {
    use crownos_config::system::{Browser, SetupConfig, SetupTask, TaskStatus};
    use serde_json::Value;

    use super::*;
    use crate::context::fake::FakeSystem;
    use crate::user::snapshot::tests::state;

    const PLAYWRIGHT_CLI: &str = "/usr/lib/crownos/playwright/node_modules/.bin/playwright-cli";
    const DOCK: &str = "/home/ada/.config/crownos/dock.ron";
    const CHROMIUM_DOCK: &str = "(items: [\"/usr/share/applications/chromium.desktop\"])";
    const SKILL: &str = "/usr/share/crownos/skills/computer-use";

    fn directories() -> UserDirectories {
        UserDirectories::new("/home/ada".into(), None, None)
    }

    fn done_snapshot(browser: Browser) -> SetupSnapshot {
        SetupSnapshot {
            config: SetupConfig {
                browser,
                ..SetupConfig::default()
            },
            generation: 12,
            states: vec![(SetupTask::Browser, state(TaskStatus::Done, 12))],
        }
    }

    #[test]
    fn an_account_without_agents_and_playwright_gets_only_the_dock() {
        let system = FakeSystem::new().with_file(DOCK, CHROMIUM_DOCK);
        let report = wire(&system, &directories(), &done_snapshot(Browser::Firefox));
        assert!(report.dock_changed);
        assert!(report.linked.is_empty() && report.merged.is_empty() && report.skipped.is_empty());
        assert!(report.failed.is_empty() && !report.playwright_configured);
    }

    #[test]
    fn every_step_runs_and_a_second_run_writes_nothing() {
        let system = FakeSystem::new()
            .with_program("codex")
            .with_program("agy")
            .with_directory(SKILL)
            .with_file(PLAYWRIGHT_CLI, "")
            .with_directory("/home/ada/.config/zed")
            .with_file(
                "/home/ada/.config/zed/settings.json",
                "{\n  // comment\n}\n",
            );
        let report = wire(&system, &directories(), &done_snapshot(Browser::Firefox));
        assert_eq!(
            report.linked,
            [
                PathBuf::from("/home/ada/.agents/skills/computer-use"),
                PathBuf::from("/home/ada/.gemini/config/skills/computer-use")
            ]
        );
        assert_eq!(report.merged.len(), 4);
        assert!(report.kept.is_empty() && report.failed.is_empty());
        assert_eq!(
            report.skipped,
            [PathBuf::from("/home/ada/.config/zed/settings.json")]
        );
        assert!(report.playwright_configured);
        assert!(system.file("/home/ada/.codex/config.toml").is_some());
        assert!(
            system
                .file("/home/ada/.gemini/config/mcp_config.json")
                .is_some()
        );

        system.clear_journal();
        let second_report = wire(&system, &directories(), &done_snapshot(Browser::Firefox));
        assert_eq!(second_report.skipped.len(), 1);
        assert!(second_report.merged.is_empty() && second_report.linked.is_empty());
        assert!(system.journal().is_empty(), "{:?}", system.journal());
    }

    #[test]
    fn a_failed_step_does_not_stop_the_next_steps() {
        let system = FakeSystem::new()
            .with_program("codex")
            .with_directory(SKILL)
            .with_racing_link("/home/ada/.agents/skills/computer-use", "/opt/other")
            .with_file(DOCK, CHROMIUM_DOCK);
        let report = wire(&system, &directories(), &done_snapshot(Browser::Brave));
        assert_eq!(report.failed.len(), 1);
        assert_eq!(
            report.failed[0].0,
            PathBuf::from("/home/ada/.agents/skills")
        );
        assert_eq!(report.merged.len(), 2);
        assert!(report.dock_changed);
    }

    #[test]
    fn playwright_cli_uses_chromium_while_the_chosen_browser_is_missing() {
        let config_file = "/home/ada/.playwright/cli.config.json";
        let system = FakeSystem::new()
            .with_file(PLAYWRIGHT_CLI, "")
            .with_file("/usr/bin/chromium", "");
        wire(&system, &directories(), &done_snapshot(Browser::Brave));
        let document: Value =
            serde_json::from_str(&system.file(config_file).expect("written")).expect("JSON");
        assert_eq!(
            document["browser"]["launchOptions"]["executablePath"],
            "/usr/bin/chromium"
        );
    }
}
