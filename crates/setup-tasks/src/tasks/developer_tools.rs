//! Installs the developer tools that the user turned on. This task never
//! removes a tool: `crownos-setup tools set <tool> off` does that.

use crownos_config::system::{SetupConfig, SetupTask};

use super::{Context, SetupTaskRunner};
use crate::error::TaskError;
use crate::utils::pacman;

pub struct DeveloperToolsTask;

fn missing_packages(context: &Context) -> Vec<&'static str> {
    context
        .config
        .developer_tools
        .enabled()
        .map(|tool| tool.package())
        .filter(|package| !pacman::is_installed(context.system, package))
        .collect()
}

impl SetupTaskRunner for DeveloperToolsTask {
    fn id(&self) -> SetupTask {
        SetupTask::DeveloperTools
    }

    fn wanted(&self, config: &SetupConfig) -> bool {
        config.developer_tools.any()
    }

    fn needs_network(&self, context: &Context) -> bool {
        !missing_packages(context).is_empty()
    }

    fn is_done(&self, context: &Context) -> Result<bool, TaskError> {
        Ok(missing_packages(context).is_empty())
    }

    fn run(&self, context: &Context) -> Result<(), TaskError> {
        let missing = missing_packages(context);
        if missing.is_empty() {
            return Ok(());
        }
        pacman::install(context.system, &missing)
    }
}
