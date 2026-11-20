use std::ffi::OsString;

use clap::{Parser, Subcommand, ValueEnum};
use crownos_config::system::{Browser, DeveloperTool};

#[derive(Debug, Parser)]
#[command(version, about = "Installs the post-install choices of CrownOS")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Do each chosen task that is not done. Needs root.
    Run,
    /// Show the choices and the state of each task.
    Status {
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Change the web browser.
    Browser {
        #[command(subcommand)]
        action: BrowserAction,
    },
    /// Turn a developer tool on or off.
    Tools {
        #[command(subcommand)]
        action: ToolsAction,
    },
    /// Connect the coding agents of this account to CrownOS and point the dock
    /// at the chosen browser. Runs as the user, not as root.
    User {
        /// Wait until the setup is complete, then show a notification.
        #[arg(long)]
        wait: bool,
    },
    /// Start the Playwright MCP server for the chosen browser. Passes each
    /// argument to the server.
    #[command(disable_help_flag = true)]
    PlaywrightMcp {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        arguments: Vec<OsString>,
    },
}

#[derive(Debug, Subcommand)]
pub enum BrowserAction {
    /// Choose the web browser: brave, chrome, chromium or firefox. Needs root.
    Set {
        #[arg(value_name = "ID")]
        browser: Browser,
    },
}

#[derive(Debug, Subcommand)]
pub enum ToolsAction {
    /// Turn claude-code, codex or cursor on or off. Needs root.
    Set {
        #[arg(value_name = "ID")]
        tool: DeveloperTool,
        state: OnOff,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum OnOff {
    On,
    Off,
}

impl OnOff {
    pub fn is_on(self) -> bool {
        matches!(self, Self::On)
    }
}

impl Command {
    /// Polkit lets Settings run only these commands as root.
    pub fn is_allowed_through_pkexec(&self) -> bool {
        matches!(self, Self::Browser { .. } | Self::Tools { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("crownos-setup").chain(args.iter().copied()))
    }

    #[test]
    fn only_the_set_commands_are_allowed_through_pkexec() {
        let allowed = |args: &[&str]| {
            parse(args)
                .expect("parses")
                .command
                .is_allowed_through_pkexec()
        };
        assert!(allowed(&["browser", "set", "firefox"]));
        assert!(allowed(&["tools", "set", "codex", "off"]));
        assert!(!allowed(&["run"]));
        assert!(!allowed(&["status", "--json"]));
        assert!(!allowed(&["user", "--wait"]));
        assert!(!allowed(&["playwright-mcp", "--headless"]));
    }

    #[test]
    fn playwright_mcp_passes_each_argument_to_the_server() {
        let Command::PlaywrightMcp { arguments } =
            parse(&["playwright-mcp", "--help", "--port", "0"])
                .expect("parses")
                .command
        else {
            panic!("not a playwright-mcp command");
        };
        assert_eq!(arguments, ["--help", "--port", "0"]);
    }

    #[test]
    fn unknown_ids_are_refused() {
        assert!(parse(&["browser", "set", "edge"]).is_err());
        assert!(parse(&["tools", "set", "vim", "on"]).is_err());
        assert!(parse(&["tools", "set", "codex", "maybe"]).is_err());
    }

    #[test]
    fn the_ids_parse_to_the_config_types() {
        let Command::Tools {
            action: ToolsAction::Set { tool, state },
        } = parse(&["tools", "set", "claude-code", "on"])
            .expect("parses")
            .command
        else {
            panic!("not a tools command");
        };
        assert_eq!(tool, DeveloperTool::ClaudeCode);
        assert!(state.is_on());
    }
}
