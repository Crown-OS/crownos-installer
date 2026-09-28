//! `crownos-installer` — the Linux half of the CrownOS dual-boot installer.
//!
//! Runs inside the archiso live environment that `crownos-windows-installer`
//! staged and arranged a one-shot UEFI boot into. Reads the handoff payload,
//! works out what to do to the disk, asks the user for the one thing the payload
//! cannot carry (a password), and does it.
//!
//! See `CLAUDE.md` for the architecture, and the module docs of [`oobe`] (the
//! cross-repo contract), [`cmdline`] (how this boot was arranged), [`payload`]
//! (finding the handoff file on a boot where the obvious path may already be
//! gone) and [`engine::plan`] (every safety rule, and why).
//!
//! # Two installers in one binary
//!
//! The same program serves both ways a user can arrive at it, and which one it is
//! comes down to a single question: **is there a configuration file?**
//!
//! * There is → this boot was arranged by `crownos-windows-installer`, and the
//!   payload says what to do.
//! * There is not → the user flashed the image and booted it, so every decision
//!   the payload would have carried is asked here instead.
//!
//! [`provenance::detect`] is that question and nothing else. A missing payload is
//! not an error any more; see that module for the two cases that still are.
//!
//! # Startup, in order
//!
//! 1. Parse the command line — ours, then the kernel's.
//! 2. Establish the provenance ([`provenance::detect`]).
//! 3. Probe the disks ([`engine::probe`]).
//! 4. Hand all of it to the wizard, which builds a plan and refuses if it must.
//!
//! 2 or 3 failing produces a [`ui::Blocker`] rather than an exit: a user sitting
//! in front of a machine that just rebooted out of Windows needs to be told what
//! happened and offered a restart, not returned to a console.

mod cmdline;
mod engine;
mod oobe;
mod payload;
mod provenance;
mod ui;

use std::process::ExitCode;

use engine::exec::{commands_for, Mode, Secrets};
use engine::plan::{plan, plan_iso, Account, IsoInput, NewIds, PlanInput};
use engine::probe::{probe, RealSystem};

/// What was asked for on *our* command line, as opposed to the kernel's.
#[derive(Debug, Default)]
struct Options {
    /// Build the plan and log every command without running any of them.
    dry_run: bool,
    /// Print the plan and the commands a dry run would issue, then exit. No UI.
    print_plan: bool,
    /// Read the payload from here instead of asking the kernel command line.
    oobe: Option<String>,
    /// Use this instead of `/proc/cmdline`.
    cmdline: Option<String>,
    /// The base `--print-plan` plans a flashed-image install for.
    distro: oobe::Distro,
    help: bool,
}

const USAGE: &str = "\
crownos-installer — install CrownOS

Two installers in one: if a handoff file left by the CrownOS Windows installer is
found, it drives the install; if not, this is a flashed image and the installer
asks for everything itself. Either way the partition editor is available.

Normally started by the ISO's kiosk unit with no arguments at all; everything it
needs is on the kernel command line. The options exist for rehearsing it.

    --dry-run           Show the wizard, but log commands instead of running them.
    --print-plan        Print the plan and the commands a dry run would issue,
                        then exit. No window, no writes; useful over SSH.
    --oobe <path>       Read the handoff payload from <path>, ignoring the
                        kernel command line's idea of where it is.
    --cmdline <string>  Use <string> instead of /proc/cmdline.
    --distro <name>     The base --print-plan installs on a flashed image:
                        arch (default), debian, ubuntu, fedora or nixos.
    --help              This.
";

fn parse_options(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dry-run" => options.dry_run = true,
            // Printing the plan is a dry run by construction — it does not run
            // anything — and saying so keeps a `--print-plan` that later grew a
            // code path from ever reaching the committing one.
            "--print-plan" => {
                options.print_plan = true;
                options.dry_run = true;
            }
            "--oobe" => {
                options.oobe = Some(args.next().ok_or("--oobe needs a path")?);
            }
            "--distro" => {
                let name = args.next().ok_or("--distro needs a name")?;
                options.distro = serde_json::from_value(serde_json::Value::String(name))
                    .map_err(|_| "--distro is one of arch, debian, ubuntu, fedora, nixos")?;
            }
            "--cmdline" => {
                options.cmdline = Some(args.next().ok_or("--cmdline needs a string")?);
            }
            "--help" | "-h" => options.help = true,
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok(options)
}

fn main() -> ExitCode {
    let options = match parse_options(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("crownos-installer: {error}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    if options.help {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let mut args = match &options.cmdline {
        Some(line) => cmdline::parse(line),
        None => cmdline::read(),
    };
    // `--oobe` overrides where to look, and nothing else. It is the one knob a
    // developer needs to rehearse against a payload on a memory stick, and
    // making it override the *marker* too would mean a hand-booted ISO could be
    // talked into a real install by one argument.
    if let Some(path) = &options.oobe {
        args.oobe_file = Some(path.into());
        args.stage = args.stage.or(Some(1));
    }

    let mode = if options.dry_run {
        Mode::DryRun
    } else {
        Mode::Commit
    };

    let startup = start_up(&args);

    if options.print_plan {
        return print_plan(startup, options.distro);
    }

    match ui::run(&args, startup, mode) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("crownos-installer: the window could not be opened: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Everything that has to happen before the first frame.
///
/// Two questions, in order: which installer is this, and what is on the disks.
/// Only the second can fail in a way that stops us dead — a machine whose disks
/// cannot be read is a machine nothing can be installed on. The first fails only
/// when a configuration file was found and cannot be trusted; see
/// [`provenance`].
fn start_up(args: &cmdline::CmdlineArgs) -> Result<ui::Context, ui::Blocker> {
    let system = RealSystem;

    let detected = provenance::detect(args, &payload::RealMedium(&system)).map_err(|blocked| {
        let mut notes = blocked.notes();
        if !args.has_staging_fallback() {
            notes.push(
                "This boot entry carries no `crownos.stage_partuuid=` / `crownos.oobe=` \
                 pair, so there was no way to look on the staging partition itself once \
                 the path on the kernel command line had gone."
                    .to_owned(),
            );
        }
        ui::Blocker {
            title: blocked.title().to_owned(),
            detail: blocked.detail(),
            notes,
        }
    })?;

    let snapshot = probe(&system).map_err(|error| ui::Blocker {
        title: "The disks could not be read".to_owned(),
        detail: format!(
            "{error:#}\n\nNothing has been changed. This usually means a tool the \
             installer needs is missing from the live environment."
        ),
        notes: detected.notes.clone(),
    })?;

    // A payload may ask to skip what can be skipped. A flashed image never can:
    // there is nothing to skip *to*, because nothing has been answered yet.
    let auto_install = detected
        .provenance
        .oobe()
        .is_some_and(|oobe| oobe.install.auto_install || args.auto_install);

    Ok(ui::Context {
        provenance: detected.provenance,
        notes: detected.notes,
        snapshot,
        auto_install,
        ids: new_ids(&system),
    })
}

/// PARTUUIDs for the partitions the plan will create.
///
/// From the kernel's own generator, which is a CSPRNG and is present on every
/// system this program can run on. Read once at startup so that the plan shown
/// on the confirmation page names the same GUIDs the plan that runs will — see
/// [`NewIds`].
///
/// A failure here falls back to a fixed pair. That is not as bad as it looks: a
/// PARTUUID has to be unique on the disk, not in the universe, and the planner
/// refuses any plan whose partitions collide with an existing one. It is still
/// logged, because a machine with no `/proc/sys/kernel/random/uuid` is a machine
/// worth knowing about.
fn new_ids(system: &dyn engine::probe::System) -> NewIds {
    let read = || {
        system
            .read_file("/proc/sys/kernel/random/uuid")
            .map(|uuid| uuid.trim().to_ascii_lowercase())
            .ok()
            .filter(|uuid| uuid.len() == 36)
    };
    let root = read();
    let swap = read();
    if root.is_none() || swap.is_none() {
        eprintln!(
            "crownos-installer: could not read /proc/sys/kernel/random/uuid; \
             falling back to fixed partition GUIDs"
        );
    }
    NewIds {
        root_partuuid: root.unwrap_or_else(|| FALLBACK_ROOT_PARTUUID.to_owned()),
        swap_partuuid: swap.unwrap_or_else(|| FALLBACK_SWAP_PARTUUID.to_owned()),
    }
}

/// Used only when the kernel's UUID generator cannot be read. See [`new_ids`].
const FALLBACK_ROOT_PARTUUID: &str = "c0c0c0c0-0001-4001-8001-c0c0c0c0c0c0";
const FALLBACK_SWAP_PARTUUID: &str = "c0c0c0c0-0002-4002-8002-c0c0c0c0c0c0";

/// `--print-plan`: the whole plan, and the commands it would run, on stdout.
///
/// Works in both modes, and says which one it is on the first line — a rehearsal
/// whose output does not say which installer produced it is a rehearsal of
/// something.
///
/// The account is a placeholder, because there is nobody to ask and the plan's
/// shape does not depend on the name. The password is a placeholder for the same
/// reason, and because [`commands_for`] renders it redacted regardless.
///
/// On the ISO path the target is [`ui::Target::best_for`]'s guess — the same one
/// the disk page opens with. There is no user to choose otherwise, and a
/// `--print-plan` that refused to guess would be unable to rehearse the ISO flow
/// at all. It never edits partitions: the queue is empty because nothing typed
/// one, which is also what keeps this path's output free of anything
/// destructive that the user did not ask for.
fn print_plan(startup: Result<ui::Context, ui::Blocker>, distro: oobe::Distro) -> ExitCode {
    let context = match startup {
        Ok(context) => context,
        Err(blocker) => {
            println!("{}\n\n{}", blocker.title, blocker.detail);
            for note in &blocker.notes {
                println!("  {note}");
            }
            return ExitCode::FAILURE;
        }
    };

    println!("mode:     {}", context.provenance.name());
    for note in &context.notes {
        println!("  {note}");
    }

    let account = context
        .oobe()
        .and_then(|oobe| oobe.user.as_ref())
        .map(Account::from_payload)
        .unwrap_or(Account {
            username: "crown".to_owned(),
            full_name: None,
            hostname: "crownos".to_owned(),
            autologin: false,
        });

    let built = match &context.provenance {
        provenance::Provenance::WindowsInstaller { oobe, .. } => {
            // Echoed back out rather than summarized: the clearest thing a log
            // can say about a handoff file is the handoff file.
            match oobe.to_json() {
                Ok(json) => println!("--- oobe.json as parsed ---\n{json}"),
                Err(error) => println!("could not re-serialize the payload: {error}"),
            }
            plan(&PlanInput {
                oobe,
                snapshot: &context.snapshot,
                account,
                ids: context.ids.clone(),
            })
        }
        provenance::Provenance::Iso => {
            let target = ui::Target::best_for(&context.snapshot);
            let (Some(disk), Some(esp), Some(region_start_sector)) = (
                target.disk.clone(),
                target.esp.clone(),
                target.region_start_sector,
            ) else {
                println!(
                    "REFUSED\n\nNo disk on this machine has both an EFI system partition \
                     and unallocated space to install into. In the wizard this is where \
                     the partition editor comes in; `--print-plan` has nobody to ask, so \
                     it stops here."
                );
                return ExitCode::FAILURE;
            };
            println!("target:   {disk} (chosen automatically — see --help)");
            plan_iso(&IsoInput {
                snapshot: &context.snapshot,
                disk,
                esp,
                region_start_sector,
                root_filesystem: target.filesystem,
                distro,
                swap: target.swap,
                locale: target.locale.clone(),
                account,
                ids: context.ids.clone(),
                edits: Vec::new(),
            })
        }
    };

    let plan = match built {
        Ok(plan) => plan,
        Err(refusal) => {
            println!("REFUSED\n\n{refusal}");
            return ExitCode::FAILURE;
        }
    };

    println!("base:     {}", plan.distro.label());
    println!("disk:     {} ({} byte sectors)", plan.disk, plan.sector_size);
    println!(
        "root:     {} — {} at sector {}",
        plan.root.path,
        engine::plan::bytes(plan.root.size_bytes()),
        plan.root.start_sector
    );
    if let Some(swap) = &plan.swap_partition {
        println!(
            "swap:     {} — {} at sector {}",
            swap.path,
            engine::plan::bytes(swap.size_bytes()),
            swap.start_sector
        );
    }
    println!("esp:      {} (shared, written at file level only)", plan.esp.path);
    match &plan.windows {
        Some(windows) => println!("kept:     {} (never touched)", windows.path),
        None => println!("kept:     nothing on this disk is designated to preserve"),
    }
    println!("reclaim:  {:?}", plan.reclaim);
    for warning in &plan.warnings {
        println!("warning:  {warning}");
    }
    println!("\ndestructive targets: {:?}", plan.destructive_devices());
    println!("file-level targets:  {:?}", plan.file_level_devices());
    println!(
        "authorized to destroy: {:?}\n",
        plan.authorized_destructive
    );

    let secrets = Secrets {
        user_password: "<not used by --print-plan>".to_owned(),
    };
    for (index, step) in plan.steps.iter().enumerate() {
        println!("{:>3}. {}", index + 1, step.label());
        for command in commands_for(&plan, &secrets, step) {
            for line in command.to_string().lines() {
                println!("       {line}");
            }
        }
    }

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_parse() {
        let options = parse_options(
            ["--dry-run", "--oobe", "/tmp/oobe.json"]
                .into_iter()
                .map(str::to_owned),
        )
        .expect("parse");
        assert!(options.dry_run);
        assert!(!options.print_plan);
        assert_eq!(options.oobe.as_deref(), Some("/tmp/oobe.json"));
    }

    /// `--print-plan` must imply `--dry-run`, so that a future code path added to
    /// it cannot reach the committing executor.
    #[test]
    fn printing_the_plan_implies_a_dry_run() {
        let options =
            parse_options(["--print-plan"].into_iter().map(str::to_owned)).expect("parse");
        assert!(options.print_plan);
        assert!(options.dry_run);
    }

    #[test]
    fn a_base_is_named_the_way_the_payload_names_it() {
        let options = parse_options(["--distro", "nixos"].into_iter().map(str::to_owned))
            .expect("parse");
        assert_eq!(options.distro, oobe::Distro::NixOs);
        assert!(parse_options(["--distro", "gentoo"].into_iter().map(str::to_owned)).is_err());
    }

    #[test]
    fn an_unknown_option_is_an_error_rather_than_ignored() {
        assert!(parse_options(["--wipe-disk"].into_iter().map(str::to_owned)).is_err());
        assert!(parse_options(["--oobe"].into_iter().map(str::to_owned)).is_err());
    }
}
