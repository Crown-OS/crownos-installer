//! Installs the CrownOS packages that the ISO does not carry and that the
//! install could not download, as listed in
//! [`ONLINE_PACKAGES_PATH`](crownos_config::system::ONLINE_PACKAGES_PATH).

use std::path::Path;

use crownos_config::system::{ONLINE_PACKAGES_PATH, SetupConfig, SetupTask};

use super::{Context, SetupTaskRunner};
use crate::error::TaskError;
use crate::utils::pacman;

pub struct OnlinePackagesTask;

fn missing_packages(context: &Context) -> Result<Vec<String>, TaskError> {
    let listed = context
        .system
        .read_file(Path::new(ONLINE_PACKAGES_PATH))?
        .unwrap_or_default();
    Ok(listed
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty() && !name.starts_with('-'))
        .filter(|name| !pacman::is_installed(context.system, name))
        .map(str::to_owned)
        .collect())
}

impl SetupTaskRunner for OnlinePackagesTask {
    fn id(&self) -> SetupTask {
        SetupTask::OnlinePackages
    }

    fn wanted(&self, _config: &SetupConfig) -> bool {
        true
    }

    fn user_session_waits(&self) -> bool {
        false
    }

    fn needs_network(&self, context: &Context) -> bool {
        missing_packages(context).is_ok_and(|missing| !missing.is_empty())
    }

    fn is_done(&self, context: &Context) -> Result<bool, TaskError> {
        Ok(missing_packages(context)?.is_empty())
    }

    fn run(&self, context: &Context) -> Result<(), TaskError> {
        let missing = missing_packages(context)?;
        if missing.is_empty() {
            return Ok(());
        }
        let names: Vec<&str> = missing.iter().map(String::as_str).collect();
        pacman::install(context.system, &names)
    }
}

#[cfg(test)]
mod tests {
    use crownos_config::system::SetupConfig;

    use super::*;
    use crate::context::fake::FakeSystem;

    #[test]
    fn only_the_listed_packages_that_are_missing_are_installed() {
        let system = FakeSystem::new()
            .with_file(ONLINE_PACKAGES_PATH, "crownos-voice\ncrownos-ai-cpu\n")
            .with_installed(&["crownos-voice"]);
        let context = Context::new(SetupConfig::default(), &system);

        assert!(!OnlinePackagesTask.is_done(&context).expect("readable"));
        OnlinePackagesTask.run(&context).expect("installs");
        assert!(
            system
                .commands()
                .iter()
                .any(|command| command == "pacman -Syu --needed --noconfirm -- crownos-ai-cpu"),
            "{:?}",
            system.commands()
        );
    }

    #[test]
    fn a_system_without_the_list_has_nothing_to_do() {
        let system = FakeSystem::new();
        let context = Context::new(SetupConfig::default(), &system);
        assert!(OnlinePackagesTask.is_done(&context).expect("readable"));
        assert!(!OnlinePackagesTask.needs_network(&context));
    }
}
