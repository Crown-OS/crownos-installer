/// An MCP server that runs as a child process and speaks over stdio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct McpServer {
    pub name: &'static str,
    pub command: &'static str,
    pub arguments: &'static [&'static str],
}

/// The servers that each coding agent gets.
pub const SERVERS: [McpServer; 2] = [
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
];
