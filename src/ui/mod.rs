//! The kiosk wizard: the only thing on the screen, under `cage`, on a machine
//! that has either just rebooted out of Windows or just booted a CrownOS image.
//!
//! # Shape
//!
//! One [`crownuikit::layouts::hero_wizard`] — a fixed-size card centred on a
//! plain page, with the CrownOS mark down the left and the step's own column
//! down the right. The layout owns the frame; this module owns navigation,
//! because which step comes next depends on how the installer was started and on
//! what the disk turns out to look like.
//!
//! # Two flows through one card
//!
//! [`Provenance`] decides which. The pages are shared where they can be and
//! separate where the two genuinely differ:
//!
//! | | from Windows | from a flashed image |
//! |---|---|---|
//! | 1 | Welcome — what the payload decided | Disk — which disk to install on |
//! | 2 | | Space — where on it, and how |
//! | 3 | Account | Account |
//! | 4 | Confirm | Confirm |
//! | 5 | Install → Restart | Install → Restart |
//!
//! Plus two states that are not steps: [`Page::Partitions`], the partition
//! editor, reachable from either flow; and [`Page::Blocked`], where the wizard
//! stops.
//!
//! The Welcome page has no counterpart in the ISO flow and the Disk and Space
//! pages have none in the Windows flow, which is the whole difference between
//! the two: one is *reporting* decisions, the other is *taking* them. Everything
//! from the account page onwards is identical, because from there on so is the
//! install.
//!
//! # The partition editor is reachable from both
//!
//! [`Page::Partitions`] is not part of either step count, and both flows can
//! reach it. A user who came from Windows and finds it set aside the wrong
//! amount of space should not have to boot back into Windows to fix that, and a
//! user who flashed the image needs it to make a root partition at all. The
//! queued operations live in [`Editor::ops`] and become the first steps of
//! whatever plan is built afterwards — see
//! [`engine::disks`](crate::engine::disks).
//!
//! # The install runs off the UI thread
//!
//! [`crate::engine::exec::execute`] blocks for minutes — `pacstrap` alone is
//! most of that — so it runs on a blocking task and reports back through a
//! [`MessageProxy`]. The progress page mounts a [`task_raw`] alongside itself;
//! that task spawns the blocking work, forwards each [`Event`] as a message, and
//! is torn down (and the blocking work with it) if the page ever leaves the tree.
//! The UI thread never does anything slower than rendering a log line.
//!
//! # The password
//!
//! [`Oobe`](crate::oobe::Oobe) deliberately carries no password — the payload
//! sits on a FAT32 partition any OS on the machine can read. This wizard is
//! where one is collected, and it is the reason the account page cannot be
//! skipped even for an unattended install. See [`Page::first_for`].

mod editor;
mod pages;

use std::sync::Arc;

use crownuikit::config::{load_appearance, theme, themed};
use crownuikit::util::inter_font;
use xilem::dpi::LogicalSize;
use xilem::core::{fork, MessageProxy};
use xilem::style::Style;
use xilem::view::{sized_box, task_raw};
use xilem::{AnyWidgetView, EventLoop, WidgetView, WindowId, WindowView, Xilem};

use crate::cmdline::CmdlineArgs;
use crate::engine::exec::{execute, Event, Mode, Secrets};
use crate::engine::plan::{
    plan, plan_iso, Account, IsoInput, NewIds, Plan, PlanInput, Refusal,
};
use crate::engine::probe::{Disk, Partition, RealSystem, SystemSnapshot};
use crate::oobe::{Distro, Locale, Oobe, RootFilesystem, SwapPlan};
use crate::provenance::Provenance;

pub use editor::Editor;

/// The step rail's labels for an install driven by a handoff payload. Four,
/// because the payload already answered the two the ISO flow has to ask.
pub const WINDOWS_STEPS: &[&str] = &["Welcome", "Account", "Confirm", "Install"];
/// And for an install from a flashed image.
pub const ISO_STEPS: &[&str] = &["Disk", "Space", "Account", "Confirm", "Install"];

/// How many log lines the progress page keeps.
///
/// A `pacstrap` produces thousands, and a scroll region holding all of them is
/// thousands of live widgets. The tail is the part anyone reads; the whole thing
/// is in the journal.
const LOG_LIMIT: usize = 400;

/// The window a developer gets when rehearsing outside `cage`, in logical
/// pixels.
///
/// Sized around the card rather than guessed: the card is
/// [`hero_wizard`](crownuikit::layouts::hero_wizard)'s fixed size, and winit's
/// own default of 800×600 is narrower than it — which clips the right-hand
/// column rather than reflowing it.
const WINDOW_SIZE: LogicalSize<f64> = LogicalSize::new(1000.0, 700.0);
/// Below this the card is genuinely cut off, so it is the floor.
const MIN_WINDOW_SIZE: LogicalSize<f64> = LogicalSize::new(920.0, 620.0);

// --- MARK: State ---

/// Which step is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// Windows flow only: what the payload decided, before doing any of it.
    Welcome,
    /// ISO flow only: which disk.
    Disk,
    /// ISO flow only: where on it, and with what filesystem.
    Space,
    /// Either flow: the partition editor. Not a step — see the module docs.
    Partitions,
    Account,
    Confirm,
    Progress,
    Done,
    /// The machine cannot be installed to, and here is why. Not a step — it is
    /// where the wizard stops.
    Blocked,
}

impl Page {
    /// Position in the step bar, and how many stops it has.
    ///
    /// The blocked page borrows position 0 rather than getting one of its own: it
    /// is not progress, and a step bar reading "step 3 of 5" above an
    /// explanation of why there will be no step 4 says the wrong thing.
    ///
    /// [`Partitions`](Self::Partitions) is not answered here at all — it has no
    /// position of its own, because it is reached from a different step in each
    /// flow and borrowing the *wrong* one moves the bar. [`App::step_indicator`]
    /// is the caller that knows which, and it is what the pages use.
    fn step(self, provenance: &Provenance) -> (usize, usize) {
        let total = Self::labels(provenance).len();
        let index = if provenance.is_iso() {
            match self {
                Self::Disk | Self::Blocked | Self::Welcome | Self::Partitions => 0,
                Self::Space => 1,
                Self::Account => 2,
                Self::Confirm => 3,
                Self::Progress | Self::Done => 4,
            }
        } else {
            match self {
                Self::Welcome | Self::Blocked | Self::Disk | Self::Partitions => 0,
                Self::Account | Self::Space => 1,
                Self::Confirm => 2,
                Self::Progress | Self::Done => 3,
            }
        };
        (total, index.min(total.saturating_sub(1)))
    }

    fn labels(provenance: &Provenance) -> &'static [&'static str] {
        if provenance.is_iso() {
            ISO_STEPS
        } else {
            WINDOWS_STEPS
        }
    }

    /// Where a flow starts.
    ///
    /// `auto_install` asks to skip straight to the confirmation, and this is
    /// where that request is honoured as far as it can be: Welcome is skipped,
    /// and the account page is not — because a password is always needed and the
    /// payload never carries one. Landing on the account page with everything
    /// else already filled in is one field away from the confirmation, which is
    /// as close to "straight to Confirm" as an installer that will not create a
    /// passwordless account can get.
    ///
    /// A flashed image always starts at the beginning. There is nothing to skip:
    /// `auto_install` means "the questions were already answered", and on that
    /// path nobody has answered anything.
    fn first_for(provenance: &Provenance, auto_install: bool) -> Self {
        match provenance {
            Provenance::Iso => Self::Disk,
            Provenance::WindowsInstaller { .. } if auto_install => Self::Account,
            Provenance::WindowsInstaller { .. } => Self::Welcome,
        }
    }
}

// --- MARK: The ISO flow's choices ---

/// What an ISO install is going to do, as chosen so far.
///
/// The values a handoff payload would have carried. `None` means "not chosen
/// yet", which is a state the Windows flow never has and this one starts in.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub disk: Option<String>,
    pub esp: Option<String>,
    /// First sector of the free region root goes in. The region is identified by
    /// where it starts rather than by an index into a list, because a queued
    /// partition edit changes the list and must not silently change which region
    /// the user picked.
    pub region_start_sector: Option<u64>,
    pub filesystem: RootFilesystem,
    pub distro: Distro,
    pub swap: SwapPlan,
    pub locale: Locale,
}

impl Default for Target {
    fn default() -> Self {
        Self {
            disk: None,
            esp: None,
            region_start_sector: None,
            filesystem: RootFilesystem::Ext4,
            distro: Distro::default(),
            swap: SwapPlan::None,
            locale: Locale::default(),
        }
    }
}

impl Target {
    /// A first guess, so the first frame has a plan to show and the disk page
    /// opens with something selected rather than with nothing.
    ///
    /// The disk with the largest unallocated region *that already has an ESP*,
    /// because that is the one an install can proceed on without editing
    /// anything. A disk with more free space and no ESP is a perfectly good
    /// target and needs a partition created first, which is a decision for the
    /// user rather than a default.
    pub fn best_for(snapshot: &SystemSnapshot) -> Self {
        let mut best: Option<(u64, &Disk, &Partition, u64)> = None;
        for disk in &snapshot.disks {
            if disk.table != "gpt" {
                continue;
            }
            let Some(esp) = esp_of(disk) else { continue };
            let Some(gap) = disk.free_gaps().into_iter().max_by_key(|gap| gap.sectors) else {
                continue;
            };
            let bytes = disk.bytes(gap.sectors);
            if best.is_none_or(|(bytes_so_far, ..)| bytes > bytes_so_far) {
                best = Some((bytes, disk, esp, gap.start_sector));
            }
        }
        match best {
            Some((_, disk, esp, start)) => Self {
                disk: Some(disk.path.clone()),
                esp: Some(esp.path.clone()),
                region_start_sector: Some(start),
                ..Self::default()
            },
            None => Self::default(),
        }
    }

    /// Point the target at `disk`, taking its ESP and its largest free region.
    ///
    /// Changing disk clears the region rather than keeping a sector number that
    /// meant something on a different device — which would be a plan built
    /// against an offset on the wrong disk.
    fn retarget(&mut self, disk: &Disk) {
        self.disk = Some(disk.path.clone());
        self.esp = esp_of(disk).map(|esp| esp.path.clone());
        self.region_start_sector = disk
            .free_gaps()
            .into_iter()
            .max_by_key(|gap| gap.sectors)
            .map(|gap| gap.start_sector);
    }
}

/// The EFI System Partition on `disk`, if it has one.
///
/// Found by type GUID and not by filesystem: a FAT32 partition is not an ESP,
/// and firmware looks at the GUID. Compared case-insensitively because `sfdisk`
/// prints GUIDs upper case and `lsblk` prints them lower case.
pub fn esp_of(disk: &Disk) -> Option<&Partition> {
    disk.partitions.iter().find(|partition| {
        partition
            .type_guid
            .as_deref()
            .is_some_and(|guid| guid.eq_ignore_ascii_case(crate::engine::disks::TYPE_EFI_SYSTEM))
    })
}

// --- MARK: The account form ---

/// What the user types on the account page.
#[derive(Clone, Debug, Default)]
pub struct Form {
    pub username: String,
    pub full_name: String,
    pub hostname: String,
    pub password: String,
    pub confirm: String,
    /// Set once the user has tried to continue, so the page does not shout about
    /// an empty password before they have had a chance to type one.
    pub touched: bool,
}

impl Form {
    /// Pre-filled from the payload where it had something to say, and from
    /// sensible defaults where it did not.
    ///
    /// Defaults rather than blanks so that [`App::restage_plan`] always has a
    /// valid account to plan against, which is what lets the disk-level refusals
    /// be reported on the very first frame instead of after the user has filled
    /// in a form for a disk we were never going to touch.
    fn from_payload(oobe: Option<&Oobe>) -> Self {
        let user = oobe.and_then(|oobe| oobe.user.as_ref());
        Self {
            username: user.map_or_else(|| "crown".to_owned(), |u| u.username.clone()),
            full_name: user.and_then(|u| u.full_name.clone()).unwrap_or_default(),
            hostname: user.map_or_else(|| "crownos".to_owned(), |u| u.hostname.clone()),
            password: String::new(),
            confirm: String::new(),
            touched: false,
        }
    }

    fn account(&self, autologin: bool) -> Account {
        Account {
            username: self.username.trim().to_owned(),
            full_name: Some(self.full_name.trim().to_owned()).filter(|n| !n.is_empty()),
            hostname: self.hostname.trim().to_owned(),
            autologin,
        }
    }

    /// Why the account page cannot be left, if it cannot.
    pub fn problem(&self) -> Option<String> {
        if self.username.trim().is_empty() {
            return Some("Choose a username.".to_owned());
        }
        if self.hostname.trim().is_empty() {
            return Some("Give this computer a name.".to_owned());
        }
        if self.password.is_empty() {
            return Some("Choose a password.".to_owned());
        }
        if self.password != self.confirm {
            return Some("The two passwords do not match.".to_owned());
        }
        None
    }
}

// --- MARK: Context ---

/// Everything learned before the first frame.
#[derive(Debug)]
pub struct Context {
    /// Which of the two installers this is, and the payload if there is one.
    pub provenance: Provenance,
    /// What the resolver tried, and how the provenance was settled. Diagnostics.
    pub notes: Vec<String>,
    pub snapshot: SystemSnapshot,
    /// GUIDs for the partitions the plan will create. Fixed once, at startup, so
    /// the plan shown on the confirmation page names the same GUIDs the one that
    /// runs will.
    pub ids: NewIds,
    pub auto_install: bool,
}

impl Context {
    pub fn oobe(&self) -> Option<&Oobe> {
        self.provenance.oobe()
    }

    /// The disk the given path names, if the snapshot still has it.
    pub fn disk(&self, path: &str) -> Option<&Disk> {
        self.snapshot.disks.iter().find(|disk| disk.path == path)
    }
}

/// The state of a running install.
#[derive(Debug, Default)]
pub struct Run {
    pub started: bool,
    pub step: usize,
    pub total: usize,
    pub label: String,
    pub log: Vec<String>,
    pub failure: Option<String>,
    pub finished: bool,
    pub was_dry_run: bool,
}

impl Run {
    /// `0.0..=1.0`, for the progress bar.
    pub fn fraction(&self) -> f64 {
        if self.finished {
            return 1.0;
        }
        if self.total == 0 {
            return 0.0;
        }
        self.step as f64 / self.total as f64
    }
}

/// Why the wizard has stopped before it started.
#[derive(Clone, Debug)]
pub struct Blocker {
    pub title: String,
    pub detail: String,
    /// Diagnostics — what was tried, what was found.
    pub notes: Vec<String>,
}

// --- MARK: App ---

/// The wizard.
pub struct App {
    is_running: bool,
    window_id: WindowId,
    pub page: Page,
    /// `Err` when the machine cannot be installed to at all — the disks were
    /// unreadable, or a payload was found and cannot be trusted.
    pub context: Result<Context, Blocker>,
    pub form: Form,
    /// The ISO flow's choices. Present in both flows because the partition
    /// editor needs a disk to edit either way, and the Windows flow's payload
    /// tells us which one that is.
    pub target: Target,
    pub editor: Editor,
    /// The step the partition editor was opened from.
    ///
    /// The editor is reachable from a different step in each flow — Space in the
    /// ISO flow, Welcome in the Windows one — so "where does Done go back to,
    /// and what should the step bar read while it is open" has no answer that
    /// depends only on the page. Recorded on the way in rather than inferred on
    /// the way out.
    pub editor_return: Option<Page>,
    /// Filters the list on whichever page has one. One field rather than one per
    /// page: only one page is showing, and a filter that persisted from the disk
    /// page onto the partition editor would hide rows for no visible reason.
    pub search: String,
    /// The plan as of the last time something that matters changed. Recomputed
    /// rather than derived per frame, so the confirmation page is showing the
    /// same value the install will run.
    staged: Option<Result<Plan, Refusal>>,
    pub run: Run,
    pub mode: Mode,
    /// The plan handed to the worker, once the user has committed.
    committed: Option<Arc<Plan>>,
    secrets: Option<Secrets>,
}

impl App {
    /// A wizard that can install.
    pub fn ready(context: Context, mode: Mode) -> Self {
        let form = Form::from_payload(context.oobe());
        let page = Page::first_for(&context.provenance, context.auto_install);
        // Both flows get a target. On the Windows path it is derived from the
        // payload, so that opening the partition editor lands on the disk the
        // install is about rather than on a guess.
        let target = match context.oobe() {
            Some(oobe) => target_from_payload(oobe, &context.snapshot),
            None => Target::best_for(&context.snapshot),
        };
        let mut app = Self {
            is_running: true,
            window_id: WindowId::next(),
            page,
            context: Ok(context),
            form,
            target,
            editor: Editor::default(),
            editor_return: None,
            search: String::new(),
            staged: None,
            run: Run::default(),
            mode,
            committed: None,
            secrets: None,
        };
        app.restage_plan();
        // A payload-driven install can be refused before the user does anything,
        // and then it is reported before the user does anything. An ISO install
        // cannot: "no disk chosen yet" is the state it starts in, not a failure,
        // so its refusals are shown on the page that can fix them.
        if !app.provenance().is_iso() {
            if let Some(Err(refusal)) = &app.staged {
                app.context = Err(Blocker {
                    title: "CrownOS cannot be installed on this computer".to_owned(),
                    detail: refusal.to_string(),
                    notes: Vec::new(),
                });
                app.page = Page::Blocked;
            }
        }
        app
    }

    /// A wizard that can only explain itself.
    pub fn blocked(blocker: Blocker, mode: Mode) -> Self {
        Self {
            is_running: true,
            window_id: WindowId::next(),
            page: Page::Blocked,
            context: Err(blocker),
            form: Form::default(),
            target: Target::default(),
            editor: Editor::default(),
            editor_return: None,
            search: String::new(),
            staged: None,
            run: Run::default(),
            mode,
            committed: None,
            secrets: None,
        }
    }

    pub fn context(&self) -> Option<&Context> {
        self.context.as_ref().ok()
    }

    /// Which installer this is. A flashed image when there is no context at all,
    /// because a wizard with nothing but a blocker to show has no payload either.
    pub fn provenance(&self) -> &Provenance {
        static ISO: Provenance = Provenance::Iso;
        match &self.context {
            Ok(context) => &context.provenance,
            Err(_) => &ISO,
        }
    }

    /// The disk the ISO flow is targeting, or the one the payload named.
    pub fn target_disk(&self) -> Option<&Disk> {
        let context = self.context()?;
        context.disk(self.target.disk.as_deref()?)
    }

    /// Keep the chosen free region pointing at a region that is still free.
    ///
    /// A queued partition edit changes where the holes in the table are. Deleting
    /// a partition, in particular, usually *merges* the space it occupied with
    /// the gap next to it — so the region the user picked a moment ago no longer
    /// starts where it did, and [`plan_iso`] would refuse with
    /// [`Refusal::ChosenRegionNotFree`].
    ///
    /// That refusal is right when the disk changed underneath us and wrong when
    /// we changed it ourselves on the user's instruction. So the selection is
    /// re-resolved against the projection after every edit: if the chosen start
    /// is still the start of a free region it is kept, and if it is not it moves
    /// to the largest region there now. Nothing silently picks a *smaller*
    /// region than the user asked for without the confirmation page naming the
    /// partition it produces.
    fn resettle_region(&mut self) {
        if !self.provenance().is_iso() {
            return;
        }
        let Some(disk) = self.target_disk().cloned() else {
            return;
        };
        let Some(snapshot) = self.context().map(|context| context.snapshot.clone()) else {
            return;
        };
        let Ok(projection) = self.editor.projection(&disk, &snapshot) else {
            return;
        };
        let gaps = projection.free_gaps();
        let still_free = self
            .target
            .region_start_sector
            .is_some_and(|start| gaps.iter().any(|gap| gap.start_sector == start));
        if still_free {
            return;
        }
        // `free_gaps` on a `Projection` is sorted largest first.
        self.target.region_start_sector = gaps.first().map(|gap| gap.start_sector);
    }

    /// Recompute the plan from everything it depends on.
    ///
    /// The one function both flows go through, and the only place either planner
    /// is called. Which planner is a function of the provenance and nothing else.
    pub fn restage_plan(&mut self) {
        self.resettle_region();
        let Ok(context) = &self.context else {
            return;
        };
        let autologin = context
            .oobe()
            .and_then(|oobe| oobe.user.as_ref())
            .is_some_and(|user| user.autologin);
        let account = self.form.account(autologin);

        self.staged = Some(match &context.provenance {
            Provenance::WindowsInstaller { oobe, .. } => plan(&PlanInput {
                oobe,
                snapshot: &context.snapshot,
                account,
                ids: context.ids.clone(),
            }),
            Provenance::Iso => {
                let Some(disk) = self.target.disk.clone() else {
                    // Nothing chosen yet. `NoTargetDisk` names the empty string,
                    // which reads correctly ("is not a disk this installer can
                    // see") and keeps `staged` a `Result` rather than growing a
                    // third "not ready" state every caller would have to handle.
                    return self.staged = Some(Err(Refusal::NoTargetDisk {
                        disk: String::new(),
                    }));
                };
                let Some(esp) = self.target.esp.clone() else {
                    return self.staged = Some(Err(Refusal::NoEspOnDisk { disk }));
                };
                let Some(region_start_sector) = self.target.region_start_sector else {
                    return self.staged = Some(Err(Refusal::NoFreeRegion { disk }));
                };
                plan_iso(&IsoInput {
                    snapshot: &context.snapshot,
                    disk,
                    esp,
                    region_start_sector,
                    root_filesystem: self.target.filesystem,
                    distro: self.target.distro,
                    swap: self.target.swap,
                    locale: self.target.locale.clone(),
                    account,
                    ids: context.ids.clone(),
                    edits: self.editor.ops.clone(),
                })
            }
        });
    }

    /// The plan the confirmation page is describing, if there is one.
    pub fn plan(&self) -> Option<&Plan> {
        match &self.staged {
            Some(Ok(plan)) => Some(plan),
            _ => None,
        }
    }

    pub fn refusal(&self) -> Option<&Refusal> {
        match &self.staged {
            Some(Err(refusal)) => Some(refusal),
            _ => None,
        }
    }

    // ---- Navigation ----

    /// What the step bar should read: how many stops, and which one.
    ///
    /// Everything except the partition editor is a property of the page and the
    /// flow. The editor borrows the position of whichever step opened it, so
    /// that opening it does not appear to move the user forwards or backwards
    /// through an install it is not part of.
    pub fn step_indicator(&self) -> (usize, usize) {
        let provenance = self.provenance();
        match (self.page, self.editor_return) {
            (Page::Partitions, Some(from)) => from.step(provenance),
            _ => self.page.step(provenance),
        }
    }

    /// The step after the current page in this flow, if there is one.
    pub fn next_page(&self) -> Option<Page> {
        // The editor is not progress, so "next" from it is the step it
        // interrupted rather than the one after that.
        if self.page == Page::Partitions {
            return self.editor_return.or(Some(self.default_editor_return()));
        }
        Some(match (self.page, self.provenance().is_iso()) {
            (Page::Welcome, _) => Page::Account,
            (Page::Disk, true) => Page::Space,
            (Page::Space, true) => Page::Account,
            (Page::Account, _) => Page::Confirm,
            _ => return None,
        })
    }

    /// The step before the current page in this flow, if there is one.
    pub fn previous_page(&self) -> Option<Page> {
        if self.page == Page::Partitions {
            return self.editor_return.or(Some(self.default_editor_return()));
        }
        Some(match (self.page, self.provenance().is_iso()) {
            (Page::Space, true) => Page::Disk,
            (Page::Account, true) => Page::Space,
            (Page::Account, false) => Page::Welcome,
            (Page::Confirm, _) => Page::Account,
            _ => return None,
        })
    }

    /// Where the editor goes back to when it does not know where it came from —
    /// a state only reachable by a caller setting `page` directly, as a test
    /// does.
    fn default_editor_return(&self) -> Page {
        if self.provenance().is_iso() {
            Page::Space
        } else {
            Page::Welcome
        }
    }

    /// Move to the next step in this flow, if there is one.
    ///
    /// The pages' own "Next step" buttons go through here rather than naming a
    /// destination, so the two flows' orders live in exactly one place —
    /// [`next_page`](Self::next_page) — and a page shared by both flows cannot
    /// send the user to a step the other one does not have.
    pub fn advance(&mut self) {
        if let Some(next) = self.next_page() {
            self.go(next);
        }
    }

    /// Move back a step, if there is one.
    pub fn retreat(&mut self) {
        if let Some(previous) = self.previous_page() {
            self.go(previous);
        }
    }

    pub fn go(&mut self, page: Page) {
        if page != Page::Partitions {
            self.editor_return = None;
        }
        self.page = page;
        self.search.clear();
        self.restage_plan();
    }

    /// Open the partition editor on the current target disk, remembering the
    /// step to come back to.
    pub fn open_editor(&mut self) {
        self.editor.reset_selection();
        self.editor_return = Some(self.page);
        self.page = Page::Partitions;
        self.search.clear();
        self.restage_plan();
    }

    // ---- The install ----

    /// Fold one worker message into the state.
    pub fn on_install_event(&mut self, event: Event) {
        match event {
            Event::StepStarted { index, total, label } => {
                self.run.step = index;
                self.run.total = total;
                self.run.label = label;
            }
            Event::Log(line) => {
                self.run.log.push(line);
                // The tail is what anyone reads, and an unbounded log is an
                // unbounded number of live widgets.
                if self.run.log.len() > LOG_LIMIT {
                    let excess = self.run.log.len() - LOG_LIMIT;
                    self.run.log.drain(..excess);
                }
            }
            Event::StepFinished { index } => self.run.step = index + 1,
            Event::Failed { message, .. } => {
                self.run.failure = Some(message);
                self.run.finished = true;
            }
            Event::Finished { dry_run } => {
                self.run.finished = true;
                self.run.was_dry_run = dry_run;
                self.page = Page::Done;
            }
        }
    }

    /// Commit: freeze the plan and the password, and move to the progress page.
    pub fn begin_install(&mut self) {
        let Some(Ok(plan)) = self.staged.take() else {
            return;
        };
        self.secrets = Some(Secrets {
            user_password: self.form.password.clone(),
        });
        // The plan is put back so the confirmation summary still has something
        // to read from if the view is rebuilt, and shared with the worker by
        // reference count rather than cloned per frame.
        let plan = Arc::new(plan);
        self.staged = Some(Ok((*plan).clone()));
        self.committed = Some(plan);
        self.run = Run::default();
        self.run.started = true;
        self.page = Page::Progress;
    }

    pub fn restart(&mut self) {
        // `systemctl reboot` rather than `reboot`, because on a systemd system
        // the latter is a symlink whose behaviour depends on whether the unit is
        // running, and a live environment's is not always what you would guess.
        let _ = std::process::Command::new("systemctl")
            .arg("reboot")
            .status();
        self.is_running = false;
    }
}

/// The disk, ESP and free region a payload names, as a [`Target`].
///
/// So that the partition editor opens on the disk the install is about. The
/// planner still finds its own disk by PARTUUID — this is for the editor's
/// benefit and nothing decides anything from it, which is why an unresolvable
/// payload falls back to the same guess a flashed image gets rather than
/// refusing here.
fn target_from_payload(oobe: &Oobe, snapshot: &SystemSnapshot) -> Target {
    let Some((disk, esp)) = snapshot.find_partition(&oobe.target.esp_partuuid) else {
        return Target::best_for(snapshot);
    };
    Target {
        disk: Some(disk.path.clone()),
        esp: Some(esp.path.clone()),
        region_start_sector: disk
            .free_gaps()
            .into_iter()
            .max_by_key(|gap| gap.sectors)
            .map(|gap| gap.start_sector),
        filesystem: oobe.install.root_filesystem,
        distro: oobe.install.distro,
        swap: oobe.install.swap,
        locale: oobe.locale.clone(),
    }
}

impl xilem::AppState for App {
    fn keep_running(&self) -> bool {
        self.is_running
    }
}

// --- MARK: Views ---

/// The message the install worker sends the UI.
///
/// A newtype so the `AnyDebug` the proxy needs is implemented on something this
/// crate owns.
#[derive(Debug)]
struct Progress(Event);

fn app_logic(app: &mut App) -> impl WidgetView<App> + use<> {
    let frame = pages::page(app);

    // The worker lives alongside the progress page and nowhere else, so leaving
    // that page tears it down — which is what makes a cancelled install stop
    // rather than keep running behind a screen nobody is looking at.
    let worker = (app.page == Page::Progress && app.run.started && !app.run.finished)
        .then(|| install_worker(app.committed.clone(), app.secrets.clone(), app.mode));

    // `themed` keeps the palette in step with `~/.config/crownos/appearance.ron`
    // — the live environment ships one, so the installer opens in the same
    // colours the installed desktop will.
    themed(
        sized_box(fork(frame, worker))
            .expand()
            .background_color(theme().surface.bg),
    )
}

/// The element-less view that runs the install.
///
/// [`task_raw`] rather than [`task`](xilem::view::task) because this closure
/// captures the plan and the password. `task_raw`'s future is spawned once, when
/// the view enters the tree, and aborted when it leaves — which is exactly the
/// lifetime the install should have.
///
/// [`execute`] blocks, so it goes on a blocking task and talks to the async side
/// through an unbounded channel. Unbounded because the alternative is a bounded
/// channel whose `send` blocks the installer while the UI catches up, and an
/// installer that runs slower because someone is watching it is a worse trade
/// than a few kilobytes of buffered log lines.
fn install_worker(
    plan: Option<Arc<Plan>>,
    secrets: Option<Secrets>,
    mode: Mode,
) -> impl xilem::core::View<App, (), xilem::ViewCtx, Element = xilem::core::NoElement>
       + xilem::core::ViewMarker
       + use<> {
    task_raw(
        move |proxy: MessageProxy<Progress>| {
            let plan = plan.clone();
            let secrets = secrets.clone();
            async move {
                let (Some(plan), Some(secrets)) = (plan, secrets) else {
                    return;
                };
                let (tx, mut rx) = xilem::tokio::sync::mpsc::unbounded_channel();
                let worker = xilem::tokio::task::spawn_blocking(move || {
                    let system = RealSystem;
                    // The error is not propagated: every failure has already
                    // been reported as `Event::Failed`, which is what the UI
                    // shows. Returning it here would only duplicate it.
                    let _ = execute(&plan, &secrets, mode, &system, &mut |event| {
                        let _ = tx.send(event);
                    });
                });
                while let Some(event) = rx.recv().await {
                    if proxy.message(Progress(event)).is_err() {
                        // The event loop has gone; nothing left to report to.
                        return;
                    }
                }
                let _ = worker.await;
            }
        },
        |app: &mut App, Progress(event)| app.on_install_event(event),
    )
}

fn window(app: &mut App) -> impl Iterator<Item = WindowView<App>> + use<> {
    let root = app_logic(app);
    std::iter::once(
        xilem::window(app.window_id, "CrownOS Installer", root).with_options(|options| {
            options
                // Under `cage` there is no other window and no compositor
                // furniture, so a title bar would be a decoration on a
                // full-screen kiosk. Turning it off also means the window cannot
                // be closed, which for an installer mid-`pacstrap` is the point.
                .with_decorations(false)
                .with_resizable(false)
                // Big enough for the card, with room around it. Under `cage`
                // the window is full-screen and this is never consulted; it is
                // what a developer rehearsing on a desktop gets, and winit's own
                // default (800×600) is narrower than the card, which clips it.
                .with_initial_inner_size(WINDOW_SIZE)
                // The card does not reflow, so a window smaller than it shows
                // less of it rather than a rearranged version. Saying so lets a
                // compositor that would otherwise offer a tiny window know
                // better.
                .with_min_inner_size(MIN_WINDOW_SIZE)
        }),
    )
}

/// Run the wizard. Returns when the user restarts or the window closes.
///
/// `args` is not consulted here — everything the wizard needs was folded into
/// `startup` by [`crate::start_up`]. It is taken so the signature says what this
/// function is downstream of, and so a future page that wants to show the boot
/// entry it came from does not have to change the call site.
pub fn run(
    args: &CmdlineArgs,
    startup: Result<Context, Blocker>,
    mode: Mode,
) -> Result<(), winit::error::EventLoopError> {
    let _ = args;
    // Before the first frame, so the window opens in the configured palette
    // rather than repainting into it a moment later.
    load_appearance();

    let app = match startup {
        Ok(context) => App::ready(context, mode),
        Err(blocker) => App::blocked(blocker, mode),
    };

    Xilem::new(app, window)
        .with_font(inter_font())
        .run_in(EventLoop::with_user_event())
}

/// Shorthand the page module uses to keep every branch the same type.
type Boxed = Box<AnyWidgetView<App>>;

#[cfg(test)]
mod tests;
