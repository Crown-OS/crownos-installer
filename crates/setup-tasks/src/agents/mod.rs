//! The coding agents on the account of the user, and the files that connect
//! them to the CrownOS skills and MCP servers.

pub mod antigravity;
pub mod codex;
pub mod cursor;
pub mod gemini_cli;
mod json_config;
pub mod playwright_cli;
pub mod servers;
pub mod skills;
pub mod zed;

use std::path::{Path, PathBuf};

use crate::context::System;
use crate::error::TaskError;
use crate::utils::json_merge::Merge;
use crate::utils::user_directories::UserDirectories;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agent {
    Cursor,
    Codex,
    Zed,
    GeminiCli,
    Antigravity,
}

impl Agent {
    pub const ALL: [Agent; 5] = [
        Self::Cursor,
        Self::Codex,
        Self::Zed,
        Self::GeminiCli,
        Self::Antigravity,
    ];

    fn programs(self) -> &'static [&'static str] {
        match self {
            Self::Cursor => &["cursor"],
            Self::Codex => &["codex"],
            Self::Zed => &["zed", "zeditor"],
            Self::GeminiCli => &["gemini"],
            Self::Antigravity => &["antigravity", "agy"],
        }
    }

    /// The path that shows that the agent was used on this account.
    fn trace_path(self, directories: &UserDirectories) -> PathBuf {
        match self {
            Self::Cursor => directories.home.join(".cursor"),
            Self::Codex => directories.home.join(".codex"),
            Self::Zed => directories.config_home.join("zed"),
            Self::GeminiCli => directories.home.join(".gemini/settings.json"),
            Self::Antigravity => directories.home.join(".gemini/config"),
        }
    }

    /// The file that lists the MCP servers of the agent.
    pub fn configuration_file(self, directories: &UserDirectories) -> PathBuf {
        match self {
            Self::Cursor => directories.home.join(".cursor/mcp.json"),
            Self::Codex => directories.home.join(".codex/config.toml"),
            Self::Zed => directories.config_home.join("zed/settings.json"),
            Self::GeminiCli => directories.home.join(".gemini/settings.json"),
            Self::Antigravity => directories.home.join(".gemini/config/mcp_config.json"),
        }
    }

    /// The folder in the home directory that the agent reads skills from.
    /// Zed has no skills.
    pub fn skill_folder(self, directories: &UserDirectories) -> Option<PathBuf> {
        match self {
            Self::Cursor | Self::Codex | Self::GeminiCli => {
                Some(directories.home.join(".agents/skills"))
            }
            Self::Antigravity => Some(directories.home.join(".gemini/config/skills")),
            Self::Zed => None,
        }
    }

    /// True when a program of the agent is on `PATH` or the agent left its
    /// configuration in the home directory.
    pub fn is_present(self, system: &dyn System, directories: &UserDirectories) -> bool {
        self.programs()
            .iter()
            .any(|program| system.find_program(program).is_some())
            || system.exists(&self.trace_path(directories))
    }

    /// Adds the CrownOS MCP servers to `path`. `None` when the merge does not
    /// edit the file: a symbolic link, or a format that the merge cannot
    /// keep. The file then stays as it is.
    pub fn merge_servers(
        self,
        system: &dyn System,
        path: &Path,
    ) -> Result<Option<Merge<'static>>, TaskError> {
        if system.read_link(path).is_some() {
            return Ok(None);
        }
        match self {
            Self::Cursor => cursor::merge_servers(system, path),
            Self::Codex => codex::merge_servers(system, path),
            Self::Zed => zed::merge_servers(system, path),
            Self::GeminiCli => gemini_cli::merge_servers(system, path),
            Self::Antigravity => antigravity::merge_servers(system, path),
        }
    }
}

pub fn present(system: &dyn System, directories: &UserDirectories) -> Vec<Agent> {
    Agent::ALL
        .into_iter()
        .filter(|agent| agent.is_present(system, directories))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::fake::FakeSystem;

    fn directories() -> UserDirectories {
        UserDirectories::new("/home/ada".into(), None, None)
    }

    #[test]
    fn an_empty_account_has_no_agents() {
        assert!(present(&FakeSystem::new(), &directories()).is_empty());
    }

    #[test]
    fn a_program_on_the_path_makes_an_agent_present() {
        let system = FakeSystem::new().with_program("codex").with_program("agy");
        assert_eq!(
            present(&system, &directories()),
            [Agent::Codex, Agent::Antigravity]
        );
    }

    #[test]
    fn a_configuration_in_the_home_directory_makes_an_agent_present() {
        let system = FakeSystem::new()
            .with_directory("/home/ada/.cursor")
            .with_directory("/home/ada/.config/zed")
            .with_file("/home/ada/.gemini/settings.json", "{}");
        assert_eq!(
            present(&system, &directories()),
            [Agent::Cursor, Agent::Zed, Agent::GeminiCli]
        );
    }

    #[test]
    fn the_configuration_files_and_skill_folders() {
        let directories = directories();
        assert_eq!(
            Agent::ALL.map(|agent| agent.configuration_file(&directories)),
            [
                "/home/ada/.cursor/mcp.json",
                "/home/ada/.codex/config.toml",
                "/home/ada/.config/zed/settings.json",
                "/home/ada/.gemini/settings.json",
                "/home/ada/.gemini/config/mcp_config.json",
            ]
            .map(PathBuf::from)
        );
        assert_eq!(
            Agent::ALL.map(|agent| agent.skill_folder(&directories)),
            [
                Some("/home/ada/.agents/skills"),
                Some("/home/ada/.agents/skills"),
                None,
                Some("/home/ada/.agents/skills"),
                Some("/home/ada/.gemini/config/skills"),
            ]
            .map(|folder| folder.map(PathBuf::from))
        );
    }

    #[test]
    fn a_configuration_file_that_is_a_symbolic_link_is_skipped() {
        let path = "/home/ada/.cursor/mcp.json";
        let system = FakeSystem::new().with_link(path, "/home/ada/dotfiles/mcp.json");
        assert_eq!(
            Agent::Cursor
                .merge_servers(&system, Path::new(path))
                .expect("reads"),
            None
        );
        assert!(system.journal().is_empty());
    }
}
