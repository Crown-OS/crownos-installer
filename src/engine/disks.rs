//! Free-form partition editing: create, delete, resize, format.
//!
//! This is the module that lets a user do to their disk the things a
//! partitioner does — as opposed to [`plan`](super::plan), which does exactly
//! one thing (install CrownOS into a region Windows freed) and refuses
//! everything else. Both exist because they answer different questions:
//!
//! * `plan` answers "the Windows installer left instructions; are they still
//!   true, and what do they mean for this disk?" It decides. Its input is a
//!   file, and every rule in it is there to catch a file that no longer
//!   describes reality.
//! * this module answers "the user asked for this; is it safe, and what
//!   commands is it?" It **decides nothing**. Every operation here was typed by
//!   a person looking at their own disk, so there is no intent to second-guess —
//!   only physics to check.
//!
//! It is available in both modes, staged and ISO. A user who came from the
//! Windows installer and finds it left the wrong amount of room should not have
//! to reboot into Windows to fix it.
//!
//! # Why `parted`
//!
//! [`plan`](super::plan) uses `sfdisk --append`, and says why: `--append` adds a
//! GPT entry and rewrites no other one, which is the strongest guarantee
//! available for "only ever adds". That guarantee is exactly what an editor
//! cannot have — deleting and resizing *are* rewrites — so the argument for
//! `sfdisk` does not carry over, and `parted` is the better tool for what is
//! left:
//!
//! * **It aligns.** `--align optimal` puts a partition on the boundary the
//!   device reports it wants, which on a 4Kn or SSD-backed disk is the
//!   difference between full speed and a permanent 30% write penalty. Getting
//!   that right by hand means reading `optimal_io_size` out of sysfs and
//!   redoing arithmetic the kernel already did.
//! * **It refuses overlaps itself.** A second check under ours, in a tool that
//!   has been doing it for twenty years.
//! * **`resizepart` exists.** `sfdisk` can only rewrite a whole table to move
//!   one boundary.
//!
//! One thing `parted` cannot do, and where `sfdisk` comes back: **naming an
//! arbitrary GPT type GUID.** `mkpart`'s filesystem argument is a *hint* that
//! picks a type from a short built-in table, and the GUID CrownOS's root
//! partition needs — the x86-64 root GUID from the Discoverable Partitions
//! Spec, [`TYPE_LINUX_ROOT_X86_64`] — is not in it. A root partition carrying
//! "Linux filesystem" instead boots, but nothing auto-discovers it. So a
//! [`DiskOp::Create`] is `parted mkpart` for the geometry followed by
//! `sfdisk --part-type` for the type, and the doc comment on
//! [`create_commands`] says so at the point it matters.
//!
//! # Resizing a filesystem is not resizing a partition
//!
//! The single most destructive mistake available in this module, and the reason
//! [`resize_commands`] is ordered rather than obvious: `parted resizepart` moves
//! a **table entry**. It does not touch the filesystem inside it. Modern
//! `parted` dropped filesystem resizing altogether.
//!
//! So shrinking is *filesystem first, then partition* — shrink the entry first
//! and the filesystem's own idea of its size now extends past the end of its
//! partition, which is a corrupt filesystem. Growing is the other way round:
//! *partition first, then filesystem* — grow the filesystem first and it writes
//! past the entry, over whatever comes next.
//!
//! `the_order_of_a_shrink_is_filesystem_then_partition` and its growing
//! counterpart are the tests that keep that straight, and they are the two tests
//! in this file worth reading before changing anything.
//!
//! # What is refused
//!
//! Only what is physically unsafe or unrecoverable, each with a test:
//!
//! 1. **Nothing mounted is touched.** Not by us; the kernel will not let a
//!    mounted filesystem be shrunk under it, and a partition table rewritten
//!    under a mount is a filesystem whose next write goes somewhere else
//!    entirely. → [`OpRefusal::PartitionInUse`]
//! 2. **Not the medium we are running from.** A live USB stick is a disk like
//!    any other, and it shows up in the editor like any other. Deleting the
//!    partition the running system is reading itself out of ends the install
//!    mid-`pacstrap`. → [`OpRefusal::IsLiveMedium`]
//! 3. **No overlaps, and nothing off the end of the disk.** Checked against the
//!    *projection* — the table as it will be after every earlier operation in
//!    the queue — because a queue is not a set and `delete 3, create at 3` is
//!    legal while `create at 3` alone is not. → [`OpRefusal::Overlaps`],
//!    [`OpRefusal::OutsideDisk`]
//! 4. **A filesystem we cannot shrink offline is not shrunk.** btrfs needs to be
//!    mounted to resize; f2fs and xfs cannot shrink at all. Refused by name
//!    rather than attempted and failed half way.
//!    → [`OpRefusal::UnsupportedShrink`]
//! 5. **GPT only.** The same rule `plan` has, for the same reason: an MBR
//!    disk's four-entry limit and 2 TiB ceiling make every offset here mean
//!    something different. A blank disk counts, once the queue starts by
//!    writing a GPT to it. → [`OpRefusal::NotGpt`], [`OpRefusal::NoTable`]
//!
//! Notably **not** refused: deleting a partition with data on it, formatting
//! Windows, or shrinking something to a tenth of its size. The user is looking
//! at the disk and said to. The confirmation page names every device that will
//! be written and the install will not start until it is read; that is where
//! "are you sure" belongs, not here.

use std::fmt;

use super::exec::Cmd;
use super::plan::{
    ALIGNMENT_BYTES, TYPE_LINUX_ROOT_X86_64, TYPE_LINUX_SWAP, align_down, align_up, bytes,
};
use super::probe::{Disk, Partition, SystemSnapshot};

// --- MARK: Constants ---

/// GPT type GUID for an EFI System Partition.
pub const TYPE_EFI_SYSTEM: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";
/// GPT type GUID for "Linux filesystem" — a data partition with no special
/// meaning to anything that auto-discovers partitions.
pub const TYPE_LINUX_DATA: &str = "0fc63daf-8483-4772-8e79-3d69d8477de4";

/// The smallest partition this editor will create.
///
/// One alignment unit. Not a judgement about what is useful — a 1 MiB partition
/// is useless for almost everything — but the point below which the arithmetic
/// stops meaning anything, since a partition shorter than the alignment it is
/// snapped to can round to zero sectors.
pub const MIN_PARTITION_BYTES: u64 = ALIGNMENT_BYTES;

/// The smallest ESP that will hold a kernel and an initramfs.
///
/// A warning rather than a refusal — a machine that boots something else from
/// its own ESP may well want a small second one.
pub const MIN_USEFUL_ESP_BYTES: u64 = 100 * 1024 * 1024;

// --- MARK: Filesystems ---

/// A filesystem this editor can create.
///
/// Wider than [`RootFilesystem`], which is only the two a CrownOS root may be:
/// an editor also has to make an ESP (`fat32`) and a swap area, and people
/// legitimately want an `xfs` home.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filesystem {
    Ext4,
    Btrfs,
    Fat32,
    Xfs,
    Swap,
}

impl Filesystem {
    /// What `mkfs` calls it, and what appears in the confirmation page.
    pub fn name(self) -> &'static str {
        match self {
            Self::Ext4 => "ext4",
            Self::Btrfs => "btrfs",
            Self::Fat32 => "fat32",
            Self::Xfs => "xfs",
            Self::Swap => "swap",
        }
    }

    /// The filesystem hint for `parted mkpart`.
    ///
    /// Only ever a hint: it decides which of `parted`'s handful of built-in
    /// type GUIDs the entry gets, and [`create_commands`] overwrites that
    /// immediately afterwards. Passed anyway because `mkpart` requires the
    /// argument on a GPT disk.
    fn parted_hint(self) -> &'static str {
        match self {
            Self::Ext4 | Self::Xfs => "ext4",
            Self::Btrfs => "btrfs",
            Self::Fat32 => "fat32",
            Self::Swap => "linux-swap",
        }
    }

    /// The `mkfs` invocation, as (program, flags before the device).
    ///
    /// Every one of these is forced (`-F`, `-f`): the executor only reaches
    /// them after the confirmation page named the device, so a prompt from
    /// `mkfs` would be a prompt nobody can answer under `cage`.
    fn mkfs(self, label: Option<&str>) -> (&'static str, Vec<String>) {
        let mut args: Vec<String> = Vec::new();
        let program = match self {
            Self::Ext4 => {
                args.push("-F".to_owned());
                "mkfs.ext4"
            }
            Self::Btrfs => {
                args.push("-f".to_owned());
                "mkfs.btrfs"
            }
            Self::Fat32 => {
                // `-F 32` rather than letting `mkfs.fat` choose: on a small
                // partition it picks FAT16, which no firmware is required to
                // read as an ESP.
                args.push("-F".to_owned());
                args.push("32".to_owned());
                "mkfs.fat"
            }
            Self::Xfs => {
                args.push("-f".to_owned());
                "mkfs.xfs"
            }
            Self::Swap => "mkswap",
        };
        if let Some(label) = label.filter(|l| !l.is_empty()) {
            // `mkfs.fat` spells it `-n` and takes 11 characters; everything
            // else spells it `-L`.
            match self {
                Self::Fat32 => {
                    args.push("-n".to_owned());
                    args.push(label.chars().take(11).collect::<String>().to_uppercase());
                }
                _ => {
                    args.push("-L".to_owned());
                    args.push(label.to_owned());
                }
            }
        }
        (program, args)
    }
}

/// Whether a filesystem can be shrunk while it is not mounted, and what to say
/// if not.
///
/// Answered by name from `lsblk`'s `FSTYPE`, so an unrecognised or absent
/// filesystem reads as "nothing in there to shrink" and the partition boundary
/// moves alone. That is the right answer for an empty partition and the wrong
/// one for a filesystem this list has not heard of, which is why the list errs
/// towards naming everything a live Arch environment can actually resize.
fn offline_shrinkable(fstype: Option<&str>) -> Result<(), &'static str> {
    match fstype.map(str::to_ascii_lowercase).as_deref() {
        // Nothing in there, or nothing we recognise as a filesystem.
        None | Some("") | Some("swap") | Some("linux-swap") => Ok(()),
        Some("ext2" | "ext3" | "ext4") => Ok(()),
        Some("ntfs") => Ok(()),
        Some("btrfs") => Err(
            "a btrfs filesystem can only be resized while it is mounted, and this \
             installer does not mount a filesystem it is about to repartition",
        ),
        Some("xfs") => Err("an xfs filesystem cannot be shrunk at all, only grown"),
        Some("f2fs") => Err("an f2fs filesystem cannot be shrunk"),
        Some("vfat" | "fat32" | "fat16" | "exfat") => Err(
            "a FAT filesystem cannot be resized in place — make a smaller one instead, \
             which means formatting it",
        ),
        Some(other) => Err(match other {
            "zfs_member" => "a ZFS pool member is not resized by moving its partition",
            _ => "this installer does not know how to shrink that filesystem offline",
        }),
    }
}

// --- MARK: Partition kinds ---

/// What a partition being created is for.
///
/// Decides the GPT type GUID, and for an ESP the `esp` flag. Kept separate from
/// [`Filesystem`] because the two are genuinely independent: an ESP is FAT32
/// *and* carries a particular GUID *and* a flag, and a data partition holding
/// FAT32 is none of those things.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartitionKind {
    EfiSystem,
    /// A CrownOS root. Carries the x86-64 root GUID so that
    /// `systemd-gpt-auto-generator` and friends find it without an `fstab`
    /// entry.
    LinuxRoot,
    LinuxSwap,
    /// Anything else the user wants the space for.
    LinuxData,
}

impl PartitionKind {
    pub fn type_guid(self) -> &'static str {
        match self {
            Self::EfiSystem => TYPE_EFI_SYSTEM,
            Self::LinuxRoot => TYPE_LINUX_ROOT_X86_64,
            Self::LinuxSwap => TYPE_LINUX_SWAP,
            Self::LinuxData => TYPE_LINUX_DATA,
        }
    }

    /// Whether `parted set <n> esp on` should follow the `mkpart`.
    fn needs_esp_flag(self) -> bool {
        matches!(self, Self::EfiSystem)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::EfiSystem => "EFI system partition",
            Self::LinuxRoot => "Linux root",
            Self::LinuxSwap => "swap",
            Self::LinuxData => "data",
        }
    }
}

// --- MARK: Operations ---

/// One change to a partition table, as the user asked for it.
///
/// Every field is resolved: a `Create` names its own sector range and partition
/// number rather than "the free space" and "the next number", because a queue of
/// operations has to be describable before any of it runs. Resolving them is
/// [`plan_ops`]' job; see [`Projection`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiskOp {
    /// Write an empty GPT onto a blank disk. Only ever queued on a disk with no
    /// table at all, so there is nothing for it to destroy.
    CreateTable { disk: String },
    Create {
        disk: String,
        /// The number `parted` will give it, predicted from the projection so
        /// that a later operation in the same queue can name it.
        number: u32,
        path: String,
        start_sector: u64,
        end_sector: u64,
        sector_size: u32,
        kind: PartitionKind,
        /// The GPT partition name. Not the filesystem label.
        name: String,
        /// `mkfs` it as part of the same operation. Almost always wanted — a
        /// partition with no filesystem is not usable — but separable, because
        /// "create the entry now, format it later" is what a plan that formats
        /// its own root does.
        format: Option<Filesystem>,
        /// Filesystem label for the `mkfs`, if there is one.
        fs_label: Option<String>,
    },
    Delete {
        disk: String,
        number: u32,
        path: String,
        /// Recorded for the log and the confirmation page. A deleted partition's
        /// PARTUUID is the only durable way to say afterwards *which* partition
        /// went.
        partuuid: Option<String>,
        size_bytes: u64,
    },
    Resize {
        disk: String,
        number: u32,
        path: String,
        sector_size: u32,
        start_sector: u64,
        old_end_sector: u64,
        new_end_sector: u64,
        /// From `lsblk`. Decides which tool resizes the contents, and whether
        /// shrinking is possible at all.
        fstype: Option<String>,
    },
    Format {
        path: String,
        filesystem: Filesystem,
        fs_label: Option<String>,
    },
}

impl DiskOp {
    /// A line for the confirmation page and the progress log.
    ///
    /// Every one of these names the device and the size, because a list of
    /// operations is the last thing the user reads before the disk is written
    /// and "Resizing a partition" is not enough to check.
    pub fn label(&self) -> String {
        match self {
            Self::CreateTable { disk } => format!("Writing a new GPT partition table to {disk}"),
            Self::Create {
                path,
                start_sector,
                end_sector,
                sector_size,
                kind,
                format,
                ..
            } => {
                let size =
                    bytes((end_sector.saturating_sub(*start_sector) + 1) * u64::from(*sector_size));
                match format {
                    Some(fs) => {
                        format!("Creating {path} — {size} {} as {}", kind.label(), fs.name())
                    }
                    None => format!("Creating {path} — {size} {}", kind.label()),
                }
            }
            Self::Delete {
                path, size_bytes, ..
            } => format!("Deleting {path} — {} will be freed", bytes(*size_bytes)),
            Self::Resize {
                path,
                sector_size,
                start_sector,
                old_end_sector,
                new_end_sector,
                ..
            } => {
                let old = bytes((old_end_sector - start_sector + 1) * u64::from(*sector_size));
                let new = bytes((new_end_sector - start_sector + 1) * u64::from(*sector_size));
                let verb = if new_end_sector < old_end_sector {
                    "Shrinking"
                } else {
                    "Growing"
                };
                format!("{verb} {path} from {old} to {new}")
            }
            Self::Format {
                path, filesystem, ..
            } => format!("Formatting {path} as {}", filesystem.name()),
        }
    }

    /// The block device this operation changes. Every operation in this module
    /// is destructive by construction — that is what an editor is — so there is
    /// no `Impact::None` case to distinguish.
    pub fn device(&self) -> &str {
        match self {
            Self::CreateTable { disk } => disk,
            Self::Create { path, .. }
            | Self::Delete { path, .. }
            | Self::Resize { path, .. }
            | Self::Format { path, .. } => path,
        }
    }

    /// Whether this operation destroys a partition that existed before the
    /// install started.
    ///
    /// The basis of the authorization list a [`Plan`](super::plan::Plan) carries:
    /// `plan`'s own guard refuses any step that destroys a pre-existing
    /// partition, and a user-authored delete has to be able to say "this one,
    /// deliberately". A `Create` never qualifies — it destroys nothing that was
    /// there.
    pub fn destroys_existing(&self) -> bool {
        !matches!(self, Self::Create { .. } | Self::CreateTable { .. })
    }
}

// --- MARK: Refusals ---

/// Why an operation will not be carried out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpRefusal {
    NotGpt {
        disk: String,
        table: String,
    },
    NoTable {
        disk: String,
    },
    TableExists {
        disk: String,
        table: String,
    },
    UnknownPartition {
        path: String,
    },
    PartitionInUse {
        path: String,
        mounted_at: Vec<String>,
    },
    IsLiveMedium {
        path: String,
    },
    Overlaps {
        path: String,
        with: String,
    },
    OutsideDisk {
        path: String,
        last_usable_sector: u64,
    },
    TooSmall {
        path: String,
        size_bytes: u64,
    },
    UnsupportedShrink {
        path: String,
        fstype: String,
        reason: &'static str,
    },
    /// A `Resize` that would leave the partition ending before it starts.
    Inverted {
        path: String,
    },
}

impl fmt::Display for OpRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotGpt { disk, table } => write!(
                f,
                "{disk} has a {table} partition table. CrownOS can only edit GPT disks, \
                 because an MBR disk's four-entry limit and 2 TiB ceiling make every \
                 offset here mean something different."
            ),
            Self::NoTable { disk } => write!(
                f,
                "{disk} has no partition table yet. A GPT has to be written to it first."
            ),
            Self::TableExists { disk, table } => write!(
                f,
                "{disk} already has a {table} partition table. Writing a new one would \
                 lose every partition on it, so CrownOS only creates a table on a blank disk."
            ),
            Self::UnknownPartition { path } => write!(
                f,
                "{path} is not a partition on the disk it was asked for. The disk has \
                 changed since it was last read."
            ),
            Self::PartitionInUse { path, mounted_at } => write!(
                f,
                "{path} is mounted at {}. A partition table rewritten under a mounted \
                 filesystem is a filesystem whose next write goes somewhere else \
                 entirely, so nothing mounted is edited. Unmount it and try again.",
                mounted_at.join(", ")
            ),
            Self::IsLiveMedium { path } => write!(
                f,
                "{path} is the medium this installer is running from. Editing it would \
                 end the install part way through, so it is left alone."
            ),
            Self::Overlaps { path, with } => {
                write!(f, "{path} would overlap {with}. Nothing has been changed.")
            }
            Self::OutsideDisk {
                path,
                last_usable_sector,
            } => write!(
                f,
                "{path} would extend past the end of the disk (the last usable sector \
                 is {last_usable_sector})."
            ),
            Self::TooSmall { path, size_bytes } => write!(
                f,
                "{path} would be {}, which is below the {} this installer will create. \
                 A partition shorter than the alignment it is snapped to can round to \
                 nothing at all.",
                bytes(*size_bytes),
                bytes(MIN_PARTITION_BYTES)
            ),
            Self::UnsupportedShrink {
                path,
                fstype,
                reason,
            } => write!(f, "{path} holds {fstype}, and {reason}."),
            Self::Inverted { path } => write!(
                f,
                "{path} would end before it starts. Nothing has been changed."
            ),
        }
    }
}

impl std::error::Error for OpRefusal {}

// --- MARK: Projection ---

/// A partition table as it will be once a queue of operations has run, plus what
/// running them costs.
///
/// The reason the queue is checked against a projection rather than against the
/// disk: a queue is ordered, and each operation sees the table the ones before
/// it left. `delete 3` then `create in the space 3 was` is a perfectly ordinary
/// pair that neither operation could be validated without the other.
///
/// It is also what the UI draws. A user shrinking a partition wants to see the
/// bar move before committing, and the projection *is* that bar's data — the
/// same value the checks ran against, so a projection that renders is a
/// projection that passed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Projection {
    /// The table as it will be. Partitions sorted by start sector, as
    /// [`Disk::free_gaps`] requires.
    pub disk: Disk,
    /// Devices that existed before and will not afterwards, or whose contents
    /// will be replaced. Feeds a [`Plan`](super::plan::Plan)'s authorization
    /// list — see [`DiskOp::destroys_existing`].
    pub destroyed: Vec<String>,
    /// Things worth saying that are not refusals.
    pub warnings: Vec<String>,
}

impl Projection {
    /// The free space the projected table leaves, largest gap first.
    pub fn free_gaps(&self) -> Vec<super::probe::Gap> {
        let mut gaps = self.disk.free_gaps();
        gaps.sort_by_key(|gap| std::cmp::Reverse(gap.sectors));
        gaps
    }
}

/// Check a queue of operations against the disk and work out what it leaves.
///
/// Pure: no I/O, no `parted`, nothing that can change the machine. Every rule in
/// the module docs is enforced here and nowhere else, which is what makes each
/// of them a unit test.
///
/// `snapshot` is consulted only for the mount table — which partitions are in
/// use, and which one the live system is reading itself out of.
pub fn plan_ops(
    disk: &Disk,
    snapshot: &SystemSnapshot,
    ops: &[DiskOp],
) -> Result<Projection, OpRefusal> {
    if disk.table != "gpt" && !disk.is_blank() {
        return Err(OpRefusal::NotGpt {
            disk: disk.path.clone(),
            table: disk.table.clone(),
        });
    }

    let mut projected = disk.clone();
    let mut destroyed = Vec::new();
    let mut warnings = Vec::new();

    for op in ops {
        match op {
            DiskOp::CreateTable { .. } if !projected.is_blank() => {
                return Err(OpRefusal::TableExists {
                    disk: projected.path.clone(),
                    table: projected.table.clone(),
                });
            }
            DiskOp::CreateTable { .. } => {
                projected.table = "gpt".to_owned();
                continue;
            }
            _ if projected.is_blank() => {
                return Err(OpRefusal::NoTable {
                    disk: projected.path.clone(),
                });
            }
            _ => {}
        }

        // Anything touching an existing partition is checked against the
        // *machine* — mounts are a fact about now, not about the projection —
        // before it is applied to the projected table.
        if op.destroys_existing() {
            guard_existing(disk, snapshot, op.device())?;
        }

        match op {
            DiskOp::Create {
                path,
                number,
                start_sector,
                end_sector,
                kind,
                name,
                sector_size,
                ..
            } => {
                if end_sector < start_sector {
                    return Err(OpRefusal::Inverted { path: path.clone() });
                }
                let size = (end_sector - start_sector + 1) * u64::from(*sector_size);
                if size < MIN_PARTITION_BYTES {
                    return Err(OpRefusal::TooSmall {
                        path: path.clone(),
                        size_bytes: size,
                    });
                }
                if *end_sector > projected.last_usable_sector {
                    return Err(OpRefusal::OutsideDisk {
                        path: path.clone(),
                        last_usable_sector: projected.last_usable_sector,
                    });
                }
                if let Some(other) = overlapping(&projected, *start_sector, *end_sector, None) {
                    return Err(OpRefusal::Overlaps {
                        path: path.clone(),
                        with: other,
                    });
                }
                if *kind == PartitionKind::EfiSystem && size < MIN_USEFUL_ESP_BYTES {
                    warnings.push(format!(
                        "{path} is a {} EFI partition. That is below the {} a kernel and \
                         an initramfs need, so it will not be able to boot CrownOS on \
                         its own.",
                        bytes(size),
                        bytes(MIN_USEFUL_ESP_BYTES)
                    ));
                }
                projected.partitions.push(Partition {
                    path: path.clone(),
                    number: *number,
                    // Unknown until the table is written — `parted` generates
                    // it. Left `None` rather than invented, so nothing
                    // downstream can match on a GUID that will not exist.
                    partuuid: None,
                    type_guid: Some(kind.type_guid().to_owned()),
                    partlabel: Some(name.clone()),
                    start_sector: *start_sector,
                    sectors: end_sector - start_sector + 1,
                    fstype: None,
                    label: None,
                });
            }

            DiskOp::Delete { path, number, .. } => {
                let before = projected.partitions.len();
                projected.partitions.retain(|p| p.number != *number);
                if projected.partitions.len() == before {
                    return Err(OpRefusal::UnknownPartition { path: path.clone() });
                }
                destroyed.push(path.clone());
            }

            DiskOp::Resize {
                path,
                number,
                start_sector,
                new_end_sector,
                sector_size,
                fstype,
                old_end_sector,
                ..
            } => {
                if *new_end_sector < *start_sector {
                    return Err(OpRefusal::Inverted { path: path.clone() });
                }
                let size = (new_end_sector - start_sector + 1) * u64::from(*sector_size);
                if size < MIN_PARTITION_BYTES {
                    return Err(OpRefusal::TooSmall {
                        path: path.clone(),
                        size_bytes: size,
                    });
                }
                if *new_end_sector > projected.last_usable_sector {
                    return Err(OpRefusal::OutsideDisk {
                        path: path.clone(),
                        last_usable_sector: projected.last_usable_sector,
                    });
                }
                // Shrinking moves data. Growing does not, so a filesystem that
                // cannot be shrunk offline can still be grown.
                if new_end_sector < old_end_sector
                    && let Err(reason) = offline_shrinkable(fstype.as_deref())
                {
                    return Err(OpRefusal::UnsupportedShrink {
                        path: path.clone(),
                        fstype: fstype
                            .clone()
                            .unwrap_or_else(|| "an unknown filesystem".to_owned()),
                        reason,
                    });
                }
                if let Some(other) =
                    overlapping(&projected, *start_sector, *new_end_sector, Some(*number))
                {
                    return Err(OpRefusal::Overlaps {
                        path: path.clone(),
                        with: other,
                    });
                }
                let Some(entry) = projected
                    .partitions
                    .iter_mut()
                    .find(|p| p.number == *number)
                else {
                    return Err(OpRefusal::UnknownPartition { path: path.clone() });
                };
                entry.sectors = new_end_sector - start_sector + 1;
                // A resize rewrites the entry and, when shrinking, the
                // filesystem's own metadata. Either way what was there is not
                // what will be there.
                destroyed.push(path.clone());
            }

            DiskOp::Format { path, .. } => {
                destroyed.push(path.clone());
            }

            DiskOp::CreateTable { .. } => {}
        }
    }

    // `free_gaps` walks the partition list in order, so a table that has had
    // entries appended out of order has to be re-sorted before anything reads
    // gaps out of it.
    projected
        .partitions
        .sort_by_key(|partition| partition.start_sector);

    Ok(Projection {
        disk: projected,
        destroyed,
        warnings,
    })
}

/// The two rules about touching something that is already there: it must exist,
/// it must not be mounted, and it must not be what we are running from.
fn guard_existing(disk: &Disk, snapshot: &SystemSnapshot, device: &str) -> Result<(), OpRefusal> {
    let Some(partition) = disk.partitions.iter().find(|p| p.path == device) else {
        return Err(OpRefusal::UnknownPartition {
            path: device.to_owned(),
        });
    };

    let mounts = snapshot.mounts_backed_by(partition);
    if mounts.is_empty() {
        return Ok(());
    }

    // The live medium is a mounted partition too, and the distinction is worth
    // making because the two have different answers: an ordinary mount can be
    // unmounted and the operation retried, and this one cannot.
    if mounts
        .iter()
        .any(|mount| mount.target.starts_with(super::probe::BOOTMNT))
    {
        return Err(OpRefusal::IsLiveMedium {
            path: device.to_owned(),
        });
    }

    Err(OpRefusal::PartitionInUse {
        path: device.to_owned(),
        mounted_at: mounts.iter().map(|m| m.target.clone()).collect(),
    })
}

/// The first partition in `disk` whose sectors intersect `start..=end`, ignoring
/// `exclude` (the partition being resized, which necessarily overlaps itself).
fn overlapping(disk: &Disk, start: u64, end: u64, exclude: Option<u32>) -> Option<String> {
    disk.partitions
        .iter()
        .filter(|p| Some(p.number) != exclude)
        .find(|p| start <= p.end_sector() && p.start_sector <= end)
        .map(|p| p.path.clone())
}

// --- MARK: Free space helpers ---

/// Carve a partition of `size_bytes` out of the front of a gap, aligned.
///
/// Returns the sector range, or `None` if the gap cannot hold it once alignment
/// has been taken off both ends. Shared by the UI (which offers to fill a gap)
/// and the ISO planner (which lays a root and a swap out inside one), so the two
/// cannot disagree about where a partition lands.
pub fn carve(disk: &Disk, gap: super::probe::Gap, size_bytes: u64) -> Option<(u64, u64)> {
    let sector = u64::from(disk.sector_size);
    let alignment = ALIGNMENT_BYTES / sector;
    let start = align_up(gap.start_sector, alignment);
    let limit = align_down(gap.end_sector() + 1, alignment);
    if limit <= start {
        return None;
    }
    let wanted = size_bytes.div_ceil(sector);
    let end = align_down(start + wanted, alignment).min(limit);
    if end <= start {
        return None;
    }
    Some((start, end - 1))
}

/// The whole of a gap, aligned at both ends.
pub fn fill(disk: &Disk, gap: super::probe::Gap) -> Option<(u64, u64)> {
    let sector = u64::from(disk.sector_size);
    let alignment = ALIGNMENT_BYTES / sector;
    let start = align_up(gap.start_sector, alignment);
    let end = align_down(gap.end_sector() + 1, alignment);
    if end <= start {
        return None;
    }
    Some((start, end - 1))
}

// --- MARK: Commands ---

/// The commands one operation becomes.
///
/// A free function of the operation alone, with no state, so a test can ask
/// "what would this do?" without an executor and without a disk — the same
/// shape [`commands_for`](super::exec::commands_for) has, and for the same
/// reason.
pub fn commands_for_op(op: &DiskOp) -> Vec<Cmd> {
    match op {
        DiskOp::CreateTable { disk } => vec![Cmd::Run {
            program: "parted".to_owned(),
            args: vec![
                "--script".to_owned(),
                disk.clone(),
                "mklabel".to_owned(),
                "gpt".to_owned(),
            ],
        }],
        DiskOp::Create { .. } => create_commands(op),
        DiskOp::Delete { disk, number, .. } => vec![Cmd::Run {
            program: "parted".to_owned(),
            args: vec![
                "--script".to_owned(),
                disk.clone(),
                "rm".to_owned(),
                number.to_string(),
            ],
        }],
        DiskOp::Resize { .. } => resize_commands(op),
        DiskOp::Format {
            path,
            filesystem,
            fs_label,
        } => vec![mkfs(path, *filesystem, fs_label.as_deref())],
    }
}

/// `parted mkpart` for the geometry, `sfdisk --part-type` for the type GUID,
/// `parted set esp on` for an ESP, and a `mkfs` if one was asked for.
///
/// # Why two tools for one partition
///
/// `parted`'s `mkpart` takes a *filesystem hint*, not a type GUID: it maps a
/// short list of names onto a short list of GUIDs, and the x86-64 Linux root
/// GUID this installer needs — see [`TYPE_LINUX_ROOT_X86_64`] and the
/// Discoverable Partitions Spec — is not on that list. A root partition typed
/// "Linux filesystem" boots perfectly well and is invisible to everything that
/// discovers partitions by GUID, which is a failure that shows up much later
/// than here.
///
/// `sfdisk --part-type` sets exactly the GUID it is given and rewrites nothing
/// else about the entry, so it is the narrowest possible correction to make
/// immediately afterwards. It is also the tool [`plan`](super::plan) already
/// uses, so this introduces no new dependency on the live medium.
fn create_commands(op: &DiskOp) -> Vec<Cmd> {
    let DiskOp::Create {
        disk,
        number,
        path,
        start_sector,
        end_sector,
        kind,
        name,
        format,
        fs_label,
        ..
    } = op
    else {
        return Vec::new();
    };

    let mut commands = vec![Cmd::Run {
        program: "parted".to_owned(),
        args: vec![
            "--script".to_owned(),
            // The device reports the boundary it wants; asking for it beats
            // recomputing it out of sysfs. See the module docs.
            "--align".to_owned(),
            "optimal".to_owned(),
            disk.clone(),
            "unit".to_owned(),
            "s".to_owned(),
            "mkpart".to_owned(),
            name.clone(),
            format.unwrap_or(Filesystem::Ext4).parted_hint().to_owned(),
            format_sector(*start_sector),
            format_sector(*end_sector),
        ],
    }];

    commands.push(Cmd::Run {
        program: "sfdisk".to_owned(),
        args: vec![
            "--part-type".to_owned(),
            disk.clone(),
            number.to_string(),
            kind.type_guid().to_owned(),
        ],
    });

    if kind.needs_esp_flag() {
        // The `esp` flag is what firmware and `bootctl` look for. `parted` sets
        // the boot flag alongside it on a GPT disk, which is the same bit.
        commands.push(Cmd::Run {
            program: "parted".to_owned(),
            args: vec![
                "--script".to_owned(),
                disk.clone(),
                "set".to_owned(),
                number.to_string(),
                "esp".to_owned(),
                "on".to_owned(),
            ],
        });
    }

    if let Some(filesystem) = format {
        commands.push(settle(disk));
        commands.push(mkfs(path, *filesystem, fs_label.as_deref()));
    }

    commands
}

/// A resize, in the order that does not corrupt the filesystem.
///
/// **Shrinking: filesystem first, then the table entry.** Moving the entry in
/// first leaves a filesystem whose superblock says it is larger than the
/// partition containing it — which is a corrupt filesystem, and one that
/// `fsck` will happily "repair" by writing over whatever now follows.
///
/// **Growing: the table entry first, then the filesystem.** Growing the
/// filesystem first makes it write past the entry's end, over the next
/// partition.
///
/// The two orders are asserted by
/// `the_order_of_a_shrink_is_filesystem_then_partition` and
/// `the_order_of_a_grow_is_partition_then_filesystem`. If you change anything in
/// this function, read them first.
fn resize_commands(op: &DiskOp) -> Vec<Cmd> {
    let DiskOp::Resize {
        disk,
        number,
        path,
        start_sector,
        old_end_sector,
        new_end_sector,
        sector_size,
        fstype,
    } = op
    else {
        return Vec::new();
    };

    let shrinking = new_end_sector < old_end_sector;
    let new_sectors = new_end_sector - start_sector + 1;
    let new_bytes = new_sectors * u64::from(*sector_size);

    let entry = Cmd::Run {
        program: "parted".to_owned(),
        args: vec![
            "--script".to_owned(),
            disk.clone(),
            "unit".to_owned(),
            "s".to_owned(),
            "resizepart".to_owned(),
            number.to_string(),
            format_sector(*new_end_sector),
        ],
    };

    let mut contents = Vec::new();
    match fstype.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("ext2" | "ext3" | "ext4") => {
            // `resize2fs` refuses to shrink a filesystem it has not checked,
            // and refuses to check one with a dirty journal. `-f -y` is the
            // pair that gets through both without a prompt nobody can answer
            // under `cage`.
            if shrinking {
                contents.push(Cmd::Run {
                    program: "e2fsck".to_owned(),
                    args: vec!["-f".to_owned(), "-y".to_owned(), path.clone()],
                });
            }
            contents.push(Cmd::Run {
                program: "resize2fs".to_owned(),
                args: if shrinking {
                    // `s` is `resize2fs`' suffix for 512-byte sectors, which is
                    // not the disk's sector size. Converted rather than passed
                    // through, because a 4Kn disk would otherwise be resized to
                    // an eighth of the intended size.
                    vec![path.clone(), format!("{}s", new_bytes / 512)]
                } else {
                    // No size: grow to fill the partition, whatever it now is.
                    vec![path.clone()]
                },
            });
        }
        Some("ntfs") => {
            contents.push(Cmd::Run {
                program: "ntfsresize".to_owned(),
                args: if shrinking {
                    vec![
                        "--force".to_owned(),
                        "--size".to_owned(),
                        new_bytes.to_string(),
                        path.clone(),
                    ]
                } else {
                    vec!["--force".to_owned(), path.clone()]
                },
            });
        }
        // Nothing recognised in there. `plan_ops` already refused a *shrink* of
        // anything it cannot shrink, so reaching here with an unknown
        // filesystem means either an empty partition or a grow, and in both
        // cases moving the entry alone is the whole job.
        _ => {}
    }

    let mut commands = Vec::new();
    if shrinking {
        commands.extend(contents);
        commands.push(entry);
        commands.push(settle(disk));
    } else {
        commands.push(entry);
        commands.push(settle(disk));
        commands.extend(contents);
    }
    commands
}

fn mkfs(path: &str, filesystem: Filesystem, label: Option<&str>) -> Cmd {
    let (program, mut args) = filesystem.mkfs(label);
    args.push(path.to_owned());
    Cmd::Run {
        program: program.to_owned(),
        args,
    }
}

/// Re-read the table and wait for udev, so the next command can name a node
/// that now exists.
///
/// `udevadm settle` and not a sleep: the node appears when udev has processed
/// the kernel's change event, and how long that takes is a property of the
/// machine.
fn settle(disk: &str) -> Cmd {
    Cmd::Run {
        program: "partprobe".to_owned(),
        args: vec![disk.to_owned()],
    }
}

/// A sector, as `parted` wants it written.
fn format_sector(sector: u64) -> String {
    format!("{sector}s")
}

// --- MARK: Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::probe::{self, Gap, fixture};

    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * MIB;

    fn machine(findmnt: &str) -> SystemSnapshot {
        probe::probe(&fixture::machine(findmnt)).expect("probe")
    }

    fn standard() -> (Disk, SystemSnapshot) {
        let snapshot = machine(fixture::FINDMNT_JSON_IN_RAM);
        (snapshot.disks[0].clone(), snapshot)
    }

    fn create(number: u32, start: u64, end: u64) -> DiskOp {
        create_as(
            number,
            start,
            end,
            PartitionKind::LinuxData,
            Filesystem::Ext4,
            "data",
        )
    }

    /// `..spread` does not work on an enum variant, so the variants a test wants
    /// to vary are parameters rather than an override of a base value.
    fn create_as(
        number: u32,
        start: u64,
        end: u64,
        kind: PartitionKind,
        format: Filesystem,
        fs_label: &str,
    ) -> DiskOp {
        DiskOp::Create {
            disk: "/dev/sda".to_owned(),
            number,
            path: format!("/dev/sda{number}"),
            start_sector: start,
            end_sector: end,
            sector_size: 512,
            kind,
            name: kind.label().to_owned(),
            format: Some(format),
            fs_label: Some(fs_label.to_owned()),
        }
    }

    fn delete(number: u32) -> DiskOp {
        let (disk, _) = standard();
        let partition = disk
            .partitions
            .iter()
            .find(|p| p.number == number)
            .expect("a partition");
        DiskOp::Delete {
            disk: "/dev/sda".to_owned(),
            number,
            path: partition.path.clone(),
            partuuid: partition.partuuid.clone(),
            size_bytes: disk.bytes(partition.sectors),
        }
    }

    fn resize(number: u32, new_end: u64, fstype: Option<&str>) -> DiskOp {
        let (disk, _) = standard();
        let partition = disk
            .partitions
            .iter()
            .find(|p| p.number == number)
            .expect("a partition");
        DiskOp::Resize {
            disk: "/dev/sda".to_owned(),
            number,
            path: partition.path.clone(),
            sector_size: 512,
            start_sector: partition.start_sector,
            old_end_sector: partition.end_sector(),
            new_end_sector: new_end,
            fstype: fstype.map(str::to_owned),
        }
    }

    // ---- Projection ----

    #[test]
    fn an_empty_queue_projects_the_disk_unchanged() {
        let (disk, snapshot) = standard();
        let projection = plan_ops(&disk, &snapshot, &[]).expect("no ops");
        assert_eq!(projection.disk, disk);
        assert!(projection.destroyed.is_empty());
    }

    /// The reason a projection exists at all: an operation is checked against
    /// what the ones before it left, not against the disk. Creating in the
    /// space `sda4` occupies is an overlap; deleting `sda4` first makes it fine.
    #[test]
    fn each_operation_sees_the_table_the_previous_ones_left() {
        let (disk, snapshot) = standard();
        let over_sda4 = create(5, 800_032_768, 802_129_919);

        match plan_ops(&disk, &snapshot, std::slice::from_ref(&over_sda4)) {
            Err(OpRefusal::Overlaps { with, .. }) => assert_eq!(with, "/dev/sda4"),
            other => panic!("expected an overlap, got {other:?}"),
        }

        let projection =
            plan_ops(&disk, &snapshot, &[delete(4), over_sda4]).expect("delete then create");
        assert_eq!(projection.destroyed, vec!["/dev/sda4".to_owned()]);
        assert!(
            projection.disk.partitions.iter().any(|p| p.number == 5),
            "the new partition should be in the projection"
        );
        assert!(
            !projection.disk.partitions.iter().any(|p| p.number == 4),
            "the deleted one should not"
        );
    }

    /// `free_gaps` walks the partition list in order, so an appended entry has
    /// to be sorted back into place or every gap after it is wrong.
    #[test]
    fn the_projected_table_stays_sorted_by_start_sector() {
        let (disk, snapshot) = standard();
        // In the middle of the trailing free region, i.e. after sda4.
        let projection =
            plan_ops(&disk, &snapshot, &[create(5, 900_000_000, 950_000_000)]).expect("create");
        let starts: Vec<u64> = projection
            .disk
            .partitions
            .iter()
            .map(|p| p.start_sector)
            .collect();
        let mut sorted = starts.clone();
        sorted.sort_unstable();
        assert_eq!(starts, sorted);

        // And the gaps either side of it are both found.
        let gaps = projection.disk.free_gaps();
        assert_eq!(gaps.len(), 2, "{gaps:?}");
    }

    // ---- Refusals ----

    #[test]
    fn a_mounted_partition_is_not_edited() {
        let snapshot = machine(fixture::FINDMNT_JSON_ON_MEDIUM);
        let disk = snapshot.disks[0].clone();
        // sda4 is the staging partition, mounted at /run/archiso/bootmnt in
        // this fixture — which is the live medium, the more specific refusal.
        match plan_ops(&disk, &snapshot, &[delete(4)]) {
            Err(OpRefusal::IsLiveMedium { path }) => assert_eq!(path, "/dev/sda4"),
            other => panic!("expected IsLiveMedium, got {other:?}"),
        }
    }

    /// An ordinary mount and the live medium are different refusals, because
    /// they have different answers: one can be unmounted and retried.
    #[test]
    fn an_ordinary_mount_is_a_different_refusal_from_the_live_medium() {
        let system = fixture::machine_with(
            r#"{"filesystems":[
                 {"source":"airootfs","target":"/","fstype":"overlay"},
                 {"source":"/dev/sda3","target":"/mnt/windows","fstype":"ntfs"}
               ]}"#,
            &fixture::sfdisk_json("gpt", fixture::STANDARD_TABLE),
        );
        let snapshot = probe::probe(&system).expect("probe");
        let disk = snapshot.disks[0].clone();
        match plan_ops(&disk, &snapshot, &[delete(3)]) {
            Err(OpRefusal::PartitionInUse { path, mounted_at }) => {
                assert_eq!(path, "/dev/sda3");
                assert_eq!(mounted_at, vec!["/mnt/windows".to_owned()]);
            }
            other => panic!("expected PartitionInUse, got {other:?}"),
        }
    }

    #[test]
    fn a_partition_past_the_end_of_the_disk_is_refused() {
        let (disk, snapshot) = standard();
        match plan_ops(
            &disk,
            &snapshot,
            &[create(5, 999_000_000, fixture::LAST_LBA + 100)],
        ) {
            Err(OpRefusal::OutsideDisk {
                last_usable_sector, ..
            }) => assert_eq!(last_usable_sector, fixture::LAST_LBA),
            other => panic!("expected OutsideDisk, got {other:?}"),
        }
    }

    #[test]
    fn a_partition_below_the_alignment_unit_is_refused() {
        let (disk, snapshot) = standard();
        // 512 sectors of 512 bytes = 256 KiB, under the 1 MiB minimum.
        match plan_ops(&disk, &snapshot, &[create(5, 900_000_000, 900_000_511)]) {
            Err(OpRefusal::TooSmall { size_bytes, .. }) => assert_eq!(size_bytes, 256 * 1024),
            other => panic!("expected TooSmall, got {other:?}"),
        }
    }

    #[test]
    fn an_inverted_range_is_refused_rather_than_wrapping() {
        let (disk, snapshot) = standard();
        match plan_ops(&disk, &snapshot, &[create(5, 900_000_000, 899_000_000)]) {
            Err(OpRefusal::Inverted { .. }) => {}
            other => panic!("expected Inverted, got {other:?}"),
        }
    }

    #[test]
    fn an_mbr_disk_is_refused() {
        let system = fixture::machine_with(
            fixture::FINDMNT_JSON_IN_RAM,
            &fixture::sfdisk_json("dos", fixture::STANDARD_TABLE),
        );
        let snapshot = probe::probe(&system).expect("probe");
        let disk = snapshot.disks[0].clone();
        match plan_ops(&disk, &snapshot, &[]) {
            Err(OpRefusal::NotGpt { table, .. }) => assert_eq!(table, "dos"),
            other => panic!("expected NotGpt, got {other:?}"),
        }
    }

    /// A btrfs cannot be shrunk without mounting it, and an xfs cannot be
    /// shrunk at all. Refused by name rather than attempted and failed half
    /// way through moving the user's data.
    #[test]
    fn a_filesystem_that_cannot_shrink_offline_is_refused_by_name() {
        let (disk, snapshot) = standard();
        for fstype in ["btrfs", "xfs", "f2fs", "vfat"] {
            match plan_ops(&disk, &snapshot, &[resize(3, 700_000_000, Some(fstype))]) {
                Err(OpRefusal::UnsupportedShrink { fstype: named, .. }) => {
                    assert_eq!(named, fstype);
                }
                other => panic!("{fstype} should be refused, got {other:?}"),
            }
        }
    }

    /// And the same filesystems can still be *grown*, because growing moves no
    /// data.
    #[test]
    fn a_filesystem_that_cannot_shrink_can_still_grow() {
        let system = fixture::machine_with(
            fixture::FINDMNT_JSON_IN_RAM,
            &fixture::sfdisk_json(
                "gpt",
                &[
                    fixture::STANDARD_TABLE[0],
                    fixture::STANDARD_TABLE[1],
                    fixture::STANDARD_TABLE[2],
                ],
            ),
        );
        let snapshot = probe::probe(&system).expect("probe");
        let disk = snapshot.disks[0].clone();
        // sda3 ends at 800032767; grow it into the space sda4 no longer takes.
        plan_ops(&disk, &snapshot, &[resize(3, 810_000_000, Some("xfs"))])
            .expect("growing an xfs is fine");
    }

    #[test]
    fn a_resize_that_would_run_into_the_next_partition_is_refused() {
        let (disk, snapshot) = standard();
        // sda3 ends at 800032767 and sda4 starts at 800032768.
        match plan_ops(&disk, &snapshot, &[resize(3, 800_100_000, Some("ntfs"))]) {
            Err(OpRefusal::Overlaps { path, with }) => {
                assert_eq!(path, "/dev/sda3");
                assert_eq!(with, "/dev/sda4");
            }
            other => panic!("expected an overlap, got {other:?}"),
        }
    }

    /// A resize necessarily overlaps its own old extent, so excluding itself is
    /// load-bearing rather than tidy.
    #[test]
    fn a_resize_does_not_overlap_itself() {
        let (disk, snapshot) = standard();
        plan_ops(&disk, &snapshot, &[resize(3, 700_000_000, Some("ntfs"))])
            .expect("shrinking into its own space is fine");
    }

    #[test]
    fn a_small_esp_is_a_warning_and_not_a_refusal() {
        let (disk, snapshot) = standard();
        let op = create_as(
            5,
            900_000_000,
            900_100_000,
            PartitionKind::EfiSystem,
            Filesystem::Fat32,
            "esp",
        );
        let projection = plan_ops(&disk, &snapshot, &[op]).expect("a small ESP is allowed");
        assert_eq!(projection.warnings.len(), 1, "{:?}", projection.warnings);
        assert!(projection.warnings[0].contains("EFI partition"));
    }

    // ---- Commands ----

    /// The most important test in this file. Shrinking is filesystem first: the
    /// other order leaves a filesystem larger than the partition holding it,
    /// which is corruption `fsck` will make worse.
    #[test]
    fn the_order_of_a_shrink_is_filesystem_then_partition() {
        let commands = commands_for_op(&resize(3, 700_000_000, Some("ntfs")));
        let rendered: Vec<String> = commands.iter().map(ToString::to_string).collect();
        let fs = rendered
            .iter()
            .position(|line| line.starts_with("ntfsresize"))
            .expect("the filesystem is resized");
        let entry = rendered
            .iter()
            .position(|line| line.contains("resizepart"))
            .expect("the entry is resized");
        assert!(
            fs < entry,
            "the filesystem must shrink before its partition does:\n{rendered:#?}"
        );
    }

    /// And growing is the other way round, for the mirror-image reason.
    #[test]
    fn the_order_of_a_grow_is_partition_then_filesystem() {
        let system = fixture::machine_with(
            fixture::FINDMNT_JSON_IN_RAM,
            &fixture::sfdisk_json(
                "gpt",
                &[
                    fixture::STANDARD_TABLE[0],
                    fixture::STANDARD_TABLE[1],
                    fixture::STANDARD_TABLE[2],
                ],
            ),
        );
        let snapshot = probe::probe(&system).expect("probe");
        let disk = snapshot.disks[0].clone();
        let op = DiskOp::Resize {
            disk: "/dev/sda".to_owned(),
            number: 3,
            path: "/dev/sda3".to_owned(),
            sector_size: 512,
            start_sector: 567_296,
            old_end_sector: 800_032_767,
            new_end_sector: 810_000_000,
            fstype: Some("ext4".to_owned()),
        };
        plan_ops(&disk, &snapshot, std::slice::from_ref(&op)).expect("a grow");

        let rendered: Vec<String> = commands_for_op(&op)
            .iter()
            .map(ToString::to_string)
            .collect();
        let entry = rendered
            .iter()
            .position(|line| line.contains("resizepart"))
            .expect("the entry is resized");
        let fs = rendered
            .iter()
            .position(|line| line.starts_with("resize2fs"))
            .expect("the filesystem is resized");
        assert!(
            entry < fs,
            "the partition must grow before its filesystem does:\n{rendered:#?}"
        );
        assert!(
            !rendered.iter().any(|line| line.starts_with("e2fsck")),
            "growing needs no prior fsck:\n{rendered:#?}"
        );
    }

    /// `resize2fs`' `s` suffix is 512-byte sectors regardless of the disk's own
    /// sector size, so the conversion has to be explicit — a 4Kn disk resized
    /// with raw sector counts would end up an eighth of the intended size.
    #[test]
    fn a_shrink_converts_to_resize2fs_own_sector_unit() {
        let op = DiskOp::Resize {
            disk: "/dev/nvme0n1".to_owned(),
            number: 2,
            path: "/dev/nvme0n1p2".to_owned(),
            sector_size: 4096,
            start_sector: 0,
            old_end_sector: 999,
            // 500 sectors of 4096 bytes = 2,048,000 bytes, which is 4000 of
            // `resize2fs`' 512-byte sectors and not the 500 the disk counts in.
            new_end_sector: 499,
            fstype: Some("ext4".to_owned()),
        };
        let rendered: Vec<String> = commands_for_op(&op)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(
            rendered
                .iter()
                .any(|line| line == "resize2fs /dev/nvme0n1p2 4000s"),
            "the size must be in resize2fs' unit, not the disk's:\n{rendered:#?}"
        );
        assert!(
            !rendered.iter().any(|line| line.contains(" 500s")),
            "500 is the disk's sector count and would be an eighth of the size:\n{rendered:#?}"
        );
    }

    /// `parted mkpart` cannot name the x86-64 root GUID, so a `Create` is
    /// always followed by an `sfdisk --part-type` that can. A root partition
    /// typed "Linux filesystem" boots and is invisible to partition discovery.
    #[test]
    fn a_created_root_gets_its_real_type_guid() {
        let op = create_as(
            5,
            900_000_000,
            950_000_000,
            PartitionKind::LinuxRoot,
            Filesystem::Ext4,
            "crownos",
        );
        let rendered: Vec<String> = commands_for_op(&op)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(
            rendered[0].starts_with("parted --script --align optimal /dev/sda unit s mkpart"),
            "{rendered:#?}"
        );
        assert!(
            rendered
                .iter()
                .any(|line| *line
                    == format!("sfdisk --part-type /dev/sda 5 {TYPE_LINUX_ROOT_X86_64}")),
            "{rendered:#?}"
        );
    }

    #[test]
    fn a_created_esp_is_flagged_and_formatted_fat32() {
        let op = create_as(
            5,
            900_000_000,
            950_000_000,
            PartitionKind::EfiSystem,
            Filesystem::Fat32,
            "crownesp",
        );
        let rendered: Vec<String> = commands_for_op(&op)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(
            rendered
                .iter()
                .any(|line| line == "parted --script /dev/sda set 5 esp on"),
            "{rendered:#?}"
        );
        assert!(
            rendered
                .iter()
                .any(|line| line == "mkfs.fat -F 32 -n CROWNESP /dev/sda5"),
            "a FAT label is upper case and at most 11 characters:\n{rendered:#?}"
        );
    }

    /// Nothing in this module may reach for `sfdisk --append` or `sgdisk`:
    /// `--append` cannot delete or resize, and `sgdisk --new` will take a
    /// partition number that is already in use.
    #[test]
    fn the_editor_uses_parted_for_geometry_and_nothing_else() {
        let ops = [
            create(5, 900_000_000, 950_000_000),
            delete(4),
            resize(3, 700_000_000, Some("ntfs")),
        ];
        for op in &ops {
            for command in commands_for_op(op) {
                let line = command.to_string();
                assert!(!line.contains("sgdisk"), "{line}");
                assert!(!line.contains("--append"), "{line}");
            }
        }
    }

    // ---- Carving ----

    #[test]
    fn carving_a_gap_aligns_both_ends() {
        let (disk, _) = standard();
        let gap = Gap {
            start_sector: 802_129_920,
            sectors: 1000,
        };
        // The gap is under an alignment unit once snapped, so nothing fits.
        assert_eq!(carve(&disk, gap, 10 * GIB), None);

        let big = *disk.free_gaps().last().expect("a gap");
        let (start, end) = carve(&disk, big, 20 * GIB).expect("20 GiB fits");
        let alignment = ALIGNMENT_BYTES / 512;
        assert_eq!(start % alignment, 0, "the start is aligned");
        assert_eq!((end + 1) % alignment, 0, "the end is aligned");
        assert!(
            disk.bytes(end - start + 1) <= 20 * GIB,
            "carve never hands back more than was asked for"
        );
        assert!(disk.bytes(end - start + 1) > 20 * GIB - ALIGNMENT_BYTES);
    }

    #[test]
    fn filling_a_gap_takes_all_of_it_within_alignment() {
        let (disk, _) = standard();
        let gap = *disk.free_gaps().last().expect("a gap");
        let (start, end) = fill(&disk, gap).expect("the gap is fillable");
        assert!(start >= gap.start_sector);
        assert!(end <= gap.end_sector());
        assert!(
            disk.bytes(gap.sectors) - disk.bytes(end - start + 1) < 2 * ALIGNMENT_BYTES,
            "filling should waste at most alignment at each end"
        );
    }

    fn blank() -> (Disk, SystemSnapshot) {
        let snapshot = probe::probe(&fixture::blank_machine(
            fixture::FINDMNT_JSON_IN_RAM,
            fixture::BLANK_LSBLK_JSON,
        ))
        .expect("probe");
        (snapshot.disks[0].clone(), snapshot)
    }

    fn create_table() -> DiskOp {
        DiskOp::CreateTable {
            disk: "/dev/sda".to_owned(),
        }
    }

    #[test]
    fn a_blank_disk_takes_partitions_once_a_table_is_queued() {
        let (disk, snapshot) = blank();
        let partition = create(1, 2048, 2048 + GIB / 512 - 1);

        match plan_ops(&disk, &snapshot, std::slice::from_ref(&partition)) {
            Err(OpRefusal::NoTable { .. }) => {}
            other => panic!("expected NoTable, got {other:?}"),
        }

        let projection = plan_ops(&disk, &snapshot, &[create_table(), partition])
            .expect("table, then partition");
        assert_eq!(projection.disk.table, "gpt");
        assert_eq!(projection.disk.partitions.len(), 1);
        assert!(projection.destroyed.is_empty());
    }

    /// Writing a table over one that exists loses every partition on it, which
    /// is not what "create a table" means.
    #[test]
    fn a_table_is_only_written_to_a_blank_disk() {
        let (disk, snapshot) = standard();
        match plan_ops(&disk, &snapshot, &[create_table()]) {
            Err(OpRefusal::TableExists { table, .. }) => assert_eq!(table, "gpt"),
            other => panic!("expected TableExists, got {other:?}"),
        }
    }

    #[test]
    fn a_new_table_is_parted_mklabel_and_destroys_nothing() {
        let op = create_table();
        assert!(!op.destroys_existing());
        assert_eq!(
            commands_for_op(&op),
            vec![Cmd::Run {
                program: "parted".to_owned(),
                args: ["--script", "/dev/sda", "mklabel", "gpt"]
                    .map(str::to_owned)
                    .to_vec(),
            }]
        );
    }
}
