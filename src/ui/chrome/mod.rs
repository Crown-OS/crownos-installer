//! The window furniture every page shares: the backdrop, the card, the step
//! track along its foot, and the motion between pages.

mod backdrop;
mod card;
mod step_track;
mod step_transition;

pub use backdrop::backdrop;
pub use card::card;
pub use step_track::{StepMark, step_track};
pub use step_transition::step_transition;
