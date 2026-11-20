//! Mounting the installer's window around a prepared wizard and recording what it shows.

use anyhow::{Context as _, ensure};
use capture::{CaptureApp, Carrier};
use crowntest::{Query, Role, role};
use crownui::kit::provide_theme;
use crownui::prelude::component;

use crate::engine::exec::Mode;
use crate::ui::palette::oobe_theme;
use crate::ui::root::shell;
use crate::ui::{Context, Page, Wizard, app_config};

/// Where the pointer rests while a frame is taken, clear of every control.
const POINTER_REST: (f32, f32) = (2.0, 2.0);

/// The wizard as the binary builds it from `context`, moved to `page`.
pub fn on(context: Context, mode: Mode, page: Page) -> Wizard {
    let mut wizard = Wizard::ready(context, mode);
    wizard.go(page);
    wizard
}

/// Fill in a valid account, as the account page would. The password fields are secure, so test
/// actions cannot type into them.
pub fn sign_in(wizard: &mut Wizard) {
    wizard.form.full_name = "Maya Lindqvist".to_owned();
    wizard.form.username = "maya".to_owned();
    wizard.form.hostname = "maya-desktop".to_owned();
    wizard.form.password = "lantern-orchard-42".to_owned();
    wizard.form.confirm = "lantern-orchard-42".to_owned();
    wizard.restage_plan();
}

/// The installer window, mounted as the binary mounts it, showing `wizard`.
pub fn open(wizard: Wizard) -> anyhow::Result<CaptureApp> {
    let mut app = capture::mount_app(
        component(move |cx| {
            provide_theme(cx, oobe_theme());
            shell(cx.signal(wizard))
        }),
        &app_config(),
        None,
    )?;
    rest_pointer(&mut app);
    Ok(app)
}

pub fn rest_pointer(app: &mut CaptureApp) {
    app.pointer_move(POINTER_REST.0, POINTER_REST.1).settle();
}

/// Captures `wizard` after `drive` has acted on its window, checking `heading` is on screen so a
/// refused plan cannot quietly swap in the blocked page.
pub fn shoot(
    carrier: &mut Carrier,
    wizard: Wizard,
    (group, name, description): (&str, &str, &str),
    heading: &str,
    drive: impl FnOnce(&mut CaptureApp) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let mut app = open(wizard)?;
    drive(&mut app).with_context(|| format!("driving `{name}`"))?;
    rest_pointer(&mut app);
    ensure!(
        showing(&mut app, heading),
        "`{name}` does not show `{heading}`:\n{}",
        app.semantics_snapshot()
    );
    carrier.capture_state(&mut app, group, name, description)?;
    Ok(())
}

/// [`shoot`] with nothing to do between mounting and capturing.
pub fn still(
    carrier: &mut Carrier,
    wizard: Wizard,
    labels: (&str, &str, &str),
    heading: &str,
) -> anyhow::Result<()> {
    shoot(carrier, wizard, labels, heading, |_| Ok(()))
}

pub fn showing(app: &mut CaptureApp, words: &str) -> bool {
    app.exists(role(Role::Heading).name(words))
        || app.exists(role(Role::Group).name(words))
        || app.exists(crowntest::text(words))
}

pub fn press(app: &mut CaptureApp, button: &str) {
    app.click(role(Role::Button).name(button)).settle();
}

/// Scrolls the scroll view around `query` by `dy`, bringing what lies below its fold into view.
pub fn scroll_by(app: &mut CaptureApp, query: Query, dy: f32) {
    app.scroll(query, dy).settle();
}

/// Scrolls the page body around the alert titled `title` to its end.
pub fn scroll_page_to(app: &mut CaptureApp, title: &str) {
    scroll_by(app, role(Role::Group).name(title), PAGE_END);
}

/// Further than any page's content reaches.
pub const PAGE_END: f32 = 4000.0;

/// Clicks the middle of the `nth` node `query` matches, as a pointer would.
pub fn click_nth(app: &mut CaptureApp, query: Query, nth: usize) -> anyhow::Result<()> {
    let bounds = query
        .find_all(app.ui().semantic_tree())
        .get(nth)
        .map(|node| node.bounds)
        .context("no such node")?;
    let x = bounds.origin.x + bounds.size.width / 2.0;
    let y = bounds.origin.y + bounds.size.height / 2.0;
    app.pointer_move(x, y)
        .pointer_down(x, y)
        .pointer_up(x, y)
        .settle();
    Ok(())
}
