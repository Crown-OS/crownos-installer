//! `crownos-setup browser set` and `crownos-setup tools set`: Settings runs
//! them through pkexec. Each one records the choice, marks the affected tasks
//! `Pending` before it returns and starts the runner.

use std::path::Path;

use anyhow::{Context as _, Result};
use crownos_config::system::{
    Browser, DeveloperTool, SETUP_CONFIG_PATH, SETUP_STATE_DIR, SetupConfig, SetupTask,
};
use crownos_setup_tasks::utils::pacman;
use crownos_setup_tasks::{StateStore, System, config};

const RUNNER_UNIT: &str = "crownos-setup.service";

pub fn set_browser(system: &dyn System, browser: Browser) -> Result<()> {
    record_choice(
        system,
        &[SetupTask::Browser, SetupTask::Playwright],
        |setup_config| setup_config.browser = browser,
    )?;
    start_runner(system);
    Ok(())
}

/// Turning a tool off also removes its package. The runner never removes a
/// package.
pub fn set_developer_tool(system: &dyn System, tool: DeveloperTool, enabled: bool) -> Result<()> {
    record_choice(system, &[SetupTask::DeveloperTools], |setup_config| {
        setup_config.developer_tools.set(tool, enabled);
    })?;
    let package = tool.package();
    let removal = if !enabled && pacman::is_installed(system, package) {
        pacman::remove(system, &[package])
    } else {
        Ok(())
    };
    start_runner(system);
    removal.with_context(|| format!("could not remove {}", tool.name()))
}

fn record_choice(
    system: &dyn System,
    affected_tasks: &[SetupTask],
    change: impl FnOnce(&mut SetupConfig),
) -> Result<()> {
    let config_path = Path::new(SETUP_CONFIG_PATH);
    let mut setup_config = config::load_or_default(system, config_path)?;
    change(&mut setup_config);
    config::store(system, config_path, &setup_config)?;
    StateStore::new(system, SETUP_STATE_DIR)
        .reset(affected_tasks, config::generation(system, config_path))?;
    Ok(())
}

fn start_runner(system: &dyn System) {
    let started = system
        .run("systemctl", &["start", "--no-block", RUNNER_UNIT])
        .and_then(|output| output.check("systemctl start"));
    if let Err(error) = started {
        tracing::warn!("could not start {RUNNER_UNIT}: {error}");
    }
}

#[cfg(test)]
mod tests {
    use crownos_config::system::{DeveloperTools, TaskStatus};
    use crownos_setup_tasks::context::fake::FakeSystem;

    use super::*;

    const START_RUNNER: &str = "systemctl start --no-block crownos-setup.service";

    fn stored_config(system: &FakeSystem) -> SetupConfig {
        config::load_or_default(system, Path::new(SETUP_CONFIG_PATH)).expect("loads")
    }

    fn pending_tasks(system: &FakeSystem) -> Vec<(SetupTask, u64)> {
        StateStore::new(system, SETUP_STATE_DIR)
            .load_all()
            .expect("loads")
            .into_iter()
            .filter(|(_, state)| state.status == TaskStatus::Pending)
            .map(|(task, state)| (task, state.generation))
            .collect()
    }

    #[test]
    fn browser_set_keeps_the_tools_and_resets_browser_and_playwright() {
        let system = FakeSystem::new().with_program("systemctl").with_file(
            SETUP_CONFIG_PATH,
            "(browser: Chromium, developer_tools: (codex: true))",
        );
        set_browser(&system, Browser::Brave).expect("sets");

        let setup_config = stored_config(&system);
        assert_eq!(setup_config.browser, Browser::Brave);
        assert!(setup_config.developer_tools.codex);
        let generation = config::generation(&system, Path::new(SETUP_CONFIG_PATH));
        assert_eq!(
            pending_tasks(&system),
            [
                (SetupTask::Browser, generation),
                (SetupTask::Playwright, generation)
            ]
        );
        assert_eq!(system.commands(), [START_RUNNER]);
    }

    #[test]
    fn tools_set_off_removes_the_installed_package_after_the_reset() {
        let system = FakeSystem::new()
            .with_program("systemctl")
            .with_file(
                SETUP_CONFIG_PATH,
                "(browser: Firefox, developer_tools: (cursor: true))",
            )
            .with_installed(&["cursor-bin"]);
        set_developer_tool(&system, DeveloperTool::Cursor, false).expect("sets");

        assert_eq!(
            stored_config(&system).developer_tools,
            DeveloperTools::default()
        );
        assert_eq!(stored_config(&system).browser, Browser::Firefox);
        assert!(!system.is_installed("cursor-bin"));
        let journal = system.journal();
        let reset = journal
            .iter()
            .position(|entry| entry.ends_with("developer-tools.ron"))
            .expect("reset");
        let removal = journal
            .iter()
            .position(|entry| entry == "run pacman -Rs --noconfirm -- cursor-bin")
            .expect("removed");
        assert!(reset < removal);
        assert_eq!(
            journal.last().map(String::as_str),
            Some(&*format!("run {START_RUNNER}"))
        );
    }

    #[test]
    fn tools_set_on_installs_nothing_itself() {
        let system = FakeSystem::new().with_program("systemctl");
        set_developer_tool(&system, DeveloperTool::Codex, true).expect("sets");

        assert!(stored_config(&system).developer_tools.codex);
        assert_eq!(pending_tasks(&system).len(), 1);
        assert_eq!(system.commands(), [START_RUNNER]);
    }
}
