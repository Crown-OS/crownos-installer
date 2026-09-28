//! The partition editor's state: what is selected, what the user typed, and the
//! queue of operations built so far.
//!
//! Everything dangerous lives in [`engine::disks`](crate::engine::disks); this
//! module is the bookkeeping between a click and a [`DiskOp`]. It holds no rules
//! of its own beyond parsing a size out of a text field, which is why it can be
//! tested without a disk and without rendering anything.
//!
//! # Nothing is applied here
//!
//! Queuing an operation writes nothing. The queue becomes the first steps of the
//! plan the confirmation page shows, and runs when the install runs. That is a
//! deliberate departure from how a standalone partition editor behaves, and it
//! is worth the surprise:
//!
//! * **One confirmation for the whole thing.** A user who deletes a partition,
//!   creates two and then changes their mind has changed nothing. An editor that
//!   applied each operation as it was clicked would have already destroyed the
//!   first partition by the time they got to the third dialog.
//! * **The refusals are checked against the queue, not one at a time.**
//!   [`plan_ops`](crate::engine::disks::plan_ops) validates every operation
//!   against the table the previous ones leave, which is only possible because
//!   the queue exists as a value before any of it runs.
//! * **A dry run rehearses the editing too.** `--dry-run` prints the `parted`
//!   commands the editor would have issued, which an apply-immediately editor
//!   could not offer at all.
//!
//! [`Editor::problem`] is where a refusal ends up, so the page can say why a
//! click did nothing rather than appearing to ignore it.

use crate::engine::disks::{
    carve, fill, plan_ops, DiskOp, Filesystem, PartitionKind, Projection,
};
use crate::engine::probe::{Disk, Gap, SystemSnapshot};

/// The filesystems the editor offers, in the order they appear in the picker.
///
/// `ext4` first because it is the one a CrownOS root should be unless the user
/// has a reason; `swap` last because it is not a filesystem anyone browses.
pub const FILESYSTEMS: &[Filesystem] = &[
    Filesystem::Ext4,
    Filesystem::Btrfs,
    Filesystem::Fat32,
    Filesystem::Xfs,
    Filesystem::Swap,
];

/// What the editor offers to make a new partition *as*.
pub const KINDS: &[PartitionKind] = &[
    PartitionKind::LinuxRoot,
    PartitionKind::EfiSystem,
    PartitionKind::LinuxSwap,
    PartitionKind::LinuxData,
];

/// One row of the editor's list: something that is on the disk, or a hole where
/// nothing is.
///
/// Free space is a row rather than a gap between rows because it is a thing the
/// user acts on — "create a partition here" needs a *here* to select.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Partition {
        path: String,
        number: u32,
        start_sector: u64,
        end_sector: u64,
        size_bytes: u64,
        fstype: Option<String>,
        label: Option<String>,
        partuuid: Option<String>,
        /// Whether this row is only in the list because a queued `Create` put it
        /// there. Those cannot be edited further — they have no node yet, so
        /// there is nothing for `parted` to resize and nothing for `mkfs` to
        /// format until the queue has run.
        pending: bool,
    },
    Free {
        start_sector: u64,
        end_sector: u64,
        size_bytes: u64,
    },
}

impl Row {
    pub fn is_free(&self) -> bool {
        matches!(self, Self::Free { .. })
    }
}

/// The editor.
#[derive(Clone, Debug, Default)]
pub struct Editor {
    /// Index into the row list the page last drew. Cleared whenever the queue
    /// changes, because the row it pointed at may not exist any more.
    pub selected: Option<usize>,
    /// Size for a new or resized partition, as typed. Empty means "all of it" —
    /// which is what a user creating one partition in one gap almost always
    /// wants, and is the default so that the common case takes no typing.
    pub size_text: String,
    /// Index into [`FILESYSTEMS`].
    pub filesystem: usize,
    /// Index into [`KINDS`].
    pub kind: usize,
    /// The operations queued so far, in order.
    pub ops: Vec<DiskOp>,
    /// Why the last attempt did nothing. Cleared by the next successful action.
    pub problem: Option<String>,
}

impl Editor {
    pub fn filesystem(&self) -> Filesystem {
        FILESYSTEMS
            .get(self.filesystem)
            .copied()
            .unwrap_or(Filesystem::Ext4)
    }

    pub fn kind(&self) -> PartitionKind {
        KINDS
            .get(self.kind)
            .copied()
            .unwrap_or(PartitionKind::LinuxData)
    }

    pub fn reset_selection(&mut self) {
        self.selected = None;
        self.problem = None;
    }

    /// The size the user typed, in bytes, or `None` for "all of it".
    ///
    /// Parsed as gibibytes, with a suffix accepted but not required: a partition
    /// editor's numbers are in GiB in every UI anyone has used, and making the
    /// unit implicit means the common case is three keystrokes. A value that is
    /// not a number at all is `None` rather than an error, so an in-progress
    /// "1." while typing does not flash a complaint.
    pub fn requested_bytes(&self) -> Option<u64> {
        let text = self.size_text.trim();
        if text.is_empty() {
            return None;
        }
        let digits: String = text
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let value: f64 = digits.parse().ok()?;
        if value <= 0.0 {
            return None;
        }
        let unit: String = text[digits.len()..].trim().to_ascii_lowercase();
        let multiplier: f64 = match unit.as_str() {
            "" | "g" | "gb" | "gib" => 1024.0 * 1024.0 * 1024.0,
            "m" | "mb" | "mib" => 1024.0 * 1024.0,
            "t" | "tb" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
            _ => return None,
        };
        Some((value * multiplier) as u64)
    }

    /// The disk as the queue will leave it, or the refusal that stops it.
    ///
    /// Every list the editor draws comes from here rather than from the disk, so
    /// what is on screen is always the table the next operation will be checked
    /// against.
    pub fn projection(
        &self,
        disk: &Disk,
        snapshot: &SystemSnapshot,
    ) -> Result<Projection, String> {
        plan_ops(disk, snapshot, &self.ops).map_err(|refusal| refusal.to_string())
    }

    /// The rows to draw: every partition in the projected table and every hole
    /// between them, in disk order.
    pub fn rows(&self, disk: &Disk, snapshot: &SystemSnapshot) -> Vec<Row> {
        let Ok(projection) = self.projection(disk, snapshot) else {
            return Vec::new();
        };
        let projected = &projection.disk;
        // A partition present in the projection but not on the real disk was put
        // there by a queued `Create`, and cannot be operated on until the queue
        // has run — see `Row::pending`.
        let existing: Vec<&str> = disk.partitions.iter().map(|p| p.path.as_str()).collect();

        let mut rows: Vec<Row> = projected
            .partitions
            .iter()
            .map(|partition| Row::Partition {
                path: partition.path.clone(),
                number: partition.number,
                start_sector: partition.start_sector,
                end_sector: partition.end_sector(),
                size_bytes: projected.bytes(partition.sectors),
                fstype: partition.fstype.clone(),
                label: partition.label.clone().or_else(|| partition.partlabel.clone()),
                partuuid: partition.partuuid.clone(),
                pending: !existing.contains(&partition.path.as_str()),
            })
            .collect();

        rows.extend(projected.free_gaps().into_iter().map(|gap| Row::Free {
            start_sector: gap.start_sector,
            end_sector: gap.end_sector(),
            size_bytes: projected.bytes(gap.sectors),
        }));

        rows.sort_by_key(|row| match row {
            Row::Partition { start_sector, .. } | Row::Free { start_sector, .. } => *start_sector,
        });
        rows
    }

    /// The selected row, if the selection still points at one.
    pub fn selection(&self, disk: &Disk, snapshot: &SystemSnapshot) -> Option<Row> {
        let rows = self.rows(disk, snapshot);
        rows.get(self.selected?).cloned()
    }

    // ---- Queuing ----

    /// Queue a new partition in the selected free space.
    ///
    /// The size comes from [`requested_bytes`](Self::requested_bytes); with none
    /// given the partition fills the gap. Either way the range is aligned by
    /// [`carve`] / [`fill`], which the ISO planner also uses — so a partition the
    /// editor makes lands exactly where a partition the planner makes would.
    pub fn create(&mut self, disk: &Disk, snapshot: &SystemSnapshot) {
        let Some(Row::Free {
            start_sector,
            end_sector,
            ..
        }) = self.selection(disk, snapshot)
        else {
            return self.complain("Select some free space to create a partition in.");
        };
        let gap = Gap {
            start_sector,
            sectors: end_sector - start_sector + 1,
        };
        let range = match self.requested_bytes() {
            Some(bytes) => carve(disk, gap, bytes),
            None => fill(disk, gap),
        };
        let Some((start, end)) = range else {
            return self.complain(
                "That is more space than this region has, once the partition has been \
                 aligned to a megabyte boundary at each end.",
            );
        };

        // Numbered against the projection so that two `Create`s queued in a row
        // do not both claim the same number.
        let Ok(projection) = self.projection(disk, snapshot) else {
            return self.complain("The queued changes are no longer valid.");
        };
        let number = projection.disk.next_partition_number();
        let kind = self.kind();
        let filesystem = match kind {
            // An ESP that is not FAT32 is not an ESP, and swap is not a
            // filesystem the picker's choice should be able to contradict.
            PartitionKind::EfiSystem => Filesystem::Fat32,
            PartitionKind::LinuxSwap => Filesystem::Swap,
            _ => self.filesystem(),
        };
        self.try_push(
            disk,
            snapshot,
            DiskOp::Create {
                disk: disk.path.clone(),
                number,
                path: projection.disk.partition_path(number),
                start_sector: start,
                end_sector: end,
                sector_size: disk.sector_size,
                kind,
                name: kind.label().to_owned(),
                format: Some(filesystem),
                fs_label: None,
            },
        );
    }

    /// Queue the deletion of the selected partition.
    pub fn delete(&mut self, disk: &Disk, snapshot: &SystemSnapshot) {
        let Some(Row::Partition {
            path,
            number,
            partuuid,
            size_bytes,
            pending,
            ..
        }) = self.selection(disk, snapshot)
        else {
            return self.complain("Select a partition to delete.");
        };
        if pending {
            return self.complain(
                "That partition has not been created yet. Undo the change that created \
                 it instead.",
            );
        }
        self.try_push(
            disk,
            snapshot,
            DiskOp::Delete {
                disk: disk.path.clone(),
                number,
                path,
                partuuid,
                size_bytes,
            },
        );
    }

    /// Queue a resize of the selected partition to the typed size.
    pub fn resize(&mut self, disk: &Disk, snapshot: &SystemSnapshot) {
        let Some(Row::Partition {
            path,
            number,
            start_sector,
            end_sector,
            fstype,
            pending,
            ..
        }) = self.selection(disk, snapshot)
        else {
            return self.complain("Select a partition to resize.");
        };
        if pending {
            return self.complain(
                "That partition has not been created yet. Undo the change that created \
                 it and create it at the size you want.",
            );
        }
        let Some(wanted) = self.requested_bytes() else {
            return self.complain("Type the size the partition should be, in GiB.");
        };
        let sectors = wanted.div_ceil(u64::from(disk.sector_size));
        let new_end = start_sector + sectors.saturating_sub(1);
        self.try_push(
            disk,
            snapshot,
            DiskOp::Resize {
                disk: disk.path.clone(),
                number,
                path,
                sector_size: disk.sector_size,
                start_sector,
                old_end_sector: end_sector,
                new_end_sector: new_end,
                fstype,
            },
        );
    }

    /// Queue a reformat of the selected partition.
    pub fn format(&mut self, disk: &Disk, snapshot: &SystemSnapshot) {
        let Some(Row::Partition { path, pending, .. }) = self.selection(disk, snapshot) else {
            return self.complain("Select a partition to format.");
        };
        if pending {
            return self.complain(
                "That partition has not been created yet, and is already going to be \
                 formatted when it is.",
            );
        }
        let filesystem = self.filesystem();
        self.try_push(
            disk,
            snapshot,
            DiskOp::Format {
                path,
                filesystem,
                fs_label: None,
            },
        );
    }

    /// Drop the last queued operation.
    ///
    /// Only the last, and only one at a time. Operations after it were validated
    /// against the table it leaves, so removing one from the middle would leave a
    /// queue whose remaining operations were checked against a disk state that
    /// never happens.
    pub fn undo(&mut self) {
        self.ops.pop();
        self.reset_selection();
    }

    pub fn clear(&mut self) {
        self.ops.clear();
        self.reset_selection();
    }

    /// Add an operation unconditionally. Private, because every path into the
    /// queue has to go through [`try_push`](Self::try_push) — an operation that
    /// was not checked is an operation whose refusal surfaces three pages later
    /// as "the queued changes are no longer valid" instead of next to the button
    /// that caused it.
    fn push(&mut self, op: DiskOp) {
        self.ops.push(op);
        self.problem = None;
        // The selection indexed a list that has just changed shape.
        self.selected = None;
    }

    /// Queue `op` only if the whole queue is still legal with it, reporting why
    /// if it is not.
    ///
    /// The one door into the queue. Every one of the four actions above comes
    /// through here, so a refusal is shown next to the button that caused it
    /// while the user still remembers what they clicked — rather than surfacing
    /// as a stale-projection error on a later page.
    pub fn try_push(&mut self, disk: &Disk, snapshot: &SystemSnapshot, op: DiskOp) -> bool {
        let mut candidate = self.ops.clone();
        candidate.push(op.clone());
        match plan_ops(disk, snapshot, &candidate) {
            Ok(_) => {
                self.push(op);
                true
            }
            Err(refusal) => {
                self.problem = Some(refusal.to_string());
                false
            }
        }
    }

    fn complain(&mut self, message: &str) {
        self.problem = Some(message.to_owned());
    }
}

// --- MARK: Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::probe::{self, fixture};

    fn machine() -> (Disk, SystemSnapshot) {
        let snapshot =
            probe::probe(&fixture::machine(fixture::FINDMNT_JSON_IN_RAM)).expect("probe");
        (snapshot.disks[0].clone(), snapshot)
    }

    /// A gibibyte is the unit a partition editor's numbers are in, so it is the
    /// one an unsuffixed number means. The suffixes are accepted because someone
    /// will type them.
    #[test]
    fn a_size_is_gibibytes_unless_it_says_otherwise() {
        let mut editor = Editor::default();
        let bytes = |editor: &Editor| editor.requested_bytes();

        editor.size_text = "40".to_owned();
        assert_eq!(bytes(&editor), Some(40 * 1024 * 1024 * 1024));
        editor.size_text = "40 GiB".to_owned();
        assert_eq!(bytes(&editor), Some(40 * 1024 * 1024 * 1024));
        editor.size_text = "512M".to_owned();
        assert_eq!(bytes(&editor), Some(512 * 1024 * 1024));
        editor.size_text = "1.5".to_owned();
        assert_eq!(bytes(&editor), Some(1024 * 1024 * 1024 * 3 / 2));
        editor.size_text = "2 TiB".to_owned();
        assert_eq!(bytes(&editor), Some(2 * 1024_u64.pow(4)));
    }

    /// Empty means "all of it", and a half-typed number is not a complaint.
    #[test]
    fn an_empty_or_unfinished_size_means_no_size_rather_than_an_error() {
        let mut editor = Editor::default();
        for text in ["", "   ", ".", "abc", "0", "-4"] {
            editor.size_text = text.to_owned();
            assert_eq!(editor.requested_bytes(), None, "{text:?}");
        }
    }

    /// The rows are the *projected* table: what is there now plus what the queue
    /// will do, in disk order, with the holes between them.
    #[test]
    fn the_rows_are_the_projected_table_in_disk_order() {
        let (disk, snapshot) = machine();
        let editor = Editor::default();
        let rows = editor.rows(&disk, &snapshot);

        // Four partitions plus the one trailing gap.
        assert_eq!(rows.len(), 5, "{rows:#?}");
        assert!(matches!(rows[0], Row::Partition { number: 1, .. }));
        assert!(rows[4].is_free());

        let starts: Vec<u64> = rows
            .iter()
            .map(|row| match row {
                Row::Partition { start_sector, .. } | Row::Free { start_sector, .. } => {
                    *start_sector
                }
            })
            .collect();
        let mut sorted = starts.clone();
        sorted.sort_unstable();
        assert_eq!(starts, sorted);
    }

    /// Queuing a delete changes what the list shows, immediately, without
    /// writing anything.
    #[test]
    fn queuing_a_delete_frees_its_space_in_the_projection() {
        let (disk, snapshot) = machine();
        let mut editor = Editor::default();

        // Select `/dev/sda4`, the CROWNSTAGE partition, and delete it.
        let rows = editor.rows(&disk, &snapshot);
        let index = rows
            .iter()
            .position(|row| matches!(row, Row::Partition { number: 4, .. }))
            .expect("sda4 is in the list");
        editor.selected = Some(index);
        editor.delete(&disk, &snapshot);

        assert_eq!(editor.ops.len(), 1);
        assert_eq!(editor.problem, None);
        let rows = editor.rows(&disk, &snapshot);
        assert!(
            !rows
                .iter()
                .any(|row| matches!(row, Row::Partition { number: 4, .. })),
            "the deleted partition should be gone from the list"
        );
        // Its space has joined the gap that followed it.
        let free: Vec<&Row> = rows.iter().filter(|row| row.is_free()).collect();
        assert_eq!(free.len(), 1, "{free:#?}");
        match free[0] {
            Row::Free { size_bytes, .. } => assert!(
                *size_bytes > 95 * 1024 * 1024 * 1024,
                "the freed gigabyte should have joined the region after it: {size_bytes}"
            ),
            other => panic!("expected free space, got {other:?}"),
        }
    }

    /// Two creates in a row must not both claim partition number 5.
    #[test]
    fn consecutive_creates_take_different_partition_numbers() {
        let (disk, snapshot) = machine();
        let mut editor = Editor::default();
        editor.kind = KINDS
            .iter()
            .position(|k| *k == PartitionKind::LinuxData)
            .expect("data");

        for _ in 0..2 {
            let rows = editor.rows(&disk, &snapshot);
            let index = rows
                .iter()
                .position(Row::is_free)
                .expect("some free space");
            editor.selected = Some(index);
            editor.size_text = "20".to_owned();
            editor.create(&disk, &snapshot);
        }

        assert_eq!(editor.ops.len(), 2, "{:?}", editor.problem);
        let numbers: Vec<u32> = editor
            .ops
            .iter()
            .map(|op| match op {
                DiskOp::Create { number, .. } => *number,
                other => panic!("expected creates, got {other:?}"),
            })
            .collect();
        assert_eq!(numbers, vec![5, 6]);
    }

    /// A partition that exists only because of a queued create has no device
    /// node yet, so nothing can be done to it but undo.
    #[test]
    fn a_pending_partition_cannot_be_edited_further() {
        let (disk, snapshot) = machine();
        let mut editor = Editor::default();

        let index = editor
            .rows(&disk, &snapshot)
            .iter()
            .position(Row::is_free)
            .expect("free space");
        editor.selected = Some(index);
        editor.size_text = "20".to_owned();
        editor.create(&disk, &snapshot);

        let rows = editor.rows(&disk, &snapshot);
        let pending = rows
            .iter()
            .position(|row| matches!(row, Row::Partition { pending: true, .. }))
            .expect("the new partition is pending");
        editor.selected = Some(pending);

        editor.delete(&disk, &snapshot);
        assert_eq!(editor.ops.len(), 1, "nothing should have been queued");
        assert!(
            editor
                .problem
                .as_deref()
                .is_some_and(|p| p.contains("not been created yet")),
            "{:?}",
            editor.problem
        );
    }

    /// An action with nothing selected says so rather than doing nothing.
    #[test]
    fn an_action_with_no_selection_explains_itself() {
        let (disk, snapshot) = machine();
        let mut editor = Editor::default();
        for action in [Editor::create, Editor::delete, Editor::resize, Editor::format] {
            editor.problem = None;
            action(&mut editor, &disk, &snapshot);
            assert!(editor.problem.is_some(), "an action must complain");
            assert!(editor.ops.is_empty());
        }
    }

    /// `try_push` is the door a refusal comes back through, so the operation is
    /// not queued and the reason is kept.
    #[test]
    fn a_refused_operation_is_not_queued_and_says_why() {
        let (disk, snapshot) = machine();
        let mut editor = Editor::default();
        // Delete the ESP, which is fine, and then delete it again, which names a
        // partition that is no longer in the projection.
        let esp = DiskOp::Delete {
            disk: "/dev/sda".to_owned(),
            number: 1,
            path: "/dev/sda1".to_owned(),
            partuuid: None,
            size_bytes: 0,
        };
        assert!(editor.try_push(&disk, &snapshot, esp.clone()));
        assert!(!editor.try_push(&disk, &snapshot, esp));
        assert_eq!(editor.ops.len(), 1);
        assert!(editor.problem.is_some());
    }

    /// Undo removes the last operation only. See [`Editor::undo`].
    #[test]
    fn undo_removes_the_most_recent_operation() {
        let (disk, snapshot) = machine();
        let mut editor = Editor::default();
        let index = editor
            .rows(&disk, &snapshot)
            .iter()
            .position(Row::is_free)
            .expect("free space");
        editor.selected = Some(index);
        editor.size_text = "20".to_owned();
        editor.create(&disk, &snapshot);
        assert_eq!(editor.ops.len(), 1);

        editor.undo();
        assert!(editor.ops.is_empty());
        assert_eq!(editor.selected, None);
    }

    /// An ESP is FAT32 whatever the filesystem picker says, and swap is swap.
    /// Neither is a choice the picker is allowed to contradict.
    #[test]
    fn an_esp_is_always_fat32_and_swap_is_always_swap() {
        let (disk, snapshot) = machine();
        for (kind, expected) in [
            (PartitionKind::EfiSystem, Filesystem::Fat32),
            (PartitionKind::LinuxSwap, Filesystem::Swap),
        ] {
            let mut editor = Editor::default();
            // Deliberately point the filesystem picker at something else.
            editor.filesystem = FILESYSTEMS
                .iter()
                .position(|f| *f == Filesystem::Btrfs)
                .expect("btrfs");
            editor.kind = KINDS.iter().position(|k| *k == kind).expect("kind");
            let index = editor
                .rows(&disk, &snapshot)
                .iter()
                .position(Row::is_free)
                .expect("free space");
            editor.selected = Some(index);
            editor.size_text = "1".to_owned();
            editor.create(&disk, &snapshot);

            match editor.ops.first() {
                Some(DiskOp::Create { format, .. }) => assert_eq!(*format, Some(expected)),
                other => panic!("expected a create, got {other:?}"),
            }
        }
    }
}
