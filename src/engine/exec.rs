//! Running a plan — or, in [`Mode::DryRun`], describing exactly what running it
//! would do.
//!
//! Every [`Step`] expands to a short list of [`Cmd`]s, and a `Cmd` is the only
//! thing in this crate that can change the machine. That indirection buys three
//! things worth the extra type:
//!
//! * **A dry run is a rehearsal, not an approximation.** The same function
//!   builds the command list in both modes; the mode only decides whether the
//!   list is executed or printed. There is no second code path that might drift.
//! * **The log is the command list.** Each `Cmd` renders as something a person
//!   could paste into a shell, so an install that failed at step 14 tells you
//!   what step 14 actually did.
//! * **Secrets stay out of argument vectors.** [`Cmd::RunWithSecretStdin`] is the
//!   only way a password reaches a subprocess, and its [`fmt::Display`] renders
//!   the stdin as `<password>`. A password on a command line is visible in
//!   `/proc`; a password on stdin is not.
//!
//! # Progress
//!
//! [`execute`] reports through an [`Event`] callback rather than returning at the
//! end, because the UI needs the log line *while* `pacstrap` is running. The
//! callback is called from whatever thread `execute` runs on — which in the app
//! is a worker, never the UI thread.
//!
//! # What this module is not allowed to do
//!
//! Decide anything. Every device node, offset, package and file body was fixed by
//! [`plan`](super::plan). The exceptions are marked in the code and exist
//! because whether a file is present is not knowable without touching the
//! disk: two "leave this alone if it is already there" checks against the
//! shared ESP, and [`Cmd::PresetUnits`], which reads the preset file that
//! `pacstrap` put into the new system.

use std::borrow::Cow;
use std::fmt;

use anyhow::{Context as _, Result};

use crate::util::{fstab, preset};

use super::base;
use super::boot;
use super::btrfs;
use super::disks::{self, Filesystem};
use super::online;
use super::plan::{BOOT_FSLABEL, Plan, Role, Step};
use super::probe::System;
use super::setup;

// --- MARK: Secrets ---

/// The credentials a plan needs but does not carry.
///
/// Separate from [`Plan`] so that a plan can be logged, printed on the
/// confirmation page and compared in a test without any of that leaking a
/// password. See the [`plan`](super::plan) module docs.
#[derive(Clone)]
pub struct Secrets {
    /// The password for the account the plan creates.
    pub user_password: String,
}

impl fmt::Debug for Secrets {
    /// Redacted, because the one thing worse than a password in a log is a
    /// password in a log nobody expected to be there.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secrets")
            .field("user_password", &"<redacted>")
            .finish()
    }
}

// --- MARK: Mode ---

/// Whether to actually do it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Build every command and log it, touching nothing.
    ///
    /// This is what `crownos-installer --dry-run` does, and it is how the
    /// install is exercised on a developer's machine.
    DryRun,
    /// Run it.
    Commit,
}

impl Mode {
    pub fn is_dry_run(self) -> bool {
        matches!(self, Self::DryRun)
    }
}

// --- MARK: Events ---

/// What the executor reports as it goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A step is about to run. `index` is zero-based.
    StepStarted {
        index: usize,
        total: usize,
        label: String,
    },
    /// One line for the log — a command about to run, or its output.
    Log(String),
    /// A step finished successfully.
    StepFinished { index: usize },
    /// A step failed. The install stops here.
    Failed { index: usize, message: String },
    /// Every step ran.
    Finished { dry_run: bool },
}

// --- MARK: Commands ---

/// The smallest unit that can change the machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    /// Run a program in the live environment.
    Run { program: String, args: Vec<String> },
    /// Run a program inside the target root, via `arch-chroot`.
    Chroot {
        root: String,
        program: String,
        args: Vec<String>,
    },
    /// Run a program with `input` on stdin — in practice, something that
    /// enters the target root and runs `chpasswd` there.
    ///
    /// The only way a secret reaches a subprocess. `redact` is what the log
    /// shows in place of the input.
    RunWithSecretStdin {
        program: String,
        args: Vec<String>,
        input: String,
        redact: &'static str,
    },
    /// Run a program and append its standard output to a file — the shell
    /// redirect `genfstab` needs, without a shell — passed through `filter`
    /// first when there is one.
    RunAppendingTo {
        program: String,
        args: Vec<String>,
        path: String,
        filter: Option<OutputFilter>,
    },
    /// Write a file, replacing whatever was there.
    WriteFile { path: String, contents: String },
    /// Copy a file, if the source exists, and never over an existing copy.
    /// Used to take a backup we may not need.
    CopyIfPresent { from: String, to: String },
    /// `systemctl preset` inside `root` for the units of the `enable` lines of
    /// `preset`. When `root` has no `preset`, `systemctl enable` for
    /// `fallback_units`: `preset` would apply the `disable *` of systemd's own
    /// preset file. The units are then written to `record`, one on each line.
    PresetUnits {
        root: String,
        preset: String,
        fallback_units: Vec<String>,
        record: String,
    },
    /// `mkdir -p`.
    MakeDirs { path: String },
    /// Run a command whose failure is logged rather than stopping the install:
    /// a download the installed system can do without.
    BestEffort(Box<Cmd>),
}

/// An edit applied to a program's output before [`Cmd::RunAppendingTo`]
/// appends it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFilter {
    /// See [`fstab::without_root_subvolume`].
    FstabRootWithoutSubvolume,
}

impl OutputFilter {
    fn apply(self, output: &str) -> Cow<'_, str> {
        match self {
            Self::FstabRootWithoutSubvolume => fstab::without_root_subvolume(output),
        }
    }

    /// The same edit as a shell pipeline, so the log stays pasteable.
    fn as_shell(self) -> &'static str {
        match self {
            Self::FstabRootWithoutSubvolume => {
                "sed -E '/[[:space:]]\\/[[:space:]]/ s/,?subvol(id)?=[^,[:space:]]*//g'"
            }
        }
    }
}

impl fmt::Display for Cmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Run { program, args } => write!(f, "{program} {}", args.join(" ")),
            Self::Chroot {
                root,
                program,
                args,
            } => write!(f, "arch-chroot {root} {program} {}", args.join(" ")),
            Self::RunWithSecretStdin {
                program,
                args,
                redact,
                ..
            } => write!(f, "{redact} | {program} {}", args.join(" ")),
            Self::RunAppendingTo {
                program,
                args,
                path,
                filter,
            } => {
                write!(f, "{program} {}", args.join(" "))?;
                if let Some(filter) = filter {
                    write!(f, " | {}", filter.as_shell())?;
                }
                write!(f, " >> {path}")
            }
            Self::WriteFile { path, contents } => write_display(f, path, contents),
            Self::CopyIfPresent { from, to } => write!(f, "cp {from} {to} (if {from} exists)"),
            Self::PresetUnits {
                root,
                preset,
                fallback_units,
                record,
            } => write!(
                f,
                "arch-chroot {root} systemctl preset -- <the enable lines of {preset}>, \
                 or systemctl enable {} without {preset}; the same units > {root}{record}",
                fallback_units.join(" ")
            ),
            Self::MakeDirs { path } => write!(f, "mkdir -p {path}"),
            Self::BestEffort(command) => write!(f, "{command}  # allowed to fail"),
        }
    }
}

/// How a file write reads in the log.
///
/// Small files are shown in full, as a heredoc. That is the difference between a
/// dry run that only gives the byte count of `EFI/refind/refind.conf`
/// and one that shows the boot entry the machine is about to depend on. The cap
/// exists so that a future step which writes something large cannot turn the log
/// into the file.
fn write_display(f: &mut fmt::Formatter<'_>, path: &str, contents: &str) -> fmt::Result {
    const INLINE_LIMIT: usize = 1024;
    if contents.len() <= INLINE_LIMIT {
        write!(f, "cat > {path} <<'EOF'\n{}EOF", ensure_newline(contents))
    } else {
        write!(f, "cat > {path}  # {} bytes", contents.len())
    }
}

fn ensure_newline(contents: &str) -> String {
    if contents.ends_with('\n') {
        contents.to_owned()
    } else {
        format!("{contents}\n")
    }
}

impl Cmd {
    pub(super) fn run(program: &str, args: &[&str]) -> Self {
        Self::Run {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    pub(super) fn chroot(root: &str, program: &str, args: &[&str]) -> Self {
        Self::Chroot {
            root: root.to_owned(),
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    pub(super) fn best_effort(command: Self) -> Self {
        Self::BestEffort(Box::new(command))
    }

    /// Carry out the command.
    fn apply(&self, system: &dyn System) -> Result<String> {
        match self {
            Self::Run { program, args } => system.run(program, &borrow(args)),
            Self::Chroot {
                root,
                program,
                args,
            } => {
                let mut all = vec![root.as_str(), program.as_str()];
                all.extend(args.iter().map(String::as_str));
                system.run("arch-chroot", &all)
            }
            Self::RunWithSecretStdin { .. } => {
                // Handled by the executor, which owns the pipe. Reaching here
                // would mean a secret went through the generic path, so it is a
                // hard error rather than a fallthrough.
                anyhow::bail!("a command carrying a secret must be run through `run_with_stdin`")
            }
            Self::RunAppendingTo {
                program,
                args,
                path,
                filter,
            } => {
                let output = system.run(program, &borrow(args))?;
                let output = match filter {
                    Some(filter) => filter.apply(&output),
                    None => Cow::Borrowed(output.as_str()),
                };
                let existing = system.read_file(path).unwrap_or_default();
                system.write_file(path, &(existing + &output))?;
                Ok(String::new())
            }
            Self::WriteFile { path, contents } => {
                system.write_file(path, contents)?;
                Ok(String::new())
            }
            Self::CopyIfPresent { from, to } => {
                if system.exists(from) {
                    system.run("cp", &["--no-clobber", "--", from, to])
                } else {
                    Ok(format!("{from} is not there — nothing to back up"))
                }
            }
            Self::PresetUnits {
                root,
                preset,
                fallback_units,
                record,
            } => preset_units(system, root, preset, fallback_units, record),
            Self::MakeDirs { path } => system.run("mkdir", &["-p", "--", path]),
            Self::BestEffort(command) => Ok(command
                .apply(system)
                .unwrap_or_else(|error| format!("skipped: {error:#}"))),
        }
    }
}

fn borrow(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

/// The exception: whether crownos-base in `root` ships the preset file is
/// known only after `pacstrap`.
fn preset_units(
    system: &dyn System,
    root: &str,
    preset: &str,
    fallback_units: &[String],
    record: &str,
) -> Result<String> {
    let preset_path = format!("{root}{preset}");
    let preset_text = if system.exists(&preset_path) {
        Some(system.read_file(&preset_path)?)
    } else {
        None
    };
    let (operation, units): (&str, Vec<&str>) = match &preset_text {
        Some(text) => ("preset", preset::enabled_units(text).collect()),
        None => ("enable", borrow(fallback_units)),
    };
    let output = if units.is_empty() {
        format!("{preset_path} enables no unit")
    } else {
        let mut args = vec![root, "systemctl", operation, "--"];
        args.extend(&units);
        system.run("arch-chroot", &args)?
    };
    system.write_file(
        &format!("{root}{record}"),
        &preset::applied_presets_record(units),
    )?;
    Ok(output)
}

// --- MARK: Execution ---

/// Run — or rehearse — every step of `plan`.
///
/// Stops at the first failure and reports [`Event::Failed`]. There is no rollback:
/// a partially installed system is recoverable (the partitions it created are
/// visible and deletable, and Windows was never touched), whereas an automatic
/// rollback that itself fails half way is not.
pub fn execute(
    plan: &Plan,
    secrets: &Secrets,
    mode: Mode,
    system: &dyn System,
    report: &mut dyn FnMut(Event),
) -> Result<()> {
    let total = plan.steps.len();

    if mode.is_dry_run() {
        report(Event::Log(
            "Dry run: the commands below are printed, not executed.".to_owned(),
        ));
    }

    for (index, step) in plan.steps.iter().enumerate() {
        report(Event::StepStarted {
            index,
            total,
            label: step.label(),
        });

        let commands = commands_for(plan, secrets, step);
        for command in &commands {
            report(Event::Log(format!("$ {command}")));
            if mode.is_dry_run() {
                continue;
            }
            let result = match command {
                Cmd::RunWithSecretStdin {
                    program,
                    args,
                    input,
                    ..
                } => run_with_stdin(program, args, input),
                other => other.apply(system),
            };
            match result {
                Ok(output) => {
                    for line in output.lines().filter(|l| !l.trim().is_empty()) {
                        report(Event::Log(line.to_owned()));
                    }
                }
                Err(error) => {
                    let message = format!("{error:#}");
                    report(Event::Failed { index, message });
                    return Err(error).with_context(|| format!("step failed: {}", step.label()));
                }
            }
        }

        report(Event::StepFinished { index });
    }

    report(Event::Finished {
        dry_run: mode.is_dry_run(),
    });
    Ok(())
}

/// Run a program with `input` on its stdin.
///
/// Not routed through [`System`] because the trait deliberately has no
/// stdin-carrying method: adding one would make it possible to pass a secret
/// through the same call path as everything else, and the point of the split is
/// that it is not possible.
fn run_with_stdin(program: &str, args: &[String], input: &str) -> Result<String> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not run `{program}`"))?;

    child
        .stdin
        .as_mut()
        .context("stdin was not piped")?
        .write_all(input.as_bytes())
        .with_context(|| format!("could not write to `{program}`"))?;
    // Dropped so the child sees EOF; `chpasswd` waits for it.
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .with_context(|| format!("`{program}` did not finish"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    anyhow::bail!(
        "`{program}` failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

// --- MARK: Step expansion ---

/// The commands one step becomes.
///
/// Deliberately a free function of `(plan, secrets, step)` with no state of its
/// own, so a test can ask "what would this step do?" without an executor.
pub fn commands_for(plan: &Plan, secrets: &Secrets, step: &Step) -> Vec<Cmd> {
    match step {
        // Expanded by the module that owns the operation, not here. `disks` has
        // both the safety rules and the command construction for a partition
        // edit, and the ordering inside a resize — filesystem before entry when
        // shrinking, after it when growing — is the kind of thing that must live
        // in exactly one place.
        Step::EditPartitionTable(op) => disks::commands_for_op(op),

        Step::UnmountStagingMedium { target } => vec![
            // Lazy, because something in the live environment may still hold a
            // descriptor open. The partition is not written to until several
            // steps later.
            Cmd::run("umount", &["--lazy", "--recursive", target]),
        ],

        Step::CreatePartition {
            disk,
            partition,
            type_guid,
            name,
            role,
        } => {
            // `sfdisk --append` adds an entry and rewrites no existing one. That
            // property is the whole reason it is used here rather than `sgdisk
            // --new`, which happily takes a partition number that is in use.
            let script = format!(
                "start={}, size={}, type={}, uuid={}, name=\"{}\"\n",
                partition.start_sector,
                partition.sectors(),
                type_guid,
                partition.partuuid,
                name,
            );
            vec![
                Cmd::WriteFile {
                    path: sfdisk_script_path(*role),
                    contents: script,
                },
                Cmd::Run {
                    program: "sh".to_owned(),
                    args: vec![
                        "-c".to_owned(),
                        format!(
                            "sfdisk --append --force {disk} < {}",
                            sfdisk_script_path(*role)
                        ),
                    ],
                },
            ]
        }

        Step::SettleTable { disk } => vec![
            Cmd::run("partprobe", &[disk]),
            Cmd::run("udevadm", &["settle"]),
        ],

        Step::MakeFilesystem { device, label } => {
            vec![Cmd::run("mkfs.btrfs", &["-f", "-L", label, device])]
        }

        Step::CreateSubvolumes { device } => btrfs::create_subvolumes(device),

        Step::MakeEsp { device } => {
            vec![disks::mkfs(device, Filesystem::Fat32, Some(BOOT_FSLABEL))]
        }

        Step::MakeSwap { device } => vec![
            Cmd::run("mkswap", &["--label", "crownos-swap", device]),
            Cmd::run("swapon", &[device]),
        ],

        Step::MountRoot { device, at } => btrfs::mount_all(device, at),

        Step::MountEsp { device, at } => vec![
            Cmd::MakeDirs { path: at.clone() },
            // `fmask`/`dmask` because a FAT32 partition has no permissions of
            // its own and the default would make the boot loader
            // world-writable.
            Cmd::run("mount", &["-o", "fmask=0137,dmask=0027", device, at]),
        ],

        Step::InstallBaseSystem { root, packages } => vec![base::bootstrap(root, packages)],

        Step::InstallOnlinePackages { root, packages } => online::commands(root, packages),

        Step::ConfigureAiAcceleration { root, plan } => plan.commands(root),

        Step::ConfigureSetup { root, config } => setup::commands(root, config),

        Step::ConfigureGraphics { root, plan } => plan.commands(root),

        Step::ConfigureRepositories { root, microarch } => {
            vec![base::configure_repositories(root, *microarch)]
        }

        Step::AddCrownosRepository { root, channel } => base::add_crownos_repository(root, channel),

        Step::GenerateFstab { root } => vec![Cmd::RunAppendingTo {
            program: "genfstab".to_owned(),
            // `-U`: mount by filesystem UUID, so the new system does not depend
            // on device names any more than this installer does.
            args: vec!["-U".to_owned(), root.clone()],
            path: format!("{root}/etc/fstab"),
            filter: Some(OutputFilter::FstabRootWithoutSubvolume),
        }],

        Step::CreateSwapFile { root, size_bytes } => btrfs::swapfile(root, *size_bytes),

        Step::ConfigureCompressedMemory { root, percent } => {
            base::compressed_memory(root, *percent)
        }

        Step::ConfigureLocale { root, locale } => base::configure_locale(root, locale),

        Step::ConfigureKeymap { root, keymap } => vec![Cmd::WriteFile {
            path: format!("{root}/etc/vconsole.conf"),
            contents: format!("KEYMAP={keymap}\n"),
        }],

        Step::ConfigureTimezone { root, timezone } => vec![
            Cmd::chroot(
                root,
                "ln",
                &[
                    "-sf",
                    &format!("/usr/share/zoneinfo/{timezone}"),
                    "/etc/localtime",
                ],
            ),
            // `--utc`: the hardware clock is left in UTC, which is what Windows
            // is also configured for on any machine made this century, so the two
            // operating systems agree about the time.
            Cmd::chroot(root, "hwclock", &["--systohc", "--utc"]),
        ],

        Step::ConfigureHostname { root, hostname } => vec![
            Cmd::WriteFile {
                path: format!("{root}/etc/hostname"),
                contents: format!("{hostname}\n"),
            },
            Cmd::WriteFile {
                path: format!("{root}/etc/hosts"),
                contents: format!(
                    "127.0.0.1\tlocalhost\n::1\t\tlocalhost\n127.0.1.1\t{hostname}.localdomain\t{hostname}\n"
                ),
            },
        ],

        Step::InstallBootloader {
            root,
            esp_rel,
            root_partuuid,
            kernel_cmdline,
            origin,
            disk,
            esp_number,
            ..
        } => boot::install_bootloader(
            root,
            esp_rel,
            kernel_cmdline,
            root_partuuid,
            *origin,
            disk,
            *esp_number,
        ),

        Step::InstallKernel { root, esp_rel, .. } => boot::install_kernel(root, esp_rel),

        Step::PrepareRecovery { root, esp_rel, .. } => boot::prepare_recovery(root, esp_rel),

        Step::CreateUser {
            root,
            username,
            full_name,
            admin_group,
        } => {
            let mut args = vec![
                "--create-home".to_owned(),
                "--groups".to_owned(),
                admin_group.clone(),
            ];
            if let Some(full_name) = full_name {
                args.push("--comment".to_owned());
                args.push(full_name.clone());
            }
            args.push(username.clone());
            vec![
                Cmd::Chroot {
                    root: root.clone(),
                    program: "useradd".to_owned(),
                    args,
                },
                // The admin group on its own grants nothing; this is the line
                // that makes it mean something, and it is a drop-in so a package
                // update replacing `/etc/sudoers` cannot silently revoke the only
                // administrative account on the machine.
                Cmd::WriteFile {
                    path: format!("{root}/etc/sudoers.d/10-crownos-admin"),
                    contents: format!("%{admin_group} ALL=(ALL:ALL) ALL\n"),
                },
            ]
        }

        Step::SetUserPassword { root, username } => {
            vec![Cmd::RunWithSecretStdin {
                program: "arch-chroot".to_owned(),
                args: vec![root.clone(), "chpasswd".to_owned()],
                input: format!("{username}:{}\n", secrets.user_password),
                redact: "printf '<username>:<password>'",
            }]
        }

        Step::LockRootAccount { root } => {
            vec![Cmd::chroot(root, "passwd", &["--lock", "root"])]
        }

        Step::EnableAutologin { root, username } => vec![base::autologin(root, username)],

        Step::EnableServices { root, units } => {
            let mut commands = vec![base::enable_preset_services(root)];
            if !units.is_empty() {
                let mut args = vec!["enable".to_owned()];
                args.extend(units.iter().cloned());
                commands.push(Cmd::Chroot {
                    root: root.clone(),
                    program: "systemctl".to_owned(),
                    args,
                });
            }
            commands
        }

        Step::Unmount { at } => vec![
            base::stop_keyring_daemons(at),
            Cmd::run("swapoff", &["--all"]),
            Cmd::run("umount", &["--recursive", at]),
        ],

        Step::DeleteStagingPartition { disk, number, .. } => {
            vec![Cmd::run("sfdisk", &["--delete", disk, &number.to_string()])]
        }

        Step::GrowRootPartition {
            disk,
            number,
            new_end_sector,
            ..
        } => {
            // `sfdisk -N` rewrites exactly one entry and leaves the rest of the
            // table alone. The start is given back unchanged so that a version of
            // sfdisk that decides to re-align cannot move it.
            let start = plan.root.start_sector;
            let size = new_end_sector - start + 1;
            vec![Cmd::Run {
                program: "sh".to_owned(),
                args: vec![
                    "-c".to_owned(),
                    format!(
                        "printf 'start={start}, size={size}\\n' | \
                         sfdisk --no-reread --force -N {number} {disk}"
                    ),
                ],
            }]
        }

        Step::GrowRootFilesystem { device } => btrfs::grow(device),
    }
}

/// Where the one-line `sfdisk --append` script is written.
///
/// One path per partition so that a dry run's log shows each script as a
/// separate file, which is the difference between a log you
/// can read and a log with the same filename in it twice.
fn sfdisk_script_path(role: Role) -> String {
    match role {
        Role::Root => "/run/crownos-installer/root.sfdisk".to_owned(),
        Role::Swap => "/run/crownos-installer/swap.sfdisk".to_owned(),
        Role::Boot => "/run/crownos-installer/boot.sfdisk".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::plan::{self, Account, NewIds, PlanInput, Reclaim};
    use crate::engine::probe::{self, fixture};
    use crate::oobe::{Install, Locale, Oobe, Staging, StagingKind, SwapPlan, Target, UserAccount};

    const SECRET: &str = "correct-horse-battery-staple";

    fn secrets() -> Secrets {
        Secrets {
            user_password: SECRET.to_owned(),
        }
    }

    fn a_plan() -> Plan {
        let snapshot =
            probe::probe(&fixture::machine(fixture::FINDMNT_JSON_IN_RAM)).expect("probe");
        let disk = &snapshot.disks[0];
        let gap = *disk.free_gaps().last().expect("a gap");

        let oobe = Oobe {
            schema_version: crate::oobe::SCHEMA_VERSION,
            generated_by: "test".to_owned(),
            generated_at_unix: 0,
            install: Install {
                auto_install: false,
                allocated_bytes: disk.bytes(gap.sectors),
                swap: SwapPlan::File {
                    size_bytes: 8 * 1024 * 1024 * 1024,
                },
                keep_windows: true,
                ai_features: crate::oobe::AiFeatures::Disabled,
                windows_apps: crate::oobe::WindowsApps::Disabled,
                browser: crate::oobe::Browser::Chromium,
                developer_tools: crate::oobe::DeveloperTools::default(),
            },
            target: Target {
                esp_partuuid: "0fc63daf-8483-4772-8e79-3d69d8477de4".to_owned(),
                esp_size_bytes: 260 * 1024 * 1024,
                windows_partuuid: "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7".to_owned(),
                disk_guid: None,
                windows_disk_number: 0,
                expected_free_start_bytes: disk.bytes(gap.start_sector),
                expected_free_bytes: disk.bytes(gap.sectors),
                logical_sector_size: 512,
            },
            staging: Staging {
                kind: StagingKind::Partition,
                partuuid: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_owned(),
                base_dir: "EFI/ArchStage".to_owned(),
                reclaim_after_install: true,
            },
            user: Some(UserAccount {
                username: "crown".to_owned(),
                full_name: Some("Crown User".to_owned()),
                hostname: "crownos".to_owned(),
                autologin: true,
            }),
            locale: Locale::default(),
        };

        plan::plan(&PlanInput {
            oobe: &oobe,
            snapshot: &snapshot,
            account: Account {
                username: "crown".to_owned(),
                full_name: Some("Crown User".to_owned()),
                hostname: "crownos".to_owned(),
                autologin: true,
            },
            ids: NewIds {
                root_partuuid: "12345678-1234-1234-1234-123456789abc".to_owned(),
                swap_partuuid: "87654321-4321-4321-4321-cba987654321".to_owned(),
                boot_partuuid: "abcdef01-2345-4678-9abc-def012345678".to_owned(),
            },
        })
        .expect("plan")
    }

    /// Everything a dry run would do, as one string.
    fn dry_run_log(plan: &Plan) -> String {
        let system = fixture::FakeSystem::new();
        let mut log = Vec::new();
        execute(plan, &secrets(), Mode::DryRun, &system, &mut |event| {
            if let Event::Log(line) = event {
                log.push(line);
            }
        })
        .expect("a dry run cannot fail");
        assert!(
            system.journal().is_empty(),
            "a dry run must not touch the system: {:?}",
            system.journal()
        );
        log.join("\n")
    }

    #[test]
    fn a_dry_run_touches_nothing_and_reports_every_step() {
        let plan = a_plan();
        let mut started = 0;
        let mut finished = 0;
        let mut done = false;
        let system = fixture::FakeSystem::new();
        execute(
            &plan,
            &secrets(),
            Mode::DryRun,
            &system,
            &mut |event| match event {
                Event::StepStarted { total, .. } => {
                    assert_eq!(total, plan.steps.len());
                    started += 1;
                }
                Event::StepFinished { .. } => finished += 1,
                Event::Finished { dry_run } => {
                    assert!(dry_run);
                    done = true;
                }
                Event::Failed { message, .. } => panic!("dry run failed: {message}"),
                Event::Log(_) => {}
            },
        )
        .expect("dry run");

        assert_eq!(started, plan.steps.len());
        assert_eq!(finished, plan.steps.len());
        assert!(done);
        assert!(system.journal().is_empty());
    }

    /// The single most important property of the log: the password is not in it.
    #[test]
    fn the_password_never_appears_in_the_log() {
        let log = dry_run_log(&a_plan());
        assert!(!log.contains(SECRET), "the password leaked into the log");
        assert!(
            log.contains("<password>"),
            "the redacted form should still show that a password is being set"
        );
    }

    /// And it is not in the plan either, nor in its `Debug`.
    #[test]
    fn the_password_is_not_in_the_plan_or_in_debug_output() {
        let plan = a_plan();
        assert!(!format!("{plan:?}").contains(SECRET));
        assert!(!format!("{:?}", secrets()).contains(SECRET));
        assert!(format!("{:?}", secrets()).contains("redacted"));
    }

    /// The dry run is the rehearsal the user is shown before they commit, so the
    /// things it must never mention are worth asserting one at a time.
    #[test]
    fn a_dry_run_never_names_the_windows_partition_destructively() {
        let log = dry_run_log(&a_plan());
        for forbidden in [
            "mkfs.ext4 -F -L crownos /dev/sda3",
            "mkfs.btrfs -f -L crownos /dev/sda3",
            "sfdisk --delete /dev/sda 3",
            "sfdisk --delete /dev/sda 1",
            "mkfs.ext4 -F -L crownos /dev/sda1",
        ] {
            assert!(
                !log.contains(forbidden),
                "the dry run would have run `{forbidden}`"
            );
        }
        // Nor is either partition mentioned by a formatting or deleting command
        // at all — a looser check that survives an argument order change.
        for line in log.lines() {
            let touches_protected = line.contains("/dev/sda3") || line.contains("/dev/sda1");
            let is_destructive = line.contains("mkfs")
                || line.contains("--delete")
                || line.contains("mkswap")
                || line.contains("btrfs filesystem resize")
                || line.contains("btrfs subvolume");
            assert!(
                !(touches_protected && is_destructive),
                "destructive command against a protected partition: {line}"
            );
        }
    }

    #[test]
    fn the_new_partition_is_appended_and_never_replaces_an_entry() {
        let log = dry_run_log(&a_plan());
        assert!(
            log.contains("sfdisk --append --force /dev/sda"),
            "partition creation must use --append: {log}"
        );
        // `--append` is the whole safety story here, so nothing may create a
        // partition any other way.
        assert!(!log.contains("sgdisk"));
        assert!(!log.contains("sfdisk --wipe"));
        assert!(!log.contains("parted"));
    }

    #[test]
    fn the_partuuid_the_bootloader_references_is_the_one_the_partition_is_created_with() {
        let plan = a_plan();
        let log = dry_run_log(&plan);
        assert!(log.contains(&format!("uuid={}", plan.root.partuuid)));
        let entry = plan
            .steps
            .iter()
            .find_map(|s| match s {
                Step::InstallBootloader { kernel_cmdline, .. } => Some(kernel_cmdline.clone()),
                _ => None,
            })
            .expect("a bootloader step");
        assert_eq!(entry, format!("root=PARTUUID={} rw", plan.root.partuuid));
    }

    #[test]
    fn a_shared_esp_boots_refind_through_a_firmware_entry_that_must_not_fail() {
        let plan = a_plan();
        let step = plan
            .steps
            .iter()
            .find(|s| matches!(s, Step::InstallBootloader { .. }))
            .expect("a bootloader step");
        let commands = commands_for(&plan, &secrets(), step);
        let entry = format!(
            "efibootmgr --create --disk {} --part {} --label CrownOS",
            plan.disk, plan.esp.number
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, Cmd::Run { .. }) && c.to_string().starts_with(&entry)),
            "{commands:?}"
        );
        let position = |needle: &str| {
            commands
                .iter()
                .position(|c| c.to_string().contains(needle))
                .unwrap_or_else(|| panic!("{needle} missing"))
        };
        assert!(
            position("refind.conf.crownos-backup")
                < position("cat > /mnt/efi/EFI/refind/refind.conf")
        );
        for command in &commands {
            let line = command.to_string();
            assert!(
                !line.contains("EFI/Microsoft") && !line.contains("EFI/BOOT"),
                "{line}"
            );
        }
    }

    #[test]
    fn a_backup_is_taken_only_of_a_file_that_is_there_and_never_over_an_older_one() {
        let from = "/mnt/efi/EFI/refind/refind.conf";
        let to = "/mnt/efi/EFI/refind/refind.conf.crownos-backup";
        let command = Cmd::CopyIfPresent {
            from: from.to_owned(),
            to: to.to_owned(),
        };
        let copy = format!("cp --no-clobber -- {from} {to}");

        let present = fixture::FakeSystem::new()
            .with_existing(from)
            .with_command(&copy, "");
        command.apply(&present).expect("apply");
        assert_eq!(present.journal(), vec![copy]);

        let absent = fixture::FakeSystem::new();
        let note = command.apply(&absent).expect("apply");
        assert!(note.contains("nothing to back up"), "{note}");
        assert!(absent.journal().is_empty(), "{:?}", absent.journal());
    }

    #[test]
    fn the_preset_file_of_the_new_system_names_the_units_and_the_record() {
        let preset = "/mnt/usr/lib/systemd/system-preset/80-crownos.preset";
        let command = base::enable_preset_services("/mnt");
        let with_preset = fixture::FakeSystem::new()
            .with_file(preset, "# CrownOS\nenable a.service\nenable b.timer\n")
            .with_command("arch-chroot /mnt systemctl preset -- a.service b.timer", "");
        command.apply(&with_preset).expect("apply");
        assert_eq!(
            with_preset.journal(),
            [
                "arch-chroot /mnt systemctl preset -- a.service b.timer",
                "write /mnt/var/lib/crownos/update/applied-presets (18 bytes)",
            ]
        );

        let fallback = format!(
            "arch-chroot /mnt systemctl enable -- {}",
            base::SERVICES.join(" ")
        );
        let without_preset = fixture::FakeSystem::new().with_command(&fallback, "");
        command.apply(&without_preset).expect("apply");
        assert_eq!(
            without_preset.journal(),
            [
                fallback,
                format!(
                    "write /mnt/var/lib/crownos/update/applied-presets ({} bytes)",
                    preset::applied_presets_record(base::SERVICES.iter().copied()).len()
                ),
            ]
        );
    }

    /// A committing run against the fake system, to prove the command paths
    /// actually get invoked and in order. Every command's fixture output is
    /// empty, which is what a successful `mkfs` prints anyway.
    #[test]
    fn a_committing_run_invokes_the_commands_it_logged() {
        let plan = a_plan();
        // Only the first few steps, so the fixture stays legible: the executor
        // stops at the first failure, and the assertion is about what ran before
        // that.
        let mut trimmed = plan.clone();
        trimmed.steps.truncate(3);

        let mut system = fixture::FakeSystem::new();
        for step in &trimmed.steps {
            for command in commands_for(&trimmed, &secrets(), step) {
                if let Cmd::Run { program, args } = &command {
                    let line = if args.is_empty() {
                        program.clone()
                    } else {
                        format!("{program} {}", args.join(" "))
                    };
                    system = system.with_command(&line, "");
                }
            }
        }

        let mut failures = Vec::new();
        execute(&trimmed, &secrets(), Mode::Commit, &system, &mut |event| {
            if let Event::Failed { message, .. } = event {
                failures.push(message);
            }
        })
        .expect("commit");
        assert!(failures.is_empty(), "{failures:?}");

        let journal = system.journal();
        assert!(
            journal.iter().any(|l| l.contains("sfdisk --append")),
            "{journal:?}"
        );
        assert!(
            journal.iter().any(|l| l.contains("partprobe")),
            "{journal:?}"
        );
    }

    #[test]
    fn a_failing_command_stops_the_install_and_reports_which_step() {
        let plan = a_plan();
        let mut trimmed = plan.clone();
        trimmed.steps.truncate(2);

        // No fixtures at all, so the very first command fails.
        let system = fixture::FakeSystem::new();
        let mut failed_at = None;
        let result = execute(&trimmed, &secrets(), Mode::Commit, &system, &mut |event| {
            if let Event::Failed { index, .. } = event {
                failed_at = Some(index);
            }
        });
        assert!(result.is_err());
        assert_eq!(failed_at, Some(0));
    }

    #[test]
    fn a_reclaiming_plan_grows_the_root_partition_from_its_original_start() {
        let mut table = fixture::STANDARD_TABLE.to_vec();
        table[3].start = 997_855_232;
        let snapshot = probe::probe(&fixture::machine_with(
            fixture::FINDMNT_JSON_IN_RAM,
            &fixture::sfdisk_json("gpt", &table),
        ))
        .expect("probe");

        // The *largest* gap, not the last: moving CROWNSTAGE to the end leaves a
        // small run of free sectors behind it as well.
        let disk = &snapshot.disks[0];
        let gap = disk
            .free_gaps()
            .into_iter()
            .max_by_key(|g| g.sectors)
            .expect("a gap");
        let mut oobe = payload_like(disk.bytes(gap.start_sector), disk.bytes(gap.sectors));
        oobe.staging.reclaim_after_install = true;

        let plan = plan::plan(&PlanInput {
            oobe: &oobe,
            snapshot: &snapshot,
            account: Account {
                username: "crown".to_owned(),
                full_name: None,
                hostname: "crownos".to_owned(),
                autologin: false,
            },
            ids: NewIds {
                root_partuuid: "12345678-1234-1234-1234-123456789abc".to_owned(),
                swap_partuuid: "87654321-4321-4321-4321-cba987654321".to_owned(),
                boot_partuuid: "abcdef01-2345-4678-9abc-def012345678".to_owned(),
            },
        })
        .expect("plan");
        assert!(matches!(plan.reclaim, Reclaim::GrowRootOver { .. }));

        let log = dry_run_log(&plan);
        assert!(log.contains("sfdisk --delete /dev/sda 4"), "{log}");
        assert!(
            log.contains(&format!("start={}", plan.root.start_sector)),
            "the grow must keep root's original start: {log}"
        );
        // The grow runs after the install's unmount, on a mount of its own,
        // and on the root partition rather than the deleted staging one.
        let umount = log.find("umount --recursive").expect("an unmount");
        let mount = log
            .find(&format!(
                "mount {} /run/crownos-installer/grow",
                plan.root.path
            ))
            .expect("the root is mounted to grow it");
        let grow = log
            .find("btrfs filesystem resize max /run/crownos-installer/grow")
            .expect("a resize");
        assert!(umount < mount && mount < grow, "{log}");
        let stop_gnupg = log
            .find("gpgconf --homedir /mnt/etc/pacman.d/gnupg --kill all")
            .expect("the keyring daemons are stopped");
        assert!(stop_gnupg < umount, "{log}");
    }

    #[test]
    fn the_swapfile_lives_in_its_own_subvolume() {
        let log = dry_run_log(&a_plan());
        assert!(
            log.contains("btrfs filesystem mkswapfile --size 8192m /swap/swapfile"),
            "{log}"
        );
        assert!(log.contains("@swap"), "{log}");
        assert!(!log.contains(" /swapfile"), "{log}");
    }

    #[test]
    fn a_filtered_append_strips_the_root_subvolume() {
        let output = "UUID=1\t/\tbtrfs\trw,compress=zstd,subvolid=256,subvol=/@\t0 0\n\
                      UUID=1\t/home\tbtrfs\trw,subvol=/@home\t0 0\n";
        let system = fixture::FakeSystem::new().with_command("genfstab -U /mnt", output);
        let command = Cmd::RunAppendingTo {
            program: "genfstab".to_owned(),
            args: vec!["-U".to_owned(), "/mnt".to_owned()],
            path: "/mnt/etc/fstab".to_owned(),
            filter: Some(OutputFilter::FstabRootWithoutSubvolume),
        };
        command.apply(&system).expect("apply");
        let expected = fstab::without_root_subvolume(output);
        assert!(expected.len() < output.len());
        assert!(
            system
                .journal()
                .contains(&format!("write /mnt/etc/fstab ({} bytes)", expected.len())),
            "{:?}",
            system.journal()
        );
    }

    fn payload_like(free_start: u64, free_bytes: u64) -> Oobe {
        Oobe {
            schema_version: crate::oobe::SCHEMA_VERSION,
            generated_by: "test".to_owned(),
            generated_at_unix: 0,
            install: Install {
                auto_install: false,
                allocated_bytes: free_bytes,
                swap: SwapPlan::None,
                keep_windows: true,
                ai_features: crate::oobe::AiFeatures::Disabled,
                windows_apps: crate::oobe::WindowsApps::Disabled,
                browser: crate::oobe::Browser::Chromium,
                developer_tools: crate::oobe::DeveloperTools::default(),
            },
            target: Target {
                esp_partuuid: "0fc63daf-8483-4772-8e79-3d69d8477de4".to_owned(),
                esp_size_bytes: 260 * 1024 * 1024,
                windows_partuuid: "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7".to_owned(),
                disk_guid: None,
                windows_disk_number: 0,
                expected_free_start_bytes: free_start,
                expected_free_bytes: free_bytes,
                logical_sector_size: 512,
            },
            staging: Staging {
                kind: StagingKind::Partition,
                partuuid: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_owned(),
                base_dir: "EFI/ArchStage".to_owned(),
                reclaim_after_install: true,
            },
            user: None,
            locale: Locale::default(),
        }
    }
}
