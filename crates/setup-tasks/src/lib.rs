//! The post-install tasks of CrownOS. The `crownos-setup` runner does each
//! task that the choices in `/etc/crownos/setup.ron` ask for, and records the
//! state of each task in `/var/lib/crownos/setup`.
//!
//! Each side effect goes through the [`System`] trait. Thus the tests replace
//! the machine with a recording fake.

pub mod config;
pub mod context;
pub mod error;
pub mod state;
pub mod tasks;
pub mod utils;

pub use context::{Output, RealSystem, System};
pub use error::TaskError;
pub use state::StateStore;
pub use tasks::{Context, PassSummary, SetupTaskRunner, run_pass};
