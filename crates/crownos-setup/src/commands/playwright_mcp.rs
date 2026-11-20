//! `crownos-setup playwright-mcp`: the coding agents start the Playwright MCP
//! server through this command, so that the server drives the chosen browser,
//! or Chromium while the chosen browser downloads.

use std::ffi::OsString;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Command, ExitCode};

use anyhow::{Context as _, Result};
use crownos_config::system::SetupConfig;
use crownos_setup_tasks::playwright_mcp::invocation;
use crownos_setup_tasks::utils::browser_fallback::resolve_browser;

const NOT_INSTALLED_MESSAGE: &str = "Playwright is not installed yet. Run `crownos-setup status` or open Settings > Developer Tools.";

/// Replaces this process with the server. Returns only when the server
/// cannot start.
pub fn start_server(extra_arguments: &[OsString]) -> Result<ExitCode> {
    let setup_config = SetupConfig::load()
        .unwrap_or_else(|error| {
            tracing::warn!("{error}; using the default browser");
            None
        })
        .unwrap_or_default();
    let browser = resolve_browser(setup_config.browser, Path::exists);
    let invocation = invocation(browser, extra_arguments);
    if !invocation.program.exists() {
        eprintln!("{NOT_INSTALLED_MESSAGE}");
        return Ok(ExitCode::FAILURE);
    }
    let mut command = Command::new(&invocation.program);
    command.args(&invocation.arguments);
    for (name, value) in &invocation.environment {
        command.env(name, value);
    }
    Err(command.exec()).with_context(|| format!("could not start {}", invocation.program.display()))
}
