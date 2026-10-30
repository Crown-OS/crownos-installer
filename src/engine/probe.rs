//! Reading the machine's actual state.
//!
//! Everything the planner is allowed to believe about the disk comes from here,
//! and everything here comes from four programs that ship on the archiso image:
//!
//! | program | what it is asked for |
//! |---|---|
//! | `lsblk --json` | which block devices exist, their PARTUUIDs, filesystems and mount points |
//! | `sfdisk --json` | the partition *table*: sector size, the usable LBA range, and where each partition starts and ends |
//! | `findmnt --json` | the mount table, which is how we find out whether the live payload is still coming off the staging medium |
//! | `blkid` | a targeted fallback for a partition `lsblk` reported no filesystem for |
//!
//! # Why two tools for the partition table
//!
//! `lsblk` knows about devices; `sfdisk` knows about the table. Only `sfdisk`
//! reports `firstlba`/`lastlba`, and without those the free space at the end of
//! a disk cannot be computed — the last usable sector is not `size / 512 - 1`,
//! because the GPT backup header and partition array live in the last 33 sectors.
//! Guessing that boundary is how a partition gets created that overlaps the
//! backup header, which produces a disk that boots today and does not survive
//! its first `partprobe`.
//!
//! `lsblk`'s own `START` column would do for offsets, but it is expressed in
//! 512-byte units regardless of the disk's real sector size, and this installer
//! runs on 4Kn disks. `sfdisk` reports the sector size it is using alongside the
//! numbers that are in it, so there is nothing to infer.
//!
//! # Nothing here decides anything
//!
//! The functions in this module return data. The judgement — is this the disk we
//! were promised, is that region the one Windows freed, is it safe to write to
//! the medium we booted from — all lives in [`plan`](super::plan), where it can
//! be tested without a disk.

use std::collections::BTreeMap;
use std::fmt;
use std::process::Command;

use anyhow::{Context as _, Result, anyhow, bail};
use serde::Deserialize;

use crate::util::cpuinfo::{self, Microarch};
use crate::util::meminfo;

use super::base::REPO_KEY_LIVE_PATH;

/// Where archiso mounts the medium it booted from.
///
/// A mount at this path whose source is the staging partition means the live
/// environment is still reading from the disk we are about to repartition.
pub const BOOTMNT: &str = "/run/archiso/bootmnt";

/// The smallest unallocated run [`Disk::free_gaps`] will report.
///
/// A GPT's first usable LBA is 34 and every partitioning tool since 2010 starts
/// the first partition at 1 MiB, so essentially every disk has a two-thousand
/// sector hole near the front. That is padding, not free space — nothing can be
/// created in it — and reporting it would mean every caller had to filter it out.
///
/// Must not exceed the planner's `ALIGNMENT_BYTES`, or the planner could be
/// handed a gap it cannot align a partition inside; there is a test for that.
pub const MIN_USEFUL_GAP_BYTES: u64 = 1024 * 1024;

/// Where archiso's `copytoram` hook puts the squashfs once it is in RAM.
///
/// The presence of this mount is positive evidence that the running system no
/// longer needs the staging medium. See
/// [`SystemSnapshot::payload_is_in_ram`].
pub const COPYTORAM: &str = "/run/archiso/copytoram";

const MEMINFO: &str = "/proc/meminfo";
const CPUINFO: &str = "/proc/cpuinfo";

// --- MARK: System access ---

/// Everything this program does to the outside world, behind one trait.
///
/// Two implementations: [`RealSystem`], which shells out, and the fixture-backed
/// fake the tests use. The point is not mockability for its own sake — it is
/// that the probe's parsing and the executor's command *construction* are the
/// parts most likely to be wrong, and neither of them should need a spare disk
/// to exercise.
pub trait System {
    /// Run `program` with `args` and return its standard output.
    ///
    /// A non-zero exit is an error, and the error carries stderr: an installer
    /// whose log says `mkfs.ext4 failed` and nothing else is an installer nobody
    /// can debug.
    fn run(&self, program: &str, args: &[&str]) -> Result<String>;

    /// Read a file. Used for sysfs and for the small number of files the
    /// installer inspects rather than writes.
    fn read_file(&self, path: &str) -> Result<String>;

    /// Write `contents` to `path`, creating parent directories.
    fn write_file(&self, path: &str, contents: &str) -> Result<()>;

    /// Whether `path` exists. Separate from [`read_file`](Self::read_file)
    /// because "already there, leave it alone" is a decision the executor makes
    /// about files it must not clobber — a Windows machine's `loader.conf`, for
    /// one.
    fn exists(&self, path: &str) -> bool;
}

/// The real thing: subprocesses and the filesystem.
#[derive(Debug, Default, Clone, Copy)]
pub struct RealSystem;

impl System for RealSystem {
    fn run(&self, program: &str, args: &[&str]) -> Result<String> {
        let output = Command::new(program)
            .args(args)
            .output()
            .with_context(|| format!("could not run `{program}`"))?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if output.status.success() {
            return Ok(stdout);
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow!(
            "`{program} {}` exited with {}: {}",
            args.join(" "),
            output
                .status
                .code()
                .map_or_else(|| "a signal".to_owned(), |c| c.to_string()),
            stderr.trim()
        ))
    }

    fn read_file(&self, path: &str) -> Result<String> {
        std::fs::read_to_string(path).with_context(|| format!("could not read {path}"))
    }

    fn write_file(&self, path: &str, contents: &str) -> Result<()> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        std::fs::write(path, contents).with_context(|| format!("could not write {path}"))
    }

    fn exists(&self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }
}

// --- MARK: Snapshot ---

/// The machine as it is right now: its disks, its mount table, its RAM, its
/// CPU and what the live image ships.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SystemSnapshot {
    /// Every whole disk found, with its partition table read.
    pub disks: Vec<Disk>,
    /// The mount table, as `findmnt` reports it.
    pub mounts: Vec<Mount>,
    /// Installed RAM, or zero when `/proc/meminfo` could not be read.
    pub memory_bytes: u64,
    /// Which package builds the CPU runs; generic ones when `/proc/cpuinfo`
    /// could not be read.
    pub microarch: Microarch,
    /// Whether the live image ships the CrownOS repository's signing key. An
    /// image built without one installs systems that do not use the repository.
    pub crownos_repo_key: bool,
}

impl SystemSnapshot {
    /// Find the partition with this PARTUUID, and the disk it belongs to.
    ///
    /// This is the only supported way to locate anything. A device name is a
    /// property of this boot's enumeration order; a partition GUID is the same
    /// sixteen bytes to Windows, to Linux and to the firmware. See
    /// [`Target`](crate::oobe::Target).
    pub fn find_partition(&self, partuuid: &str) -> Option<(&Disk, &Partition)> {
        let wanted = partuuid.trim().to_ascii_lowercase();
        self.disks.iter().find_map(|disk| {
            disk.partitions
                .iter()
                .find(|part| part.partuuid.as_deref() == Some(wanted.as_str()))
                .map(|part| (disk, part))
        })
    }

    /// Every mount currently backed by `partition`.
    ///
    /// `findmnt` reports whatever source string was used to mount the
    /// filesystem, which for an archiso boot is a `/dev/disk/by-partuuid/…`
    /// symlink rather than a device node — so both spellings have to be
    /// recognised, and neither can be canonicalised without touching the disk.
    pub fn mounts_backed_by<'a>(&'a self, partition: &Partition) -> Vec<&'a Mount> {
        self.mounts
            .iter()
            .filter(|mount| partition.is_source_of(&mount.source))
            .collect()
    }

    /// Whether the live environment can survive the staging medium going away.
    ///
    /// Two forms of evidence, either of which is enough:
    ///
    /// * archiso's `copytoram` hook has a mount at [`COPYTORAM`], which is where
    ///   it puts the squashfs once it has read it into RAM; or
    /// * nothing at all is mounted from the staging partition, which is what
    ///   that same hook leaves behind after it unmounts [`BOOTMNT`].
    ///
    /// Note what is *not* consulted: `copytoram=y` on the kernel command line.
    /// That parameter is a request made before the fact, and a request that
    /// silently failed — a medium too slow, a machine short on RAM — looks
    /// identical to one that succeeded. The mount table is the outcome, so the
    /// mount table is what we read.
    pub fn payload_is_in_ram(&self, staging: &Partition) -> bool {
        self.mounts.iter().any(|mount| mount.target == COPYTORAM)
            || self.mounts_backed_by(staging).is_empty()
    }
}

/// One whole disk and its partition table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disk {
    /// Device node, e.g. `/dev/sda` or `/dev/nvme0n1`.
    pub path: String,
    /// The table's own label, lowercased: `"gpt"`, `"dos"`, or whatever
    /// `sfdisk` said. The installer only works on GPT.
    pub table: String,
    /// GPT disk GUID, lowercased, when the table has one.
    pub disk_guid: Option<String>,
    /// Logical sector size in bytes — the unit
    /// [`start_sector`](Partition::start_sector) is expressed in.
    pub sector_size: u32,
    /// First sector a partition may occupy, from the GPT header.
    pub first_usable_sector: u64,
    /// Last sector a partition may occupy, inclusive. Below the end of the
    /// device by the GPT backup header and partition array.
    pub last_usable_sector: u64,
    /// Partitions in table order.
    pub partitions: Vec<Partition>,
}

impl Disk {
    /// No partition table yet. See [`DiskOp::CreateTable`](super::disks::DiskOp::CreateTable).
    pub fn is_blank(&self) -> bool {
        self.table.is_empty()
    }

    /// Convert sectors to bytes with this disk's sector size.
    pub fn bytes(&self, sectors: u64) -> u64 {
        sectors * u64::from(self.sector_size)
    }

    /// The unallocated runs of the disk, in address order, excluding anything
    /// smaller than [`MIN_USEFUL_GAP_BYTES`].
    ///
    /// Computed from the table rather than asked of a tool, because every tool
    /// that reports free space reports it rounded for human consumption and this
    /// number is about to become a partition boundary.
    pub fn free_gaps(&self) -> Vec<Gap> {
        let mut used: Vec<(u64, u64)> = self
            .partitions
            .iter()
            .map(|p| (p.start_sector, p.end_sector()))
            .collect();
        used.sort_unstable();

        let minimum = (MIN_USEFUL_GAP_BYTES / u64::from(self.sector_size)).max(1);
        let mut gaps = Vec::new();
        let push = |start: u64, sectors: u64, gaps: &mut Vec<Gap>| {
            if sectors >= minimum {
                gaps.push(Gap {
                    start_sector: start,
                    sectors,
                });
            }
        };

        let mut cursor = self.first_usable_sector;
        for (start, end) in used {
            if start > cursor {
                // `start - cursor`, not `+ 1`: the gap ends the sector before the
                // partition begins.
                push(cursor, start - cursor, &mut gaps);
            }
            cursor = cursor.max(end + 1);
        }
        if cursor <= self.last_usable_sector {
            push(cursor, self.last_usable_sector - cursor + 1, &mut gaps);
        }
        gaps
    }

    /// The lowest partition number not currently in use.
    ///
    /// Not `len() + 1`: a disk whose partitions have been deleted and re-created
    /// has holes in its numbering, and reusing a live number is how a `sfdisk`
    /// script silently overwrites an existing entry.
    pub fn next_partition_number(&self) -> u32 {
        let used: Vec<u32> = self.partitions.iter().map(|p| p.number).collect();
        (1..).find(|n| !used.contains(n)).expect("u32 is not full")
    }

    /// The device node partition `number` on this disk would have.
    ///
    /// `nvme0n1` needs a `p` before the number and `sda` must not have one; the
    /// rule the kernel uses is "insert `p` when the parent name already ends in
    /// a digit", and it is reproduced rather than looked up because the partition
    /// does not exist yet.
    pub fn partition_path(&self, number: u32) -> String {
        if self.path.ends_with(|c: char| c.is_ascii_digit()) {
            format!("{}p{number}", self.path)
        } else {
            format!("{}{number}", self.path)
        }
    }

    /// Look up a partition on this disk by PARTUUID.
    pub fn partition(&self, partuuid: &str) -> Option<&Partition> {
        let wanted = partuuid.trim().to_ascii_lowercase();
        self.partitions
            .iter()
            .find(|p| p.partuuid.as_deref() == Some(wanted.as_str()))
    }
}

/// One partition, as the table describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Partition {
    /// Device node, e.g. `/dev/sda3`.
    pub path: String,
    /// Partition number within the table.
    pub number: u32,
    /// PARTUUID, lowercased and unbraced. `None` only for a table that has no
    /// per-partition GUIDs, which is a table this installer refuses to work on.
    pub partuuid: Option<String>,
    /// GPT partition type GUID, lowercased.
    pub type_guid: Option<String>,
    /// GPT partition name, if set.
    pub partlabel: Option<String>,
    /// First sector, in the parent disk's sector size.
    pub start_sector: u64,
    /// Length in sectors.
    pub sectors: u64,
    /// Filesystem `lsblk`/`blkid` recognised, lowercased.
    pub fstype: Option<String>,
    /// Filesystem label.
    pub label: Option<String>,
}

impl Partition {
    /// Last sector, inclusive.
    pub fn end_sector(&self) -> u64 {
        self.start_sector + self.sectors - 1
    }

    /// Whether `source` — a string out of the mount table — names this
    /// partition.
    ///
    /// Matches the device node, and any `by-partuuid` / `by-uuid` style symlink
    /// ending in this partition's PARTUUID. Deliberately conservative in the
    /// only direction that matters: a source we fail to recognise makes the
    /// planner think a partition is *unmounted* when it is not, so the match is
    /// broad rather than exact.
    pub fn is_source_of(&self, source: &str) -> bool {
        let source = source.trim();
        if source == self.path {
            return true;
        }
        match self.partuuid.as_deref() {
            // `/dev/disk/by-partuuid/aaaa-…` is what archiso's `archisodevice=`
            // resolves to, and it is what shows up in the mount table.
            Some(partuuid) => source
                .rsplit('/')
                .next()
                .is_some_and(|last| last.eq_ignore_ascii_case(partuuid)),
            None => false,
        }
    }
}

/// An unallocated run of a disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gap {
    pub start_sector: u64,
    pub sectors: u64,
}

impl Gap {
    /// Last sector of the gap, inclusive.
    pub fn end_sector(self) -> u64 {
        self.start_sector + self.sectors - 1
    }
}

/// One row of the mount table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    /// Whatever was passed to `mount` — a device node, a `by-partuuid` symlink,
    /// or a pseudo-source like `tmpfs`.
    pub source: String,
    pub target: String,
    pub fstype: String,
}

impl fmt::Display for Mount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} on {} ({})", self.source, self.target, self.fstype)
    }
}

// --- MARK: Probing ---

/// Read the machine's disks and mount table.
pub fn probe(system: &dyn System) -> Result<SystemSnapshot> {
    let devices = read_block_devices(system)?;
    let mounts = read_mounts(system)?;

    let mut disks = Vec::new();
    for device in devices.values().filter(|d| d.kind == "disk") {
        // A disk whose table cannot be read is not fatal on its own — a card
        // reader with no card in it is a normal thing to find — so it is skipped
        // rather than propagated. The planner reports "no partition with that
        // PARTUUID", which is the failure the user can act on.
        match read_table(system, &device.path, &devices) {
            Ok(disk) => disks.push(disk),
            Err(_) if device.is_blank() => disks.extend(read_blank_disk(system, &device.path).ok()),
            Err(_) => continue,
        }
    }
    disks.sort_by(|a, b| a.path.cmp(&b.path));

    let memory_bytes = system
        .read_file(MEMINFO)
        .ok()
        .and_then(|meminfo| meminfo::total_bytes(&meminfo))
        .unwrap_or_default();
    let microarch = system
        .read_file(CPUINFO)
        .map(|cpuinfo| cpuinfo::microarch(&cpuinfo))
        .unwrap_or_default();

    Ok(SystemSnapshot {
        disks,
        mounts,
        memory_bytes,
        microarch,
        crownos_repo_key: system.exists(REPO_KEY_LIVE_PATH),
    })
}

/// What `lsblk` says about one block device, before it is matched up with the
/// partition table.
#[derive(Clone, Debug)]
struct BlockDevice {
    path: String,
    kind: String,
    partuuid: Option<String>,
    partlabel: Option<String>,
    fstype: Option<String>,
    label: Option<String>,
    pttype: Option<String>,
}

impl BlockDevice {
    /// Nothing on it at all: no partition table and no filesystem written
    /// straight onto the device. A brand-new disk, which is the commonest
    /// install target there is.
    fn is_blank(&self) -> bool {
        self.pttype.is_none() && self.fstype.is_none()
    }
}

/// Columns to ask `lsblk` for. `--list` rather than the default tree so the JSON
/// is a flat array: nesting buys nothing here (the parent is recovered from
/// `PKNAME`) and costs a recursive deserializer.
const LSBLK_COLUMNS: &str = "PATH,TYPE,PARTUUID,PARTLABEL,FSTYPE,LABEL,PTTYPE,PKNAME";

fn read_block_devices(system: &dyn System) -> Result<BTreeMap<String, BlockDevice>> {
    #[derive(Deserialize)]
    struct Output {
        blockdevices: Vec<Row>,
    }
    #[derive(Deserialize)]
    struct Row {
        path: String,
        #[serde(rename = "type")]
        kind: Option<String>,
        partuuid: Option<String>,
        partlabel: Option<String>,
        fstype: Option<String>,
        label: Option<String>,
        pttype: Option<String>,
    }

    let json = system.run(
        "lsblk",
        &[
            "--json",
            "--bytes",
            "--list",
            "--paths",
            "--output",
            LSBLK_COLUMNS,
        ],
    )?;
    let output: Output = serde_json::from_str(&json).context("could not parse `lsblk --json`")?;

    Ok(output
        .blockdevices
        .into_iter()
        .map(|row| {
            let device = BlockDevice {
                path: row.path.clone(),
                kind: row.kind.unwrap_or_default(),
                partuuid: lower(row.partuuid),
                partlabel: row.partlabel.filter(|s| !s.is_empty()),
                fstype: lower(row.fstype),
                label: row.label.filter(|s| !s.is_empty()),
                pttype: lower(row.pttype),
            };
            (row.path, device)
        })
        .collect())
}

fn read_mounts(system: &dyn System) -> Result<Vec<Mount>> {
    #[derive(Deserialize)]
    struct Output {
        filesystems: Vec<Row>,
    }
    #[derive(Deserialize)]
    struct Row {
        source: Option<String>,
        target: Option<String>,
        fstype: Option<String>,
    }

    let json = system.run(
        "findmnt",
        &["--json", "--list", "--output", "SOURCE,TARGET,FSTYPE"],
    )?;
    let output: Output = serde_json::from_str(&json).context("could not parse `findmnt --json`")?;

    Ok(output
        .filesystems
        .into_iter()
        .filter_map(|row| {
            Some(Mount {
                // `findmnt` decorates a bind or subvolume source as
                // `/dev/sda2[/@home]`; the device is the part before the
                // bracket and that is the only part a partition can be matched
                // against.
                source: row.source?.split('[').next().unwrap_or_default().to_owned(),
                target: row.target?,
                fstype: row.fstype.unwrap_or_default(),
            })
        })
        .collect())
}

fn read_table(
    system: &dyn System,
    disk_path: &str,
    devices: &BTreeMap<String, BlockDevice>,
) -> Result<Disk> {
    #[derive(Deserialize)]
    struct Output {
        partitiontable: Table,
    }
    #[derive(Deserialize)]
    struct Table {
        label: Option<String>,
        id: Option<String>,
        unit: Option<String>,
        sectorsize: Option<u32>,
        firstlba: Option<u64>,
        lastlba: Option<u64>,
        #[serde(default)]
        partitions: Vec<Entry>,
    }
    #[derive(Deserialize)]
    struct Entry {
        node: String,
        start: u64,
        size: u64,
        #[serde(rename = "type")]
        type_guid: Option<String>,
        uuid: Option<String>,
        name: Option<String>,
    }

    let json = system.run("sfdisk", &["--json", disk_path])?;
    let table = serde_json::from_str::<Output>(&json)
        .with_context(|| format!("could not parse `sfdisk --json {disk_path}`"))?
        .partitiontable;

    // `sfdisk --json` reports sectors unless asked otherwise. If a future
    // version changes that, every offset below would be silently wrong by a
    // factor of the sector size, so it is checked rather than assumed.
    if let Some(unit) = &table.unit
        && unit != "sectors"
    {
        bail!("`sfdisk --json {disk_path}` reported offsets in {unit}, not sectors");
    }

    let sector_size = match table.sectorsize {
        Some(size) if size > 0 => size,
        // Fall back to sysfs. `sfdisk` has reported `sectorsize` since
        // util-linux 2.27, but the number is load-bearing enough to be worth a
        // second source rather than a default.
        _ => read_logical_sector_size(system, disk_path)?,
    };

    let mut partitions = Vec::with_capacity(table.partitions.len());
    for entry in table.partitions {
        let device = devices.get(&entry.node);
        // `lsblk` reports no filesystem for a device it has not scanned since it
        // appeared, which on a disk that was repartitioned a moment ago is
        // common. `blkid` reads the superblock directly. Failures are ignored:
        // a partition with genuinely no filesystem is a normal thing to find,
        // and the planner never depends on knowing what is on one it does not
        // intend to touch.
        let scanned = match device.and_then(|d| d.fstype.clone()) {
            Some(fstype) => Some((Some(fstype), device.and_then(|d| d.label.clone()))),
            None => blkid(system, &entry.node).ok().map(|fields| {
                (
                    lower(fields.get("TYPE").cloned()),
                    fields.get("LABEL").cloned(),
                )
            }),
        };
        let (fstype, label) = scanned.unwrap_or((None, None));

        partitions.push(Partition {
            number: partition_number(&entry.node)?,
            partuuid: lower(entry.uuid).or_else(|| device.and_then(|d| d.partuuid.clone())),
            type_guid: lower(entry.type_guid),
            partlabel: entry
                .name
                .filter(|s| !s.is_empty())
                .or_else(|| device.and_then(|d| d.partlabel.clone())),
            start_sector: entry.start,
            sectors: entry.size,
            fstype,
            label,
            path: entry.node,
        });
    }
    partitions.sort_by_key(|p| p.start_sector);

    let sectors = read_device_sectors(system, disk_path, sector_size).unwrap_or(0);
    Ok(Disk {
        table: table.label.unwrap_or_default().to_ascii_lowercase(),
        disk_guid: lower(table.id),
        sector_size,
        // A GPT's first usable LBA is 34 on a 512-byte disk; `sfdisk` reports it
        // rather than the aligned 2048 that partitioning tools actually use, and
        // the planner does its own alignment.
        first_usable_sector: table.firstlba.unwrap_or(34),
        last_usable_sector: table
            .lastlba
            // Without `lastlba` the end of the disk is unknown, and the only
            // safe unknown end is one that yields no trailing free space.
            .unwrap_or_else(|| sectors.saturating_sub(34).saturating_sub(1)),
        partitions,
        path: disk_path.to_owned(),
    })
}

/// A disk with no partition table, described as the GPT `parted mklabel gpt`
/// would give it, so the editor can lay partitions out before the table exists.
///
/// An empty device is refused: a card reader with no card in it reports zero
/// sectors, and offering it as a target helps nobody.
fn read_blank_disk(system: &dyn System, disk_path: &str) -> Result<Disk> {
    let sector_size = read_logical_sector_size(system, disk_path)?;
    let sectors = read_device_sectors(system, disk_path, sector_size)?;
    let (first_usable_sector, last_usable_sector) = gpt_usable_range(sectors, sector_size)
        .with_context(|| format!("{disk_path} is too small to hold a GPT"))?;
    Ok(Disk {
        path: disk_path.to_owned(),
        table: String::new(),
        disk_guid: None,
        sector_size,
        first_usable_sector,
        last_usable_sector,
        partitions: Vec::new(),
    })
}

/// The bytes a GPT reserves for its partition entry array, at each end of the
/// disk: 128 entries of 128 bytes.
const GPT_ENTRY_ARRAY_BYTES: u64 = 128 * 128;

/// The first and last sector a fresh GPT leaves usable: the protective MBR, the
/// header and the entry array at the front, the backup entry array and backup
/// header at the back.
fn gpt_usable_range(sectors: u64, sector_size: u32) -> Option<(u64, u64)> {
    let reserved = 1 + GPT_ENTRY_ARRAY_BYTES.div_ceil(u64::from(sector_size));
    let first = 1 + reserved;
    let last = sectors.checked_sub(1 + reserved)?;
    (last > first).then_some((first, last))
}

/// The disk's logical sector size, from sysfs.
fn read_logical_sector_size(system: &dyn System, disk_path: &str) -> Result<u32> {
    let name = disk_path.trim_start_matches("/dev/").replace('/', "!");
    let raw = system.read_file(&format!("/sys/block/{name}/queue/logical_block_size"))?;
    raw.trim()
        .parse()
        .with_context(|| format!("{disk_path} reported a nonsensical logical block size: {raw:?}"))
}

/// The disk's size in its own sectors, from sysfs.
///
/// `/sys/block/*/size` is in 512-byte units regardless of the device's real
/// sector size — a kernel interface quirk that has bitten every tool that ever
/// read it, so the conversion is spelled out here.
fn read_device_sectors(system: &dyn System, disk_path: &str, sector_size: u32) -> Result<u64> {
    let name = disk_path.trim_start_matches("/dev/").replace('/', "!");
    let raw = system.read_file(&format!("/sys/block/{name}/size"))?;
    let sectors_512: u64 = raw
        .trim()
        .parse()
        .with_context(|| format!("{disk_path} reported a nonsensical size: {raw:?}"))?;
    Ok(sectors_512 * 512 / u64::from(sector_size))
}

/// Ask `blkid` about one device.
///
/// The fallback for a partition `lsblk` reported no filesystem for, which happens
/// on a device that has not been scanned since it appeared. Returns the
/// `KEY=VALUE` pairs `blkid -o export` prints.
pub fn blkid(system: &dyn System, device: &str) -> Result<BTreeMap<String, String>> {
    let output = system.run("blkid", &["--output", "export", device])?;
    Ok(output
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect())
}

/// The trailing number of a partition device node: `/dev/sda3` → 3,
/// `/dev/nvme0n1p12` → 12.
fn partition_number(node: &str) -> Result<u32> {
    let digits: String = node
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits
        .parse()
        .with_context(|| format!("{node} does not end in a partition number"))
}

fn lower(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty())
}

// --- MARK: Test fixtures ---

#[cfg(test)]
pub(crate) mod fixture {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A [`System`] that answers from recorded output.
    ///
    /// Keyed on the whole command line, so a test that changes the arguments a
    /// probe passes fails loudly instead of quietly returning the old fixture.
    #[derive(Default)]
    pub(crate) struct FakeSystem {
        commands: HashMap<String, String>,
        files: HashMap<String, String>,
        present: Vec<String>,
        /// Everything that was run or written, in order — this is what the
        /// executor's tests assert against.
        pub(crate) journal: RefCell<Vec<String>>,
    }

    impl FakeSystem {
        pub(crate) fn new() -> Self {
            Self::default()
        }

        pub(crate) fn with_command(mut self, line: &str, output: &str) -> Self {
            self.commands.insert(line.to_owned(), output.to_owned());
            self
        }

        pub(crate) fn with_file(mut self, path: &str, contents: &str) -> Self {
            self.files.insert(path.to_owned(), contents.to_owned());
            self.present.push(path.to_owned());
            self
        }

        pub(crate) fn with_existing(mut self, path: &str) -> Self {
            self.present.push(path.to_owned());
            self
        }

        pub(crate) fn journal(&self) -> Vec<String> {
            self.journal.borrow().clone()
        }
    }

    impl System for FakeSystem {
        fn run(&self, program: &str, args: &[&str]) -> Result<String> {
            let line = if args.is_empty() {
                program.to_owned()
            } else {
                format!("{program} {}", args.join(" "))
            };
            self.journal.borrow_mut().push(line.clone());
            self.commands
                .get(&line)
                .cloned()
                .ok_or_else(|| anyhow!("no fixture for `{line}`"))
        }

        fn read_file(&self, path: &str) -> Result<String> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| anyhow!("no fixture for reading {path}"))
        }

        fn write_file(&self, path: &str, contents: &str) -> Result<()> {
            self.journal
                .borrow_mut()
                .push(format!("write {path} ({} bytes)", contents.len()));
            Ok(())
        }

        fn exists(&self, path: &str) -> bool {
            self.present.iter().any(|p| p == path)
        }
    }

    /// One row of a synthetic partition table.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Part {
        pub(crate) node: &'static str,
        pub(crate) start: u64,
        pub(crate) size: u64,
        pub(crate) type_guid: &'static str,
        pub(crate) uuid: &'static str,
        pub(crate) name: &'static str,
    }

    /// The last sector a disk of [`DISK_SECTORS`] sectors may put a partition
    /// in — the device's size less the GPT backup header and array.
    pub(crate) const LAST_LBA: u64 = 1_000_215_182;
    /// A 512 GB disk in 512-byte sectors.
    pub(crate) const DISK_SECTORS: u64 = 1_000_215_216;

    /// A 512 GB SATA disk with a Windows install on it, shrunk to leave ~94 GiB
    /// free, and a 1 GiB `CROWNSTAGE` partition carved out of the *front* of
    /// that free space — which is where Windows' own partitioning puts a new
    /// partition in a free extent.
    ///
    /// | # | contents | sectors |
    /// |---|---|---|
    /// | 1 | ESP, 260 MiB | 2048 – 534527 |
    /// | 2 | Microsoft reserved, 16 MiB | 534528 – 567295 |
    /// | 3 | Windows C:, ~381 GiB | 567296 – 800032767 |
    /// | 4 | CROWNSTAGE, 1 GiB | 800032768 – 802129919 |
    /// | — | free, ~94 GiB | 802129920 – 1000215182 |
    pub(crate) const STANDARD_TABLE: &[Part] = &[
        Part {
            node: "/dev/sda1",
            start: 2048,
            size: 532_480,
            type_guid: "c12a7328-f81f-11d2-ba4b-00a0c93ec93b",
            uuid: "0FC63DAF-8483-4772-8E79-3D69D8477DE4",
            name: "EFI system partition",
        },
        Part {
            node: "/dev/sda2",
            start: 534_528,
            size: 32_768,
            type_guid: "e3c9e316-0b5c-4db8-817d-f92df00215ae",
            uuid: "11111111-2222-3333-4444-555555555555",
            name: "Microsoft reserved partition",
        },
        Part {
            node: "/dev/sda3",
            start: 567_296,
            size: 799_465_472,
            type_guid: "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7",
            uuid: "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7",
            name: "Basic data partition",
        },
        Part {
            node: "/dev/sda4",
            start: 800_032_768,
            size: 2_097_152,
            type_guid: "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7",
            uuid: "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE",
            name: "CROWNSTAGE",
        },
    ];

    /// Render a table as `sfdisk --json` would print it.
    ///
    /// A builder rather than a pile of string literals so that a variant fixture
    /// is an edited array — string surgery on JSON is how a test ends up
    /// asserting against a table with two overlapping partitions in it.
    pub(crate) fn sfdisk_json(label: &str, partitions: &[Part]) -> String {
        let rows: Vec<String> = partitions
            .iter()
            .map(|p| {
                format!(
                    r#"{{"node": "{}", "start": {}, "size": {}, "type": "{}", "uuid": "{}", "name": "{}"}}"#,
                    p.node, p.start, p.size, p.type_guid, p.uuid, p.name
                )
            })
            .collect();
        format!(
            r#"{{"partitiontable": {{
                 "label": "{label}",
                 "id": "01234567-89AB-CDEF-0123-456789ABCDEF",
                 "device": "/dev/sda",
                 "unit": "sectors",
                 "firstlba": 34,
                 "lastlba": {LAST_LBA},
                 "sectorsize": 512,
                 "partitions": [{}]
               }}}}"#,
            rows.join(",\n")
        )
    }

    pub(crate) const LSBLK_JSON: &str = r#"{
       "blockdevices": [
          {"path":"/dev/sda","type":"disk","partuuid":null,"partlabel":null,"fstype":null,"label":null,"pttype":"gpt","pkname":null},
          {"path":"/dev/sda1","type":"part","partuuid":"0fc63daf-8483-4772-8e79-3d69d8477de4","partlabel":"EFI system partition","fstype":"vfat","label":"SYSTEM","pkname":"sda"},
          {"path":"/dev/sda2","type":"part","partuuid":"11111111-2222-3333-4444-555555555555","partlabel":"Microsoft reserved partition","fstype":null,"label":null,"pkname":"sda"},
          {"path":"/dev/sda3","type":"part","partuuid":"ebd0a0a2-b9e5-4433-87c0-68b6b72699c7","partlabel":"Basic data partition","fstype":"ntfs","label":"Windows","pkname":"sda"},
          {"path":"/dev/sda4","type":"part","partuuid":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","partlabel":"CROWNSTAGE","fstype":"vfat","label":"CROWNSTAGE","pkname":"sda"}
       ]
    }"#;

    pub(crate) const BLANK_LSBLK_JSON: &str = r#"{
       "blockdevices": [
          {"path":"/dev/sda","type":"disk","partuuid":null,"partlabel":null,"fstype":null,"label":null,"pttype":null,"pkname":null}
       ]
    }"#;

    /// The mount table of a live environment that has already copied its
    /// squashfs to RAM: `bootmnt` is gone, `copytoram` is there.
    pub(crate) const FINDMNT_JSON_IN_RAM: &str = r#"{
       "filesystems": [
          {"source":"airootfs","target":"/","fstype":"overlay"},
          {"source":"tmpfs","target":"/run/archiso/copytoram","fstype":"tmpfs"},
          {"source":"/dev/loop0","target":"/run/archiso/airootfs","fstype":"squashfs"}
       ]
    }"#;

    /// The same machine, but still reading the payload off the staging
    /// partition.
    pub(crate) const FINDMNT_JSON_ON_MEDIUM: &str = r#"{
       "filesystems": [
          {"source":"airootfs","target":"/","fstype":"overlay"},
          {"source":"/dev/disk/by-partuuid/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","target":"/run/archiso/bootmnt","fstype":"vfat"},
          {"source":"/dev/loop0","target":"/run/archiso/airootfs","fstype":"squashfs"}
       ]
    }"#;

    /// A [`FakeSystem`] wired up with the fixtures above.
    pub(crate) fn machine(findmnt: &str) -> FakeSystem {
        machine_with(findmnt, &sfdisk_json("gpt", STANDARD_TABLE))
    }

    /// A machine whose only disk is brand new: `sfdisk` finds no table on it.
    pub(crate) fn blank_machine(findmnt: &str, lsblk: &str) -> FakeSystem {
        FakeSystem::new()
            .with_command(
                &format!(
                    "lsblk --json --bytes --list --paths --output {}",
                    super::LSBLK_COLUMNS
                ),
                lsblk,
            )
            .with_command(
                "findmnt --json --list --output SOURCE,TARGET,FSTYPE",
                findmnt,
            )
            .with_file("/sys/block/sda/size", &format!("{DISK_SECTORS}\n"))
            .with_file("/sys/block/sda/queue/logical_block_size", "512\n")
    }

    /// [`machine`] with a hand-built partition table.
    pub(crate) fn machine_with(findmnt: &str, sfdisk: &str) -> FakeSystem {
        FakeSystem::new()
            .with_command(
                &format!(
                    "lsblk --json --bytes --list --paths --output {}",
                    super::LSBLK_COLUMNS
                ),
                LSBLK_JSON,
            )
            .with_command("sfdisk --json /dev/sda", sfdisk)
            .with_command(
                "findmnt --json --list --output SOURCE,TARGET,FSTYPE",
                findmnt,
            )
            .with_file("/sys/block/sda/size", &format!("{DISK_SECTORS}\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::*;
    use super::*;

    #[test]
    fn a_windows_disk_is_read_back_faithfully() {
        let snapshot = probe(&machine(FINDMNT_JSON_IN_RAM)).expect("probe");
        assert_eq!(snapshot.disks.len(), 1);

        let disk = &snapshot.disks[0];
        assert_eq!(disk.path, "/dev/sda");
        assert_eq!(disk.table, "gpt");
        assert_eq!(disk.sector_size, 512);
        assert_eq!(disk.last_usable_sector, 1000215182);
        assert_eq!(disk.partitions.len(), 4);

        // PARTUUIDs come out of `sfdisk` upper case and out of `lsblk` lower
        // case; everything downstream compares them as strings, so they are
        // normalized here or not at all.
        let esp = disk
            .partition("0FC63DAF-8483-4772-8E79-3D69D8477DE4")
            .expect("esp");
        assert_eq!(esp.number, 1);
        assert_eq!(esp.fstype.as_deref(), Some("vfat"));
        assert_eq!(esp.start_sector, 2048);
        assert_eq!(esp.end_sector(), 534527);
    }

    #[test]
    fn the_free_region_windows_left_is_found() {
        let snapshot = probe(&machine(FINDMNT_JSON_IN_RAM)).expect("probe");
        let disk = &snapshot.disks[0];
        let gaps = disk.free_gaps();

        // One gap: everything after CROWNSTAGE. The partitions in the fixture
        // are contiguous, so the only free space is at the end.
        assert_eq!(gaps.len(), 1, "{gaps:?}");
        assert_eq!(gaps[0].start_sector, 802129920);
        assert_eq!(gaps[0].end_sector(), 1000215182);
    }

    /// The gap arithmetic is the part that turns into partition boundaries, so
    /// it is exercised on a table with a hole in the middle as well.
    #[test]
    fn gaps_between_partitions_are_found() {
        let disk = Disk {
            path: "/dev/sda".to_owned(),
            table: "gpt".to_owned(),
            disk_guid: None,
            sector_size: 512,
            first_usable_sector: 34,
            last_usable_sector: 102_400,
            partitions: vec![
                partition(1, 4096, 4096),  // 4096..=8191
                partition(2, 20480, 4096), // 20480..=24575
            ],
        };
        assert_eq!(
            disk.free_gaps(),
            vec![
                Gap {
                    start_sector: 34,
                    sectors: 4062
                },
                Gap {
                    start_sector: 8192,
                    sectors: 12288
                },
                Gap {
                    start_sector: 24576,
                    sectors: 77825
                },
            ]
        );
    }

    /// The 2014-sector hole between the GPT header and the first aligned
    /// partition is padding, and reporting it as free space would make every
    /// caller filter it out.
    #[test]
    fn a_gap_too_small_to_partition_is_not_reported() {
        let disk = Disk {
            path: "/dev/sda".to_owned(),
            table: "gpt".to_owned(),
            disk_guid: None,
            sector_size: 512,
            first_usable_sector: 34,
            last_usable_sector: 102_400,
            partitions: vec![partition(1, 2048, 100_353)], // 2048..=102400
        };
        assert!(disk.free_gaps().is_empty(), "{:?}", disk.free_gaps());
    }

    #[test]
    fn a_partition_is_matched_by_node_or_by_partuuid_symlink() {
        let part = Partition {
            partuuid: Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_owned()),
            ..partition(4, 0, 1)
        };
        assert!(part.is_source_of("/dev/sda4"));
        assert!(part.is_source_of("/dev/disk/by-partuuid/AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE"));
        assert!(!part.is_source_of("/dev/sda3"));
        assert!(!part.is_source_of("tmpfs"));
    }

    #[test]
    fn a_live_payload_still_on_the_medium_is_detected() {
        let on_medium = probe(&machine(FINDMNT_JSON_ON_MEDIUM)).expect("probe");
        let in_ram = probe(&machine(FINDMNT_JSON_IN_RAM)).expect("probe");

        let staging = |s: &SystemSnapshot| {
            s.find_partition("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
                .expect("staging")
                .1
                .clone()
        };

        assert!(!on_medium.payload_is_in_ram(&staging(&on_medium)));
        assert_eq!(on_medium.mounts_backed_by(&staging(&on_medium)).len(), 1);

        assert!(in_ram.payload_is_in_ram(&staging(&in_ram)));
        assert!(in_ram.mounts_backed_by(&staging(&in_ram)).is_empty());
    }

    #[test]
    fn partition_numbering_skips_what_is_in_use() {
        let snapshot = probe(&machine(FINDMNT_JSON_IN_RAM)).expect("probe");
        let disk = &snapshot.disks[0];
        assert_eq!(disk.next_partition_number(), 5);
        assert_eq!(disk.partition_path(5), "/dev/sda5");
    }

    #[test]
    fn an_nvme_partition_node_takes_a_p() {
        let disk = Disk {
            path: "/dev/nvme0n1".to_owned(),
            table: "gpt".to_owned(),
            disk_guid: None,
            sector_size: 512,
            first_usable_sector: 34,
            last_usable_sector: 999,
            partitions: vec![],
        };
        assert_eq!(disk.partition_path(1), "/dev/nvme0n1p1");
        assert_eq!(partition_number("/dev/nvme0n1p12").expect("number"), 12);
        assert_eq!(partition_number("/dev/sda3").expect("number"), 3);
        assert!(partition_number("/dev/sda").is_err());
    }

    #[test]
    fn a_subvolume_source_is_reduced_to_its_device() {
        let system = machine(
            r#"{"filesystems":[{"source":"/dev/sda4[/@live]","target":"/run/archiso/bootmnt","fstype":"btrfs"}]}"#,
        );
        let snapshot = probe(&system).expect("probe");
        assert_eq!(snapshot.mounts[0].source, "/dev/sda4");
    }

    #[test]
    fn blkid_export_output_is_parsed() {
        let system = FakeSystem::new().with_command(
            "blkid --output export /dev/sda1",
            "DEVNAME=/dev/sda1\nLABEL=SYSTEM\nUUID=1234-ABCD\nTYPE=vfat\n",
        );
        let fields = blkid(&system, "/dev/sda1").expect("blkid");
        assert_eq!(fields.get("TYPE").map(String::as_str), Some("vfat"));
        assert_eq!(fields.get("UUID").map(String::as_str), Some("1234-ABCD"));
    }

    /// A future `sfdisk` that changed units would make every offset wrong by a
    /// factor of 512, so it has to be an error rather than a default.
    #[test]
    fn a_table_reported_in_anything_but_sectors_is_refused() {
        let system = machine(FINDMNT_JSON_IN_RAM).with_command(
            "sfdisk --json /dev/sda",
            r#"{"partitiontable":{"label":"gpt","unit":"bytes","sectorsize":512,"partitions":[]}}"#,
        );
        // `probe` skips a disk whose table will not read, so the failure shows
        // up as an empty disk list rather than an error — which is exactly what
        // the planner then refuses on.
        let snapshot = probe(&system).expect("probe");
        assert!(snapshot.disks.is_empty());
    }

    fn partition(number: u32, start: u64, sectors: u64) -> Partition {
        Partition {
            path: format!("/dev/sda{number}"),
            number,
            partuuid: None,
            type_guid: None,
            partlabel: None,
            start_sector: start,
            sectors,
            fstype: None,
            label: None,
        }
    }

    /// A brand-new disk has no table for `sfdisk` to read. It is still the
    /// commonest install target there is, so it is offered with the geometry
    /// the GPT the editor writes will give it.
    #[test]
    fn a_blank_disk_is_offered_with_the_geometry_of_a_new_gpt() {
        let snapshot = probe(&blank_machine(FINDMNT_JSON_IN_RAM, BLANK_LSBLK_JSON)).expect("probe");
        let disk = &snapshot.disks[0];
        assert!(disk.is_blank());
        assert!(disk.partitions.is_empty());
        assert_eq!(disk.first_usable_sector, 34);
        assert_eq!(disk.last_usable_sector, LAST_LBA);
    }

    /// A filesystem written straight onto the device has no table either, and
    /// is somebody's data rather than an empty disk.
    #[test]
    fn a_disk_formatted_without_a_table_is_not_blank() {
        let lsblk = BLANK_LSBLK_JSON.replace(r#""fstype":null"#, r#""fstype":"ext4""#);
        let snapshot = probe(&blank_machine(FINDMNT_JSON_IN_RAM, &lsblk)).expect("probe");
        assert!(snapshot.disks.is_empty());
    }

    /// A card reader with no card in it is a zero-sector disk.
    #[test]
    fn an_empty_device_is_not_a_blank_disk() {
        let system = blank_machine(FINDMNT_JSON_IN_RAM, BLANK_LSBLK_JSON)
            .with_file("/sys/block/sda/size", "0\n");
        assert!(probe(&system).expect("probe").disks.is_empty());
    }

    /// The entry array is a fixed number of bytes, so a 4Kn disk reserves fewer
    /// sectors for it than a 512-byte one.
    #[test]
    fn a_new_gpt_reserves_its_entry_array_in_the_disk_own_sectors() {
        assert_eq!(gpt_usable_range(1_000, 512), Some((34, 966)));
        assert_eq!(gpt_usable_range(1_000, 4096), Some((6, 994)));
        assert_eq!(gpt_usable_range(60, 512), None);
    }
}
