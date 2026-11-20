//! `crownos-setup`: the post-install runner of CrownOS. `crownos-setup.service`
//! starts `crownos-setup run` on each boot. Settings changes the choices
//! through `pkexec crownos-setup browser set` and `tools set`. At each login,
//! `crownos-setup user` connects the coding agents of the user. The agents
//! start the Playwright MCP server through `crownos-setup playwright-mcp`.

mod cli;
mod commands;
mod utils;

use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::Parser as _;
use crownos_setup_tasks::RealSystem;
use tracing_subscriber::EnvFilter;

use cli::{BrowserAction, Cli, Command, ToolsAction};
use utils::privileges::{refuse_root, require_root};

fn main() -> Result<ExitCode> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .without_time()
        .init();

    let cli = Cli::parse();
    if std::env::var_os("PKEXEC_UID").is_some() && !cli.command.is_allowed_through_pkexec() {
        bail!("this command is not available through pkexec");
    }

    let system = RealSystem;
    match cli.command {
        Command::Run => {
            require_root()?;
            commands::run::run(&system)
        }
        Command::Status { json } => {
            commands::status::print_status(json).map(|()| ExitCode::SUCCESS)
        }
        Command::Browser {
            action: BrowserAction::Set { browser },
        } => {
            require_root()?;
            commands::set::set_browser(&system, browser).map(|()| ExitCode::SUCCESS)
        }
        Command::Tools {
            action: ToolsAction::Set { tool, state },
        } => {
            require_root()?;
            commands::set::set_developer_tool(&system, tool, state.is_on())
                .map(|()| ExitCode::SUCCESS)
        }
        Command::User { wait } => {
            refuse_root()?;
            commands::user::set_up_user(&system, wait).map(|()| ExitCode::SUCCESS)
        }
        Command::PlaywrightMcp { arguments } => commands::playwright_mcp::start_server(&arguments),
    }
}
