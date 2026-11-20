//! Installs the chosen web browser and makes it the default for each new
//! account. The defaults change only after the install succeeded, so that the
//! dock never points at a missing browser.

use std::path::Path;

use crownos_config::system::{PackageSource, SetupConfig, SetupTask};

use super::{Context, SetupTaskRunner, aur};
use crate::error::TaskError;
use crate::utils::{dock, mimeapps, pacman};

const MIMEAPPS_PATH: &str = "/etc/xdg/mimeapps.list";
const MIMEAPPS_DIRECTORY: &str = "/etc/xdg";
const SKEL_DOCK_PATH: &str = "/etc/skel/.config/crownos/dock.ron";
const SYSTEM_FILE_MODE: u32 = 0o644;
const SYSTEM_DIRECTORY_MODE: u32 = 0o755;

pub struct BrowserTask;

impl SetupTaskRunner for BrowserTask {
    fn id(&self) -> SetupTask {
        SetupTask::Browser
    }

    fn wanted(&self, _config: &SetupConfig) -> bool {
        true
    }

    fn needs_network(&self, context: &Context) -> bool {
        !pacman::is_installed(context.system, context.config.browser.package())
    }

    fn is_done(&self, context: &Context) -> Result<bool, TaskError> {
        let browser = context.config.browser;
        let system = context.system;
        if !pacman::is_installed(system, browser.package()) {
            return Ok(false);
        }
        let mimeapps_text = system
            .read_file(Path::new(MIMEAPPS_PATH))?
            .unwrap_or_default();
        if !mimeapps::has_defaults(&mimeapps_text, &mimeapps::defaults_for(browser)) {
            return Ok(false);
        }
        Ok(system
            .read_file(Path::new(SKEL_DOCK_PATH))?
            .is_none_or(|dock_text| dock::replace_browser(&dock_text, browser).is_none()))
    }

    fn run(&self, context: &Context) -> Result<(), TaskError> {
        let browser = context.config.browser;
        let system = context.system;
        let package = browser.package();
        if !pacman::is_installed(system, package) {
            match browser.source() {
                PackageSource::Repository => pacman::install(system, &[package])?,
                PackageSource::Aur => aur::install(context, package)?,
            }
        }

        let mimeapps_path = Path::new(MIMEAPPS_PATH);
        let mimeapps_text = system.read_file(mimeapps_path)?.unwrap_or_default();
        let defaults = mimeapps::defaults_for(browser);
        if !mimeapps::has_defaults(&mimeapps_text, &defaults) {
            system.create_dir_all(Path::new(MIMEAPPS_DIRECTORY), SYSTEM_DIRECTORY_MODE)?;
            system.write_file(
                mimeapps_path,
                &mimeapps::set_defaults(&mimeapps_text, &defaults),
                SYSTEM_FILE_MODE,
            )?;
        }

        let dock_path = Path::new(SKEL_DOCK_PATH);
        if let Some(dock_text) = system.read_file(dock_path)?
            && let Some(edited_dock) = dock::replace_browser(&dock_text, browser)
        {
            system.write_file(dock_path, &edited_dock, SYSTEM_FILE_MODE)?;
        }
        Ok(())
    }
}
