//! `crownos-setup run`: the work of `crownos-setup.service`.

use std::path::Path;
use std::process::ExitCode;

use anyhow::Result;
use crownos_config::system::{SETUP_CONFIG_PATH, SETUP_STATE_DIR};
use crownos_setup_tasks::utils::network;
use crownos_setup_tasks::{Context, StateStore, System, config, run_pass};

use crate::utils::task_status;

/// The passes in one run when the choices change during each pass.
const MAX_PASSES: usize = 3;

#[derive(Debug, PartialEq, Eq)]
struct RunSummary {
    passes: usize,
    is_complete: bool,
}

/// The exit code is a failure while a task failed or waits for the network,
/// so that the unit starts again later.
pub fn run(system: &dyn System) -> Result<ExitCode> {
    let summary = run_until_settled(system)?;
    let store = StateStore::new(system, SETUP_STATE_DIR);
    for (task, state) in store.load_all()? {
        match task_status::message(&state.status) {
            Some(message) => tracing::warn!("{task}: failed: {message}"),
            None => tracing::info!("{task}: {}", task_status::label(&state.status)),
        }
    }
    Ok(if summary.is_complete {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// Runs passes until the choices stay the same during a pass, at most
/// [`MAX_PASSES`] times.
fn run_until_settled(system: &dyn System) -> Result<RunSummary> {
    let config_path = Path::new(SETUP_CONFIG_PATH);
    let store = StateStore::new(system, SETUP_STATE_DIR);
    let mut summary = RunSummary {
        passes: 0,
        is_complete: false,
    };
    while summary.passes < MAX_PASSES {
        summary.passes += 1;
        let generation = config::generation(system, config_path);
        let setup_config = config::load_or_default(system, config_path)?;
        let context = Context {
            config: setup_config,
            system,
            online: network::is_online(system),
        };
        let pass_summary = run_pass(&context, &store, generation)?;
        let choices_changed = config::generation(system, config_path) != generation
            || config::load_or_default(system, config_path)? != setup_config;
        summary.is_complete = pass_summary.is_complete() && !choices_changed;
        if !choices_changed {
            break;
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use crownos_setup_tasks::context::fake::FakeSystem;

    use super::*;

    const DOCK: &str = "/etc/skel/.config/crownos/dock.ron";
    const PLAYWRIGHT_BIN: &str = "/usr/lib/crownos/playwright/node_modules/.bin";
    const CODEX_CONFIG: &str = "(browser: Chromium, developer_tools: (codex: true))";

    fn ready_system() -> FakeSystem {
        FakeSystem::new()
            .with_installed(&["chromium"])
            .with_file(
                DOCK,
                "(items: [\"/usr/share/applications/chromium.desktop\"])",
            )
            .with_file("/usr/bin/node", "")
            .with_file("/usr/bin/npm", "")
            .with_file(format!("{PLAYWRIGHT_BIN}/playwright-mcp"), "")
            .with_file(format!("{PLAYWRIGHT_BIN}/playwright-cli"), "")
            .with_link(
                "/usr/local/bin/playwright-cli",
                format!("{PLAYWRIGHT_BIN}/playwright-cli"),
            )
    }

    fn summary(passes: usize, is_complete: bool) -> RunSummary {
        RunSummary {
            passes,
            is_complete,
        }
    }

    #[test]
    fn a_settled_system_completes_in_one_pass() {
        let system = ready_system().with_network();
        assert_eq!(run_until_settled(&system).expect("runs"), summary(1, true));
    }

    #[test]
    fn a_failed_task_is_not_complete() {
        let system = ready_system()
            .with_network()
            .with_file(SETUP_CONFIG_PATH, CODEX_CONFIG)
            .with_failure(
                "pacman -Syu --needed --noconfirm -- openai-codex",
                "error: target not found: openai-codex",
            );
        assert_eq!(run_until_settled(&system).expect("runs"), summary(1, false));
    }

    #[test]
    fn waiting_for_the_network_is_not_complete() {
        let system = ready_system().with_file(SETUP_CONFIG_PATH, CODEX_CONFIG);
        assert_eq!(run_until_settled(&system).expect("runs"), summary(1, false));
        assert!(!system.is_installed("openai-codex"));
    }

    #[test]
    fn a_change_during_a_pass_runs_one_more_pass() {
        let firefox_with_codex = "(browser: Firefox, developer_tools: (codex: true))";
        let system = ready_system()
            .with_network()
            .with_directory("/usr/share/playwright/firefox-1578")
            .with_file(SETUP_CONFIG_PATH, "(browser: Firefox)")
            .with_command_writes(
                "pacman -Syu --needed --noconfirm -- firefox",
                SETUP_CONFIG_PATH,
                firefox_with_codex,
            );

        assert_eq!(run_until_settled(&system).expect("runs"), summary(2, true));
        assert!(system.is_installed("openai-codex"));
    }

    #[test]
    fn choices_that_keep_changing_stop_after_three_passes() {
        let system = ready_system().with_network().with_command_writes(
            "pacman -Qq -- chromium",
            SETUP_CONFIG_PATH,
            "(browser: Chromium)",
        );
        assert_eq!(run_until_settled(&system).expect("runs"), summary(3, false));
    }
}
