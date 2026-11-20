//! `/etc/crownos/setup.ron`: the browser and developer tools the user chose.
//! The installer only records the choices. The `crownos-setup` runner on the
//! installed system downloads and sets them up once the system is online.

use crownos_config::system::{self, SETUP_CONFIG_PATH, SetupConfig};

use crate::oobe::{self, Install};

use super::exec::Cmd;

/// The package that ships the runner.
pub const PACKAGE: &str = "crownos-setup";

/// The runner, enabled only when the live image ships [`PACKAGE`].
pub const SERVICE: &str = "crownos-setup.service";

impl From<oobe::Browser> for system::Browser {
    fn from(browser: oobe::Browser) -> Self {
        match browser {
            oobe::Browser::Brave => Self::Brave,
            oobe::Browser::Chrome => Self::Chrome,
            oobe::Browser::Chromium => Self::Chromium,
            oobe::Browser::Firefox => Self::Firefox,
        }
    }
}

impl From<oobe::DeveloperTools> for system::DeveloperTools {
    fn from(tools: oobe::DeveloperTools) -> Self {
        Self {
            claude_code: tools.claude_code,
            codex: tools.codex,
            cursor: tools.cursor,
        }
    }
}

impl From<system::DeveloperTools> for oobe::DeveloperTools {
    fn from(tools: system::DeveloperTools) -> Self {
        Self {
            claude_code: tools.claude_code,
            codex: tools.codex,
            cursor: tools.cursor,
        }
    }
}

/// The choices of `install`, in the shape the installed system reads.
pub fn setup_config(install: &Install) -> SetupConfig {
    SetupConfig {
        browser: install.browser.into(),
        developer_tools: install.developer_tools.into(),
    }
}

/// Writes `config` into the system mounted at `root`.
pub fn commands(root: &str, config: &SetupConfig) -> Vec<Cmd> {
    vec![Cmd::WriteFile {
        path: format!("{root}{SETUP_CONFIG_PATH}"),
        contents: config.render(),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choices_are_written_into_the_new_system() {
        let config = SetupConfig {
            browser: system::Browser::Brave,
            ..SetupConfig::default()
        };
        let written = commands("/mnt", &config);
        let [Cmd::WriteFile { path, contents }] = written.as_slice() else {
            panic!("one file is written");
        };
        assert_eq!(path, "/mnt/etc/crownos/setup.ron");
        assert!(contents.contains("browser: Brave"), "{contents}");
    }

    #[test]
    fn every_payload_choice_reaches_the_config() {
        let developer_tools = oobe::DeveloperTools {
            cursor: true,
            ..oobe::DeveloperTools::default()
        };
        let config = SetupConfig {
            browser: oobe::Browser::Chrome.into(),
            developer_tools: developer_tools.into(),
        };
        assert_eq!(config.browser, system::Browser::Chrome);
        assert!(config.developer_tools.cursor);
        assert!(!config.developer_tools.claude_code && !config.developer_tools.codex);
    }

    #[test]
    fn developer_tools_convert_back_unchanged() {
        for tool in system::DeveloperTool::ALL {
            let mut tools = system::DeveloperTools::default();
            tools.set(tool, true);
            let payload = oobe::DeveloperTools::from(tools);
            assert_eq!(system::DeveloperTools::from(payload), tools, "{tool}");
        }
    }
}
