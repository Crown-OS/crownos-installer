//! Either flow: the install running.
//!
//! [`execute`] blocks for minutes — `pacstrap` alone is most of that — so it
//! runs on the blocking pool and reports back through a worker this page
//! starts. The worker lives as long as the page: leaving it cancels the
//! forwarding, and the UI thread never does anything slower than drawing a
//! log line.

use std::sync::Arc;

use crownui::kit::{ButtonVariant, icon_colored, icons};
use crownui::prelude::{
    Cx, Role, Semantic, Signal, Styled, View, WHITE, bind, hstack, keyed, text, vstack,
};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::engine::exec::{Event, Mode, Secrets, execute};
use crate::engine::plan::Plan;
use crate::engine::probe::RealSystem;
use crate::ui::components::{PILL_HEIGHT, Status, Tone, alert, meter, percent, stage_row, surface};
use crate::ui::layout::{BLOCK_GAP, PageParts, caption, muted, nothing, restart_pill, tiles};
use crate::ui::palette::{BRAND, INK, OUTLINE};
use crate::ui::{InstallProgress, Wizard};
use crate::util::worker::{MessageProxy, spawn_worker};

/// How many steps the page shows at once, around the one running.
pub const WINDOW: usize = 4;
/// How many finished steps stay in view above the running one.
const TRAILING: usize = 2;
const PERCENT_SIZE: f32 = 24.0;
const LABEL_SIZE: f32 = 15.0;
const LOG_SIZE: f32 = 11.0;
const ROW_RISE: f32 = 10.0;

pub fn build(cx: &mut Cx, wizard: Signal<Wizard>) -> PageParts {
    start_install(cx, wizard);
    let fraction =
        cx.memo(move |runtime| wizard.with(runtime, |wizard| wizard.progress.fraction()));

    let headline = surface((
        hstack((
            vstack((
                text(bind(move |runtime| {
                    wizard.with(runtime, |wizard| headline(&wizard.progress))
                }))
                .text_size(LABEL_SIZE)
                .font_semibold()
                .text_color(INK)
                .truncate(),
                muted(bind(move |runtime| {
                    wizard.with(runtime, |wizard| detail(&wizard.progress))
                }))
                .truncate(),
            ))
            .gap(4.0)
            .flex_1(),
            text(bind(move |runtime| percent(fraction.get(runtime))))
                .text_size(PERCENT_SIZE)
                .font_semibold()
                .text_color(BRAND),
        ))
        .gap(16.0)
        .items_center(),
        meter("Overall progress", fraction),
    ));

    let window = keyed(
        move |runtime| {
            wizard.with(runtime, |wizard| {
                let total = wizard.progress.total.max(step_labels(wizard).len());
                window_start(wizard.progress.step, total)
            })
        },
        move |cx, start| {
            let labels = wizard.with_untracked(cx, step_labels);
            let rows: Vec<_> = labels
                .into_iter()
                .enumerate()
                .skip(start)
                .take(WINDOW)
                .map(|(index, label)| {
                    let status = cx.memo(move |runtime| {
                        wizard.with(runtime, |wizard| step_status(&wizard.progress, index))
                    });
                    let caption = bind(move |runtime| {
                        wizard.with(runtime, |wizard| caption_for(&wizard.progress, index))
                    });
                    stage_row(label, caption, status)
                        .enter(|style| style.opacity(0.0).translate_y(ROW_RISE))
                })
                .collect();
            tiles(rows)
        },
    );

    let failure = keyed(
        move |runtime| wizard.with(runtime, |wizard| wizard.progress.failure.clone()),
        |_, failure| match failure {
            Some(failure) => alert(Tone::Danger, "The install stopped", failure).boxed(),
            None => nothing(),
        },
    );

    // The whole log is in the journal; the page keeps its tail for when
    // something went wrong.
    let log = keyed(
        move |runtime| wizard.with(runtime, |wizard| wizard.progress.failure.is_some()),
        move |_, failed| {
            if !failed {
                return nothing();
            }
            surface(
                text(bind(move |runtime| {
                    wizard.with(runtime, |wizard| {
                        wizard
                            .progress
                            .log
                            .iter()
                            .map(String::as_str)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                }))
                .text_size(LOG_SIZE)
                .text_color(INK),
            )
            .boxed()
        },
    );

    // Nothing to press while it runs. The install cannot be paused, and a Back
    // button that silently did nothing would be worse than no button.
    let action = keyed(
        move |runtime| wizard.with(runtime, |wizard| wizard.progress.failure.is_some()),
        move |_, failed| {
            if failed {
                restart_pill(wizard, "Restart", ButtonVariant::Primary).boxed()
            } else {
                keep_on().boxed()
            }
        },
    );

    PageParts::new(
        "Installing CrownOS",
        "Sit back and relax — this takes a few minutes.",
    )
    .content(
        vstack((
            headline,
            failure,
            surface((caption("Installation steps"), window)),
            log,
        ))
        .gap(BLOCK_GAP),
    )
    .actions(action)
}

/// The quiet reminder where a button would be.
fn keep_on() -> impl View {
    hstack((
        icon_colored(icons::TRIANGLE_ALERT, 2.0, |_| BRAND).size(16.0),
        text("Keep your computer on")
            .text_size(14.0)
            .text_color(INK),
    ))
    .gap(8.0)
    .items_center()
    .h(PILL_HEIGHT)
    .px(20.0)
    .rounded(PILL_HEIGHT / 2.0)
    .bg(WHITE)
    .border(1.0)
    .border_color(OUTLINE)
    .role(Role::Group)
    .label("Keep your computer on")
    .enter(|style| style.opacity(0.0))
}

/// The first step of the window that keeps the running step in view, with a
/// couple of finished ones above it, without running past the end.
pub fn window_start(current: usize, total: usize) -> usize {
    current
        .saturating_sub(TRAILING)
        .min(total.saturating_sub(WINDOW))
}

/// The state of step `index` as the progress stands.
pub fn step_status(progress: &InstallProgress, index: usize) -> Status {
    let failed_here = progress.failure.is_some() && index == progress.step;
    if failed_here {
        Status::Failed
    } else if index < progress.step || (progress.finished && progress.failure.is_none()) {
        Status::Done
    } else if index == progress.step && progress.is_running() {
        Status::Working
    } else {
        Status::Waiting(index + 1)
    }
}

fn caption_for(progress: &InstallProgress, index: usize) -> String {
    match step_status(progress, index) {
        Status::Working => progress
            .log
            .back()
            .cloned()
            .unwrap_or_else(|| "In progress…".to_owned()),
        status => status.spoken().to_owned(),
    }
}

fn step_labels(wizard: &Wizard) -> Vec<String> {
    wizard.committed.as_ref().map_or_else(Vec::new, |plan| {
        plan.steps.iter().map(|step| step.label()).collect()
    })
}

fn headline(progress: &InstallProgress) -> String {
    if progress.failure.is_some() {
        "Stopped".to_owned()
    } else if progress.finished {
        "Finishing up".to_owned()
    } else if progress.label.is_empty() {
        "Starting…".to_owned()
    } else {
        progress.label.clone()
    }
}

fn detail(progress: &InstallProgress) -> String {
    if progress.total == 0 {
        return "Preparing the install".to_owned();
    }
    let position = format!(
        "Step {} of {}",
        (progress.step + 1).min(progress.total),
        progress.total
    );
    match progress.log.back() {
        Some(line) => format!("{position} · {line}"),
        None => position,
    }
}

/// Start the install, once: the page is built once per commit, and a page that
/// is rebuilt for any other reason finds the install already finished.
fn start_install(cx: &mut Cx, wizard: Signal<Wizard>) {
    let job = wizard.with_untracked(cx, |wizard| {
        let plan = wizard.committed.clone()?;
        let secrets = wizard.secrets.clone()?;
        wizard
            .progress
            .is_running()
            .then_some((plan, secrets, wizard.mode))
    });
    let Some((plan, secrets, mode)) = job else {
        return;
    };
    spawn_worker(
        cx,
        move |proxy, _: UnboundedReceiver<()>| install(plan, secrets, mode, proxy),
        move |cx, event| wizard.update(cx, |wizard| wizard.on_install_event(event)),
    );
}

/// Run [`execute`] on the blocking pool, posting every event to the page and
/// every log line to stderr. Failures are not returned: each has already been
/// reported as [`Event::Failed`], which is what the page shows.
async fn install(plan: Arc<Plan>, secrets: Secrets, mode: Mode, proxy: MessageProxy<Event>) {
    let _ = tokio::task::spawn_blocking(move || {
        let _ = execute(&plan, &secrets, mode, &RealSystem, &mut |event| {
            if let Event::Log(line) = &event {
                eprintln!("{line}");
            }
            let _ = proxy.message(event);
        });
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(step: usize, total: usize) -> InstallProgress {
        InstallProgress {
            started: true,
            step,
            total,
            ..InstallProgress::default()
        }
    }

    #[test]
    fn the_window_keeps_two_finished_steps_above_the_running_one() {
        assert_eq!(window_start(0, 22), 0);
        assert_eq!(window_start(1, 22), 0);
        assert_eq!(window_start(5, 22), 3);
        assert_eq!(window_start(21, 22), 18);
        assert_eq!(window_start(3, 3), 0);
    }

    #[test]
    fn steps_before_the_running_one_are_done_and_after_it_wait_their_turn() {
        let progress = running(4, 10);
        assert_eq!(step_status(&progress, 3), Status::Done);
        assert_eq!(step_status(&progress, 4), Status::Working);
        assert_eq!(step_status(&progress, 5), Status::Waiting(6));
    }

    #[test]
    fn a_failure_marks_the_step_it_happened_on_and_nothing_else() {
        let mut progress = running(4, 10);
        progress.apply(Event::Failed {
            index: 4,
            message: "pacstrap exited with status 1".to_owned(),
        });
        assert_eq!(step_status(&progress, 4), Status::Failed);
        assert_eq!(step_status(&progress, 3), Status::Done);
        assert_eq!(step_status(&progress, 5), Status::Waiting(6));
    }

    #[test]
    fn a_failed_install_reads_where_it_stopped_not_complete() {
        let mut progress = running(4, 10);
        progress.apply(Event::Failed {
            index: 4,
            message: "pacstrap exited with status 1".to_owned(),
        });
        assert_eq!(percent(progress.fraction()), "40%");
        assert_eq!(headline(&progress), "Stopped");
    }

    #[test]
    fn a_finished_install_has_every_step_done() {
        let mut progress = running(9, 10);
        progress.apply(Event::Finished { dry_run: false });
        assert!((0..10).all(|index| step_status(&progress, index) == Status::Done));
    }

    #[test]
    fn the_running_step_shows_the_latest_log_line() {
        let mut progress = running(2, 10);
        assert_eq!(caption_for(&progress, 2), "In progress…");
        progress.apply(Event::Log("pacstrap -K /mnt base".to_owned()));
        assert_eq!(caption_for(&progress, 2), "pacstrap -K /mnt base");
        assert_eq!(detail(&progress), "Step 3 of 10 · pacstrap -K /mnt base");
        assert_eq!(caption_for(&progress, 1), "Done");
    }
}
