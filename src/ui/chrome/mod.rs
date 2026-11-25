//! The window furniture every page shares: the backdrop, the column pages are
//! laid out in, the step track along its foot, and the motion between pages.

mod backdrop;
mod column;
mod sized;
mod step_track;
mod step_transition;

pub use backdrop::backdrop;
pub use column::column;
pub use sized::sized;
pub use step_track::{StepMark, step_track};
pub use step_transition::step_transition;
