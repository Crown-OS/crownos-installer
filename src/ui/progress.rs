//! The state of a running install, folded from the executor's events.

use std::collections::VecDeque;

use crate::engine::exec::Event;

/// How many log lines the progress page keeps.
///
/// The page shows the tail that fits; every line also goes to stderr, which the
/// kiosk unit sends to the journal.
pub const LOG_LIMIT: usize = 40;

#[derive(Debug, Default)]
pub struct InstallProgress {
    pub started: bool,
    pub step: usize,
    pub total: usize,
    pub label: String,
    pub log: VecDeque<String>,
    pub failure: Option<String>,
    pub finished: bool,
    pub was_dry_run: bool,
}

impl InstallProgress {
    /// `0.0..=1.0`, for the progress bar. A failed install stays where it
    /// stopped rather than reading as complete.
    pub fn fraction(&self) -> f32 {
        if self.finished && self.failure.is_none() {
            return 1.0;
        }
        if self.total == 0 {
            return 0.0;
        }
        self.step as f32 / self.total as f32
    }

    /// Whether the worker should be running.
    pub fn is_running(&self) -> bool {
        self.started && !self.finished
    }

    /// Fold one executor event in.
    pub fn apply(&mut self, event: Event) {
        match event {
            Event::StepStarted {
                index,
                total,
                label,
            } => {
                self.step = index;
                self.total = total;
                self.label = label;
            }
            Event::Log(line) => {
                if self.log.len() == LOG_LIMIT {
                    self.log.pop_front();
                }
                self.log.push_back(line);
            }
            Event::StepFinished { index } => self.step = index + 1,
            Event::Failed { message, .. } => {
                self.failure = Some(message);
                self.finished = true;
            }
            Event::Finished { dry_run } => {
                self.finished = true;
                self.was_dry_run = dry_run;
            }
        }
    }
}
