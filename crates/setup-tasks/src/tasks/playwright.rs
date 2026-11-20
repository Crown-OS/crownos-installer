//! Installs Node.js and the Playwright command line and MCP server under
//! `/usr/lib/crownos/playwright`. For Firefox, it also downloads the Firefox
//! build of Playwright, because Playwright cannot drive the Firefox package.

use std::path::{Path, PathBuf};

use crownos_config::system::{
    Browser, PLAYWRIGHT_BROWSERS_PATH, PLAYWRIGHT_PREFIX, SetupConfig, SetupTask,
};

use super::{Context, SetupTaskRunner};
use crate::context::System;
use crate::error::TaskError;
use crate::utils::pacman;

/// The `@playwright/cli` release that the runner installs.
pub const PLAYWRIGHT_CLI_VERSION: &str = "0.1.25";
/// The `@playwright/mcp` release that the runner installs.
pub const PLAYWRIGHT_MCP_VERSION: &str = "0.0.86";
/// The server binary of [`PLAYWRIGHT_MCP_VERSION`]. Releases after 0.0.55 do
/// not have `mcp-server-playwright`.
pub const PLAYWRIGHT_MCP_SERVER_BINARY: &str = "playwright-mcp";
const PLAYWRIGHT_CLI_BINARY: &str = "playwright-cli";

const NODE_PACKAGES: [&str; 2] = ["nodejs", "npm"];
const NODE_EXECUTABLES: [&str; 2] = ["/usr/bin/node", "/usr/bin/npm"];
const CLI_LINK_DIRECTORY: &str = "/usr/local/bin";
const CLI_LINK: &str = "/usr/local/bin/playwright-cli";
const NPM_CACHE: &str = "/var/lib/crownos/setup/npm-cache";
const FIREFOX_BUILD_PREFIX: &str = "firefox-";
const SYSTEM_DIRECTORY_MODE: u32 = 0o755;

pub struct PlaywrightTask;

fn prefix_binary(name: &str) -> PathBuf {
    Path::new(PLAYWRIGHT_PREFIX)
        .join("node_modules/.bin")
        .join(name)
}

fn has_node(system: &dyn System) -> bool {
    NODE_EXECUTABLES
        .iter()
        .all(|executable| system.exists(Path::new(executable)))
}

fn has_playwright_packages(system: &dyn System) -> bool {
    system.exists(&prefix_binary(PLAYWRIGHT_MCP_SERVER_BINARY))
        && system.exists(&prefix_binary(PLAYWRIGHT_CLI_BINARY))
}

fn needs_firefox_build(context: &Context) -> Result<bool, TaskError> {
    if context.config.browser != Browser::Firefox {
        return Ok(false);
    }
    let builds = context
        .system
        .list_dir(Path::new(PLAYWRIGHT_BROWSERS_PATH))?;
    Ok(!builds.iter().any(|build| {
        build
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(FIREFOX_BUILD_PREFIX))
    }))
}

fn has_cli_link(system: &dyn System) -> bool {
    system.read_link(Path::new(CLI_LINK)) == Some(prefix_binary(PLAYWRIGHT_CLI_BINARY))
}

impl SetupTaskRunner for PlaywrightTask {
    fn id(&self) -> SetupTask {
        SetupTask::Playwright
    }

    fn wanted(&self, config: &SetupConfig) -> bool {
        config.wants_playwright()
    }

    fn needs_network(&self, context: &Context) -> bool {
        !has_node(context.system)
            || !has_playwright_packages(context.system)
            || needs_firefox_build(context).unwrap_or(true)
    }

    fn is_done(&self, context: &Context) -> Result<bool, TaskError> {
        let system = context.system;
        Ok(has_node(system)
            && has_playwright_packages(system)
            && has_cli_link(system)
            && !needs_firefox_build(context)?)
    }

    fn run(&self, context: &Context) -> Result<(), TaskError> {
        let system = context.system;
        if !has_node(system) {
            pacman::install(system, &NODE_PACKAGES)?;
        }

        let cli = prefix_binary(PLAYWRIGHT_CLI_BINARY);
        if !has_playwright_packages(system) {
            let cli_package = format!("@playwright/cli@{PLAYWRIGHT_CLI_VERSION}");
            let mcp_package = format!("@playwright/mcp@{PLAYWRIGHT_MCP_VERSION}");
            system
                .run(
                    "npm",
                    &[
                        "install",
                        "--prefix",
                        PLAYWRIGHT_PREFIX,
                        "--cache",
                        NPM_CACHE,
                        "--no-fund",
                        "--no-audit",
                        &cli_package,
                        &mcp_package,
                    ],
                )?
                .check("npm install")?;
        }

        if !has_cli_link(system) {
            system.create_dir_all(Path::new(CLI_LINK_DIRECTORY), SYSTEM_DIRECTORY_MODE)?;
            system.remove_file(Path::new(CLI_LINK))?;
            system.symlink(&cli, Path::new(CLI_LINK))?;
        }

        if needs_firefox_build(context)? {
            let browsers_path = format!("PLAYWRIGHT_BROWSERS_PATH={PLAYWRIGHT_BROWSERS_PATH}");
            system
                .run(
                    "env",
                    &[
                        &browsers_path,
                        &cli.to_string_lossy(),
                        "install-browser",
                        "firefox",
                    ],
                )?
                .check("playwright-cli install-browser firefox")?;
        }
        Ok(())
    }
}
