//! The handoff payload: everything the Linux side needs to finish the install.
//!
//! This is the contract between two programs that never run at the same time.
//! `windows-migration-assistant` writes one of these to the staging medium as
//! `oobe.json`; `crownos-installer`, booted from the staged live environment on
//! the next restart, reads it and does the actual installation.
//!
//! This crate is the *reading* half of that contract. In production it only ever
//! parses a payload — nothing here writes one. [`Oobe::to_json`] exists so the
//! golden test below can pin the on-disk shape from this side as well, which is
//! the whole point of the duplication.
//!
//! # This file is duplicated on purpose
//!
//! An identical copy of this module lives in
//! `windows-migration-assistant/src/oobe.rs`, and that is the copy that writes the
//! file. The alternative — a shared crate — would put a dependency edge between a
//! Windows-only binary and a Linux-only one, and drag one platform's build graph
//! onto the other for the sake of ~200 lines of `serde` structs. Instead both
//! copies carry the same [`SCHEMA_VERSION`] and the same golden-JSON test, so a
//! field that changes on one side without the other agreeing fails that side's
//! `cargo test` rather than failing silently on a user's disk at 3am.
//!
//! When you change anything here, change it in both places and update both
//! golden tests.
//!
//! # Why JSON
//!
//! CrownOS configuration is RON everywhere else (see `crownos-config`). This one
//! file is JSON because of who reads it: the payload sits on a FAT32 staging
//! medium and may need to be inspected from an initramfs shell, or by a recovery
//! script, or by a human with nothing but `busybox` — and every one of those can
//! already read JSON. RON would buy consistency at the cost of the one property
//! this file actually needs, which is being readable by anything.
//!
//! # What is deliberately not in here
//!
//! No password, in any form. The payload lives on a FAT32 partition with no
//! permissions worth the name, readable by any OS that can see the disk — so it
//! is the wrong place for a credential even hashed. [`UserAccount`] therefore
//! carries no password field at all, and the Linux installer prompts for one.
//! [`Oobe::user`] exists so an unattended deployment can pre-fill the *rest* of
//! the account details and skip that page.

use serde::{Deserialize, Serialize};

/// The version of this schema.
///
/// Bump it when a change is not backward compatible — a removed field, a renamed
/// one, or a changed meaning. Adding an optional field does not need a bump,
/// because [`Oobe::is_supported`] accepts anything up to and including the
/// version it was compiled with.
///
/// The Linux side refuses to act on a payload from a *newer* schema than it
/// understands. That is the safe direction to fail: a stale ISO booting a
/// payload it only half-understands is how a disk gets repartitioned wrongly.
pub const SCHEMA_VERSION: u32 = 1;

/// The file name this payload is written as, on whichever medium is staged.
pub const FILE_NAME: &str = "oobe.json";

// --- MARK: Root ---

/// The complete handoff payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Oobe {
    /// See [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Name and version of the program that wrote this, for diagnostics in the
    /// install log. Not interpreted.
    pub generated_by: String,
    /// Unix seconds at which this payload was written. Purely diagnostic — the
    /// Linux side must not make decisions from it, since a machine whose clock
    /// is wrong is common and not an error.
    pub generated_at_unix: u64,
    /// What to install and how much room it gets.
    pub install: Install,
    /// Which disk, and which parts of it are off limits.
    pub target: Target,
    /// Where the live payload was staged, and what to do with it afterwards.
    pub staging: Staging,
    /// Pre-filled account details, when the flow collected them. `None` means
    /// the Linux installer asks. Never carries a password — see the module docs.
    pub user: Option<UserAccount>,
    /// Language, keyboard and clock.
    pub locale: Locale,
}

impl Oobe {
    /// Whether this build can safely act on `self`.
    ///
    /// Older payloads are fine; newer ones are refused, because acting on a
    /// schema you only partly understand is how the wrong partition gets
    /// erased.
    pub fn is_supported(&self) -> bool {
        self.schema_version <= SCHEMA_VERSION
    }
}

// --- MARK: Install ---

/// What the user asked for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Install {
    /// Proceed without asking again on the Linux side.
    ///
    /// `false` means the staged installer shows its own confirmation before it
    /// touches the disk — the right default for a flow where the user has been
    /// away from the machine since it rebooted.
    pub auto_install: bool,
    /// How much room CrownOS gets, in bytes.
    ///
    /// This is what the user chose, and it is the authoritative number. It is
    /// *not* necessarily the size of the free region: shrinking is quantized by
    /// Windows and by the partition alignment, so the region actually freed is
    /// recorded separately in [`Target::expected_free_bytes`].
    pub allocated_bytes: u64,
    /// How to provide swap.
    pub swap: SwapPlan,
    /// The root filesystem.
    pub root_filesystem: RootFilesystem,
    /// Whether the Windows install must be preserved.
    ///
    /// Always `true` in the dual-boot flow this schema exists for. It is written
    /// out explicitly rather than assumed so that a payload which somehow lost
    /// its meaning cannot be read as "wipe the disk" by default.
    pub keep_windows: bool,
}

/// How swap is provided.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SwapPlan {
    /// No swap at all.
    None,
    /// A swapfile inside the root filesystem, `size_bytes` large. Preferred over
    /// a partition: it can be resized later without touching the partition
    /// table, which on a disk shared with Windows is worth a lot.
    File { size_bytes: u64 },
    /// A dedicated swap partition carved out of the allocated region.
    Partition { size_bytes: u64 },
}

/// The filesystem for the root partition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootFilesystem {
    /// The default. Boring, universally recoverable, and what a dual-boot user
    /// hunting for a rescue tool is most likely to find support for.
    #[default]
    Ext4,
    /// Subvolumes and snapshots, for a user who asked for them.
    Btrfs,
}

// --- MARK: Target ---

/// The disk being installed to, described in terms that survive the reboot.
///
/// Every identifier here is a GPT GUID rather than a device name. `\\.\PhysicalDrive0`
/// and `/dev/sda` are two different namings of a thing that may not even
/// enumerate in the same order twice, and picking the wrong one is unrecoverable.
/// A partition GUID is the same 16 bytes to Windows, to Linux and to the
/// firmware.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target {
    /// PARTUUID of the EFI System Partition — lowercase, unbraced, e.g.
    /// `"0fc63daf-8483-4772-8e79-3d69d8477de4"`.
    ///
    /// This is the anchor for everything: the Linux side finds this partition,
    /// and the disk it belongs to *is* the target disk. It is also the ESP the
    /// CrownOS bootloader is installed into, so Windows and CrownOS share one.
    pub esp_partuuid: String,
    /// Size of the ESP in bytes. The Linux side warns rather than fails when
    /// this is too small to hold a second bootloader and its kernels.
    pub esp_size_bytes: u64,
    /// PARTUUID of the Windows system partition (the shrunk `C:`).
    ///
    /// Recorded so the Linux installer can refuse, explicitly, to touch it —
    /// a denylist is worth more than an allowlist when the failure mode is
    /// destroying the user's other operating system.
    pub windows_partuuid: String,
    /// GPT disk GUID, when it could be read. Diagnostic and cross-check only;
    /// [`esp_partuuid`](Self::esp_partuuid) is what actually locates the disk.
    pub disk_guid: Option<String>,
    /// The Windows disk number the ESP was found on. Diagnostic only — it has no
    /// meaning under Linux.
    pub windows_disk_number: u32,
    /// Byte offset at which the freed region is expected to start.
    ///
    /// The Linux side re-reads the partition table rather than trusting this;
    /// it is here so a mismatch can be *reported* — "the disk is not in the
    /// state we left it in" is a much better failure than partitioning blind.
    pub expected_free_start_bytes: u64,
    /// Expected size of the freed region, in bytes. Same role as
    /// [`expected_free_start_bytes`](Self::expected_free_start_bytes).
    pub expected_free_bytes: u64,
    /// The disk's logical sector size, in bytes — 512 or 4096.
    ///
    /// Needed to convert the byte offsets above into the LBA units that
    /// partition tables and UEFI device paths are actually expressed in.
    pub logical_sector_size: u32,
}

// --- MARK: Staging ---

/// Where the live environment was put, and what becomes of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Staging {
    /// Which medium holds the payload.
    pub kind: StagingKind,
    /// PARTUUID of the partition the payload was written to — lowercase,
    /// unbraced. For [`StagingKind::Esp`] this equals
    /// [`Target::esp_partuuid`].
    ///
    /// This is what the kernel command line's `archisodevice=` resolves to, and
    /// what the installer must avoid writing to until the payload is in RAM.
    pub partuuid: String,
    /// Directory on that partition holding `<base_dir>/x86_64/airootfs.sfs`,
    /// as a forward-slashed relative path — e.g. `"EFI/ArchStage"`.
    ///
    /// Passed to the kernel as `archisobasedir`. Forward slashes even though the
    /// writing side is Windows: this string is consumed by Linux.
    pub base_dir: String,
    /// Whether the staging partition should be reclaimed once the install is
    /// done.
    ///
    /// `true` for [`StagingKind::Partition`] — it was carved out of the space the
    /// user allocated to CrownOS, so leaving it behind would quietly cost them a
    /// gigabyte. Reclaiming it is only safe because the payload is copied to RAM
    /// at boot (`copytoram=y`); an installer that finds it is still running from
    /// the medium must refuse.
    ///
    /// Always `false` for [`StagingKind::Esp`], where deleting the partition
    /// would destroy the machine's ability to boot anything.
    pub reclaim_after_install: bool,
}

/// Which medium the payload was staged onto.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StagingKind {
    /// Everything sits in a directory on the EFI System Partition.
    ///
    /// The tidiest layout, and the one the design was written around — but it
    /// needs an ESP with roughly a gigabyte free, and a Windows ESP is usually
    /// 100 MB. Used only when the ESP genuinely has the room.
    Esp,
    /// A dedicated FAT32 partition created in the space freed by shrinking
    /// Windows, holding the whole payload.
    ///
    /// The common case, for the reason above. The EFI binary the firmware loads
    /// still lives on the ESP; only the bulk is out here.
    Partition,
}

// --- MARK: User ---

/// Account details collected up front, so the Linux side can skip asking.
///
/// Carries no password: see the module docs for why. An installer that receives
/// a `UserAccount` still prompts for the password, and nothing else.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserAccount {
    /// Unix login name. The Linux side validates it; this side does not pretend
    /// to know the rules.
    pub username: String,
    /// Display name for the account, if given.
    pub full_name: Option<String>,
    /// Machine hostname.
    pub hostname: String,
    /// Whether to log this user in automatically.
    pub autologin: bool,
}

// --- MARK: Locale ---

/// Language, keyboard and clock.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Locale {
    /// glibc locale, e.g. `"en_US.UTF-8"`.
    pub locale: String,
    /// Console keymap, e.g. `"us"`.
    pub keymap: String,
    /// tzdata zone, e.g. `"Europe/Berlin"`.
    pub timezone: String,
}

impl Default for Locale {
    /// A neutral, always-present fallback. `UTC` rather than a guess: a wrong
    /// timezone is a visible daily annoyance, and UTC is at least unambiguously
    /// a default rather than a bad guess about where the user is.
    fn default() -> Self {
        Self {
            locale: "en_US.UTF-8".to_owned(),
            keymap: "us".to_owned(),
            timezone: "UTC".to_owned(),
        }
    }
}

// --- MARK: Serialization ---

impl Oobe {
    /// Serialize to the exact bytes that belong in `oobe.json`.
    ///
    /// Pretty-printed with a trailing newline, because this file gets read by a
    /// human in a recovery shell more often than by anything else.
    ///
    /// This side never writes the real file — the Windows installer does. It is
    /// kept here so the golden test can assert the shape from both ends of the
    /// contract, and because echoing the parsed payload back out is the clearest
    /// thing an installer log can say about what it thinks it was asked to do.
    pub fn to_json(&self) -> serde_json::Result<String> {
        let mut json = serde_json::to_string_pretty(self)?;
        json.push('\n');
        Ok(json)
    }

    /// Parse a payload.
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A payload with every field populated, for the round-trip and golden
    /// tests. Values are deliberately distinctive so a field that gets crossed
    /// with its neighbour during a refactor shows up.
    fn sample() -> Oobe {
        Oobe {
            schema_version: SCHEMA_VERSION,
            generated_by: "windows-migration-assistant 0.1.0".to_owned(),
            generated_at_unix: 1_756_000_000,
            install: Install {
                auto_install: false,
                allocated_bytes: 128 * 1024 * 1024 * 1024,
                swap: SwapPlan::File {
                    size_bytes: 8 * 1024 * 1024 * 1024,
                },
                root_filesystem: RootFilesystem::Ext4,
                keep_windows: true,
            },
            target: Target {
                esp_partuuid: "0fc63daf-8483-4772-8e79-3d69d8477de4".to_owned(),
                esp_size_bytes: 260 * 1024 * 1024,
                windows_partuuid: "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7".to_owned(),
                disk_guid: Some("01234567-89ab-cdef-0123-456789abcdef".to_owned()),
                windows_disk_number: 0,
                expected_free_start_bytes: 400 * 1024 * 1024 * 1024,
                expected_free_bytes: 128 * 1024 * 1024 * 1024,
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
                autologin: false,
            }),
            locale: Locale {
                locale: "en_US.UTF-8".to_owned(),
                keymap: "us".to_owned(),
                timezone: "Europe/Berlin".to_owned(),
            },
        }
    }

    #[test]
    fn round_trips() {
        let original = sample();
        let json = original.to_json().expect("serialize");
        let parsed = Oobe::from_json(&json).expect("deserialize");
        assert_eq!(original, parsed);
    }

    #[test]
    fn a_payload_that_still_names_a_base_distribution_is_read() {
        let mut json: serde_json::Value =
            serde_json::from_str(&sample().to_json().expect("serialize")).expect("valid json");
        json["install"]["distro"] = "arch".into();
        let parsed: Oobe = serde_json::from_value(json).expect("deserialize");
        assert_eq!(parsed, sample());
    }

    #[test]
    fn a_payload_with_no_user_round_trips() {
        let mut original = sample();
        original.user = None;
        let json = original.to_json().expect("serialize");
        assert_eq!(Oobe::from_json(&json).expect("deserialize"), original);
    }

    /// Newer payloads are refused, older ones accepted. The Linux side depends
    /// on this direction being the one that fails.
    #[test]
    fn only_known_schema_versions_are_supported() {
        let mut oobe = sample();
        assert!(oobe.is_supported());
        oobe.schema_version = SCHEMA_VERSION + 1;
        assert!(!oobe.is_supported(), "a newer schema must be refused");
        oobe.schema_version = 0;
        assert!(oobe.is_supported(), "an older schema stays readable");
    }

    /// The golden shape. This is the cross-repo contract: the identical test in
    /// `windows-migration-assistant` pins this exact string, so a field renamed on
    /// one side without the other fails there too.
    ///
    /// If this test fails because you meant to change the schema, update the
    /// literal here *and* in `windows-migration-assistant/src/oobe.rs`, and bump
    /// [`SCHEMA_VERSION`] if the change is not backward compatible.
    #[test]
    fn the_json_shape_is_pinned() {
        let json = sample().to_json().expect("serialize");
        let got: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        let want: serde_json::Value = serde_json::from_str(GOLDEN).expect("valid golden json");
        assert_eq!(got, want, "the on-disk shape of oobe.json changed");
    }

    /// Kept as a separate const so the diff on a schema change is readable.
    const GOLDEN: &str = r#"{
      "schema_version": 1,
      "generated_by": "windows-migration-assistant 0.1.0",
      "generated_at_unix": 1756000000,
      "install": {
        "auto_install": false,
        "allocated_bytes": 137438953472,
        "swap": { "kind": "file", "size_bytes": 8589934592 },
        "root_filesystem": "ext4",
        "keep_windows": true
      },
      "target": {
        "esp_partuuid": "0fc63daf-8483-4772-8e79-3d69d8477de4",
        "esp_size_bytes": 272629760,
        "windows_partuuid": "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7",
        "disk_guid": "01234567-89ab-cdef-0123-456789abcdef",
        "windows_disk_number": 0,
        "expected_free_start_bytes": 429496729600,
        "expected_free_bytes": 137438953472,
        "logical_sector_size": 512
      },
      "staging": {
        "kind": "partition",
        "partuuid": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        "base_dir": "EFI/ArchStage",
        "reclaim_after_install": true
      },
      "user": {
        "username": "crown",
        "full_name": "Crown User",
        "hostname": "crownos",
        "autologin": false
      },
      "locale": {
        "locale": "en_US.UTF-8",
        "keymap": "us",
        "timezone": "Europe/Berlin"
      }
    }"#;

    /// A payload must never grow a password field. This is a real review
    /// mistake to make — the Windows wizard has the user's attention and it is
    /// tempting to collect everything there — and the file lands on a FAT32
    /// partition that any OS on the machine can read.
    #[test]
    fn no_credential_fields_are_serialized() {
        let json = sample().to_json().expect("serialize").to_lowercase();
        for forbidden in ["password", "passwd", "secret", "hash", "credential"] {
            assert!(
                !json.contains(forbidden),
                "oobe.json must not carry a {forbidden} field"
            );
        }
    }
}
