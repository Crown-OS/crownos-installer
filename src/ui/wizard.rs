//! The wizard's state and every rule about moving through it. No views: the
//! pages read this and call into it, and the tests drive it directly.

use std::sync::Arc;

use crate::engine::exec::{Event, Mode, Secrets};
use crate::engine::plan::{IsoInput, Plan, PlanInput, Refusal, plan, plan_iso};
use crate::engine::probe::Disk;
use crate::network::Network;
use crate::provenance::Provenance;

use super::editor::{PartitionAction, Row};
use super::{Blocker, Context, Editor, Form, InstallProgress, Page, Target};

const REFUSED_TITLE: &str = "CrownOS cannot be installed on this computer";

pub struct Wizard {
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
    /// The plan as of the last time something that matters changed. Recomputed
    /// rather than derived per frame, so the confirmation page is showing the
    /// same value the install will run.
    staged: Option<Result<Plan, Refusal>>,
    pub progress: InstallProgress,
    pub mode: Mode,
    pub network: Network,
    /// The plan handed to the worker, once the user has committed.
    pub committed: Option<Arc<Plan>>,
    pub secrets: Option<Secrets>,
    /// Set once the user has asked to restart: the window is on its way out.
    pub closing: bool,
}

impl Wizard {
    pub fn new(startup: Result<Context, Blocker>, mode: Mode) -> Self {
        match startup {
            Ok(context) => Self::ready(context, mode),
            Err(blocker) => Self::blocked(blocker, mode),
        }
    }

    /// A wizard that can install.
    pub fn ready(context: Context, mode: Mode) -> Self {
        let form = Form::from_payload(context.oobe());
        let page = Page::first_for(&context.provenance, context.auto_install, context.online);
        let target = match context.oobe() {
            Some(oobe) => Target::from_payload(oobe, &context.snapshot),
            None => Target::best_for(&context.snapshot),
        };
        let mut wizard = Self {
            page,
            form,
            target,
            ..Self::blocked_on(Ok(context), mode)
        };
        wizard.restage_plan();
        // A payload-driven install can be refused before the user does anything,
        // and then it is reported before the user does anything. An ISO install
        // cannot: "no disk chosen yet" is the state it starts in, not a failure,
        // so its refusals are shown on the page that can fix them.
        if !wizard.provenance().is_iso()
            && let Some(Err(refusal)) = &wizard.staged
        {
            wizard.context = Err(Blocker {
                title: REFUSED_TITLE.to_owned(),
                detail: refusal.to_string(),
                notes: Vec::new(),
            });
            wizard.page = Page::Blocked;
        }
        wizard
    }

    /// A wizard that can only explain itself.
    pub fn blocked(blocker: Blocker, mode: Mode) -> Self {
        Self::blocked_on(Err(blocker), mode)
    }

    fn blocked_on(context: Result<Context, Blocker>, mode: Mode) -> Self {
        Self {
            page: Page::Blocked,
            context,
            form: Form::default(),
            target: Target::default(),
            editor: Editor::default(),
            editor_return: None,
            staged: None,
            progress: InstallProgress::default(),
            mode,
            network: Network::default(),
            committed: None,
            secrets: None,
            closing: false,
        }
    }

    pub fn context(&self) -> Option<&Context> {
        self.context.as_ref().ok()
    }

    /// Which installer this is. A flashed image when there is no context at all,
    /// because a wizard with nothing but a blocker to show has no payload either.
    pub fn provenance(&self) -> &Provenance {
        static ISO: Provenance = Provenance::Iso;
        self.context().map_or(&ISO, |context| &context.provenance)
    }

    /// Whether the machine is online, by the startup check or the network
    /// page's own.
    pub fn online(&self) -> bool {
        self.network.online() || self.context().is_some_and(|context| context.online)
    }

    /// `page`, or the network step first when the machine is not online yet.
    fn via_network(&self, page: Page) -> Page {
        if self.online() { page } else { Page::Network }
    }

    /// The disk the ISO flow is targeting, or the one the payload named.
    pub fn target_disk(&self) -> Option<&Disk> {
        self.context()?.disk(self.target.disk.as_deref()?)
    }

    /// Point the target at another disk. A different disk means a different
    /// table, so the queued edits — every one of which names sectors on the old
    /// one — go with it.
    pub fn choose_disk(&mut self, path: &str) {
        let Some(disk) = self
            .context()
            .and_then(|context| context.disk(path))
            .cloned()
        else {
            return;
        };
        if self.target.disk.as_deref() != Some(path) {
            self.editor.clear();
        }
        self.target.retarget(&disk);
        self.restage_plan();
    }

    /// The editor's selected row, if the selection still points at one.
    pub fn selected_row(&self) -> Option<Row> {
        self.editor
            .selection(self.target_disk()?, &self.context()?.snapshot)
    }

    /// Queue a partition edit on the target disk, and re-plan: a queued
    /// operation changes the table the install is laid out against.
    pub fn edit_partitions(&mut self, action: PartitionAction) {
        let Ok(context) = &self.context else {
            return;
        };
        let Some(disk) = self
            .target
            .disk
            .as_deref()
            .and_then(|path| context.disk(path))
        else {
            return;
        };
        self.editor.apply(action, disk, &context.snapshot);
        self.restage_plan();
    }

    /// Drop the last queued edit and re-plan.
    pub fn undo_partition_edit(&mut self) {
        self.editor.undo();
        self.restage_plan();
    }

    /// Drop every queued edit and re-plan.
    pub fn discard_partition_edits(&mut self) {
        self.editor.clear();
        self.restage_plan();
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
    /// to the largest region there now.
    fn resettle_region(&mut self) {
        if !self.provenance().is_iso() {
            return;
        }
        let (Some(context), Some(disk)) = (self.context(), self.target_disk()) else {
            return;
        };
        let Ok(projection) = self.editor.projection(disk, &context.snapshot) else {
            return;
        };
        let gaps = projection.free_gaps();
        let still_free = self
            .target
            .region_start_sector
            .is_some_and(|start| gaps.iter().any(|gap| gap.start_sector == start));
        if !still_free {
            // `free_gaps` on a `Projection` is sorted largest first.
            self.target.region_start_sector = gaps.first().map(|gap| gap.start_sector);
        }
    }

    /// Recompute the plan from everything it depends on.
    ///
    /// The only place either planner is called. Which planner is a function of
    /// the provenance and nothing else.
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
            Provenance::Iso => self.plan_iso(context, account),
        });
    }

    fn plan_iso(
        &self,
        context: &Context,
        account: crate::engine::plan::Account,
    ) -> Result<Plan, Refusal> {
        // Nothing chosen yet. `NoTargetDisk` names the empty string, which reads
        // correctly ("is not a disk this installer can see") and keeps `staged`
        // a `Result` rather than growing a third "not ready" state.
        let disk = self.target.disk.clone().ok_or(Refusal::NoTargetDisk {
            disk: String::new(),
        })?;
        let Some(esp) = self.target.esp.clone() else {
            return Err(Refusal::NoEspOnDisk { disk });
        };
        let Some(region_start_sector) = self.target.region_start_sector else {
            return Err(Refusal::NoFreeRegion { disk });
        };
        plan_iso(&IsoInput {
            snapshot: &context.snapshot,
            disk,
            esp,
            region_start_sector,
            root_filesystem: self.target.filesystem,
            swap: self.target.swap,
            locale: self.target.locale.clone(),
            account,
            ids: context.ids.clone(),
            edits: self.editor.ops.clone(),
        })
    }

    /// The plan the confirmation page is describing, if there is one.
    pub fn plan(&self) -> Option<&Plan> {
        self.staged.as_ref()?.as_ref().ok()
    }

    pub fn refusal(&self) -> Option<&Refusal> {
        self.staged.as_ref()?.as_ref().err()
    }

    /// The title, detail and diagnostics the blocked page shows.
    pub fn blocker(&self) -> Blocker {
        match &self.context {
            Err(blocker) => blocker.clone(),
            // Reached when the plan itself was refused on a page that has no way
            // to fix it. The refusal's own message is the explanation.
            Ok(context) => Blocker {
                title: REFUSED_TITLE.to_owned(),
                detail: self.refusal().map_or_else(
                    || "The install plan could not be built.".to_owned(),
                    ToString::to_string,
                ),
                notes: context.notes.clone(),
            },
        }
    }

    // ---- Navigation ----

    /// What the step bar should read: how many stops, and which one.
    ///
    /// The editor borrows the position of whichever step opened it, so that
    /// opening it does not appear to move the user forwards or backwards through
    /// an install it is not part of.
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
            return Some(self.editor_origin());
        }
        Some(match (self.page, self.provenance().is_iso()) {
            (Page::Intro, true) => self.via_network(Page::Disk),
            (Page::Welcome, _) => self.via_network(Page::Account),
            (Page::Network, true) => Page::Disk,
            (Page::Network, false) => Page::Account,
            (Page::Disk, true) => Page::Space,
            (Page::Space, true) => Page::Account,
            (Page::Account, _) => Page::Confirm,
            _ => return None,
        })
    }

    /// The step before the current page in this flow, if there is one.
    pub fn previous_page(&self) -> Option<Page> {
        if self.page == Page::Partitions {
            return Some(self.editor_origin());
        }
        Some(match (self.page, self.provenance().is_iso()) {
            (Page::Network, true) => Page::Intro,
            (Page::Network, false) => Page::Welcome,
            (Page::Disk, true) => Page::Network,
            (Page::Space, true) => Page::Disk,
            (Page::Account, true) => Page::Space,
            (Page::Account, false) => self.via_network(Page::Welcome),
            (Page::Confirm, _) => Page::Account,
            _ => return None,
        })
    }

    /// Where the editor goes back to. A caller that set `page` directly, as a
    /// test does, gets the step each flow opens it from.
    fn editor_origin(&self) -> Page {
        self.editor_return.unwrap_or(if self.provenance().is_iso() {
            Page::Space
        } else {
            Page::Welcome
        })
    }

    /// Move to the next step in this flow, if there is one.
    ///
    /// The pages' "Next step" buttons go through here rather than naming a
    /// destination, so the two flows' orders live in exactly one place —
    /// [`next_page`](Self::next_page).
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
        self.restage_plan();
    }

    /// Open the partition editor on the current target disk, remembering the
    /// step to come back to.
    pub fn open_editor(&mut self) {
        self.editor.reset_selection();
        self.editor_return = Some(self.page);
        self.page = Page::Partitions;
        self.restage_plan();
    }

    /// Leave the account page, if it can be left.
    ///
    /// `touched` is set even when it cannot, so that pressing the button — or
    /// hitting Return in the last field — explains itself rather than doing
    /// nothing.
    pub fn submit_account(&mut self) {
        self.form.touched = true;
        if self.form.problem().is_some() {
            return;
        }
        self.restage_plan();
        if self.plan().is_some() {
            self.go(Page::Confirm);
        }
    }

    // ---- The install ----

    /// Fold one worker message into the state.
    pub fn on_install_event(&mut self, event: Event) {
        let finished = matches!(event, Event::Finished { .. });
        self.progress.apply(event);
        if finished {
            self.page = Page::Done;
        }
    }

    /// Commit: freeze the plan and the password, and move to the progress page.
    ///
    /// The plan stays staged so the summary still has something to read, and is
    /// shared with the worker by reference count rather than cloned.
    pub fn begin_install(&mut self) {
        let Some(plan) = self.plan().cloned() else {
            return;
        };
        self.secrets = Some(Secrets {
            user_password: self.form.password.clone(),
        });
        self.committed = Some(Arc::new(plan));
        self.progress = InstallProgress {
            started: true,
            ..InstallProgress::default()
        };
        self.page = Page::Progress;
    }

    /// Start leaving. The window animates away and then calls
    /// [`restart`](Self::restart); nothing can be pressed in between.
    pub fn close(&mut self) {
        self.closing = true;
    }

    /// Close the installer, leaving the live desktop it was opened over. A dry
    /// run stays here, so it can be rehearsed outside the image.
    pub fn try_live(&mut self) {
        if !self.mode.is_dry_run() {
            crate::live::leave();
        }
    }

    /// Reboot into whatever was installed. A dry run wrote nothing, so it has
    /// nothing to reboot into and only the window closes.
    ///
    /// `systemctl reboot` rather than `reboot`, because on a systemd system the
    /// latter is a symlink whose behaviour depends on whether the unit is
    /// running, and a live environment's is not always what you would guess.
    pub fn restart(&self) {
        if !self.mode.is_dry_run() {
            let _ = std::process::Command::new("systemctl")
                .arg("reboot")
                .status();
        }
    }
}
