//! Finding `oobe.json`, on a boot where the obvious place to look may already be
//! gone.
//!
//! # The problem
//!
//! The kernel command line carries `oobe_file=/run/archiso/bootmnt/EFI/ArchStage/oobe.json`,
//! and that path is only valid for part of the boot. The same command line
//! carries `copytoram=y` — which is what makes reclaiming the staging partition
//! legal at all — and archiso's `copytoram` hook unmounts `bootmnt` as soon as
//! the squashfs is in RAM. So on a machine that got that far, the path named on
//! the command line does not exist; on a machine that has not, it does. Reading
//! only from there would produce an installer that works on some hardware and
//! not on other hardware, for reasons invisible from the logs.
//!
//! # The two strategies
//!
//! 1. **Read [`CmdlineArgs::oobe_file`] directly.** Cheap, needs no privileges,
//!    and correct whenever `bootmnt` is still mounted.
//! 2. **Mount the staging partition ourselves and read
//!    [`CmdlineArgs::oobe_relative`] inside it.** The partition is named by
//!    PARTUUID (`crownos.stage_partuuid`), which is durable in the way a mount
//!    point is not. Mounted read-only at [`FALLBACK_MOUNT`], read, unmounted.
//!
//! Strategy 2 always works, so it would be defensible to only ever do that. It is
//! the fallback rather than the default for one reason: mounting is the only part
//! of reading a config file that can fail in a way that matters, and a boot where
//! `bootmnt` is still there does not need to take that risk.
//!
//! The mount is **read-only**, and that is load-bearing rather than tidy. This is
//! the same partition the planner may later delete, and a read-write mount of it
//! would both dirty a filesystem we are about to discard and leave a mount that
//! [`plan`](crate::engine::plan) would then refuse to write past.
//!
//! # Cross-checking
//!
//! The command line and the payload each name the staging partition, and they
//! come from the same run of the Windows installer or they come from different
//! ones. [`check_stage_partuuid`] compares them, and a mismatch is refused rather
//! than reconciled: two disagreeing descriptions of which partition is the
//! installer medium is precisely the situation in which guessing deletes the
//! wrong one.
//!
//! # Testing
//!
//! Everything goes through [`Medium`], so the resolver's decisions —, which
//! strategy, in what order, unmounting even when the read fails — are unit tests
//! against a fake whose mount actually changes what is visible. No real mounts,
//! and no `/run/archiso` on the developer's machine.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::cmdline::CmdlineArgs;
use crate::engine::probe::{BOOTMNT, System};
use crate::oobe::{FILE_NAME, Oobe};

/// Where the fallback strategy mounts the staging partition.
///
/// Under `/run` because it is a tmpfs on any live environment, so creating the
/// directory costs nothing and leaves nothing behind.
pub const FALLBACK_MOUNT: &str = "/run/crownos-installer/stage";

/// The filesystem operations the resolver needs, and no others.
///
/// Narrow on purpose: the whole value of this trait is that the fake in the
/// tests can make a mount *change what is readable*, which is the behaviour the
/// resolver exists to cope with and which a general filesystem mock would not
/// model.
pub trait Medium {
    /// Read a file, or `None` if it is not there.
    ///
    /// One method rather than `exists` plus `read` because the two would race in
    /// production and because "not there" and "there but unreadable" lead to the
    /// same next action here.
    fn read(&self, path: &Path) -> Option<String>;

    /// Mount the partition with this PARTUUID read-only at `at`, creating the
    /// mount point.
    fn mount_read_only(&self, partuuid: &str, at: &Path) -> Result<(), String>;

    /// Unmount `at`. Errors are reported but never fatal — see [`locate`].
    fn unmount(&self, at: &Path) -> Result<(), String>;
}

/// A [`Medium`] backed by the real machine.
pub struct RealMedium<'a>(pub &'a dyn System);

impl Medium for RealMedium<'_> {
    fn read(&self, path: &Path) -> Option<String> {
        self.0.read_file(&path.to_string_lossy()).ok()
    }

    fn mount_read_only(&self, partuuid: &str, at: &Path) -> Result<(), String> {
        let at = at.to_string_lossy().into_owned();
        self.0
            .run("mkdir", &["-p", "--", &at])
            .map_err(|e| format!("{e:#}"))?;
        // By PARTUUID, never by device name — the same rule the planner works
        // under, for the same reason.
        let device = format!("/dev/disk/by-partuuid/{partuuid}");
        self.0
            .run("mount", &["-o", "ro", "--", &device, &at])
            .map(|_| ())
            .map_err(|e| format!("{e:#}"))
    }

    fn unmount(&self, at: &Path) -> Result<(), String> {
        self.0
            .run("umount", &["--", &at.to_string_lossy()])
            .map(|_| ())
            .map_err(|e| format!("{e:#}"))
    }
}

/// Where a payload was found. Recorded so the install log says which strategy
/// worked, which is the first thing anyone debugging a boot will want.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// Read straight from the path on the kernel command line.
    CmdlinePath(PathBuf),
    /// Read after mounting the staging partition ourselves.
    StagingPartition { partuuid: String, relative: String },
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CmdlinePath(path) => write!(f, "{}", path.display()),
            Self::StagingPartition { partuuid, relative } => {
                write!(f, "{relative} on partition {partuuid}")
            }
        }
    }
}

/// A payload and where it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Located {
    pub oobe: Oobe,
    pub source: Source,
    /// What was tried, in order, and what happened. Goes into the install log.
    pub attempts: Vec<String>,
}

/// Why the payload could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// This boot was not arranged by the Windows installer at all.
    NotAStagedBoot,
    /// A staged boot whose command line carries no way to find the payload.
    NoRouteToPayload { attempts: Vec<String> },
    /// Both strategies ran and neither found the file.
    NotFound { attempts: Vec<String> },
    /// The file was found and is not a payload this build can parse.
    Malformed { source: Source, message: String },
    /// The command line and the payload name different staging partitions.
    StagePartuuidMismatch { cmdline: String, payload: String },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAStagedBoot => write!(
                f,
                "This live environment was not started by the CrownOS Windows \
                 installer, so there is no handoff file to act on. Run the Windows \
                 installer first."
            ),
            Self::NoRouteToPayload { attempts } => write!(
                f,
                "The boot entry does not say where the handoff file is. Tried:\n{}",
                attempts.join("\n")
            ),
            Self::NotFound { attempts } => write!(
                f,
                "The handoff file ({FILE_NAME}) could not be found. Tried:\n{}",
                attempts.join("\n")
            ),
            Self::Malformed { source, message } => write!(
                f,
                "The handoff file at {source} could not be read: {message}"
            ),
            Self::StagePartuuidMismatch { cmdline, payload } => write!(
                f,
                "The boot entry says the installer files are on partition {cmdline} \
                 and the handoff file says {payload}. Those came from two different \
                 runs of the Windows installer, so neither can be trusted and \
                 nothing has been changed."
            ),
        }
    }
}

impl std::error::Error for LoadError {}

/// Find and parse the handoff payload.
///
/// Tries the command line's path first and the staging partition second; see the
/// module docs for why in that order. The fallback's mount is always unmounted,
/// including when the read or the parse fails — a leftover mount of the staging
/// partition is exactly what would make
/// [`Refusal::StagingMediumStillInUse`](crate::engine::Refusal::StagingMediumStillInUse)
/// fire later, and an installer that trips its own safety check is an installer
/// nobody can use.
pub fn locate(args: &CmdlineArgs, medium: &dyn Medium) -> Result<Located, LoadError> {
    if !args.is_staged_boot() {
        return Err(LoadError::NotAStagedBoot);
    }

    let mut attempts = Vec::new();

    // Strategy 1: the path on the command line.
    if let Some(path) = &args.oobe_file {
        match medium.read(path) {
            Some(json) => {
                attempts.push(format!("{} — found", path.display()));
                return finish(json, Source::CmdlinePath(path.clone()), attempts);
            }
            None => attempts.push(format!(
                "{} — not there (copytoram has probably already unmounted {BOOTMNT})",
                path.display()
            )),
        }
    } else {
        attempts.push("oobe_file= was not on the kernel command line".to_owned());
    }

    // Strategy 2: mount the staging partition and look inside it.
    let (Some(partuuid), Some(relative)) = (&args.stage_partuuid, &args.oobe_relative) else {
        attempts.push(
            "crownos.stage_partuuid= and crownos.oobe= were not both on the kernel \
             command line, so there is nothing left to try"
                .to_owned(),
        );
        return Err(LoadError::NoRouteToPayload { attempts });
    };

    let mount = Path::new(FALLBACK_MOUNT);
    if let Err(error) = medium.mount_read_only(partuuid, mount) {
        attempts.push(format!(
            "mounting partition {partuuid} read-only at {FALLBACK_MOUNT} — failed: {error}"
        ));
        return Err(LoadError::NotFound { attempts });
    }
    attempts.push(format!(
        "mounted partition {partuuid} read-only at {FALLBACK_MOUNT}"
    ));

    let path = mount.join(relative);
    let read = medium.read(&path);

    // Unmounted before the result is inspected, so there is no path out of this
    // function that leaves the staging partition mounted.
    if let Err(error) = medium.unmount(mount) {
        attempts.push(format!("unmounting {FALLBACK_MOUNT} — failed: {error}"));
    } else {
        attempts.push(format!("unmounted {FALLBACK_MOUNT}"));
    }

    match read {
        Some(json) => {
            attempts.push(format!("{relative} — found on the staging partition"));
            finish(
                json,
                Source::StagingPartition {
                    partuuid: partuuid.clone(),
                    relative: relative.clone(),
                },
                attempts,
            )
        }
        None => {
            attempts.push(format!("{relative} — not on the staging partition either"));
            Err(LoadError::NotFound { attempts })
        }
    }
}

fn finish(json: String, source: Source, attempts: Vec<String>) -> Result<Located, LoadError> {
    match Oobe::from_json(&json) {
        Ok(oobe) => Ok(Located {
            oobe,
            source,
            attempts,
        }),
        Err(error) => Err(LoadError::Malformed {
            source,
            message: error.to_string(),
        }),
    }
}

/// Check that the command line and the payload agree about which partition the
/// installer files are on.
///
/// A command line with no `crownos.stage_partuuid=` is not a mismatch — an older
/// boot entry simply does not carry the field, and refusing on its absence would
/// break the very upgrade path the schema version exists to manage.
pub fn check_stage_partuuid(args: &CmdlineArgs, oobe: &Oobe) -> Result<(), LoadError> {
    let Some(cmdline) = &args.stage_partuuid else {
        return Ok(());
    };
    let payload = oobe.staging.partuuid.trim().to_ascii_lowercase();
    if cmdline.eq_ignore_ascii_case(&payload) {
        return Ok(());
    }
    Err(LoadError::StagePartuuidMismatch {
        cmdline: cmdline.clone(),
        payload,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A [`Medium`] where mounting genuinely changes what is readable — which is
    /// the whole behaviour the resolver exists to handle.
    pub(crate) struct FakeMedium {
        /// Paths readable without mounting anything.
        always: HashMap<String, String>,
        /// Paths readable only while the staging partition is mounted, keyed by
        /// their path *within* the partition.
        on_partition: HashMap<String, String>,
        /// PARTUUID the fake will accept a mount of; anything else fails.
        mountable: Option<String>,
        mounted: RefCell<bool>,
        unmount_fails: bool,
        journal: RefCell<Vec<String>>,
    }

    impl FakeMedium {
        pub(crate) fn new() -> Self {
            Self {
                always: HashMap::new(),
                on_partition: HashMap::new(),
                mountable: Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_owned()),
                mounted: RefCell::new(false),
                unmount_fails: false,
                journal: RefCell::new(Vec::new()),
            }
        }

        pub(crate) fn readable_now(mut self, path: &str, contents: &str) -> Self {
            self.always.insert(path.to_owned(), contents.to_owned());
            self
        }

        fn on_partition(mut self, relative: &str, contents: &str) -> Self {
            self.on_partition
                .insert(relative.to_owned(), contents.to_owned());
            self
        }

        fn unmountable(mut self) -> Self {
            self.mountable = None;
            self
        }

        fn with_failing_unmount(mut self) -> Self {
            self.unmount_fails = true;
            self
        }

        fn journal(&self) -> Vec<String> {
            self.journal.borrow().clone()
        }

        fn is_mounted(&self) -> bool {
            *self.mounted.borrow()
        }
    }

    impl Medium for FakeMedium {
        fn read(&self, path: &Path) -> Option<String> {
            let path = path.to_string_lossy().into_owned();
            self.journal.borrow_mut().push(format!("read {path}"));
            if let Some(contents) = self.always.get(&path) {
                return Some(contents.clone());
            }
            if *self.mounted.borrow() {
                let prefix = format!("{FALLBACK_MOUNT}/");
                if let Some(relative) = path.strip_prefix(&prefix) {
                    return self.on_partition.get(relative).cloned();
                }
            }
            None
        }

        fn mount_read_only(&self, partuuid: &str, at: &Path) -> Result<(), String> {
            self.journal
                .borrow_mut()
                .push(format!("mount ro {partuuid} at {}", at.display()));
            match &self.mountable {
                Some(allowed) if allowed == partuuid => {
                    *self.mounted.borrow_mut() = true;
                    Ok(())
                }
                _ => Err("no such device".to_owned()),
            }
        }

        fn unmount(&self, at: &Path) -> Result<(), String> {
            self.journal
                .borrow_mut()
                .push(format!("umount {}", at.display()));
            if self.unmount_fails {
                return Err("target is busy".to_owned());
            }
            *self.mounted.borrow_mut() = false;
            Ok(())
        }
    }

    pub(crate) fn payload_json() -> String {
        // Built from the schema rather than pasted, so a schema change breaks the
        // golden test in `oobe` and not this one as well.
        let oobe = Oobe {
            schema_version: crate::oobe::SCHEMA_VERSION,
            generated_by: "windows-migration-assistant 0.1.0".to_owned(),
            generated_at_unix: 1_756_000_000,
            install: crate::oobe::Install {
                auto_install: true,
                allocated_bytes: 128 * 1024 * 1024 * 1024,
                swap: crate::oobe::SwapPlan::None,
                root_filesystem: crate::oobe::RootFilesystem::Ext4,
                keep_windows: true,
            },
            target: crate::oobe::Target {
                esp_partuuid: "0fc63daf-8483-4772-8e79-3d69d8477de4".to_owned(),
                esp_size_bytes: 260 * 1024 * 1024,
                windows_partuuid: "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7".to_owned(),
                disk_guid: None,
                windows_disk_number: 0,
                expected_free_start_bytes: 0,
                expected_free_bytes: 0,
                logical_sector_size: 512,
            },
            staging: crate::oobe::Staging {
                kind: crate::oobe::StagingKind::Partition,
                partuuid: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_owned(),
                base_dir: "EFI/ArchStage".to_owned(),
                reclaim_after_install: true,
            },
            user: None,
            locale: crate::oobe::Locale::default(),
        };
        oobe.to_json().expect("serialize")
    }

    fn staged_args() -> CmdlineArgs {
        crate::cmdline::parse(crate::cmdline::tests::STAGED)
    }

    pub(crate) const CMDLINE_PATH: &str = "/run/archiso/bootmnt/EFI/ArchStage/oobe.json";
    const RELATIVE: &str = "EFI/ArchStage/oobe.json";

    /// The fast path: `bootmnt` is still mounted, so nothing is mounted by us.
    #[test]
    fn the_cmdline_path_is_preferred_when_it_exists() {
        let medium = FakeMedium::new().readable_now(CMDLINE_PATH, &payload_json());
        let located = locate(&staged_args(), &medium).expect("locate");

        assert_eq!(located.source, Source::CmdlinePath(CMDLINE_PATH.into()));
        assert!(
            !medium.journal().iter().any(|l| l.starts_with("mount")),
            "nothing should have been mounted: {:?}",
            medium.journal()
        );
    }

    /// The case the whole module exists for: `copytoram` already unmounted
    /// `bootmnt`, so the path on the command line is gone.
    #[test]
    fn the_staging_partition_is_mounted_when_the_cmdline_path_has_gone() {
        let medium = FakeMedium::new().on_partition(RELATIVE, &payload_json());
        let located = locate(&staged_args(), &medium).expect("locate");

        assert_eq!(
            located.source,
            Source::StagingPartition {
                partuuid: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_owned(),
                relative: RELATIVE.to_owned(),
            }
        );
        let journal = medium.journal();
        let mount = journal
            .iter()
            .position(|l| l.starts_with("mount"))
            .expect("a mount");
        let read = journal
            .iter()
            .position(|l| l.contains(FALLBACK_MOUNT) && l.starts_with("read"))
            .expect("a read inside the mount");
        let umount = journal
            .iter()
            .position(|l| l.starts_with("umount"))
            .expect("an unmount");
        assert!(mount < read && read < umount, "{journal:?}");
        assert!(!medium.is_mounted(), "the mount must not be left behind");
    }

    /// A mount left behind would trip the planner's own safety check later, so
    /// the unmount has to happen even when the file is not there.
    #[test]
    fn the_fallback_mount_is_released_even_when_the_payload_is_missing() {
        let medium = FakeMedium::new();
        let error = locate(&staged_args(), &medium).expect_err("must fail");
        assert!(matches!(error, LoadError::NotFound { .. }), "{error:?}");
        assert!(!medium.is_mounted());
        assert!(medium.journal().iter().any(|l| l.starts_with("umount")));
    }

    /// And even when the file is there but unparseable.
    #[test]
    fn the_fallback_mount_is_released_even_when_the_payload_is_malformed() {
        let medium = FakeMedium::new().on_partition(RELATIVE, "{ not json");
        let error = locate(&staged_args(), &medium).expect_err("must fail");
        assert!(matches!(error, LoadError::Malformed { .. }), "{error:?}");
        assert!(!medium.is_mounted());
    }

    /// A failed unmount is recorded and does not lose the payload — the file was
    /// already read by then.
    #[test]
    fn a_failed_unmount_is_reported_but_does_not_lose_the_payload() {
        let medium = FakeMedium::new()
            .on_partition(RELATIVE, &payload_json())
            .with_failing_unmount();
        let located = locate(&staged_args(), &medium).expect("locate");
        assert!(
            located
                .attempts
                .iter()
                .any(|a| a.contains("unmounting") && a.contains("failed"))
        );
    }

    #[test]
    fn a_partition_that_will_not_mount_is_reported_with_both_attempts() {
        let medium = FakeMedium::new().unmountable();
        let error = locate(&staged_args(), &medium).expect_err("must fail");
        match error {
            LoadError::NotFound { attempts } => {
                assert!(attempts.iter().any(|a| a.contains(CMDLINE_PATH)));
                assert!(attempts.iter().any(|a| a.contains("failed")));
            }
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    /// A staged boot with no route to the payload at all is a different error
    /// from one where the routes were tried and came up empty.
    #[test]
    fn a_staged_boot_with_no_route_to_the_payload_says_so() {
        let args = crate::cmdline::parse("crownos.stage=1 copytoram=y quiet");
        let medium = FakeMedium::new();
        let error = locate(&args, &medium).expect_err("must fail");
        assert!(
            matches!(error, LoadError::NoRouteToPayload { .. }),
            "{error:?}"
        );
        assert!(
            !medium.journal().iter().any(|l| l.starts_with("mount")),
            "nothing to mount, so nothing should have been mounted"
        );
    }

    /// A hand-booted live ISO is not an error to shout about.
    #[test]
    fn a_hand_booted_iso_is_reported_as_such() {
        let args = crate::cmdline::parse("archisobasedir=arch quiet");
        let error = locate(&args, &FakeMedium::new()).expect_err("must fail");
        assert_eq!(error, LoadError::NotAStagedBoot);
    }

    /// The fallback needs `crownos.oobe=` as well as the PARTUUID; with only one
    /// of the two there is nothing to try.
    #[test]
    fn half_a_fallback_is_no_fallback() {
        let args = crate::cmdline::parse(
            "crownos.stage=1 crownos.stage_partuuid=aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        );
        let error = locate(&args, &FakeMedium::new()).expect_err("must fail");
        assert!(
            matches!(error, LoadError::NoRouteToPayload { .. }),
            "{error:?}"
        );
    }

    // --- Cross-check ---

    #[test]
    fn a_matching_stage_partuuid_passes_the_cross_check() {
        let oobe = Oobe::from_json(&payload_json()).expect("parse");
        check_stage_partuuid(&staged_args(), &oobe).expect("they agree");
    }

    #[test]
    fn a_disagreeing_stage_partuuid_is_refused() {
        let mut oobe = Oobe::from_json(&payload_json()).expect("parse");
        oobe.staging.partuuid = "99999999-9999-9999-9999-999999999999".to_owned();
        let error = check_stage_partuuid(&staged_args(), &oobe).expect_err("must refuse");
        match error {
            LoadError::StagePartuuidMismatch { cmdline, payload } => {
                assert_eq!(cmdline, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
                assert_eq!(payload, "99999999-9999-9999-9999-999999999999");
            }
            other => panic!("expected a mismatch, got {other:?}"),
        }
    }

    /// Case is not a disagreement — `sfdisk` prints these upper case and
    /// `lsblk` prints them lower case.
    #[test]
    fn the_cross_check_ignores_case() {
        let mut oobe = Oobe::from_json(&payload_json()).expect("parse");
        oobe.staging.partuuid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE".to_owned();
        check_stage_partuuid(&staged_args(), &oobe).expect("case must not matter");
    }

    /// An older boot entry with no `crownos.stage_partuuid=` has nothing to
    /// disagree with, and refusing on its absence would break the upgrade path.
    #[test]
    fn a_cmdline_without_the_field_cannot_disagree() {
        let args = crate::cmdline::parse("crownos.stage=1");
        let oobe = Oobe::from_json(&payload_json()).expect("parse");
        check_stage_partuuid(&args, &oobe).expect("nothing to check");
    }
}
