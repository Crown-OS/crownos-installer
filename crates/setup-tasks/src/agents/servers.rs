use crate::context::System;

/// An MCP server that runs as a child process and speaks over stdio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct McpServer {
    pub name: &'static str,
    pub command: &'static str,
    pub arguments: &'static [&'static str],
}

/// The servers that each coding agent gets when their command is installed.
pub const SERVERS: [McpServer; 3] = [
    McpServer {
        name: "crownos-computer-use",
        command: "crownos-computer-use",
        arguments: &["mcp"],
    },
    McpServer {
        name: "playwright",
        command: "crownos-setup",
        arguments: &["playwright-mcp"],
    },
    McpServer {
        name: "crownos-secrets",
        command: "crownos-secrets-mcp",
        arguments: &[],
    },
];

/// The [`SERVERS`] whose command is on `PATH`. An agent gets no entry for a
/// server that cannot start.
pub fn installed_servers(system: &dyn System) -> impl Iterator<Item = &'static McpServer> {
    SERVERS
        .iter()
        .filter(|server| system.find_program(server.command).is_some())
}

#[cfg(test)]
pub(crate) fn with_every_server_command(
    system: crate::context::fake::FakeSystem,
) -> crate::context::fake::FakeSystem {
    SERVERS
        .iter()
        .fold(system, |system, server| system.with_program(server.command))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::fake::FakeSystem;

    #[test]
    fn only_servers_with_an_installed_command_are_offered() {
        let system = FakeSystem::new().with_program("crownos-secrets-mcp");
        let names: Vec<&str> = installed_servers(&system)
            .map(|server| server.name)
            .collect();
        assert_eq!(names, ["crownos-secrets"]);
        assert_eq!(installed_servers(&FakeSystem::new()).count(), 0);
    }
}
