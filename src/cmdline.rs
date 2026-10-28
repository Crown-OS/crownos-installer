//! The kernel command line the Windows side injected, and what it means.
//!
//! `windows-migration-assistant` creates a one-shot UEFI boot entry whose load
//! options are an archiso command line with the CrownOS handoff parameters
//! bolted on:
//!
//! ```text
//! initrd=\EFI\ArchStage\boot\x86_64\initramfs-linux.img
//! archisobasedir=EFI/ArchStage
//! archisodevice=/dev/disk/by-partuuid/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee
//! copytoram=y
//! cow_spacesize=2048M
//! crownos.stage=1
//! crownos.stage_partuuid=aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee
//! crownos.oobe=EFI/ArchStage/oobe.json
//! oobe_file=/run/archiso/bootmnt/EFI/ArchStage/oobe.json
//! auto_install=true
//! ```
//!
//! The `crownos.*` parameters plus `oobe_file` and `auto_install` are this
//! module's business. Everything else on the line belongs to archiso's own hooks
//! and is deliberately ignored.
//!
//! # Why there are two ways to name the same file
//!
//! [`oobe_file`](CmdlineArgs::oobe_file) is an absolute path under
//! `/run/archiso/bootmnt`, and that path **stops existing part way through
//! boot**: `copytoram=y` is on the line (it is what makes reclaiming the staging
//! partition legal at all), and archiso's `copytoram` hook unmounts `bootmnt`
//! once the squashfs is in RAM. A boot that got as far as the hook has no
//! `bootmnt`; a boot that has not yet does. Reading the payload only from there
//! would work on some machines and not others, which is the worst kind of bug to
//! ship in a partitioner.
//!
//! So the line also carries the two pieces that do not depend on anything being
//! mounted: [`stage_partuuid`](CmdlineArgs::stage_partuuid), the partition the
//! payload is on, and [`oobe_relative`](CmdlineArgs::oobe_relative), the path
//! within it. [`payload::locate`](crate::payload::locate) prefers the fast path
//! and falls back to mounting the partition itself.
//!
//! # Why the command line at all, when there is a JSON file
//!
//! Because the installer has to *find* the JSON file. The kernel command line is
//! the one channel guaranteed to have survived the reboot intact and to be
//! readable before anything is mounted. So it carries the pointers, and the
//! payload carries the decisions.
//!
//! [`crownos.stage`](CmdlineArgs::stage) is a marker rather than data: it says
//! "this boot was arranged by the CrownOS Windows installer" and nothing else.
//! Without it a live ISO booted by hand looks exactly like a staged boot whose
//! parameters went missing, and those two want very different behaviour — the
//! first is a user poking around, the second is a bug worth shouting about.
//!
//! # What is parsed but not trusted
//!
//! [`copytoram`](CmdlineArgs::copytoram) is read for the log and for nothing
//! else. It is a *request* made before the fact, and a request that silently
//! failed — a medium too slow, a machine short on RAM — looks identical on the
//! command line to one that succeeded. Whether the payload is really in RAM is
//! decided from the mount table; see
//! [`SystemSnapshot::payload_is_in_ram`](crate::engine::SystemSnapshot::payload_is_in_ram).
//!
//! # Parsing is separate from reading `/proc`
//!
//! [`parse`] takes a `&str`. [`read`] is the two-line wrapper that hands it the
//! contents of `/proc/cmdline`. Keeping them apart is what makes the quoting and
//! precedence rules below testable on a machine that was not booted this way.

use std::path::PathBuf;

/// The parameter naming the handoff payload as an absolute path. Valid only
/// while the staging medium is still mounted.
pub const OOBE_FILE: &str = "oobe_file";
/// The parameter asking the installer not to stop for confirmation.
pub const AUTO_INSTALL: &str = "auto_install";
/// The marker parameter identifying a staged CrownOS boot.
pub const CROWNOS_STAGE: &str = "crownos.stage";
/// The PARTUUID of the partition the payload was staged on. The half of the
/// handoff that survives `copytoram` unmounting `bootmnt`.
pub const CROWNOS_STAGE_PARTUUID: &str = "crownos.stage_partuuid";
/// The payload's path *within* the staging partition, forward-slashed.
pub const CROWNOS_OOBE: &str = "crownos.oobe";
/// archiso's own parameter, read for diagnostics only. See the module docs.
pub const COPYTORAM: &str = "copytoram";

/// Where `/proc/cmdline` lives. A constant so a test can point [`read`] at a
/// fixture without a filesystem mock.
pub const PROC_CMDLINE: &str = "/proc/cmdline";

/// The handoff parameters, as found on the kernel command line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CmdlineArgs {
    /// Path named by `oobe_file=`, if present.
    ///
    /// The fast path, and the one that expires: see the module docs. Absent means
    /// either a hand-booted live ISO or a staged boot that lost its parameter;
    /// [`stage`](Self::stage) is what tells those apart.
    pub oobe_file: Option<PathBuf>,
    /// PARTUUID from `crownos.stage_partuuid=`, lowercased.
    ///
    /// The durable half of the handoff. Cross-checked against the payload's own
    /// [`Staging::partuuid`](crate::oobe::Staging::partuuid) — if they disagree,
    /// the command line and the file came from different runs of the Windows
    /// installer and neither can be trusted.
    pub stage_partuuid: Option<String>,
    /// Path from `crownos.oobe=`, relative to the staging partition's root, with
    /// forward slashes.
    pub oobe_relative: Option<String>,
    /// Whether `auto_install=true` was given.
    ///
    /// This is only a *request*. The payload's own
    /// [`Install::auto_install`](crate::oobe::Install::auto_install) is the
    /// authoritative copy, and the UI still stops for a password when it has no
    /// account to work from.
    pub auto_install: bool,
    /// Whether `copytoram` was asked for. Diagnostic only — see the module docs.
    pub copytoram: bool,
    /// The `crownos.stage=` generation, if this boot was staged by the Windows
    /// installer. Currently always `1`.
    pub stage: Option<u32>,
}

impl CmdlineArgs {
    /// Whether this boot was arranged by `windows-migration-assistant`.
    pub fn is_staged_boot(&self) -> bool {
        self.stage.is_some()
    }

    /// Whether the durable fallback route to the payload is available.
    ///
    /// Without both halves there is exactly one place to look for the payload,
    /// and it may already have been unmounted.
    pub fn has_staging_fallback(&self) -> bool {
        self.stage_partuuid.is_some() && self.oobe_relative.is_some()
    }
}

/// Parse a kernel command line.
///
/// Rules, all of them chosen to match how the kernel itself reads the line:
///
/// * Parameters are whitespace separated, and a value may be double-quoted to
///   contain spaces (`oobe_file="/run/archiso/bootmnt/EFI/Crown Stage/oobe.json"`).
///   The quotes are stripped and are not part of the value.
/// * A bare flag (`auto_install`, no `=`) counts as true, because that is what a
///   human editing the line by hand at the boot prompt will type.
/// * The *last* occurrence wins. The kernel appends what the bootloader hands it
///   after the built-in line, so an override typed at the prompt has to be the
///   one that takes effect.
/// * Anything unrecognised is ignored rather than an error. The line is shared
///   with archiso, the kernel and any number of hooks, and a strict parser here
///   would fail on `quiet`.
///
/// Values other than `true`/`1`/`yes`/`on` (case insensitive) read as false, so
/// a typo fails towards asking the user rather than towards partitioning
/// unattended.
pub fn parse(cmdline: &str) -> CmdlineArgs {
    let mut args = CmdlineArgs::default();

    for (key, value) in tokens(cmdline) {
        match key {
            OOBE_FILE => {
                // An empty value is a parameter that was templated wrong, not a
                // request to read `""`.
                args.oobe_file = value.filter(|v| !v.is_empty()).map(PathBuf::from);
            }
            CROWNOS_STAGE_PARTUUID => {
                args.stage_partuuid = value
                    .map(|v| v.trim().to_ascii_lowercase())
                    .filter(|v| !v.is_empty());
            }
            CROWNOS_OOBE => {
                // Normalized to a relative path: a leading slash here would make
                // every `join` in the resolver escape the mount point.
                args.oobe_relative = value
                    .map(|v| v.trim().trim_start_matches('/').to_owned())
                    .filter(|v| !v.is_empty());
            }
            AUTO_INSTALL => args.auto_install = value.is_none_or(|v| is_truthy(&v)),
            COPYTORAM => args.copytoram = value.is_none_or(|v| is_truthy(&v) || v == "y"),
            CROWNOS_STAGE => {
                args.stage = match value {
                    // A bare marker is generation 1 — the only one there has
                    // ever been, and what a human types at the boot prompt.
                    None => Some(1),
                    // A non-numeric generation is a corrupted line. Reading it
                    // as 1 would claim knowledge we do not have, so the marker
                    // is dropped and the installer takes the "hand-booted ISO"
                    // path, which asks the user everything. That is the
                    // conservative direction to fail in.
                    Some(v) => v.parse().ok(),
                };
            }
            _ => {}
        }
    }

    args
}

/// Read and parse `/proc/cmdline`.
///
/// A missing or unreadable `/proc/cmdline` yields the default — every parameter
/// absent — which the caller reads as "not a staged boot". There is no useful
/// error to report: a system without `/proc` mounted is not one this installer
/// can run on at all, and it will fail with something much clearer a moment
/// later when it tries to probe the disk.
pub fn read() -> CmdlineArgs {
    std::fs::read_to_string(PROC_CMDLINE)
        .map(|line| parse(&line))
        .unwrap_or_default()
}

/// Split a command line into `key`/`value` pairs, honouring double quotes.
///
/// Written by hand rather than with `split_whitespace` because a quoted value
/// may contain spaces, and a staging directory with a space in its name is a
/// perfectly ordinary thing for a Windows-side path to produce.
fn tokens(cmdline: &str) -> Vec<(&str, Option<String>)> {
    let mut out = Vec::new();
    let bytes = cmdline.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Skip separators. `\n` matters: `/proc/cmdline` ends with one.
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }

        let start = i;
        let mut eq = None;
        let mut in_quotes = false;
        while i < bytes.len() {
            match bytes[i] {
                b'"' => in_quotes = !in_quotes,
                b'=' if eq.is_none() && !in_quotes => eq = Some(i),
                c if c.is_ascii_whitespace() && !in_quotes => break,
                _ => {}
            }
            i += 1;
        }

        let token = &cmdline[start..i];
        match eq {
            Some(eq) => {
                let key = &cmdline[start..eq];
                let value = &cmdline[eq + 1..i];
                out.push((key, Some(unquote(value))));
            }
            None => out.push((token, None)),
        }
    }

    out
}

/// Strip double quotes from a value. Only the quotes are removed — there is no
/// escaping in a kernel command line, so there is nothing else to unescape.
fn unquote(value: &str) -> String {
    value.replace('"', "")
}

/// Whether a value means yes. Anything else — including a typo — means no.
fn is_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes" | "y" | "on"
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The line the Windows side actually writes, as of schema 1. Copied from
    /// the finalized contract, whitespace and all.
    pub(crate) const STAGED: &str = "initrd=\\EFI\\ArchStage\\boot\\x86_64\\initramfs-linux.img \
         archisobasedir=EFI/ArchStage \
         archisodevice=/dev/disk/by-partuuid/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee \
         copytoram=y cow_spacesize=2048M \
         crownos.stage=1 \
         crownos.stage_partuuid=aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee \
         crownos.oobe=EFI/ArchStage/oobe.json \
         oobe_file=/run/archiso/bootmnt/EFI/ArchStage/oobe.json \
         auto_install=true\n";

    #[test]
    fn the_real_staged_command_line_parses() {
        let args = parse(STAGED);
        assert_eq!(
            args.oobe_file.as_deref(),
            Some(std::path::Path::new(
                "/run/archiso/bootmnt/EFI/ArchStage/oobe.json"
            ))
        );
        assert_eq!(
            args.stage_partuuid.as_deref(),
            Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
        );
        assert_eq!(
            args.oobe_relative.as_deref(),
            Some("EFI/ArchStage/oobe.json")
        );
        assert!(args.auto_install);
        assert!(args.copytoram);
        assert_eq!(args.stage, Some(1));
        assert!(args.is_staged_boot());
        assert!(args.has_staging_fallback());
    }

    /// The durable half is what makes the resolver's fallback possible, so its
    /// absence has to be visible rather than inferred.
    #[test]
    fn a_line_missing_either_durable_half_has_no_fallback() {
        assert!(
            !parse("crownos.stage=1 crownos.oobe=EFI/ArchStage/oobe.json").has_staging_fallback()
        );
        assert!(!parse("crownos.stage=1 crownos.stage_partuuid=aaaa").has_staging_fallback());
    }

    /// PARTUUIDs are compared as strings against `sfdisk` and `lsblk` output, so
    /// the case they arrive in cannot be allowed to matter.
    #[test]
    fn a_stage_partuuid_is_lowercased() {
        let args = parse("crownos.stage_partuuid=AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE");
        assert_eq!(
            args.stage_partuuid.as_deref(),
            Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
        );
    }

    /// A leading slash would make every `join` in the resolver escape the mount
    /// point and read from the live filesystem instead.
    #[test]
    fn an_absolute_crownos_oobe_is_made_relative() {
        assert_eq!(
            parse("crownos.oobe=/EFI/ArchStage/oobe.json")
                .oobe_relative
                .as_deref(),
            Some("EFI/ArchStage/oobe.json")
        );
    }

    /// `copytoram=y` is archiso's spelling, and it is parsed — but only so it can
    /// be logged. Nothing decides anything from it.
    #[test]
    fn copytoram_is_parsed_in_archisos_own_spelling() {
        assert!(parse("copytoram=y").copytoram);
        assert!(parse("copytoram").copytoram);
        assert!(!parse("copytoram=n").copytoram);
        assert!(!parse("quiet").copytoram);
    }

    /// A plain live ISO. Nothing of ours is on the line, and the installer must
    /// be able to tell.
    #[test]
    fn a_hand_booted_iso_has_no_handoff() {
        let args = parse(
            "initrd=\\arch\\boot\\x86_64\\initramfs-linux.img \
             archisobasedir=arch archisosearchuuid=2024-01-01-00-00-00-00 quiet\n",
        );
        assert_eq!(args, CmdlineArgs::default());
        assert!(!args.is_staged_boot());
        assert!(!args.auto_install);
    }

    /// A staging directory with a space in it is an ordinary thing for a
    /// Windows-side path to produce.
    #[test]
    fn a_quoted_value_keeps_its_spaces() {
        let args = parse("quiet oobe_file=\"/run/archiso/bootmnt/EFI/Crown Stage/oobe.json\" ro");
        assert_eq!(
            args.oobe_file.as_deref(),
            Some(std::path::Path::new(
                "/run/archiso/bootmnt/EFI/Crown Stage/oobe.json"
            ))
        );
    }

    /// `=` inside a quoted value is part of the value, not a second separator.
    #[test]
    fn only_the_first_unquoted_equals_splits_a_token() {
        let args = parse("oobe_file=\"/mnt/a=b/oobe.json\"");
        assert_eq!(
            args.oobe_file.as_deref(),
            Some(std::path::Path::new("/mnt/a=b/oobe.json"))
        );
    }

    /// The kernel appends the bootloader's line after the built-in one, so a
    /// value typed at the prompt has to be the one that wins.
    #[test]
    fn the_last_occurrence_wins() {
        let args = parse(
            "auto_install=true oobe_file=/a/oobe.json auto_install=false oobe_file=/b/oobe.json",
        );
        assert!(!args.auto_install);
        assert_eq!(
            args.oobe_file.as_deref(),
            Some(std::path::Path::new("/b/oobe.json"))
        );
    }

    /// A bare flag is what a human types at the boot prompt.
    #[test]
    fn a_bare_flag_is_true() {
        assert!(parse("quiet auto_install rw").auto_install);
        assert_eq!(parse("crownos.stage").stage, Some(1));
    }

    /// Every spelling of yes, and the fact that nothing else counts. A typo has
    /// to fail towards asking the user, never towards partitioning unattended.
    #[test]
    fn only_affirmative_values_enable_auto_install() {
        for yes in ["true", "TRUE", "1", "yes", "Y", "on"] {
            assert!(
                parse(&format!("auto_install={yes}")).auto_install,
                "{yes} should enable auto install"
            );
        }
        for no in ["false", "0", "no", "off", "ture", ""] {
            assert!(
                !parse(&format!("auto_install={no}")).auto_install,
                "{no:?} must not enable auto install"
            );
        }
    }

    /// A parameter that got templated into the line with no value is a bug on
    /// the writing side, and reading `""` as a path would turn it into a
    /// confusing one here.
    #[test]
    fn an_empty_oobe_file_is_treated_as_absent() {
        assert_eq!(parse("oobe_file= quiet").oobe_file, None);
        assert_eq!(parse("oobe_file=\"\"").oobe_file, None);
    }

    /// Whitespace, including the trailing newline `/proc/cmdline` carries, and
    /// runs of it, must not produce empty tokens or shift a value.
    #[test]
    fn irregular_whitespace_is_tolerated() {
        let args = parse("  quiet   auto_install=true\t crownos.stage=1  \n");
        assert!(args.auto_install);
        assert_eq!(args.stage, Some(1));
    }

    /// A corrupted generation drops the marker, because "staged by a version we
    /// cannot name" is not something to act on.
    #[test]
    fn a_non_numeric_stage_is_not_a_marker() {
        let args = parse("crownos.stage=banana auto_install=true");
        assert_eq!(args.stage, None);
        assert!(!args.is_staged_boot());
    }

    /// A staged line whose `oobe_file` is missing is the case the marker exists
    /// to distinguish: still a staged boot, and therefore still an error worth
    /// shouting about rather than a live ISO to shrug at.
    #[test]
    fn a_staged_boot_without_a_payload_path_is_still_a_staged_boot() {
        let args = parse("crownos.stage=1 copytoram=y quiet");
        assert!(args.is_staged_boot());
        assert_eq!(args.oobe_file, None);
    }
}
