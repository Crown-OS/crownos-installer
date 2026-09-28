//! Turning a handoff payload plus the disk's actual state into an ordered plan —
//! or refusing to.
//!
//! This module contains no I/O. It takes an [`Oobe`] (what the user asked for,
//! on the other side of a reboot) and a [`SystemSnapshot`] (what is really on the
//! disk now) and returns either a [`Plan`] or a [`Refusal`]. That is deliberate
//! and it is the single most important structural decision in this crate: every
//! rule below is a pure function of two values, so every rule below is a unit
//! test rather than a thing we hope someone noticed in review.
//!
//! # The safety invariants
//!
//! In rough order of how bad it would be to get them wrong:
//!
//! 1. **The disk is located by PARTUUID, never by name.** The payload names the
//!    ESP's partition GUID; the disk that partition belongs to *is* the target.
//!    `/dev/sda` is a property of this boot's enumeration order and nothing else.
//!    See [`locate_target`].
//! 2. **A payload from a newer schema is refused.** Acting on a description you
//!    only half understand is how the wrong partition gets erased.
//!    [`Refusal::UnsupportedSchema`].
//! 3. **`keep_windows: false` is refused outright.** This installer only ever
//!    does dual-boot. A payload that says otherwise has lost its meaning
//!    somewhere, and "wipe the disk" is not a reasonable reading of a corrupted
//!    file. [`Refusal::WholeDiskInstallRefused`].
//! 4. **The Windows partition is never touched.** It is looked up, checked to be
//!    on the same disk, and then treated as immovable. Every generated step is
//!    scanned afterwards to prove no destructive operation names it —
//!    [`verify_nothing_pre_existing_is_destroyed`], which is a belt to the
//!    braces of simply not emitting such a step.
//! 5. **The free region has to be where we left it.** The payload records where
//!    Windows said the freed space was; if the actual table disagrees by more
//!    than [`REGION_TOLERANCE`], the disk changed under us and we stop.
//!    [`Refusal::FreeRegionMismatch`] — "the disk is not in the state we left it
//!    in" is a vastly better failure than partitioning blind.
//! 6. **Nothing is written to the staging medium while the live system is still
//!    reading from it.** If it is mounted and the payload is not in RAM, we
//!    refuse ([`Refusal::StagingMediumStillInUse`]); if it is mounted and the
//!    payload *is* in RAM, the plan unmounts it first, as its own auditable step.
//! 7. **The staging partition is only reclaimed when all three of** the payload
//!    asked for it, the payload is provably in RAM, and the partition sits
//!    immediately after the new root partition **hold**. Otherwise the space is
//!    left alone and the reason is recorded in [`Plan::warnings`].
//! 8. **No pre-existing partition is destroyed.** The ESP is *written to* — that
//!    is the whole point of a shared ESP — but only at the file level, and never
//!    under `EFI/Microsoft`. Every other existing partition is untouched at
//!    every level. See [`Impact`].
//!
//! # Why the plan is a value
//!
//! [`exec`](super::exec) is not allowed to decide anything. Every offset, every
//! device node, every package name and every file's contents is fixed here, which
//! means the confirmation screen can show the user the actual plan rather than a
//! summary of one, a dry run is a faithful rehearsal rather than an approximation,
//! and the install log is a diff away from the plan that produced it.
//!
//! The one thing a plan does not carry is the password — see
//! [`Secrets`](super::exec::Secrets).

use std::fmt;

use crate::oobe::{Locale, Oobe, RootFilesystem, StagingKind, SwapPlan, UserAccount};

use super::distro::{nixos, Distro, ImperativeDistro};
use super::probe::{Disk, Gap, Partition, SystemSnapshot};

// --- MARK: Constants ---

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

/// Partition alignment. One mebibyte, which is what every partitioning tool has
/// used since 4Kn disks and SSD erase blocks made 63-sector alignment a
/// performance bug.
pub const ALIGNMENT_BYTES: u64 = MIB;

/// How far the actual free region may differ from the payload's description
/// before we call it a different disk.
///
/// Generous, because there are three legitimate sources of drift between what
/// Windows reported and what the partition table says: Windows quantizes a
/// shrink to its own cluster size, the freed region's ends get aligned, and a
/// staging partition may have been carved out of one end of it. Sixty-four
/// mebibytes covers all three and is still four orders of magnitude smaller than
/// the region itself, so it cannot mask a *different* free region being picked.
pub const REGION_TOLERANCE: u64 = 64 * MIB;

/// The smallest root filesystem worth creating.
///
/// A base Arch install is about 2 GiB and a desktop is about 8; ten leaves room
/// to boot, update once and log in. Below this the install would technically
/// succeed and then fill up during the first `pacman -Syu`, which is a much worse
/// outcome than refusing here.
pub const MIN_ROOT_BYTES: u64 = 10 * GIB;

/// Below this, a shared ESP gets a warning: it will hold one kernel and one
/// initramfs alongside Windows' bootloader, and not two.
pub const COMFORTABLE_ESP_BYTES: u64 = 300 * MIB;

/// Where the target root filesystem is mounted while we build it.
pub const ROOT_MOUNT: &str = "/mnt";

/// Where the shared ESP is mounted, inside the target root.
///
/// `/efi` rather than `/boot`: a Windows ESP is typically 100–300 MiB, which is
/// not enough to be a Linux `/boot`, and mounting it there would put pacman's
/// kernel packages on a filesystem that cannot hold them. The kernel and
/// initramfs are copied onto it explicitly instead — see [`Step::InstallKernel`].
pub const ESP_MOUNT_REL: &str = "/efi";

/// GPT type GUID for a discoverable x86-64 root partition.
pub const TYPE_LINUX_ROOT_X86_64: &str = "4f68bce3-e8cd-4db1-96e7-fbcaf984b709";
/// GPT type GUID for a Linux swap partition.
pub const TYPE_LINUX_SWAP: &str = "0657fd6d-a4ab-43c4-84e5-0933c84b4f4f";

/// GPT partition name given to the root partition. Shows up in Windows' Disk
/// Management, so it says what it is.
pub const ROOT_PARTLABEL: &str = "CrownOS";
/// GPT partition name given to the swap partition.
pub const SWAP_PARTLABEL: &str = "CrownOS swap";
/// Filesystem label on the root partition.
pub const ROOT_FSLABEL: &str = "crownos";

/// Directory on the ESP that CrownOS owns, relative to the ESP root.
///
/// Everything this installer writes to the shared ESP goes under here, under
/// `loader/`, or where the base's own boot tooling keeps kernels —
/// [`BLS_ENTRY_TOKEN`](super::distro::BLS_ENTRY_TOKEN)`/` for `kernel-install`,
/// `EFI/nixos/` for NixOS. Nothing goes near `EFI/Microsoft`.
pub const ESP_CROWNOS_DIR: &str = "EFI/CrownOS";

/// The one path on the shared ESP that `bootctl install` may overwrite, and
/// which is therefore backed up first. See [`Step::BackUpFallbackBootloader`].
pub const ESP_FALLBACK_LOADER: &str = "EFI/BOOT/BOOTX64.EFI";

// --- MARK: Account ---

/// The account details the plan will create, with no credential in sight.
///
/// Built either from [`Oobe::user`] or from what the user typed on the account
/// page. The password travels separately — see
/// [`Secrets`](super::exec::Secrets).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Account {
    pub username: String,
    pub full_name: Option<String>,
    pub hostname: String,
    pub autologin: bool,
}

impl Account {
    /// The account a payload pre-filled.
    pub fn from_payload(user: &UserAccount) -> Self {
        Self {
            username: user.username.clone(),
            full_name: user.full_name.clone(),
            hostname: user.hostname.clone(),
            autologin: user.autologin,
        }
    }

    /// Check the name against the rules `useradd` actually enforces, so a bad
    /// name is a message on the account page rather than a failure two thirds of
    /// the way through a `pacstrap`.
    fn validate(&self) -> Result<(), Refusal> {
        let name = &self.username;
        let bad = |reason: &str| {
            Err(Refusal::InvalidUsername {
                username: name.clone(),
                reason: reason.to_owned(),
            })
        };

        if name.is_empty() {
            return bad("it is empty");
        }
        if name.len() > 32 {
            return bad("it is longer than 32 characters");
        }
        if !name
            .starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
        {
            return bad("it must start with a lowercase letter or an underscore");
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        {
            return bad("it may only contain lowercase letters, digits, `_` and `-`");
        }
        // These exist already, and `useradd` would fail on them — but so would
        // silently installing over one of them, which is worse.
        if matches!(
            name.as_str(),
            "root" | "bin" | "daemon" | "nobody" | "systemd-network" | "mail" | "ftp" | "http"
        ) {
            return bad("it is a reserved system account name");
        }

        let host = &self.hostname;
        let bad_host = |reason: &str| {
            Err(Refusal::InvalidHostname {
                hostname: host.clone(),
                reason: reason.to_owned(),
            })
        };
        if host.is_empty() {
            return bad_host("it is empty");
        }
        if host.len() > 63 {
            return bad_host("it is longer than 63 characters");
        }
        if host.starts_with('-') || host.ends_with('-') {
            return bad_host("it may not start or end with a hyphen");
        }
        if !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return bad_host("it may only contain letters, digits and hyphens");
        }

        Ok(())
    }
}

/// The partition GUIDs the new partitions will be created with.
///
/// Supplied by the caller rather than generated here so that [`plan`] stays a
/// pure function — and so that the plan shown to the user names the exact GUID
/// the bootloader entry will reference, instead of promising to fill it in later.
/// [`crate::main`] reads them from `/proc/sys/kernel/random/uuid`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewIds {
    pub root_partuuid: String,
    pub swap_partuuid: String,
}

/// Everything [`plan`] needs.
#[derive(Clone, Debug)]
pub struct PlanInput<'a> {
    /// What the Windows side decided.
    pub oobe: &'a Oobe,
    /// What the disk actually looks like now.
    pub snapshot: &'a SystemSnapshot,
    /// Who to create.
    pub account: Account,
    /// GUIDs for the partitions about to be created.
    pub ids: NewIds,
}

// --- MARK: Refusals ---

/// Why we are not going to do this.
///
/// Every variant is a state in which continuing could destroy something the user
/// cares about. They are values rather than strings so the UI can react to them —
/// and so each one can have a test named after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload was written by a newer version of the schema than this build
    /// understands.
    UnsupportedSchema { found: u32, understood: u32 },
    /// `keep_windows` was not set. This installer does not do whole-disk
    /// installs, at all, ever.
    WholeDiskInstallRefused,
    /// No partition anywhere has the ESP's PARTUUID, so the target disk cannot
    /// be identified.
    EspNotFound { partuuid: String },
    /// The disk holding the ESP is not GPT, which means the payload's PARTUUIDs
    /// describe a table that no longer exists.
    NotGpt { disk: String, table: String },
    /// The Windows partition is not on the disk we found the ESP on.
    WindowsPartitionNotFound { partuuid: String, disk: String },
    /// The payload says a whole partition was staged, and there is no such
    /// partition.
    StagingPartitionNotFound { partuuid: String },
    /// Something still has the staging medium mounted, and the live system has
    /// not been copied into RAM, so it is still being read from.
    StagingMediumStillInUse {
        partuuid: String,
        mounted_at: Vec<String>,
    },
    /// The disk has no unallocated space at all.
    NoFreeRegion { disk: String },
    /// The unallocated space is not where the payload said it would be.
    FreeRegionMismatch {
        expected_start_bytes: u64,
        expected_bytes: u64,
        actual_start_bytes: u64,
        actual_bytes: u64,
        tolerance_bytes: u64,
    },
    /// There is a free region in the right place, and it is too small to install
    /// into.
    FreeRegionTooSmall { available_bytes: u64, needed_bytes: u64 },
    /// The requested swap partition leaves too little room for a root partition.
    SwapDoesNotFit {
        swap_bytes: u64,
        region_bytes: u64,
        min_root_bytes: u64,
    },
    /// A generated step would destroy a partition that was already on the disk.
    /// This should be impossible; it is checked because "should be impossible"
    /// is not a safety property.
    WouldDestroyExistingPartition { device: String, step: String },
    /// The account details will not survive `useradd`.
    InvalidUsername { username: String, reason: String },
    /// The hostname will not survive `hostnamectl`.
    InvalidHostname { hostname: String, reason: String },

    // --- ISO installs only. See `plan_iso`. ---
    /// No disk was chosen, or the chosen one is no longer there.
    NoTargetDisk { disk: String },
    /// The chosen disk has no EFI System Partition, and a UEFI machine cannot
    /// boot without one.
    NoEspOnDisk { disk: String },
    /// The chosen ESP is not on the chosen disk. A firmware that boots from one
    /// disk's ESP into another disk's root is a machine that stops booting the
    /// moment either is removed.
    EspOnAnotherDisk { esp: String, disk: String },
    /// The free region the user picked is not free any more.
    ChosenRegionNotFree { start_sector: u64 },
    /// A queued partition-editor operation was refused. Carried as a string
    /// because [`OpRefusal`](super::disks::OpRefusal) explains itself far better
    /// than a planner-level restatement of it could.
    EditRefused { detail: String },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema { found, understood } => write!(
                f,
                "This installer understands handoff schema {understood}, and the \
                 file it was given is schema {found}. A newer payload may describe \
                 the disk in ways this build would get wrong, so nothing has been \
                 changed. Use the CrownOS image that came with your installer."
            ),
            Self::WholeDiskInstallRefused => write!(
                f,
                "The handoff file does not say to keep Windows. This installer only \
                 ever installs alongside an existing system, so it has stopped \
                 rather than guess what that means."
            ),
            Self::EspNotFound { partuuid } => write!(
                f,
                "No partition on this machine has the identifier {partuuid}. That is \
                 the EFI System Partition the Windows installer recorded, and it is \
                 how the target disk is identified — without it there is nothing safe \
                 to install to."
            ),
            Self::NotGpt { disk, table } => write!(
                f,
                "{disk} has a {table} partition table, not GPT. The identifiers in \
                 the handoff file describe a GPT disk, so this is not the disk they \
                 were written for."
            ),
            Self::WindowsPartitionNotFound { partuuid, disk } => write!(
                f,
                "The Windows partition {partuuid} is not on {disk}. The disk is not \
                 in the state the Windows installer left it in, and continuing would \
                 mean partitioning a disk we cannot account for."
            ),
            Self::StagingPartitionNotFound { partuuid } => write!(
                f,
                "The installer files were staged on partition {partuuid}, and no such \
                 partition exists any more."
            ),
            Self::StagingMediumStillInUse { partuuid, mounted_at } => write!(
                f,
                "The live installer is still reading from partition {partuuid} \
                 (mounted at {}), and the plan needs to write to it. The system was \
                 supposed to copy itself into memory at boot and did not, so writing \
                 there now would pull the floor out from under this program.",
                mounted_at.join(", ")
            ),
            Self::NoFreeRegion { disk } => write!(
                f,
                "{disk} has no unallocated space. The room the Windows installer \
                 freed is gone — something has used it since."
            ),
            Self::FreeRegionMismatch {
                expected_start_bytes,
                expected_bytes,
                actual_start_bytes,
                actual_bytes,
                tolerance_bytes,
            } => write!(
                f,
                "The disk is not in the state we left it in. Windows freed {} at \
                 offset {}, and the largest unallocated region now is {} at offset \
                 {} — further apart than the {} of slack this check allows. Nothing \
                 has been changed.",
                bytes(*expected_bytes),
                bytes(*expected_start_bytes),
                bytes(*actual_bytes),
                bytes(*actual_start_bytes),
                bytes(*tolerance_bytes),
            ),
            Self::FreeRegionTooSmall { available_bytes, needed_bytes } => write!(
                f,
                "There is only {} of unallocated space, and CrownOS needs at least \
                 {}.",
                bytes(*available_bytes),
                bytes(*needed_bytes)
            ),
            Self::SwapDoesNotFit { swap_bytes, region_bytes, min_root_bytes } => write!(
                f,
                "A {} swap partition inside {} would leave less than {} for the \
                 system itself.",
                bytes(*swap_bytes),
                bytes(*region_bytes),
                bytes(*min_root_bytes)
            ),
            Self::WouldDestroyExistingPartition { device, step } => write!(
                f,
                "Internal check failed: the step `{step}` would destroy {device}, \
                 which already existed before this install. This is a bug in the \
                 installer and it has stopped rather than proceed."
            ),
            Self::InvalidUsername { username, reason } => {
                write!(f, "`{username}` is not a usable username: {reason}.")
            }
            Self::InvalidHostname { hostname, reason } => {
                write!(f, "`{hostname}` is not a usable computer name: {reason}.")
            }

            // --- ISO installs only ---
            Self::NoTargetDisk { disk } => write!(
                f,
                "{disk} is not a disk this installer can see. It may have been removed, \
                 or its partition table may have changed since the list was drawn."
            ),
            Self::NoEspOnDisk { disk } => write!(
                f,
                "{disk} has no EFI system partition. CrownOS boots through UEFI and \
                 needs one to put its kernel and boot loader on. Use the partition \
                 editor to make one — a few hundred megabytes of FAT32 flagged `esp` \
                 is enough — and then come back to this step."
            ),
            Self::EspOnAnotherDisk { esp, disk } => write!(
                f,
                "{esp} is not on {disk}. CrownOS will not put a root partition on one \
                 disk and its boot loader on another: a machine set up that way stops \
                 booting the moment either disk is removed or reordered."
            ),
            Self::ChosenRegionNotFree { start_sector } => write!(
                f,
                "The free space that was chosen (starting at sector {start_sector}) is \
                 not unallocated any more. The partition table changed after the list \
                 was drawn, so nothing has been written to it."
            ),
            Self::EditRefused { detail } => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for Refusal {}

// --- MARK: Plan ---

/// A partition this plan is going to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewPartition {
    /// Partition number in the GPT.
    pub number: u32,
    /// The device node it will have once the table is re-read.
    pub path: String,
    /// The PARTUUID it will be created with.
    pub partuuid: String,
    pub start_sector: u64,
    /// Inclusive.
    pub end_sector: u64,
    pub sector_size: u32,
}

impl NewPartition {
    pub fn sectors(&self) -> u64 {
        self.end_sector - self.start_sector + 1
    }

    pub fn size_bytes(&self) -> u64 {
        self.sectors() * u64::from(self.sector_size)
    }
}

/// An existing partition the plan refers to but (mostly) does not change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExistingPartition {
    pub path: String,
    pub partuuid: String,
    pub size_bytes: u64,
}

/// What becomes of the staging partition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reclaim {
    /// Delete it and grow the root partition over it.
    GrowRootOver {
        staging: ExistingPartition,
        staging_number: u32,
        /// Root's new last sector, inclusive.
        new_root_end_sector: u64,
    },
    /// Leave it where it is. `reason` is shown to the user, because a gigabyte
    /// of unexplained FAT32 partition is worse than a gigabyte of explained one.
    Keep { reason: String },
}

/// An ordered, complete description of an install.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The disk, located by PARTUUID and never by name.
    pub disk: String,
    pub sector_size: u32,
    /// Total size of the disk, for the allocation bar.
    pub disk_size_bytes: u64,
    pub root: NewPartition,
    pub swap_partition: Option<NewPartition>,
    /// A swapfile's size, when swap is a file rather than a partition.
    pub swap_file_bytes: Option<u64>,
    pub root_filesystem: RootFilesystem,
    /// The distribution under the desktop.
    pub distro: Distro,
    /// The shared ESP. Written to at the file level; never reformatted.
    pub esp: ExistingPartition,
    /// The partition of the operating system already on this disk, which this
    /// install must not touch.
    ///
    /// `Some` for every install driven by a handoff payload, where it is the
    /// Windows partition and proving it untouched is the whole point.
    ///
    /// `None` only for an ISO install onto a disk with nothing on it to
    /// protect. It is an `Option` rather than a sentinel because "there is no
    /// other operating system" and "there is one and it is `/dev/sda0`" must not
    /// be the same value: [`verify_nothing_pre_existing_is_destroyed`] asserts
    /// this partition is never a destructive target, and a check against a
    /// partition that does not exist is a check that always passes.
    pub windows: Option<ExistingPartition>,
    /// What happens to the staging medium.
    pub reclaim: Reclaim,
    pub account: Account,
    pub locale: Locale,
    /// How much space Windows is left holding, for the allocation bar.
    pub windows_bytes: u64,
    /// The steps, in the order they must run.
    pub steps: Vec<Step>,
    /// Things the user should know that are not reasons to stop.
    pub warnings: Vec<String>,
    /// Devices this plan is *permitted* to destroy even though they existed
    /// before it ran, because the user named them one at a time in the partition
    /// editor.
    ///
    /// [`verify_nothing_pre_existing_is_destroyed`] refuses any step that
    /// destroys a pre-existing partition. That rule is what stops a bug from
    /// eating a disk, and it also stops the editor from doing its job — so the
    /// exception is a list rather than a flag. A plan may destroy `/dev/sda3`
    /// only if `/dev/sda3` is in here, which is only if a
    /// [`DiskOp`](super::disks::DiskOp) in the queue named it. "The user asked
    /// for partition editing" is not enough; "the user asked to delete this
    /// partition" is.
    pub authorized_destructive: Vec<String>,
}

impl Plan {
    /// Total bytes this install is going to occupy.
    pub fn crownos_bytes(&self) -> u64 {
        self.root.size_bytes()
            + self
                .swap_partition
                .as_ref()
                .map_or(0, NewPartition::size_bytes)
    }

    /// The device nodes that will be created, formatted, resized or deleted.
    ///
    /// This is the list the confirmation page shows. If a device the user cares
    /// about is on it, that is the moment to find out.
    pub fn destructive_devices(&self) -> Vec<&str> {
        let mut devices: Vec<&str> = self
            .steps
            .iter()
            .filter_map(|step| match step.impact() {
                Impact::Destructive(device) => Some(device),
                _ => None,
            })
            .collect();
        devices.sort_unstable();
        devices.dedup();
        devices
    }

    /// The device nodes that get files written into them without being
    /// reformatted — in practice, the shared ESP and nothing else.
    pub fn file_level_devices(&self) -> Vec<&str> {
        let mut devices: Vec<&str> = self
            .steps
            .iter()
            .filter_map(|step| match step.impact() {
                Impact::FileLevel(device) => Some(device),
                _ => None,
            })
            .collect();
        devices.sort_unstable();
        devices.dedup();
        devices
    }
}

// --- MARK: Steps ---

/// One operation in a plan.
///
/// Coarse on purpose: a step is a unit the user can be told about ("Installing
/// packages"), and it expands to a handful of commands in
/// [`exec`](super::exec). Splitting `pacstrap` into thirty commands would make
/// the progress bar smoother and the plan unreadable, which is the wrong trade
/// for a program whose plan is meant to be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Carry out one partition-table change the user asked for in the editor.
    ///
    /// Always before everything else in a plan: the editor's whole purpose is to
    /// make the disk into the shape the rest of the plan needs. The operation is
    /// carried by value rather than expanded into `Step`s of its own because
    /// [`disks`](super::disks) already owns both the safety rules and the
    /// command expansion for it, and splitting it here would put half of a
    /// destructive operation in each of two modules.
    EditPartitionTable(super::disks::DiskOp),
    /// Unmount the live payload's copy of the staging medium, now that it is in
    /// RAM and we are about to write to that partition.
    UnmountStagingMedium { target: String },
    /// Add a partition to the GPT. Only ever *adds*: the sfdisk invocation this
    /// becomes appends an entry and rewrites no existing one.
    CreatePartition {
        disk: String,
        partition: NewPartition,
        type_guid: String,
        name: String,
        role: Role,
    },
    /// Re-read the partition table and wait for udev to create the nodes.
    SettleTable { disk: String },
    /// `mkfs` the root partition.
    MakeFilesystem {
        device: String,
        filesystem: RootFilesystem,
        label: String,
    },
    /// `mkswap` the swap partition.
    MakeSwap { device: String },
    /// Mount the root partition at [`ROOT_MOUNT`].
    MountRoot { device: String, at: String },
    /// Mount the shared ESP inside the target root.
    MountEsp { device: String, at: String },
    /// Install the base system with the distribution's own bootstrapper.
    InstallBaseSystem {
        root: String,
        distro: ImperativeDistro,
        packages: Vec<String>,
    },
    /// Add the CrownOS repository and install the desktop from it.
    InstallDesktop {
        root: String,
        distro: ImperativeDistro,
    },
    /// Generate the hardware configuration and write CrownOS's NixOS
    /// configuration over it.
    ConfigureNixos { root: String, configuration: String },
    /// `nixos-install`, which also installs systemd-boot into the shared ESP.
    InstallNixos { device: String, root: String },
    /// `genfstab` into the new system's `/etc/fstab`.
    GenerateFstab { root: String },
    /// Create a swapfile and add it to fstab.
    CreateSwapFile {
        root: String,
        size_bytes: u64,
        filesystem: RootFilesystem,
    },
    /// Generate and select the locale.
    ConfigureLocale {
        root: String,
        locale: String,
        distro: ImperativeDistro,
    },
    /// `/etc/vconsole.conf`.
    ConfigureKeymap { root: String, keymap: String },
    /// `/etc/localtime` and `hwclock`.
    ConfigureTimezone { root: String, timezone: String },
    /// `/etc/hostname` and `/etc/hosts`.
    ConfigureHostname { root: String, hostname: String },
    /// Copy the ESP's fallback loader aside before `bootctl` can overwrite it.
    BackUpFallbackBootloader { device: String, esp_at: String },
    /// `bootctl install` plus a loader entry, into the shared ESP.
    InstallBootloader {
        device: String,
        root: String,
        esp_rel: String,
        root_partuuid: String,
        kernel_cmdline: String,
        distro: ImperativeDistro,
    },
    /// Put the kernel and initramfs on the ESP, where systemd-boot can read
    /// them.
    InstallKernel {
        device: String,
        root: String,
        esp_rel: String,
        distro: ImperativeDistro,
    },
    /// `useradd`, into the group that may use `sudo`.
    CreateUser {
        root: String,
        username: String,
        full_name: Option<String>,
        admin_group: String,
    },
    /// `chpasswd`, taking the password on stdin. The only step that consumes a
    /// secret.
    SetUserPassword {
        root: String,
        username: String,
        distro: Distro,
    },
    /// `passwd --lock root`.
    LockRootAccount { root: String },
    /// A getty drop-in that logs the user straight in.
    EnableAutologin { root: String, username: String },
    /// `systemctl enable`.
    EnableServices { root: String, units: Vec<String> },
    /// Unmount everything under the target root.
    Unmount { at: String },
    /// Delete the staging partition, now that the install is finished and the
    /// space can go back to the user.
    DeleteStagingPartition {
        disk: String,
        number: u32,
        device: String,
        partuuid: String,
    },
    /// Extend the root partition's table entry over the freed space.
    GrowRootPartition {
        disk: String,
        number: u32,
        device: String,
        new_end_sector: u64,
    },
    /// Extend the root filesystem to fill its partition.
    GrowRootFilesystem {
        device: String,
        filesystem: RootFilesystem,
    },
}

/// What a partition a plan creates is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Root,
    Swap,
}

/// What a step does to a block device, in the only two categories that matter
/// for safety.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Impact<'a> {
    /// Rewrites a partition table entry, or a whole filesystem. Whatever was
    /// there is gone.
    Destructive(&'a str),
    /// Adds or replaces files inside an existing filesystem, leaving everything
    /// else in place. The shared ESP, and only the shared ESP.
    FileLevel(&'a str),
    /// Touches no block device destructively — a mount, a chroot command, a file
    /// under `/mnt`.
    None,
}

impl Step {
    /// A short label for the progress page.
    pub fn label(&self) -> String {
        match self {
            Self::EditPartitionTable(op) => op.label(),
            Self::UnmountStagingMedium { .. } => "Releasing the installer medium".to_owned(),
            Self::CreatePartition { role: Role::Root, .. } => "Creating the CrownOS partition".to_owned(),
            Self::CreatePartition { role: Role::Swap, .. } => "Creating the swap partition".to_owned(),
            Self::SettleTable { .. } => "Re-reading the partition table".to_owned(),
            Self::MakeFilesystem { filesystem, .. } => {
                format!("Formatting as {}", filesystem_name(*filesystem))
            }
            Self::MakeSwap { .. } => "Preparing swap".to_owned(),
            Self::MountRoot { .. } => "Mounting the new system".to_owned(),
            Self::MountEsp { .. } => "Mounting the EFI partition".to_owned(),
            Self::InstallBaseSystem { .. } | Self::InstallNixos { .. } => {
                "Installing packages".to_owned()
            }
            Self::InstallDesktop { .. } => "Installing the CrownOS desktop".to_owned(),
            Self::ConfigureNixos { .. } => "Writing the system configuration".to_owned(),
            Self::GenerateFstab { .. } => "Writing the filesystem table".to_owned(),
            Self::CreateSwapFile { .. } => "Creating the swap file".to_owned(),
            Self::ConfigureLocale { .. } => "Setting the language".to_owned(),
            Self::ConfigureKeymap { .. } => "Setting the keyboard layout".to_owned(),
            Self::ConfigureTimezone { .. } => "Setting the time zone".to_owned(),
            Self::ConfigureHostname { .. } => "Naming the computer".to_owned(),
            Self::BackUpFallbackBootloader { .. } => "Backing up the existing boot loader".to_owned(),
            Self::InstallBootloader { .. } => "Installing the boot loader".to_owned(),
            Self::InstallKernel { .. } => "Installing the kernel".to_owned(),
            Self::CreateUser { username, .. } => format!("Creating the account for {username}"),
            Self::SetUserPassword { .. } => "Setting the password".to_owned(),
            Self::LockRootAccount { .. } => "Locking the root account".to_owned(),
            Self::EnableAutologin { .. } => "Enabling automatic sign-in".to_owned(),
            Self::EnableServices { .. } => "Enabling services".to_owned(),
            Self::Unmount { .. } => "Unmounting the new system".to_owned(),
            Self::DeleteStagingPartition { .. } => "Reclaiming the installer partition".to_owned(),
            Self::GrowRootPartition { .. } => "Growing the CrownOS partition".to_owned(),
            Self::GrowRootFilesystem { .. } => "Growing the CrownOS filesystem".to_owned(),
        }
    }

    /// What this step does to a block device. The basis of
    /// [`verify_nothing_pre_existing_is_destroyed`].
    pub fn impact(&self) -> Impact<'_> {
        match self {
            // Every operation the editor can perform rewrites either a table
            // entry or a filesystem, so all of them are destructive. Which of
            // them are *allowed* to be is `Plan::authorized_destructive`'s
            // business, not this function's.
            Self::EditPartitionTable(op) => Impact::Destructive(op.device()),
            Self::CreatePartition { partition, .. } => Impact::Destructive(&partition.path),
            Self::MakeFilesystem { device, .. }
            | Self::MakeSwap { device }
            | Self::GrowRootFilesystem { device, .. } => Impact::Destructive(device),
            Self::DeleteStagingPartition { device, .. }
            | Self::GrowRootPartition { device, .. } => Impact::Destructive(device),
            Self::BackUpFallbackBootloader { device, .. }
            | Self::InstallBootloader { device, .. }
            | Self::InstallKernel { device, .. }
            | Self::InstallNixos { device, .. } => Impact::FileLevel(device),
            _ => Impact::None,
        }
    }
}

fn filesystem_name(fs: RootFilesystem) -> &'static str {
    match fs {
        RootFilesystem::Ext4 => "ext4",
        RootFilesystem::Btrfs => "btrfs",
    }
}

// --- MARK: Planning ---

/// Build the install plan, or explain why we will not.
pub fn plan(input: &PlanInput<'_>) -> Result<Plan, Refusal> {
    let oobe = input.oobe;

    // 1. Do we understand the payload at all? Everything below reads fields
    //    whose meaning is only pinned by the schema version.
    if !oobe.is_supported() {
        return Err(Refusal::UnsupportedSchema {
            found: oobe.schema_version,
            understood: crate::oobe::SCHEMA_VERSION,
        });
    }

    // 2. This installer does dual-boot. Nothing else.
    if !oobe.install.keep_windows {
        return Err(Refusal::WholeDiskInstallRefused);
    }

    input.account.validate()?;

    // 3. Find the disk, by PARTUUID.
    let (disk, esp) = locate_target(input.snapshot, &oobe.target.esp_partuuid)?;

    if disk.table != "gpt" {
        return Err(Refusal::NotGpt {
            disk: disk.path.clone(),
            table: if disk.table.is_empty() {
                "missing".to_owned()
            } else {
                disk.table.clone()
            },
        });
    }

    // 4. The partition we must not touch has to be here, on this disk. If it is
    //    not, the disk we are looking at is not the disk that was described.
    let windows = disk.partition(&oobe.target.windows_partuuid).ok_or_else(|| {
        Refusal::WindowsPartitionNotFound {
            partuuid: oobe.target.windows_partuuid.clone(),
            disk: disk.path.clone(),
        }
    })?;

    // 5. The staging medium. For `StagingKind::Esp` this is the ESP itself.
    let staging = match oobe.staging.kind {
        StagingKind::Esp => esp,
        StagingKind::Partition => disk
            .partition(&oobe.staging.partuuid)
            .ok_or_else(|| Refusal::StagingPartitionNotFound {
                partuuid: oobe.staging.partuuid.clone(),
            })?,
    };

    let mut warnings = Vec::new();

    // 6. Is the free space where Windows said it would be?
    let region = choose_region(disk, oobe, staging, &mut warnings)?;

    // 7. Lay the new partitions out inside it.
    let layout = lay_out(disk, oobe, region, &input.ids)?;

    // 8. Decide what happens to the staging medium, and whether we are allowed
    //    to write to it at all.
    let payload_in_ram = input.snapshot.payload_is_in_ram(staging);
    let staging_mounts: Vec<String> = input
        .snapshot
        .mounts_backed_by(staging)
        .iter()
        .map(|m| m.target.clone())
        .collect();

    // Whether the *payload* asks for anything that writes to the staging
    // partition. Checked here rather than after [`decide_reclaim`] on purpose:
    // if it is asked for and the medium is still in use, that is a refusal, not
    // a step we quietly drop. A user who asked for the space back is owed an
    // explanation, and "the live system never made it into RAM" is a real
    // problem with this boot rather than a detail to paper over.
    //
    // The ESP counts unconditionally, because the boot loader goes there.
    let wants_to_write_to_staging =
        oobe.staging.kind == StagingKind::Esp || oobe.staging.reclaim_after_install;
    if wants_to_write_to_staging && !staging_mounts.is_empty() && !payload_in_ram {
        return Err(Refusal::StagingMediumStillInUse {
            partuuid: staging.partuuid.clone().unwrap_or_default(),
            mounted_at: staging_mounts,
        });
    }

    let reclaim = decide_reclaim(disk, oobe, staging, &layout, payload_in_ram);

    // What the plan will *actually* write to the staging partition.
    let writes_to_staging =
        oobe.staging.kind == StagingKind::Esp || matches!(reclaim, Reclaim::GrowRootOver { .. });

    let mut steps = Vec::new();
    if writes_to_staging && !staging_mounts.is_empty() {
        // In RAM (the refusal above proved it), so the mount is a leftover we
        // can drop — but it is dropped as its own visible step rather than as a
        // side effect of something else.
        for target in &staging_mounts {
            warnings.push(format!(
                "The installer files are in memory, so {target} will be unmounted \
                 before anything is written to the medium they came from."
            ));
            steps.push(Step::UnmountStagingMedium {
                target: target.clone(),
            });
        }
    }

    if oobe.target.esp_size_bytes < COMFORTABLE_ESP_BYTES {
        warnings.push(format!(
            "The shared EFI partition is only {}. CrownOS will fit one kernel on it \
             alongside Windows' boot loader, which is enough to boot but leaves no \
             room for a fallback kernel.",
            bytes(oobe.target.esp_size_bytes)
        ));
    }

    let esp_record = ExistingPartition {
        path: esp.path.clone(),
        partuuid: esp.partuuid.clone().unwrap_or_default(),
        size_bytes: disk.bytes(esp.sectors),
    };

    steps.extend(build_steps(disk, oobe, &layout, &esp_record, &reclaim, &input.account));

    let plan = Plan {
        disk: disk.path.clone(),
        sector_size: disk.sector_size,
        disk_size_bytes: disk.bytes(disk.last_usable_sector + 1),
        root: layout.root,
        swap_partition: layout.swap,
        swap_file_bytes: match oobe.install.swap {
            SwapPlan::File { size_bytes } => Some(size_bytes),
            _ => None,
        },
        root_filesystem: oobe.install.root_filesystem,
        distro: oobe.install.distro,
        esp: esp_record,
        windows: Some(ExistingPartition {
            path: windows.path.clone(),
            partuuid: windows.partuuid.clone().unwrap_or_default(),
            size_bytes: disk.bytes(windows.sectors),
        }),
        reclaim,
        account: input.account.clone(),
        locale: oobe.locale.clone(),
        windows_bytes: disk.bytes(windows.sectors),
        steps,
        warnings,
        // A payload-driven install never edits the table freely: every
        // destructive step it emits is either a partition it just created or the
        // staging partition under an authorized reclaim, both of which the guard
        // allows by name.
        authorized_destructive: Vec::new(),
    };

    // 9. Prove it. Not because the steps above might be wrong today, but because
    //    the check outlives whoever adds the next step.
    verify_nothing_pre_existing_is_destroyed(&plan, disk)?;

    Ok(plan)
}

/// Find the disk by way of the ESP's PARTUUID.
///
/// The anchor of the whole design: the payload names one partition GUID, and
/// the disk that partition lives on is the target. Nothing consults
/// `windows_disk_number`, which has no meaning under Linux, and nothing consults
/// a device name.
fn locate_target<'a>(
    snapshot: &'a SystemSnapshot,
    esp_partuuid: &str,
) -> Result<(&'a Disk, &'a Partition), Refusal> {
    snapshot
        .find_partition(esp_partuuid)
        .ok_or_else(|| Refusal::EspNotFound {
            partuuid: esp_partuuid.to_owned(),
        })
}

/// The free region we are going to install into, checked against the payload's
/// description of it.
///
/// # Why two candidate readings
///
/// The payload records the region Windows freed. By the time we look, part of
/// that region may have become the staging partition — the Windows side carves
/// `CROWNSTAGE` out of the space it just freed, and depending on which end it
/// took, the unallocated gap we now see is either the whole freed region or the
/// freed region minus a gigabyte at one end.
///
/// Rather than guess which convention the writing side used, both readings are
/// checked: the bare gap, and the gap unioned with an adjacent staging
/// partition. If either is within [`REGION_TOLERANCE`] of what the payload
/// describes, the disk is in the state we left it in. If neither is, it is not,
/// and that is a refusal.
fn choose_region(
    disk: &Disk,
    oobe: &Oobe,
    staging: &Partition,
    warnings: &mut Vec<String>,
) -> Result<Gap, Refusal> {
    let gaps = disk.free_gaps();
    if gaps.is_empty() {
        return Err(Refusal::NoFreeRegion {
            disk: disk.path.clone(),
        });
    }

    let expected_start = oobe.target.expected_free_start_bytes;
    let expected_size = oobe.target.expected_free_bytes;

    // Pick the gap that overlaps the expected region most; failing any overlap,
    // the largest one, so the error message describes the disk's real shape
    // rather than an arbitrary gap.
    let expected_end = expected_start.saturating_add(expected_size);
    let best = gaps
        .iter()
        .copied()
        .max_by_key(|gap| {
            let start = disk.bytes(gap.start_sector);
            let end = disk.bytes(gap.end_sector() + 1);
            let overlap = end.min(expected_end).saturating_sub(start.max(expected_start));
            (overlap, end - start)
        })
        .expect("gaps is not empty");

    let gap_start = disk.bytes(best.start_sector);
    let gap_size = disk.bytes(best.sectors);

    let staging_start = disk.bytes(staging.start_sector);
    let staging_end = disk.bytes(staging.end_sector() + 1);
    let alignment_slack = ALIGNMENT_BYTES;

    // The union reading: the gap plus the staging partition, when the two are
    // adjacent to within one alignment unit.
    let union = {
        let adjacent_before = staging_end <= gap_start && gap_start - staging_end <= alignment_slack;
        let adjacent_after = {
            let gap_end = gap_start + gap_size;
            staging_start >= gap_end && staging_start - gap_end <= alignment_slack
        };
        if oobe.staging.kind == StagingKind::Partition && (adjacent_before || adjacent_after) {
            Some((
                gap_start.min(staging_start),
                (gap_start + gap_size).max(staging_end) - gap_start.min(staging_start),
            ))
        } else {
            None
        }
    };

    let matches = |(start, size): (u64, u64)| {
        start.abs_diff(expected_start) <= REGION_TOLERANCE
            && size.abs_diff(expected_size) <= REGION_TOLERANCE
    };

    if !matches((gap_start, gap_size)) && !union.is_some_and(matches) {
        return Err(Refusal::FreeRegionMismatch {
            expected_start_bytes: expected_start,
            expected_bytes: expected_size,
            actual_start_bytes: gap_start,
            actual_bytes: gap_size,
            tolerance_bytes: REGION_TOLERANCE,
        });
    }

    // The user was promised a certain amount of room. If the region actually
    // available is materially smaller, say so — it is not a reason to stop, but
    // it is a reason for the number on the confirmation page to be the real one.
    if gap_size + REGION_TOLERANCE < oobe.install.allocated_bytes {
        warnings.push(format!(
            "You asked for {} and the unallocated region is {}. CrownOS will use \
             what is actually there.",
            bytes(oobe.install.allocated_bytes),
            bytes(gap_size)
        ));
    }

    Ok(best)
}

/// The partitions to create inside `region`.
struct Layout {
    root: NewPartition,
    swap: Option<NewPartition>,
}

/// Place the new partitions in the free region.
///
/// Swap goes *first* and root takes everything after it, so that root always
/// ends at the region's end. That is what makes reclaiming the staging partition
/// possible at all: growing a partition means moving its last sector, and only
/// the last partition in the region has a last sector that anything can be
/// grown into.
fn lay_out(disk: &Disk, oobe: &Oobe, region: Gap, ids: &NewIds) -> Result<Layout, Refusal> {
    let align = (ALIGNMENT_BYTES / u64::from(disk.sector_size)).max(1);
    let start = align_up(region.start_sector, align);
    // The last usable sector, rounded *down* to an alignment boundary minus one,
    // so the partition ends exactly where an aligned run would.
    let end = align_down(region.end_sector() + 1, align).saturating_sub(1);

    if end <= start {
        return Err(Refusal::FreeRegionTooSmall {
            available_bytes: disk.bytes(region.sectors),
            needed_bytes: MIN_ROOT_BYTES,
        });
    }

    let region_bytes = disk.bytes(end - start + 1);
    let min_root_sectors = MIN_ROOT_BYTES / u64::from(disk.sector_size);

    let (swap, root_start) = match oobe.install.swap {
        SwapPlan::Partition { size_bytes } => {
            let swap_sectors = align_up(
                size_bytes.div_ceil(u64::from(disk.sector_size)),
                align,
            );
            let swap_end = start + swap_sectors - 1;
            if swap_end >= end || end - swap_end < min_root_sectors {
                return Err(Refusal::SwapDoesNotFit {
                    swap_bytes: size_bytes,
                    region_bytes,
                    min_root_bytes: MIN_ROOT_BYTES,
                });
            }
            let number = disk.next_partition_number();
            (
                Some(NewPartition {
                    number,
                    path: disk.partition_path(number),
                    partuuid: ids.swap_partuuid.to_ascii_lowercase(),
                    start_sector: start,
                    end_sector: swap_end,
                    sector_size: disk.sector_size,
                }),
                swap_end + 1,
            )
        }
        SwapPlan::None | SwapPlan::File { .. } => (None, start),
    };

    if end - root_start + 1 < min_root_sectors {
        return Err(Refusal::FreeRegionTooSmall {
            available_bytes: disk.bytes(end - root_start + 1),
            needed_bytes: MIN_ROOT_BYTES,
        });
    }

    // Root's number comes after swap's, so the two do not collide. Both are
    // derived from `next_partition_number`, which skips whatever is in use.
    let root_number = match &swap {
        Some(swap) => next_free_after(disk, swap.number),
        None => disk.next_partition_number(),
    };

    Ok(Layout {
        root: NewPartition {
            number: root_number,
            path: disk.partition_path(root_number),
            partuuid: ids.root_partuuid.to_ascii_lowercase(),
            start_sector: root_start,
            end_sector: end,
            sector_size: disk.sector_size,
        },
        swap,
    })
}

/// The lowest partition number greater than `taken` that the table is not
/// already using.
fn next_free_after(disk: &Disk, taken: u32) -> u32 {
    let used: Vec<u32> = disk.partitions.iter().map(|p| p.number).collect();
    (taken + 1..)
        .find(|n| !used.contains(n))
        .expect("u32 is not full")
}

/// Whether — and how — the staging partition's space goes back to the user.
///
/// Three conditions, all required, each with its own recorded reason for failing:
///
/// * the payload asked for it ([`Staging::reclaim_after_install`]);
/// * the payload is provably in RAM, so deleting the partition cannot pull the
///   floor out from under the running installer; and
/// * the staging partition sits immediately after the new root partition, since
///   growing a partition can only ever move its *last* sector.
///
/// [`Staging::reclaim_after_install`]: crate::oobe::Staging::reclaim_after_install
///
/// # Why at the end and not the beginning
///
/// Deleting the staging partition before the install and giving root the whole
/// region would be simpler and would need no resize at all. It is not done
/// because the staged medium is the only thing that can boot this installer
/// again: an install that fails half way through is recoverable while the
/// staging partition still exists, and needs the Windows installer run again if
/// it does not.
fn decide_reclaim(
    disk: &Disk,
    oobe: &Oobe,
    staging: &Partition,
    layout: &Layout,
    payload_in_ram: bool,
) -> Reclaim {
    let keep = |reason: String| Reclaim::Keep { reason };

    if !oobe.staging.reclaim_after_install {
        return keep(
            "The installer files are on the EFI System Partition, which has to stay \
             where it is."
                .to_owned(),
        );
    }
    if oobe.staging.kind != StagingKind::Partition {
        return keep(
            "The installer files are on the EFI System Partition, which has to stay \
             where it is."
                .to_owned(),
        );
    }
    if !payload_in_ram {
        return keep(format!(
            "The installer is still reading from {}, so that partition will be left \
             in place. You can delete it from Disk Management once CrownOS has \
             started.",
            staging.path
        ));
    }

    // Growing root means moving its last sector forward, so the staging
    // partition has to be the thing immediately after it. One alignment unit of
    // slack, because root's end was rounded down to an alignment boundary.
    let align = (ALIGNMENT_BYTES / u64::from(disk.sector_size)).max(1);
    let root_end = layout.root.end_sector;
    let adjacent = staging.start_sector > root_end && staging.start_sector - root_end <= align + 1;
    if !adjacent {
        return keep(format!(
            "{} sits before the CrownOS partition rather than after it, so \
             reclaiming its space would mean moving the whole filesystem. It will be \
             left in place; you can delete it from Disk Management once CrownOS has \
             started.",
            staging.path
        ));
    }

    // Only ext4 can be grown while nothing is mounted, with tools that are on
    // the ISO. btrfs needs the filesystem mounted to resize, and remounting the
    // freshly installed system to reclaim a gigabyte is not a trade worth making.
    if oobe.install.root_filesystem != RootFilesystem::Ext4 {
        return keep(format!(
            "{} can be reclaimed, but growing a {} filesystem afterwards needs it \
             mounted, so the space is left unallocated for now. You can add it to \
             CrownOS from within CrownOS.",
            staging.path,
            filesystem_name(oobe.install.root_filesystem)
        ));
    }

    let new_end = align_down(staging.end_sector() + 1, align).saturating_sub(1);
    Reclaim::GrowRootOver {
        staging: ExistingPartition {
            path: staging.path.clone(),
            partuuid: staging.partuuid.clone().unwrap_or_default(),
            size_bytes: disk.bytes(staging.sectors),
        },
        staging_number: staging.number,
        new_root_end_sector: new_end,
    }
}

/// The ordered steps. Pure assembly — every decision was made above.
fn build_steps(
    disk: &Disk,
    oobe: &Oobe,
    layout: &Layout,
    esp: &ExistingPartition,
    reclaim: &Reclaim,
    account: &Account,
) -> Vec<Step> {
    let root_mount = ROOT_MOUNT.to_owned();
    let esp_mount = format!("{ROOT_MOUNT}{ESP_MOUNT_REL}");
    let mut steps = Vec::new();

    if let Some(swap) = &layout.swap {
        steps.push(Step::CreatePartition {
            disk: disk.path.clone(),
            partition: swap.clone(),
            type_guid: TYPE_LINUX_SWAP.to_owned(),
            name: SWAP_PARTLABEL.to_owned(),
            role: Role::Swap,
        });
    }
    steps.push(Step::CreatePartition {
        disk: disk.path.clone(),
        partition: layout.root.clone(),
        type_guid: TYPE_LINUX_ROOT_X86_64.to_owned(),
        name: ROOT_PARTLABEL.to_owned(),
        role: Role::Root,
    });
    steps.push(Step::SettleTable {
        disk: disk.path.clone(),
    });

    if let Some(swap) = &layout.swap {
        steps.push(Step::MakeSwap {
            device: swap.path.clone(),
        });
    }
    steps.push(Step::MakeFilesystem {
        device: layout.root.path.clone(),
        filesystem: oobe.install.root_filesystem,
        label: ROOT_FSLABEL.to_owned(),
    });
    steps.push(Step::MountRoot {
        device: layout.root.path.clone(),
        at: root_mount.clone(),
    });
    steps.push(Step::MountEsp {
        device: esp.path.clone(),
        at: esp_mount.clone(),
    });
    // Before anything is installed: a base's package scripts may run
    // `bootctl` themselves the moment the ESP is visible inside the target.
    steps.push(Step::BackUpFallbackBootloader {
        device: esp.path.clone(),
        esp_at: esp_mount,
    });

    match oobe.install.distro.imperative() {
        Some(distro) => steps.extend(imperative_steps(distro, oobe, layout, esp, account)),
        None => steps.extend(nixos_steps(oobe, esp, account)),
    }

    steps.push(Step::Unmount { at: root_mount });

    if let Reclaim::GrowRootOver {
        staging,
        staging_number,
        new_root_end_sector,
    } = reclaim
    {
        steps.push(Step::DeleteStagingPartition {
            disk: disk.path.clone(),
            number: *staging_number,
            device: staging.path.clone(),
            partuuid: staging.partuuid.clone(),
        });
        steps.push(Step::GrowRootPartition {
            disk: disk.path.clone(),
            number: layout.root.number,
            device: layout.root.path.clone(),
            new_end_sector: *new_root_end_sector,
        });
        steps.push(Step::SettleTable {
            disk: disk.path.clone(),
        });
        steps.push(Step::GrowRootFilesystem {
            device: layout.root.path.clone(),
            filesystem: oobe.install.root_filesystem,
        });
    }

    steps
}

/// Everything between mounting the target and unmounting it, for a base that is
/// installed package by package.
fn imperative_steps(
    distro: ImperativeDistro,
    oobe: &Oobe,
    layout: &Layout,
    esp: &ExistingPartition,
    account: &Account,
) -> Vec<Step> {
    let root = ROOT_MOUNT.to_owned();
    let backend = distro.backend();
    let mut steps = vec![
        Step::InstallBaseSystem {
            root: root.clone(),
            distro,
            packages: backend.packages(oobe.install.root_filesystem, &oobe.locale.locale),
        },
        Step::GenerateFstab { root: root.clone() },
    ];

    if let SwapPlan::File { size_bytes } = oobe.install.swap {
        steps.push(Step::CreateSwapFile {
            root: root.clone(),
            size_bytes,
            filesystem: oobe.install.root_filesystem,
        });
    }

    steps.extend([
        Step::ConfigureLocale {
            root: root.clone(),
            locale: oobe.locale.locale.clone(),
            distro,
        },
        Step::ConfigureKeymap {
            root: root.clone(),
            keymap: oobe.locale.keymap.clone(),
        },
        Step::ConfigureTimezone {
            root: root.clone(),
            timezone: oobe.locale.timezone.clone(),
        },
        Step::ConfigureHostname {
            root: root.clone(),
            hostname: account.hostname.clone(),
        },
        Step::InstallBootloader {
            device: esp.path.clone(),
            root: root.clone(),
            esp_rel: ESP_MOUNT_REL.to_owned(),
            root_partuuid: layout.root.partuuid.clone(),
            kernel_cmdline: format!("root=PARTUUID={} rw", layout.root.partuuid),
            distro,
        },
        Step::InstallKernel {
            device: esp.path.clone(),
            root: root.clone(),
            esp_rel: ESP_MOUNT_REL.to_owned(),
            distro,
        },
        Step::InstallDesktop {
            root: root.clone(),
            distro,
        },
        Step::CreateUser {
            root: root.clone(),
            username: account.username.clone(),
            full_name: account.full_name.clone(),
            admin_group: backend.admin_group().to_owned(),
        },
        Step::SetUserPassword {
            root: root.clone(),
            username: account.username.clone(),
            distro: oobe.install.distro,
        },
        // The password the user typed is theirs, not root's. Locking root means
        // the only way to administrate the machine is `sudo` as that user, which
        // is what every desktop distribution has settled on.
        Step::LockRootAccount { root: root.clone() },
    ]);
    if account.autologin {
        steps.push(Step::EnableAutologin {
            root: root.clone(),
            username: account.username.clone(),
        });
    }
    steps.push(Step::EnableServices {
        root,
        units: backend.services().iter().map(|u| (*u).to_owned()).collect(),
    });
    steps
}

/// Everything between mounting the target and unmounting it, for NixOS. Swap,
/// the account, autologin, services and the boot loader are all in the
/// configuration; only the password is set afterwards.
fn nixos_steps(oobe: &Oobe, esp: &ExistingPartition, account: &Account) -> Vec<Step> {
    let root = ROOT_MOUNT.to_owned();
    let configuration = nixos::configuration(&nixos::Choices {
        hostname: &account.hostname,
        username: &account.username,
        full_name: account.full_name.as_deref(),
        autologin: account.autologin,
        locale: &oobe.locale,
        swap_file_bytes: match oobe.install.swap {
            SwapPlan::File { size_bytes } => Some(size_bytes),
            _ => None,
        },
        esp_rel: ESP_MOUNT_REL,
    });
    vec![
        Step::ConfigureNixos {
            root: root.clone(),
            configuration,
        },
        Step::InstallNixos {
            device: esp.path.clone(),
            root: root.clone(),
        },
        Step::SetUserPassword {
            root,
            username: account.username.clone(),
            distro: Distro::NixOs,
        },
    ]
}

/// Prove that no partition which existed before this install is destroyed.
///
/// The one exception is the staging partition under an authorized reclaim, and
/// it is spelled out here rather than inferred. The ESP is allowed to be written
/// to at the *file* level and nowhere else: it is checked to never appear as a
/// destructive target, which is what "we share the ESP with Windows, we do not
/// take it over" means in code.
///
/// This cannot fail as the steps are generated today. It is here because the
/// next person to add a step will not read all of the above, and this is the
/// check that will read it for them.
fn verify_nothing_pre_existing_is_destroyed(plan: &Plan, disk: &Disk) -> Result<(), Refusal> {
    let reclaimed: Option<&str> = match &plan.reclaim {
        Reclaim::GrowRootOver { staging, .. } => Some(&staging.path),
        Reclaim::Keep { .. } => None,
    };

    for step in &plan.steps {
        let Impact::Destructive(device) = step.impact() else {
            continue;
        };
        // The partitions this plan creates are fair game — by construction their
        // numbers were not in use, so their nodes did not exist.
        if device == plan.root.path
            || plan
                .swap_partition
                .as_ref()
                .is_some_and(|s| s.path == device)
        {
            continue;
        }
        if Some(device) == reclaimed {
            continue;
        }
        // Named by the user, one partition at a time, in the editor. See
        // `Plan::authorized_destructive` for why this is a list and not a flag.
        if plan.authorized_destructive.iter().any(|d| d == device) {
            continue;
        }
        if disk.partitions.iter().any(|p| p.path == device) {
            return Err(Refusal::WouldDestroyExistingPartition {
                device: device.to_owned(),
                step: step.label(),
            });
        }
    }

    // Said plainly, because it is the invariant a reviewer will look for: the
    // partition of the operating system already here, and the ESP, are never
    // destructive targets.
    //
    // The editor's authorization list deliberately does *not* apply to these
    // two. It cannot: a plan is built against a chosen ESP and a chosen
    // installed OS, and an install whose own boot partition is reformatted part
    // way through is not something a confirmation page can meaningfully warn
    // about. A user who genuinely wants to erase them does it in the editor with
    // no install queued behind it.
    let protected = plan
        .windows
        .iter()
        .map(|windows| &windows.path)
        .chain(std::iter::once(&plan.esp.path));
    let destructive = plan.destructive_devices();
    for device in protected {
        if destructive.contains(&device.as_str()) {
            return Err(Refusal::WouldDestroyExistingPartition {
                device: device.clone(),
                step: "a protected partition appeared as a destructive target".to_owned(),
            });
        }
    }

    Ok(())
}

// --- MARK: ISO installs ---

/// What an ISO install needs, all of it chosen by the user on this side of the
/// reboot.
///
/// The mirror image of [`PlanInput`]: same output, opposite provenance. A
/// payload-driven install is handed decisions and spends its planning checking
/// they are still true; an ISO install has no decisions to check, only choices to
/// lay out. That is why this carries a chosen disk and a chosen gap rather than
/// an expected-region description — there is nothing to compare against, because
/// the user is looking at the disk right now.
#[derive(Clone, Debug)]
pub struct IsoInput<'a> {
    pub snapshot: &'a SystemSnapshot,
    /// The disk to install onto, by node. By name and not by PARTUUID —
    /// deliberately, and it is the one place this crate does that.
    ///
    /// [`plan`]'s first invariant is that the disk is found by PARTUUID because
    /// its input crossed a reboot: `/dev/sda` was written down by a program that
    /// is no longer running, on a machine that has since re-enumerated its
    /// disks. Here the name was read out of the snapshot this same planning pass
    /// is using, seconds ago, from a list the user is looking at. There is no
    /// reboot to survive, and a PARTUUID would be a worse identifier anyway: a
    /// blank disk has no partitions to have a PARTUUID.
    pub disk: String,
    /// The EFI System Partition to install the boot loader into. Must be on
    /// `disk` — see [`Refusal::EspOnAnotherDisk`].
    pub esp: String,
    /// Where the root partition goes: the first sector of the unallocated region
    /// the user picked. Matched against the projected table's own gaps, so a
    /// region that stopped being free between drawing the list and pressing the
    /// button is a refusal rather than an overwrite.
    pub region_start_sector: u64,
    pub root_filesystem: RootFilesystem,
    pub distro: Distro,
    pub swap: SwapPlan,
    pub locale: Locale,
    pub account: Account,
    pub ids: NewIds,
    /// Partition-table changes the user queued in the editor, in order. Applied
    /// before anything else in the plan, and the reason the plan may destroy
    /// pre-existing partitions at all — see [`Plan::authorized_destructive`].
    pub edits: Vec<super::disks::DiskOp>,
}

/// Build an install plan from the user's own choices, with no handoff payload.
///
/// # Why this is a second entry point and not a flag on [`plan`]
///
/// The two share everything after "where do the partitions go": the layout
/// arithmetic ([`lay_out`]), the step list ([`build_steps`]), and the guard that
/// proves nothing pre-existing is destroyed. What they do *not* share is a single
/// one of the checks that makes [`plan`] safe, because every one of those checks
/// compares the disk against a description of the disk written before a reboot —
/// and there is no such description here.
///
/// Folding that into one function with an `Option<&Oobe>` would mean every rule
/// in [`plan`] growing an "unless we are on the ISO path" clause. Rules with
/// exceptions in them are rules nobody can audit; the invariants in [`plan`]'s
/// module docs are worth more than the eighty lines this duplicates.
///
/// # What is checked here instead
///
/// 1. **The account is valid**, exactly as in [`plan`].
/// 2. **The disk exists and is GPT.** Same reason: an MBR table makes every
///    offset below mean something different.
/// 3. **Every queued edit is legal.** Delegated wholesale to
///    [`disks::plan_ops`](super::disks::plan_ops), which is where the mount
///    checks, overlap checks and shrink-safety rules live. This function does not
///    re-implement any of them.
/// 4. **The layout happens against the *projected* table** — the disk as the
///    edits will leave it — so "delete this partition, then install into the
///    space it was using" is one coherent plan rather than a plan built against a
///    disk that no longer exists by the time it runs.
/// 5. **There is an ESP, on this disk.** A UEFI machine has to boot from one, and
///    it has to be the same disk as the root, or removing either disk stops the
///    machine booting.
/// 6. **The chosen region is still free** in that projected table.
///
/// The pre-existing operating system, if there is one, is *not* identified or
/// protected here. It cannot be: on a blank disk there is nothing to protect, and
/// on a disk with something on it the user may well be deliberately replacing it
/// with the editor. [`Plan::windows`] is therefore `None` for an ISO install, and
/// the guard's protection reduces to the ESP — plus the authorization list, which
/// requires the user to have named each destroyed partition individually.
pub fn plan_iso(input: &IsoInput<'_>) -> Result<Plan, Refusal> {
    input.account.validate()?;

    let disk = input
        .snapshot
        .disks
        .iter()
        .find(|disk| disk.path == input.disk)
        .ok_or_else(|| Refusal::NoTargetDisk {
            disk: input.disk.clone(),
        })?;

    if disk.table != "gpt" {
        return Err(Refusal::NotGpt {
            disk: disk.path.clone(),
            table: if disk.table.is_empty() {
                "missing".to_owned()
            } else {
                disk.table.clone()
            },
        });
    }

    // Every safety rule about editing partitions lives in `disks`, and is
    // applied here by calling it rather than by restating it.
    let projection = super::disks::plan_ops(disk, input.snapshot, &input.edits)
        .map_err(|refusal| Refusal::EditRefused {
            detail: refusal.to_string(),
        })?;
    let projected = &projection.disk;

    let mut warnings = projection.warnings.clone();

    // The ESP is looked for in the *projection*, so an ESP the user just created
    // in the editor counts — which is the whole point of being able to create
    // one on a blank disk.
    let esp = projected
        .partitions
        .iter()
        .find(|partition| partition.path == input.esp)
        .ok_or_else(|| {
            if projected
                .partitions
                .iter()
                .any(|p| p.type_guid.as_deref() == Some(super::disks::TYPE_EFI_SYSTEM))
            {
                Refusal::EspOnAnotherDisk {
                    esp: input.esp.clone(),
                    disk: disk.path.clone(),
                }
            } else {
                Refusal::NoEspOnDisk {
                    disk: disk.path.clone(),
                }
            }
        })?;

    let region = projected
        .free_gaps()
        .into_iter()
        .find(|gap| gap.start_sector == input.region_start_sector)
        .ok_or(Refusal::ChosenRegionNotFree {
            start_sector: input.region_start_sector,
        })?;

    // `lay_out` and `build_steps` are parameterised by an `Oobe`, so the user's
    // choices are assembled into one. Not a hack: `Oobe` is the crate's word for
    // "the decisions an install was made from", and the only difference on this
    // path is who made them and how recently. The fields with no meaning here
    // are filled from the projection rather than left at defaults, so nothing
    // downstream can read a placeholder and believe it.
    let synthesized = synthesize_oobe(input, projected, esp);

    let layout = lay_out(projected, &synthesized, region, &input.ids)?;

    if projected.bytes(esp.sectors) < COMFORTABLE_ESP_BYTES {
        warnings.push(format!(
            "The EFI partition is {}. CrownOS will fit one kernel on it, which is \
             enough to boot but leaves no room for a fallback kernel.",
            bytes(projected.bytes(esp.sectors))
        ));
    }
    if projected
        .partitions
        .iter()
        .any(|p| p.path != esp.path && p.fstype.is_some())
    {
        warnings.push(
            "There are other filesystems on this disk. Nothing outside the partitions \
             listed below is touched, but this installer does not add them to the boot \
             menu — systemd-boot finds other operating systems by itself where it can."
                .to_owned(),
        );
    }

    let esp_record = ExistingPartition {
        path: esp.path.clone(),
        partuuid: esp.partuuid.clone().unwrap_or_default(),
        size_bytes: projected.bytes(esp.sectors),
    };

    // The editor's operations go first: the rest of the plan was laid out
    // against the table they produce, so running them in any other order would
    // be laying partitions into space that is still occupied.
    let mut steps: Vec<Step> = input
        .edits
        .iter()
        .cloned()
        .map(Step::EditPartitionTable)
        .collect();
    if !steps.is_empty() {
        steps.push(Step::SettleTable {
            disk: disk.path.clone(),
        });
    }
    // `Reclaim::Keep` because there is no staging medium to reclaim: an ISO boot
    // runs off a removable stick, and the space on it is not this machine's to
    // give back.
    let reclaim = Reclaim::Keep {
        reason: "This install was started from a CrownOS image rather than from Windows, \
                 so there is no installer partition on this disk to reclaim."
            .to_owned(),
    };
    steps.extend(build_steps(
        projected,
        &synthesized,
        &layout,
        &esp_record,
        &reclaim,
        &input.account,
    ));

    let plan = Plan {
        disk: disk.path.clone(),
        sector_size: projected.sector_size,
        disk_size_bytes: projected.bytes(projected.last_usable_sector + 1),
        root: layout.root,
        swap_partition: layout.swap,
        swap_file_bytes: match input.swap {
            SwapPlan::File { size_bytes } => Some(size_bytes),
            _ => None,
        },
        root_filesystem: input.root_filesystem,
        distro: input.distro,
        esp: esp_record,
        // Nothing on this path is designated as an operating system to preserve.
        // See this function's docs.
        windows: None,
        reclaim,
        account: input.account.clone(),
        locale: input.locale.clone(),
        windows_bytes: 0,
        steps,
        warnings,
        authorized_destructive: projection.destroyed.clone(),
    };

    // Against the disk as it *is*, not as the edits will leave it: the guard's
    // question is "does this plan destroy something that was already here", and
    // the projected table has the deletions already applied, which would make it
    // answer no every time.
    verify_nothing_pre_existing_is_destroyed(&plan, disk)?;

    Ok(plan)
}

/// The user's choices, as the [`Oobe`] the shared planning helpers read.
///
/// Every field that has a meaning on this path carries the user's answer. The
/// three that describe the *other* side of a handoff — `target`'s expected free
/// region, and `staging` — are filled from the projection so that they describe
/// this disk truthfully rather than describing nothing. Nothing downstream of
/// here reads them (`lay_out` reads `install.swap`; `build_steps` reads
/// `install.root_filesystem`, `install.swap` and `locale`), and filling them with
/// zeroes would be a value that looks like an answer.
fn synthesize_oobe(input: &IsoInput<'_>, projected: &Disk, esp: &Partition) -> Oobe {
    Oobe {
        schema_version: crate::oobe::SCHEMA_VERSION,
        generated_by: format!(
            "crownos-installer {} (no handoff payload — choices made on this machine)",
            env!("CARGO_PKG_VERSION")
        ),
        generated_at_unix: 0,
        install: crate::oobe::Install {
            // Never true here: `auto_install` means "the Windows side already
            // asked these questions", and on this path nobody has.
            auto_install: false,
            allocated_bytes: 0,
            swap: input.swap,
            root_filesystem: input.root_filesystem,
            distro: input.distro,
            // Not consulted — `plan_iso` never calls the check that reads it —
            // but true is the honest value: an ISO install preserves everything
            // the user did not explicitly delete in the editor.
            keep_windows: true,
        },
        target: crate::oobe::Target {
            esp_partuuid: esp.partuuid.clone().unwrap_or_default(),
            esp_size_bytes: projected.bytes(esp.sectors),
            windows_partuuid: String::new(),
            disk_guid: projected.disk_guid.clone(),
            windows_disk_number: 0,
            expected_free_start_bytes: projected.bytes(input.region_start_sector),
            expected_free_bytes: projected
                .free_gaps()
                .iter()
                .find(|gap| gap.start_sector == input.region_start_sector)
                .map_or(0, |gap| projected.bytes(gap.sectors)),
            logical_sector_size: projected.sector_size,
        },
        staging: crate::oobe::Staging {
            kind: StagingKind::Esp,
            partuuid: String::new(),
            base_dir: String::new(),
            // The live medium is a removable stick that is not part of this
            // disk. There is nothing here to reclaim.
            reclaim_after_install: false,
        },
        user: None,
        locale: input.locale.clone(),
    }
}

// --- MARK: Arithmetic ---

pub fn align_up(value: u64, alignment: u64) -> u64 {
    value.div_ceil(alignment) * alignment
}

pub fn align_down(value: u64, alignment: u64) -> u64 {
    value / alignment * alignment
}

/// Format a byte count the way the UI shows it. Binary units, because that is
/// what a partition table is measured in and a wizard that says 128 GiB next to
/// a Disk Management window that says 128 GB invites a support ticket.
pub fn bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = value as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::probe::{self, fixture, BOOTMNT};
    use crate::oobe::{Install, Staging, Target};

    // --- Fixtures ---

    fn snapshot(findmnt: &str) -> SystemSnapshot {
        probe::probe(&fixture::machine(findmnt)).expect("probe")
    }

    const ESP_PARTUUID: &str = "0fc63daf-8483-4772-8e79-3d69d8477de4";
    const WINDOWS_PARTUUID: &str = "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7";
    const STAGING_PARTUUID: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

    /// The free region in `fixture::SFDISK_JSON`: sectors 802129920..=1000215182
    /// at 512 bytes each.
    const FREE_START: u64 = 802_129_920 * 512;
    const FREE_BYTES: u64 = (1_000_215_182 - 802_129_920 + 1) * 512;

    fn payload() -> Oobe {
        Oobe {
            schema_version: crate::oobe::SCHEMA_VERSION,
            generated_by: "crownos-windows-installer 0.1.0".to_owned(),
            generated_at_unix: 1_756_000_000,
            install: Install {
                auto_install: false,
                allocated_bytes: FREE_BYTES,
                swap: SwapPlan::File {
                    size_bytes: 8 * GIB,
                },
                root_filesystem: RootFilesystem::Ext4,
                keep_windows: true,
                distro: crate::oobe::Distro::Arch,
            },
            target: Target {
                esp_partuuid: ESP_PARTUUID.to_owned(),
                esp_size_bytes: 260 * MIB,
                windows_partuuid: WINDOWS_PARTUUID.to_owned(),
                disk_guid: Some("01234567-89ab-cdef-0123-456789abcdef".to_owned()),
                windows_disk_number: 0,
                expected_free_start_bytes: FREE_START,
                expected_free_bytes: FREE_BYTES,
                logical_sector_size: 512,
            },
            staging: Staging {
                kind: StagingKind::Partition,
                partuuid: STAGING_PARTUUID.to_owned(),
                base_dir: "EFI/ArchStage".to_owned(),
                reclaim_after_install: true,
            },
            user: Some(UserAccount {
                username: "crown".to_owned(),
                full_name: Some("Crown User".to_owned()),
                hostname: "crownos".to_owned(),
                autologin: false,
            }),
            locale: Locale::default(),
        }
    }

    fn account() -> Account {
        Account {
            username: "crown".to_owned(),
            full_name: Some("Crown User".to_owned()),
            hostname: "crownos".to_owned(),
            autologin: false,
        }
    }

    fn ids() -> NewIds {
        NewIds {
            root_partuuid: "12345678-1234-1234-1234-123456789abc".to_owned(),
            swap_partuuid: "87654321-4321-4321-4321-cba987654321".to_owned(),
        }
    }

    /// Plan against the standard machine, with `edit` applied to the payload.
    fn plan_with(edit: impl FnOnce(&mut Oobe)) -> Result<Plan, Refusal> {
        plan_on(fixture::FINDMNT_JSON_IN_RAM, edit)
    }

    fn plan_on(findmnt: &str, edit: impl FnOnce(&mut Oobe)) -> Result<Plan, Refusal> {
        let mut oobe = payload();
        edit(&mut oobe);
        plan_against(&snapshot(findmnt), oobe)
    }

    fn snapshot_with(findmnt: &str, sfdisk: &str) -> SystemSnapshot {
        probe::probe(&fixture::machine_with(findmnt, sfdisk)).expect("probe")
    }

    fn plan_against(snapshot: &SystemSnapshot, oobe: Oobe) -> Result<Plan, Refusal> {
        plan(&PlanInput {
            oobe: &oobe,
            snapshot,
            account: account(),
            ids: ids(),
        })
    }

    fn plan_against_as(
        snapshot: &SystemSnapshot,
        oobe: Oobe,
        account: Account,
    ) -> Result<Plan, Refusal> {
        plan(&PlanInput {
            oobe: &oobe,
            snapshot,
            account,
            ids: ids(),
        })
    }

    // --- The happy path ---

    #[test]
    fn the_ordinary_dual_boot_install_plans() {
        let plan = plan_with(|_| {}).expect("plan");

        assert_eq!(plan.disk, "/dev/sda");
        assert_eq!(plan.sector_size, 512);
        // Partition 5, since 1–4 are taken.
        assert_eq!(plan.root.number, 5);
        assert_eq!(plan.root.path, "/dev/sda5");
        assert_eq!(plan.root.partuuid, ids().root_partuuid);
        assert!(plan.swap_partition.is_none());
        assert_eq!(plan.swap_file_bytes, Some(8 * GIB));

        // Aligned to 1 MiB at both ends, inside the gap.
        assert_eq!(plan.root.start_sector % 2048, 0);
        assert_eq!((plan.root.end_sector + 1) % 2048, 0);
        assert!(plan.root.start_sector >= 802_129_920);
        assert!(plan.root.end_sector <= 1_000_215_182);

        assert_eq!(plan.esp.path, "/dev/sda1");
        assert_eq!(
            plan.windows.as_ref().expect("a payload names an OS to keep").path,
            "/dev/sda3"
        );
    }

    #[test]
    fn the_plan_covers_every_stage_of_an_install() {
        let plan = plan_with(|_| {}).expect("plan");
        let labels: Vec<String> = plan.steps.iter().map(Step::label).collect();

        // Not an exhaustive ordering assertion — that would break on every
        // legitimate change — but the stages that must all be present and in
        // this relative order.
        let index = |needle: &str| {
            labels
                .iter()
                .position(|l| l.contains(needle))
                .unwrap_or_else(|| panic!("no step for {needle} in {labels:?}"))
        };
        assert!(index("Creating the CrownOS partition") < index("Formatting"));
        assert!(index("Formatting") < index("Mounting the new system"));
        assert!(index("Mounting the new system") < index("Installing packages"));
        assert!(index("Installing packages") < index("Writing the filesystem table"));
        assert!(index("Writing the filesystem table") < index("Installing the boot loader"));
        assert!(index("Installing the boot loader") < index("Installing the kernel"));
        assert!(index("Installing the kernel") < index("Creating the account"));
        assert!(index("Creating the account") < index("Setting the password"));
        assert!(index("Setting the password") < index("Enabling services"));
        assert!(index("Enabling services") < index("Unmounting the new system"));

        for needle in [
            "Setting the language",
            "Setting the keyboard layout",
            "Setting the time zone",
            "Naming the computer",
            "Creating the swap file",
            "Locking the root account",
            "Backing up the existing boot loader",
        ] {
            let _ = index(needle);
        }
    }

    #[test]
    fn a_swap_partition_is_placed_before_root_so_root_can_grow() {
        let plan = plan_with(|o| {
            o.install.swap = SwapPlan::Partition {
                size_bytes: 8 * GIB,
            }
        })
        .expect("plan");

        let swap = plan.swap_partition.as_ref().expect("swap partition");
        assert_eq!(swap.number, 5);
        assert_eq!(plan.root.number, 6);
        assert!(swap.end_sector < plan.root.start_sector);
        assert_eq!(swap.start_sector % 2048, 0);
        // Root still ends at the region's end, which is what reclaim needs.
        assert!(plan.root.end_sector > swap.end_sector);
        assert_eq!(plan.swap_file_bytes, None);
        assert!(plan
            .steps
            .iter()
            .any(|s| matches!(s, Step::MakeSwap { device } if device == &swap.path)));
    }

    #[test]
    fn btrfs_pulls_in_its_own_tools() {
        let plan = plan_with(|o| o.install.root_filesystem = RootFilesystem::Btrfs).expect("plan");
        let packages = plan
            .steps
            .iter()
            .find_map(|s| match s {
                Step::InstallBaseSystem { packages, .. } => Some(packages.clone()),
                _ => None,
            })
            .expect("pacstrap step");
        assert!(packages.contains(&"btrfs-progs".to_owned()));
    }

    // --- MARK: Refusal: schema ---

    #[test]
    fn refuses_a_payload_from_a_newer_schema() {
        let refusal = plan_with(|o| o.schema_version = crate::oobe::SCHEMA_VERSION + 1)
            .expect_err("must refuse");
        assert_eq!(
            refusal,
            Refusal::UnsupportedSchema {
                found: crate::oobe::SCHEMA_VERSION + 1,
                understood: crate::oobe::SCHEMA_VERSION,
            }
        );
    }

    #[test]
    fn an_older_schema_is_still_installable() {
        // `is_supported` accepts anything at or below our version, and the
        // planner must not add a second, stricter opinion.
        assert!(plan_with(|o| o.schema_version = 0).is_ok());
    }

    // --- MARK: Refusal: keep_windows ---

    #[test]
    fn refuses_to_do_a_whole_disk_install() {
        let refusal = plan_with(|o| o.install.keep_windows = false).expect_err("must refuse");
        assert_eq!(refusal, Refusal::WholeDiskInstallRefused);
    }

    // --- MARK: Refusal: locating the disk ---

    #[test]
    fn refuses_when_no_partition_has_the_esp_partuuid() {
        let refusal = plan_with(|o| {
            o.target.esp_partuuid = "deadbeef-0000-0000-0000-000000000000".to_owned()
        })
        .expect_err("must refuse");
        assert_eq!(
            refusal,
            Refusal::EspNotFound {
                partuuid: "deadbeef-0000-0000-0000-000000000000".to_owned()
            }
        );
    }

    /// `windows_disk_number` is diagnostic only, and nothing in the planner may
    /// consult it. A payload carrying a nonsense value still plans correctly,
    /// because the disk came from the ESP's PARTUUID.
    #[test]
    fn the_disk_is_found_by_partuuid_and_not_by_name() {
        let plan = plan_with(|o| o.target.windows_disk_number = 99).expect("plan");
        assert_eq!(plan.disk, "/dev/sda");
    }

    #[test]
    fn refuses_a_disk_whose_table_is_not_gpt() {
        let snapshot = snapshot_with(
            fixture::FINDMNT_JSON_IN_RAM,
            &fixture::sfdisk_json("dos", fixture::STANDARD_TABLE),
        );
        let refusal = plan_against(&snapshot, payload()).expect_err("must refuse");
        assert_eq!(
            refusal,
            Refusal::NotGpt {
                disk: "/dev/sda".to_owned(),
                table: "dos".to_owned()
            }
        );
    }

    // --- MARK: Refusal: the Windows partition ---

    #[test]
    fn refuses_when_the_windows_partition_is_not_where_it_was() {
        let refusal = plan_with(|o| {
            o.target.windows_partuuid = "99999999-9999-9999-9999-999999999999".to_owned()
        })
        .expect_err("must refuse");
        assert_eq!(
            refusal,
            Refusal::WindowsPartitionNotFound {
                partuuid: "99999999-9999-9999-9999-999999999999".to_owned(),
                disk: "/dev/sda".to_owned(),
            }
        );
    }

    /// The central promise. No step, under any configuration, may destroy the
    /// Windows partition or the ESP.
    #[test]
    fn nothing_destructive_ever_names_windows_or_the_esp() {
        let configurations: Vec<(&str, Box<dyn Fn(&mut Oobe)>)> = vec![
            ("swapfile", Box::new(|_: &mut Oobe| {})),
            (
                "swap partition",
                Box::new(|o: &mut Oobe| {
                    o.install.swap = SwapPlan::Partition {
                        size_bytes: 8 * GIB,
                    }
                }),
            ),
            (
                "no swap",
                Box::new(|o: &mut Oobe| o.install.swap = SwapPlan::None),
            ),
            (
                "btrfs",
                Box::new(|o: &mut Oobe| o.install.root_filesystem = RootFilesystem::Btrfs),
            ),
            (
                "esp staging",
                Box::new(|o: &mut Oobe| {
                    o.staging.kind = StagingKind::Esp;
                    o.staging.partuuid = ESP_PARTUUID.to_owned();
                    o.staging.reclaim_after_install = false;
                }),
            ),
            (
                "no reclaim",
                Box::new(|o: &mut Oobe| o.staging.reclaim_after_install = false),
            ),
        ];

        for (name, edit) in configurations {
            let plan = plan_with(|o| edit(o)).unwrap_or_else(|e| panic!("{name}: {e}"));
            let destructive = plan.destructive_devices();
            assert!(
                !destructive.contains(&"/dev/sda3"),
                "{name}: Windows appeared as a destructive target: {destructive:?}"
            );
            assert!(
                !destructive.contains(&"/dev/sda1"),
                "{name}: the ESP appeared as a destructive target: {destructive:?}"
            );
            assert!(
                !destructive.contains(&"/dev/sda2"),
                "{name}: the Microsoft reserved partition was touched: {destructive:?}"
            );
            // Everything destructive is a partition this plan creates, or the
            // staging partition it was explicitly authorized to reclaim.
            for device in &destructive {
                let is_new = *device == plan.root.path
                    || plan
                        .swap_partition
                        .as_ref()
                        .is_some_and(|s| s.path == *device);
                let is_reclaim = matches!(
                    &plan.reclaim,
                    Reclaim::GrowRootOver { staging, .. } if staging.path == *device
                );
                assert!(is_new || is_reclaim, "{name}: {device} is neither new nor reclaimed");
            }
        }
    }

    /// The ESP is written to — that is the point of sharing it — but only ever
    /// at the file level.
    #[test]
    fn the_shared_esp_is_only_written_at_the_file_level() {
        let plan = plan_with(|_| {}).expect("plan");
        assert_eq!(plan.file_level_devices(), vec!["/dev/sda1"]);
        assert!(!plan.destructive_devices().contains(&"/dev/sda1"));
    }

    /// The internal guard has to actually fire, or it is decoration. A plan is
    /// built and then mutated to add a step that formats Windows.
    #[test]
    fn the_internal_guard_catches_a_step_that_would_destroy_windows() {
        let mut plan = plan_with(|_| {}).expect("plan");
        let snapshot = snapshot(fixture::FINDMNT_JSON_IN_RAM);
        let disk = &snapshot.disks[0];

        plan.steps.push(Step::MakeFilesystem {
            device: "/dev/sda3".to_owned(),
            filesystem: RootFilesystem::Ext4,
            label: "oops".to_owned(),
        });

        let refusal =
            verify_nothing_pre_existing_is_destroyed(&plan, disk).expect_err("must refuse");
        assert!(matches!(
            refusal,
            Refusal::WouldDestroyExistingPartition { ref device, .. } if device == "/dev/sda3"
        ));
    }

    // --- MARK: Refusal: the free region ---

    #[test]
    fn refuses_when_the_free_region_moved() {
        let refusal = plan_with(|o| {
            // Off by 4 GiB — far outside the 64 MiB tolerance.
            o.target.expected_free_start_bytes = FREE_START + 4 * GIB;
        })
        .expect_err("must refuse");
        assert!(
            matches!(refusal, Refusal::FreeRegionMismatch { .. }),
            "{refusal:?}"
        );
        assert!(refusal.to_string().contains("not in the state we left it in"));
    }

    #[test]
    fn refuses_when_the_free_region_changed_size() {
        let refusal = plan_with(|o| o.target.expected_free_bytes = FREE_BYTES + 8 * GIB)
            .expect_err("must refuse");
        assert!(matches!(refusal, Refusal::FreeRegionMismatch { .. }));
    }

    /// Alignment and Windows' own cluster rounding move these numbers by a few
    /// mebibytes as a matter of course, and refusing on that would make the
    /// installer useless.
    #[test]
    fn small_differences_are_within_tolerance() {
        assert!(plan_with(|o| {
            o.target.expected_free_start_bytes = FREE_START + 32 * MIB;
            o.target.expected_free_bytes = FREE_BYTES - 32 * MIB;
        })
        .is_ok());
    }

    /// The payload may describe the freed region either before or after the
    /// staging partition was carved out of it. Both readings have to be
    /// accepted, or half of all real payloads would be refused.
    #[test]
    fn the_freed_region_may_be_described_with_the_staging_partition_included() {
        // CROWNSTAGE is sectors 800032768..=802129919, immediately before the
        // gap. A payload that recorded the region *before* staging was carved
        // out describes the union of the two.
        let union_start = 800_032_768 * 512;
        let union_bytes = (1_000_215_182 - 800_032_768 + 1) * 512;
        assert!(
            plan_with(|o| {
                o.target.expected_free_start_bytes = union_start;
                o.target.expected_free_bytes = union_bytes;
            })
            .is_ok(),
            "the pre-staging reading of the freed region must be accepted"
        );
    }

    #[test]
    fn refuses_a_free_region_too_small_to_install_into() {
        // Grow CROWNSTAGE until the only gap left is about 2 GiB.
        let mut table = fixture::STANDARD_TABLE.to_vec();
        table[3].size = 196_114_432; // 800032768..=996147199
        let snapshot = snapshot_with(
            fixture::FINDMNT_JSON_IN_RAM,
            &fixture::sfdisk_json("gpt", &table),
        );

        let disk = &snapshot.disks[0];
        let gap = *disk.free_gaps().last().expect("a gap");
        let gap_bytes = disk.bytes(gap.sectors);
        assert!(
            gap_bytes < MIN_ROOT_BYTES,
            "fixture should be too small, got {}",
            bytes(gap_bytes)
        );

        let mut oobe = payload();
        oobe.target.expected_free_start_bytes = disk.bytes(gap.start_sector);
        oobe.target.expected_free_bytes = gap_bytes;
        // Nothing to grow into, so the reclaim question never arises.
        oobe.staging.reclaim_after_install = false;

        let refusal = plan_against(&snapshot, oobe).expect_err("must refuse");
        assert!(
            matches!(refusal, Refusal::FreeRegionTooSmall { .. }),
            "{refusal:?}"
        );
    }

    #[test]
    fn refuses_a_swap_partition_that_crowds_out_the_system() {
        let refusal = plan_with(|o| {
            o.install.swap = SwapPlan::Partition {
                size_bytes: 120 * GIB,
            }
        })
        .expect_err("must refuse");
        assert!(matches!(refusal, Refusal::SwapDoesNotFit { .. }), "{refusal:?}");
    }

    #[test]
    fn refuses_a_disk_with_no_unallocated_space() {
        // Windows over the whole disk, no CROWNSTAGE.
        let mut table = fixture::STANDARD_TABLE[..3].to_vec();
        table[2].size = fixture::LAST_LBA - table[2].start + 1;
        let snapshot = snapshot_with(
            fixture::FINDMNT_JSON_IN_RAM,
            &fixture::sfdisk_json("gpt", &table),
        );
        assert!(
            snapshot.disks[0].free_gaps().is_empty(),
            "fixture should have no gaps: {:?}",
            snapshot.disks[0].free_gaps()
        );

        let mut oobe = payload();
        oobe.staging.kind = StagingKind::Esp;
        oobe.staging.partuuid = ESP_PARTUUID.to_owned();
        oobe.staging.reclaim_after_install = false;

        let refusal = plan_against(&snapshot, oobe).expect_err("must refuse");
        assert_eq!(
            refusal,
            Refusal::NoFreeRegion {
                disk: "/dev/sda".to_owned()
            }
        );
    }

    // --- MARK: Refusal: the staging medium ---

    /// The payload is still being read off the staging partition, and the plan
    /// wants to delete that partition. Stop.
    #[test]
    fn refuses_to_write_to_the_staging_partition_while_it_is_mounted() {
        // The payload asks for the staging partition to be reclaimed — that is a
        // request to write to it — and the live system is still reading from it.
        let refusal = plan_on(fixture::FINDMNT_JSON_ON_MEDIUM, |o| {
            assert!(o.staging.reclaim_after_install);
        })
        .expect_err("must refuse");
        match refusal {
            Refusal::StagingMediumStillInUse {
                partuuid,
                mounted_at,
            } => {
                assert_eq!(partuuid, STAGING_PARTUUID);
                assert_eq!(mounted_at, vec![BOOTMNT.to_owned()]);
            }
            other => panic!("expected StagingMediumStillInUse, got {other:?}"),
        }
    }

    /// The same machine, but the payload is on the ESP — which the plan must
    /// write the boot loader to. Writing to a partition the live system is
    /// reading from is the thing we refuse.
    #[test]
    fn refuses_to_write_to_a_mounted_esp_staging_medium() {
        let findmnt = fixture::FINDMNT_JSON_ON_MEDIUM
            .replace(STAGING_PARTUUID, ESP_PARTUUID);
        let refusal = plan_on(&findmnt, |o| {
            o.staging.kind = StagingKind::Esp;
            o.staging.partuuid = ESP_PARTUUID.to_owned();
            o.staging.reclaim_after_install = false;
        })
        .expect_err("must refuse");
        match refusal {
            Refusal::StagingMediumStillInUse { partuuid, mounted_at } => {
                assert_eq!(partuuid, ESP_PARTUUID);
                assert_eq!(mounted_at, vec![BOOTMNT.to_owned()]);
            }
            other => panic!("expected StagingMediumStillInUse, got {other:?}"),
        }
    }

    /// In RAM but still mounted (archiso leaves the mount around until its hook
    /// tidies up): the plan unmounts it, as an explicit step, before writing.
    ///
    /// Uses the reclaimable layout, because that is the configuration in which
    /// the plan actually writes to the staging partition.
    const FINDMNT_JSON_IN_RAM_BUT_STILL_MOUNTED: &str = r#"{
       "filesystems": [
          {"source":"airootfs","target":"/","fstype":"overlay"},
          {"source":"tmpfs","target":"/run/archiso/copytoram","fstype":"tmpfs"},
          {"source":"/dev/disk/by-partuuid/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","target":"/run/archiso/bootmnt","fstype":"vfat"}
       ]
    }"#;

    #[test]
    fn a_stale_mount_over_an_in_ram_payload_is_unmounted_first() {
        let snapshot =
            snapshot_with_staging_after_the_region(FINDMNT_JSON_IN_RAM_BUT_STILL_MOUNTED);
        let plan =
            plan_against(&snapshot, payload_for_staging_after_the_region()).expect("plan");

        assert!(
            matches!(plan.reclaim, Reclaim::GrowRootOver { .. }),
            "{:?}",
            plan.reclaim
        );
        assert_eq!(
            plan.steps.first(),
            Some(&Step::UnmountStagingMedium {
                target: BOOTMNT.to_owned()
            }),
            "the unmount must come before anything else"
        );
        assert!(plan.warnings.iter().any(|w| w.contains("unmounted")));
    }

    // --- MARK: Reclaim ---

    /// CROWNSTAGE in the fixture sits *before* the free region, so growing root
    /// over it would mean moving the filesystem. The space is left alone and the
    /// reason is recorded.
    #[test]
    fn a_staging_partition_before_the_region_is_not_reclaimed() {
        let plan = plan_with(|_| {}).expect("plan");
        match &plan.reclaim {
            Reclaim::Keep { reason } => {
                assert!(reason.contains("sits before"), "{reason}");
            }
            other => panic!("expected the space to be kept, got {other:?}"),
        }
        assert!(!plan
            .steps
            .iter()
            .any(|s| matches!(s, Step::DeleteStagingPartition { .. })));
    }

    /// The reclaimable shape: CROWNSTAGE at the *end* of the disk, immediately
    /// after the region root will occupy.
    ///
    /// Gap 800032768..=997855231, CROWNSTAGE 997855232..=999952383.
    const REGION_THEN_STAGING_START: u64 = 800_032_768;
    const REGION_THEN_STAGING_END: u64 = 997_855_231;

    fn snapshot_with_staging_after_the_region(findmnt: &str) -> SystemSnapshot {
        let mut table = fixture::STANDARD_TABLE.to_vec();
        table[3].start = REGION_THEN_STAGING_END + 1;
        snapshot_with(findmnt, &fixture::sfdisk_json("gpt", &table))
    }

    fn payload_for_staging_after_the_region() -> Oobe {
        let mut oobe = payload();
        oobe.target.expected_free_start_bytes = REGION_THEN_STAGING_START * 512;
        oobe.target.expected_free_bytes =
            (REGION_THEN_STAGING_END - REGION_THEN_STAGING_START + 1) * 512;
        oobe
    }

    #[test]
    fn a_staging_partition_after_the_region_is_reclaimed() {
        let snapshot = snapshot_with_staging_after_the_region(fixture::FINDMNT_JSON_IN_RAM);
        let plan = plan_against(&snapshot, payload_for_staging_after_the_region()).expect("plan");

        let Reclaim::GrowRootOver {
            staging,
            staging_number,
            new_root_end_sector,
        } = &plan.reclaim
        else {
            panic!("expected a reclaim, got {:?}", plan.reclaim);
        };
        assert_eq!(staging.path, "/dev/sda4");
        assert_eq!(*staging_number, 4);
        assert!(*new_root_end_sector > plan.root.end_sector);

        // And the steps for it come last, after the unmount.
        let labels: Vec<String> = plan.steps.iter().map(Step::label).collect();
        let unmount = labels
            .iter()
            .position(|l| l == "Unmounting the new system")
            .expect("unmount");
        let delete = labels
            .iter()
            .position(|l| l.contains("Reclaiming"))
            .expect("delete");
        assert!(unmount < delete, "{labels:?}");
        assert!(labels.iter().any(|l| l.contains("Growing the CrownOS partition")));
        assert!(labels.iter().any(|l| l.contains("Growing the CrownOS filesystem")));
    }

    #[test]
    fn a_staging_partition_is_not_reclaimed_when_the_payload_is_not_in_ram() {
        // Reclaim requires the payload to be in RAM. With the medium mounted,
        // `plan` refuses outright — so switch reclaim off in the payload and
        // check the recorded reason instead, which is the same code path with
        // the refusal removed.
        let plan = plan_on(fixture::FINDMNT_JSON_ON_MEDIUM, |o| {
            o.staging.reclaim_after_install = false;
        })
        .expect("plan");
        assert!(matches!(plan.reclaim, Reclaim::Keep { .. }));
        assert!(!plan
            .steps
            .iter()
            .any(|s| matches!(s, Step::DeleteStagingPartition { .. })));
    }

    #[test]
    fn an_esp_staged_payload_is_never_reclaimed() {
        let plan = plan_with(|o| {
            o.staging.kind = StagingKind::Esp;
            o.staging.partuuid = ESP_PARTUUID.to_owned();
            // Even if the payload asks — the ESP must survive.
            o.staging.reclaim_after_install = true;
        })
        .expect("plan");
        match &plan.reclaim {
            Reclaim::Keep { reason } => assert!(reason.contains("EFI System Partition"), "{reason}"),
            other => panic!("the ESP must never be reclaimed, got {other:?}"),
        }
    }

    #[test]
    fn refuses_when_the_staging_partition_has_vanished() {
        let refusal = plan_with(|o| {
            o.staging.partuuid = "00000000-0000-0000-0000-000000000000".to_owned()
        })
        .expect_err("must refuse");
        assert!(matches!(refusal, Refusal::StagingPartitionNotFound { .. }));
    }

    #[test]
    fn a_btrfs_root_is_not_grown_over_the_reclaimed_space() {
        let snapshot = snapshot_with_staging_after_the_region(fixture::FINDMNT_JSON_IN_RAM);
        let mut oobe = payload_for_staging_after_the_region();
        oobe.install.root_filesystem = RootFilesystem::Btrfs;

        let plan = plan_against(&snapshot, oobe).expect("plan");
        match &plan.reclaim {
            Reclaim::Keep { reason } => assert!(reason.contains("btrfs"), "{reason}"),
            other => panic!("expected btrfs to be kept, got {other:?}"),
        }
    }

    // --- MARK: Refusal: the account ---

    #[test]
    fn refuses_an_unusable_username() {
        let snapshot = snapshot(fixture::FINDMNT_JSON_IN_RAM);
        for (username, needle) in [
            ("", "empty"),
            ("Crown", "lowercase"),
            ("crown user", "may only contain"),
            ("root", "reserved"),
            ("1crown", "must start with"),
        ] {
            let refusal = plan_against_as(
                &snapshot,
                payload(),
                Account {
                    username: username.to_owned(),
                    ..account()
                },
            )
            .expect_err("must refuse");
            match refusal {
                Refusal::InvalidUsername { reason, .. } => {
                    assert!(reason.contains(needle), "{username:?}: {reason}");
                }
                other => panic!("{username:?}: expected InvalidUsername, got {other:?}"),
            }
        }
    }

    #[test]
    fn refuses_an_unusable_hostname() {
        let snapshot = snapshot(fixture::FINDMNT_JSON_IN_RAM);
        let refusal = plan_against_as(
            &snapshot,
            payload(),
            Account {
                hostname: "-nope-".to_owned(),
                ..account()
            },
        )
        .expect_err("must refuse");
        assert!(matches!(refusal, Refusal::InvalidHostname { .. }));
    }

    // --- MARK: Warnings ---

    #[test]
    fn a_cramped_esp_is_a_warning_and_not_a_refusal() {
        let plan = plan_with(|o| o.target.esp_size_bytes = 100 * MIB).expect("plan");
        assert!(plan
            .warnings
            .iter()
            .any(|w| w.contains("shared EFI partition")));
    }

    #[test]
    fn asking_for_more_than_the_disk_has_is_a_warning() {
        let plan = plan_with(|o| {
            o.install.allocated_bytes = FREE_BYTES + 40 * GIB;
            // Keep the region check happy — the payload's own idea of the region
            // is still right, it is the *allocation* that was optimistic.
        })
        .expect("plan");
        assert!(plan.warnings.iter().any(|w| w.contains("You asked for")));
    }

    // --- MARK: Arithmetic ---

    #[test]
    fn alignment_rounds_the_right_way() {
        assert_eq!(align_up(0, 2048), 0);
        assert_eq!(align_up(1, 2048), 2048);
        assert_eq!(align_up(2048, 2048), 2048);
        assert_eq!(align_down(4095, 2048), 2048);
        assert_eq!(align_down(4096, 2048), 4096);
    }

    #[test]
    fn byte_counts_read_the_way_a_partition_table_measures_them() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1024), "1.0 KiB");
        assert_eq!(bytes(128 * GIB), "128.0 GiB");
    }

    #[test]
    fn the_allocation_bar_adds_up() {
        let plan = plan_with(|_| {}).expect("plan");
        assert!(plan.crownos_bytes() > 0);
        assert!(plan.windows_bytes > 0);
        assert!(plan.crownos_bytes() + plan.windows_bytes <= plan.disk_size_bytes);
    }

    // --- MARK: ISO installs ---

    /// The whole ISO input, built against the standard fixture machine, with
    /// `edit` applied. Mirrors `plan_with` for the payload path.
    fn iso_plan_with(edit: impl FnOnce(&mut IsoTarget)) -> Result<Plan, Refusal> {
        let snapshot = snapshot(fixture::FINDMNT_JSON_IN_RAM);
        let disk = &snapshot.disks[0];
        let region = disk
            .free_gaps()
            .into_iter()
            .max_by_key(|gap| gap.sectors)
            .expect("a gap");
        let mut target = IsoTarget {
            disk: disk.path.clone(),
            esp: "/dev/sda1".to_owned(),
            region_start_sector: region.start_sector,
            filesystem: RootFilesystem::Ext4,
            distro: Distro::Arch,
            swap: SwapPlan::None,
            edits: Vec::new(),
        };
        edit(&mut target);
        plan_iso(&IsoInput {
            snapshot: &snapshot,
            disk: target.disk,
            esp: target.esp,
            region_start_sector: target.region_start_sector,
            root_filesystem: target.filesystem,
            distro: target.distro,
            swap: target.swap,
            locale: Locale::default(),
            account: account(),
            ids: ids(),
            edits: target.edits,
        })
    }

    /// The knobs an ISO-flow test wants to turn. A struct rather than eight
    /// arguments, because a call site passing `"/dev/sda1", 802129920, ...` says
    /// nothing about which one it is varying.
    struct IsoTarget {
        disk: String,
        esp: String,
        region_start_sector: u64,
        filesystem: RootFilesystem,
        distro: Distro,
        swap: SwapPlan,
        edits: Vec<crate::engine::disks::DiskOp>,
    }

    /// The baseline: no payload, no Windows partition designated, and a complete
    /// install laid out in the free space the user picked.
    #[test]
    fn an_iso_install_lays_out_a_root_in_the_chosen_region() {
        let plan = iso_plan_with(|_| {}).expect("a plan");
        assert_eq!(plan.disk, "/dev/sda");
        assert_eq!(plan.esp.path, "/dev/sda1");
        assert_eq!(plan.root.start_sector, 802_129_920);
        assert!(plan.root.size_bytes() > 90 * GIB);
        assert!(
            plan.steps
                .iter()
                .any(|step| matches!(step, Step::InstallBaseSystem { .. })),
            "an ISO plan installs the same base system as a payload-driven one"
        );
    }

    /// An ISO install designates nothing to preserve, and that has to be the
    /// absence of a value rather than an empty string — see [`Plan::windows`].
    #[test]
    fn an_iso_install_designates_nothing_to_preserve() {
        let plan = iso_plan_with(|_| {}).expect("a plan");
        assert_eq!(plan.windows, None);
        assert_eq!(plan.windows_bytes, 0);
    }

    /// A UEFI machine boots from an ESP, so an install onto a disk without one
    /// is refused — with the message that says how to make one.
    #[test]
    fn a_disk_with_no_esp_is_refused() {
        // A table with the ESP taken out of it.
        let table: Vec<fixture::Part> = fixture::STANDARD_TABLE[1..].to_vec();
        let snapshot = snapshot_with(
            fixture::FINDMNT_JSON_IN_RAM,
            &fixture::sfdisk_json("gpt", &table),
        );
        let disk = &snapshot.disks[0];
        let region = disk
            .free_gaps()
            .into_iter()
            .max_by_key(|gap| gap.sectors)
            .expect("a gap");
        let refusal = plan_iso(&IsoInput {
            snapshot: &snapshot,
            disk: disk.path.clone(),
            esp: "/dev/sda1".to_owned(),
            region_start_sector: region.start_sector,
            root_filesystem: RootFilesystem::Ext4,
            distro: Distro::Arch,
            swap: SwapPlan::None,
            locale: Locale::default(),
            account: account(),
            ids: ids(),
            edits: Vec::new(),
        })
        .expect_err("no ESP, no install");
        assert!(matches!(refusal, Refusal::NoEspOnDisk { .. }), "{refusal:?}");
        assert!(refusal.to_string().contains("partition editor"));
    }

    /// Root on one disk and the boot loader on another is a machine that stops
    /// booting the moment either is unplugged.
    #[test]
    fn an_esp_on_another_disk_is_refused() {
        let refusal = iso_plan_with(|target| target.esp = "/dev/sdz1".to_owned())
            .expect_err("the ESP must be on the target disk");
        assert!(
            matches!(refusal, Refusal::EspOnAnotherDisk { .. }),
            "{refusal:?}"
        );
    }

    /// The region is identified by where it starts, and a start that is not the
    /// start of a hole is a table that changed after the list was drawn.
    #[test]
    fn a_region_that_is_not_free_is_refused() {
        let refusal = iso_plan_with(|target| target.region_start_sector = 600_000_000)
            .expect_err("that sector is inside the Windows partition");
        assert!(
            matches!(refusal, Refusal::ChosenRegionNotFree { .. }),
            "{refusal:?}"
        );
    }

    /// A GPT table is required on this path too, for the same reason it is on
    /// the payload path.
    #[test]
    fn an_iso_install_onto_an_mbr_disk_is_refused() {
        let snapshot = snapshot_with(
            fixture::FINDMNT_JSON_IN_RAM,
            &fixture::sfdisk_json("dos", fixture::STANDARD_TABLE),
        );
        let refusal = plan_iso(&IsoInput {
            snapshot: &snapshot,
            disk: "/dev/sda".to_owned(),
            esp: "/dev/sda1".to_owned(),
            region_start_sector: 802_129_920,
            root_filesystem: RootFilesystem::Ext4,
            distro: Distro::Arch,
            swap: SwapPlan::None,
            locale: Locale::default(),
            account: account(),
            ids: ids(),
            edits: Vec::new(),
        })
        .expect_err("MBR is refused");
        assert!(matches!(refusal, Refusal::NotGpt { .. }), "{refusal:?}");
    }

    /// An account that will not survive `useradd` is refused on this path too.
    /// The check is shared, and a test on each path is what keeps it shared.
    #[test]
    fn an_iso_install_validates_the_account() {
        let snapshot = snapshot(fixture::FINDMNT_JSON_IN_RAM);
        let refusal = plan_iso(&IsoInput {
            snapshot: &snapshot,
            disk: "/dev/sda".to_owned(),
            esp: "/dev/sda1".to_owned(),
            region_start_sector: 802_129_920,
            root_filesystem: RootFilesystem::Ext4,
            distro: Distro::Arch,
            swap: SwapPlan::None,
            locale: Locale::default(),
            account: Account {
                username: "Root Beer".to_owned(),
                full_name: None,
                hostname: "crownos".to_owned(),
                autologin: false,
            },
            ids: ids(),
            edits: Vec::new(),
        })
        .expect_err("a username with a space in it is refused");
        assert!(matches!(refusal, Refusal::InvalidUsername { .. }), "{refusal:?}");
    }

    // --- MARK: Base distributions ---

    /// Every command a plan would run, one per line, as a dry run logs them.
    fn commands(plan: &Plan) -> String {
        let secrets = crate::engine::exec::Secrets {
            user_password: "hunter2".to_owned(),
        };
        plan.steps
            .iter()
            .flat_map(|step| crate::engine::exec::commands_for(plan, &secrets, step))
            .map(|command| command.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn every_base_plans_on_the_same_disk_without_touching_anything_else() {
        let arch = iso_plan_with(|_| {}).expect("an arch plan");
        for distro in Distro::ALL {
            let plan = iso_plan_with(|t| t.distro = distro).expect("a plan");
            assert_eq!(plan.distro, distro);
            assert_eq!(plan.root, arch.root, "{distro:?}");
            assert_eq!(plan.destructive_devices(), arch.destructive_devices(), "{distro:?}");
            assert_eq!(plan.file_level_devices(), vec!["/dev/sda1"], "{distro:?}");
            assert!(!commands(&plan).contains("hunter2"), "{distro:?}");
        }
    }

    #[test]
    fn the_fallback_loader_is_backed_up_before_any_base_is_installed() {
        for distro in Distro::ALL {
            let plan = iso_plan_with(|t| t.distro = distro).expect("a plan");
            let position = |wanted: fn(&Step) -> bool| plan.steps.iter().position(wanted);
            let backup = position(|s| matches!(s, Step::BackUpFallbackBootloader { .. }));
            let install = position(|s| {
                matches!(s, Step::InstallBaseSystem { .. } | Step::InstallNixos { .. })
            });
            assert!(backup.is_some() && backup < install, "{distro:?}");
        }
    }

    #[test]
    fn debian_is_bootstrapped_with_debootstrap_and_boots_through_kernel_install() {
        let plan = iso_plan_with(|t| t.distro = Distro::Debian).expect("a plan");
        let log = commands(&plan);
        assert!(log.contains("debootstrap --arch=amd64"), "{log}");
        assert!(log.contains("kernel-install add"), "{log}");
        assert!(log.contains("%sudo ALL=(ALL:ALL) ALL"), "{log}");
        assert!(log.contains("apt-get install --yes crownos-desktop"), "{log}");
        for arch_only in ["pacstrap", "mkinitcpio", "loader/entries/crownos.conf"] {
            assert!(!log.contains(arch_only), "`{arch_only}` in a Debian plan: {log}");
        }
    }

    #[test]
    fn fedora_is_bootstrapped_with_dnf_and_relabelled_on_first_boot() {
        let plan = iso_plan_with(|t| t.distro = Distro::Fedora).expect("a plan");
        let log = commands(&plan);
        assert!(log.contains("dnf --installroot=/mnt --releasever="), "{log}");
        assert!(log.contains("glibc-langpack-en"), "{log}");
        assert!(log.contains("/mnt/.autorelabel"), "{log}");
        assert!(log.contains("chronyd.service"), "{log}");
    }

    #[test]
    fn nixos_is_one_configuration_and_nixos_install() {
        let plan = iso_plan_with(|t| t.distro = Distro::NixOs).expect("a plan");
        assert!(plan.steps.iter().any(|s| matches!(s, Step::ConfigureNixos { .. })));
        assert!(plan.steps.iter().any(|s| matches!(s, Step::InstallNixos { .. })));
        let imperative = plan.steps.iter().find(|s| {
            matches!(
                s,
                Step::InstallBaseSystem { .. }
                    | Step::GenerateFstab { .. }
                    | Step::InstallBootloader { .. }
                    | Step::CreateUser { .. }
                    | Step::LockRootAccount { .. }
            )
        });
        assert_eq!(imperative, None, "NixOS plans are declarative");
        let log = commands(&plan);
        assert!(log.contains("nixos-install --root /mnt"), "{log}");
        assert!(log.contains("nixos-enter --root /mnt -- chpasswd"), "{log}");
    }

    // --- MARK: The editor's authorization ---

    /// The feature the authorization list exists for: a partition the user
    /// deliberately deleted in the editor may be destroyed, and the guard says
    /// so by name rather than by a blanket exemption.
    #[test]
    fn a_partition_the_user_deleted_is_authorized_and_the_plan_runs() {
        let plan = iso_plan_with(|target| {
            // Deleting `sda4` merges its gigabyte into the region that followed
            // it, so the region now starts where `sda4` did. The planner does
            // not re-resolve that by itself — a chosen region that is not free
            // is a refusal, deliberately, because the same symptom means "the
            // disk changed under us" when nothing was queued. The wizard's
            // `App::resettle_region` is what moves the selection after an edit;
            // here it is named directly.
            target.region_start_sector = 800_032_768;
            target.edits = vec![crate::engine::disks::DiskOp::Delete {
                disk: "/dev/sda".to_owned(),
                number: 4,
                path: "/dev/sda4".to_owned(),
                partuuid: Some(STAGING_PARTUUID.to_owned()),
                size_bytes: GIB,
            }];
        })
        .expect("a deliberate delete is allowed");

        assert_eq!(plan.authorized_destructive, vec!["/dev/sda4".to_owned()]);
        assert!(
            plan.destructive_devices().contains(&"/dev/sda4"),
            "the delete is a destructive step: {:?}",
            plan.destructive_devices()
        );
        // And the edit runs before anything the layout depends on.
        assert!(
            matches!(plan.steps.first(), Some(Step::EditPartitionTable(_))),
            "{:?}",
            plan.steps.first().map(Step::label)
        );
    }

    /// Deleting a partition merges its space into the region after it, and the
    /// root partition is laid out against that merged region rather than against
    /// the table as it is now.
    #[test]
    fn the_layout_uses_the_table_the_edits_leave() {
        let untouched = iso_plan_with(|_| {}).expect("a plan");
        let after_delete = iso_plan_with(|target| {
            target.region_start_sector = 800_032_768;
            target.edits = vec![crate::engine::disks::DiskOp::Delete {
                disk: "/dev/sda".to_owned(),
                number: 4,
                path: "/dev/sda4".to_owned(),
                partuuid: Some(STAGING_PARTUUID.to_owned()),
                size_bytes: GIB,
            }];
        })
        .expect("a plan with the staging partition deleted");

        assert!(
            after_delete.root.size_bytes() > untouched.root.size_bytes(),
            "the freed gigabyte should have gone to root: {} vs {}",
            after_delete.root.size_bytes(),
            untouched.root.size_bytes()
        );
        assert_eq!(after_delete.root.start_sector, 800_032_768);
    }

    /// The authorization list is a list and not a flag: an edit naming one
    /// partition does not license destroying another. Constructed by hand
    /// because no code path produces it — which is the point of the guard.
    #[test]
    fn authorizing_one_partition_does_not_authorize_another() {
        let mut plan = iso_plan_with(|_| {}).expect("a plan");
        let disk = snapshot(fixture::FINDMNT_JSON_IN_RAM).disks[0].clone();

        plan.authorized_destructive = vec!["/dev/sda4".to_owned()];
        plan.steps.push(Step::MakeFilesystem {
            device: "/dev/sda3".to_owned(),
            filesystem: RootFilesystem::Ext4,
            label: ROOT_FSLABEL.to_owned(),
        });

        let refusal = verify_nothing_pre_existing_is_destroyed(&plan, &disk)
            .expect_err("sda3 was never authorized");
        match refusal {
            Refusal::WouldDestroyExistingPartition { device, .. } => {
                assert_eq!(device, "/dev/sda3");
            }
            other => panic!("expected a refusal about sda3, got {other:?}"),
        }
    }

    /// And the ESP is never destroyable, authorization list or not: a plan whose
    /// own boot partition is reformatted part way through is not something a
    /// confirmation page can meaningfully warn about.
    #[test]
    fn the_esp_is_not_destroyable_even_when_authorized() {
        let mut plan = iso_plan_with(|_| {}).expect("a plan");
        let disk = snapshot(fixture::FINDMNT_JSON_IN_RAM).disks[0].clone();

        plan.authorized_destructive = vec!["/dev/sda1".to_owned()];
        plan.steps.push(Step::MakeFilesystem {
            device: "/dev/sda1".to_owned(),
            filesystem: RootFilesystem::Ext4,
            label: ROOT_FSLABEL.to_owned(),
        });

        let refusal = verify_nothing_pre_existing_is_destroyed(&plan, &disk)
            .expect_err("the ESP is protected unconditionally");
        assert!(
            matches!(refusal, Refusal::WouldDestroyExistingPartition { .. }),
            "{refusal:?}"
        );
    }

    /// An ISO install has no staging medium of its own to reclaim, and says so
    /// rather than leaving the field to be read as "we forgot".
    #[test]
    fn an_iso_install_has_nothing_to_reclaim() {
        let plan = iso_plan_with(|_| {}).expect("a plan");
        match &plan.reclaim {
            Reclaim::Keep { reason } => assert!(reason.contains("CrownOS image"), "{reason}"),
            other => panic!("expected Keep, got {other:?}"),
        }
    }

}
