//! `--autoinstall <config>`: the ISO flow with every answer given up front and
//! no window. The ISO's QEMU install test drives it, handing the config in
//! through fw_cfg, which only a virtual machine has.
//!
//! The target is the one the disk page would open on — the largest free region
//! on a GPT disk with an EFI system partition — and nothing is edited.

use std::path::Path;

use anyhow::{Context as _, Result, anyhow};
use serde::Deserialize;

use crate::engine::exec::{Event, Mode, Secrets, execute};
use crate::engine::plan::{Account, IsoInput, Plan, Step, plan_iso};
use crate::engine::probe::RealSystem;
use crate::oobe::{Locale, RootFilesystem, SwapPlan};
use crate::ui;

/// The line a successful run ends with, which the install test waits for.
pub const COMPLETE: &str = "crownos-installer: install complete";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub username: String,
    pub password: String,
    pub hostname: String,
    #[serde(default)]
    pub filesystem: RootFilesystem,
    /// Appended to the installed system's kernel command line.
    #[serde(default)]
    pub kernel_args: String,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_slice(&raw).with_context(|| format!("parsing {}", path.display()))
    }
}

pub fn run(context: &ui::Context, config: &Config, mode: Mode) -> Result<()> {
    let mut plan = plan_for(context, config)?;
    append_kernel_args(&mut plan, &config.kernel_args);
    let secrets = Secrets {
        user_password: config.password.clone(),
    };
    execute(&plan, &secrets, mode, &RealSystem, &mut report)?;
    println!("{COMPLETE}");
    Ok(())
}

fn plan_for(context: &ui::Context, config: &Config) -> Result<Plan> {
    let target = ui::Target::best_for(&context.snapshot);
    let (Some(disk), Some(esp), Some(region_start_sector)) =
        (target.disk, target.esp, target.region_start_sector)
    else {
        return Err(anyhow!(
            "no GPT disk has both an EFI system partition and free space"
        ));
    };
    plan_iso(&IsoInput {
        snapshot: &context.snapshot,
        disk,
        esp,
        region_start_sector,
        root_filesystem: config.filesystem,
        swap: SwapPlan::None,
        locale: Locale::default(),
        account: Account {
            username: config.username.clone(),
            full_name: None,
            hostname: config.hostname.clone(),
            autologin: false,
        },
        ids: context.ids.clone(),
        edits: Vec::new(),
    })
    .context("the install was refused")
}

fn append_kernel_args(plan: &mut Plan, extra: &str) {
    if extra.is_empty() {
        return;
    }
    for step in &mut plan.steps {
        if let Step::InstallBootloader { kernel_cmdline, .. } = step {
            kernel_cmdline.push(' ');
            kernel_cmdline.push_str(extra);
        }
    }
}

fn report(event: Event) {
    match event {
        Event::StepStarted {
            index,
            total,
            label,
        } => println!("[{}/{total}] {label}", index + 1),
        Event::Log(line) => println!("{line}"),
        Event::Failed { message, .. } => eprintln!("crownos-installer: {message}"),
        Event::StepFinished { .. } | Event::Finished { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_config_needs_only_the_account() {
        let config: Config =
            serde_json::from_str(r#"{"username":"crown","password":"pw","hostname":"box"}"#)
                .expect("a minimal config parses");
        assert_eq!(config.filesystem, RootFilesystem::Ext4);
        assert!(config.kernel_args.is_empty());
    }

    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() {
        let parsed = serde_json::from_str::<Config>(
            r#"{"username":"crown","password":"pw","hostname":"box","disk":"/dev/sda"}"#,
        );
        assert!(parsed.is_err());
    }
}
