//! The install engine: what the disk looks like, what we are going to do to it,
//! and doing it.
//!
//! The three modules are deliberately layered so that the dangerous part is the
//! boring part:
//!
//! * [`probe`] talks to the outside world and produces a [`probe::SystemSnapshot`] —
//!   a plain data description of the disks and mounts as they *actually are*.
//!   Every command it runs goes through the [`probe::System`] trait, so tests feed it
//!   recorded `lsblk`/`sfdisk`/`findmnt` output instead of a disk.
//! * [`plan`] takes an [`Oobe`](crate::oobe::Oobe) and a snapshot and produces an
//!   ordered [`plan::Plan`]. It performs no I/O whatsoever, which is what makes every
//!   safety rule in it a unit test rather than a code review. If the disk is not
//!   in a state we are willing to touch, this is where it is refused.
//! * [`distro`] is what changes between base distributions: how the base system
//!   and the CrownOS desktop get onto the disk, locales, kernels.
//! * [`exec`] walks a plan and runs it, or — in [`exec::Mode::DryRun`] — prints
//!   exactly what it would have run.
//!
//! The seam that matters is between the second and third: a [`plan::Plan`] is a value.
//! It can be printed, diffed, asserted about and shown to the user in full before
//! anything is written, and [`exec`] is not allowed to make decisions the plan
//! did not already record.
//!
//! # Credentials are not part of the plan
//!
//! [`plan::Plan`] is designed to be logged. The user's password therefore never enters
//! it: [`exec::execute`] takes a separate [`exec::Secrets`], and the one
//! command that consumes it takes it on stdin so it never appears in an argument
//! vector either. See [`exec::Cmd`].

pub mod disks;
pub mod distro;
pub mod exec;
pub mod plan;
pub mod probe;
