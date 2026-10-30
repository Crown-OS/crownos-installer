//! The wizard's pages: one module per page, each returning the [`PageParts`]
//! the card is assembled from.
//!
//! A page is built once when it opens. What changes while it is open — a list,
//! a refusal, whether a button is enabled — is a `keyed` branch or a binding
//! over the wizard signal, so typing in a field rebuilds nothing.

mod account;
mod blocked;
mod confirm;
mod disk;
mod done;
mod installing;
mod intro;
mod network;
mod partitions;
mod space;
mod welcome;

use crownui::prelude::{AnyView, Cx, Signal, View};

use super::layout::PageParts;
use super::{Page, Wizard};

pub use intro::build as intro;

/// The current page: the intro on its own, every other laid out for the card.
pub fn page(cx: &mut Cx, wizard: Signal<Wizard>, page: Page) -> AnyView {
    let parts: PageParts = match page {
        Page::Intro => return intro(wizard).boxed(),
        Page::Welcome => welcome::build(cx, wizard),
        Page::Network => network::build(cx, wizard),
        Page::Disk => disk::build(cx, wizard),
        Page::Space => space::build(wizard),
        Page::Partitions => partitions::build(cx, wizard),
        Page::Account => account::build(cx, wizard),
        Page::Confirm => confirm::build(cx, wizard),
        Page::Progress => installing::build(cx, wizard),
        Page::Done => done::build(cx, wizard),
        Page::Blocked => blocked::build(cx, wizard),
    };
    parts.into_view().boxed()
}
