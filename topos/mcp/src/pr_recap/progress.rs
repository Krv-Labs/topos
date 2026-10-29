//! Where a recap run is, and how long each part took.
//!
//! The MCP tool shares one [`Progress`] between the worker that builds the
//! recap and the executor that enforces the deadline. The worker moves it
//! through the phases; the executor reads it when the deadline passes, so
//! the timeout error says what the run was doing instead of guessing.

use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use super::ScoreProgress;

/// The part of a run the worker is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Waiting for execution capacity: the worker has not started.
    Queued,
    /// Resolving the range, diffing, and building the coupling graphs.
    Preparing,
    /// Scoring the changed files at base and head.
    Scoring,
    /// Gates, clusters and the response, after the last file was scored.
    Finishing,
}

impl Phase {
    fn code(self) -> u8 {
        self as u8
    }

    fn from_code(code: u8) -> Phase {
        match code {
            0 => Phase::Queued,
            1 => Phase::Preparing,
            2 => Phase::Scoring,
            _ => Phase::Finishing,
        }
    }
}

/// Wall time of a run: all of it, and the scoring part on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Timing {
    /// From the start of the run, graph preparation included.
    pub elapsed_ms: u64,
    /// From the first file scored to the last; `None` when nothing was scored.
    pub scoring_ms: Option<u64>,
}

/// Live progress of one recap run.
#[derive(Debug)]
pub(crate) struct Progress {
    /// Set when the worker starts, so queueing is not counted.
    started: OnceLock<Instant>,
    phase: AtomicU8,
    files: AtomicUsize,
    done: AtomicUsize,
    /// Milliseconds since `started`, plus one; zero while unset.
    scoring_from: AtomicU64,
    scoring_to: AtomicU64,
}

impl Default for Progress {
    fn default() -> Self {
        Progress {
            started: OnceLock::new(),
            phase: AtomicU8::new(Phase::Queued.code()),
            files: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            scoring_from: AtomicU64::new(0),
            scoring_to: AtomicU64::new(0),
        }
    }
}

impl Progress {
    pub fn phase(&self) -> Phase {
        Phase::from_code(self.phase.load(Ordering::Relaxed))
    }

    /// The worker has started: the clock runs from here.
    pub fn begin(&self) {
        self.started.get_or_init(Instant::now);
        self.enter(Phase::Preparing);
    }

    /// Scoring is over and the rest of the run is under way.
    pub fn finish_up(&self) {
        self.enter(Phase::Finishing);
    }

    fn enter(&self, phase: Phase) {
        self.phase.store(phase.code(), Ordering::Relaxed);
    }

    fn stamp(&self) -> u64 {
        let millis = self
            .started
            .get()
            .map_or(0, |started| started.elapsed().as_millis());
        u64::try_from(millis).unwrap_or(u64::MAX).saturating_add(1)
    }

    pub fn timing(&self) -> Timing {
        let from = self.scoring_from.load(Ordering::Relaxed);
        let to = self.scoring_to.load(Ordering::Relaxed);
        Timing {
            elapsed_ms: self.stamp() - 1,
            scoring_ms: (from > 0 && to >= from).then(|| to - from),
        }
    }

    /// What the run is doing, for a timeout error to name.
    pub fn describe(&self) -> String {
        let files = self.files.load(Ordering::Relaxed);
        let done = self.done.load(Ordering::Relaxed);
        match self.phase() {
            Phase::Queued => "while queued for execution capacity".to_string(),
            Phase::Preparing => {
                "while preparing (resolving the range, diffing, building coupling graphs)"
                    .to_string()
            }
            Phase::Scoring => format!("while scoring {files} file(s), {done} done"),
            Phase::Finishing => {
                format!("while finishing (gates, clusters, response) after scoring {files} file(s)")
            }
        }
    }
}

/// Feeds the scoring pass's progress into a shared [`Progress`].
pub(crate) struct Watch<'a>(pub &'a Progress);

impl ScoreProgress for Watch<'_> {
    fn start(&mut self, total: usize) {
        let progress = self.0;
        progress.files.store(total, Ordering::Relaxed);
        if total == 0 {
            // Nothing to score: the run goes straight on to the rest.
            progress.finish_up();
            return;
        }
        progress
            .scoring_from
            .store(progress.stamp(), Ordering::Relaxed);
        progress.enter(Phase::Scoring);
    }

    fn scoring(&mut self, _: &str) {}

    fn scored(&mut self) {
        let progress = self.0;
        let done = progress.done.fetch_add(1, Ordering::Relaxed) + 1;
        progress
            .scoring_to
            .store(progress.stamp(), Ordering::Relaxed);
        if done >= progress.files.load(Ordering::Relaxed) {
            progress.finish_up();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_reports_the_phase_it_is_in() {
        let progress = Progress::default();
        assert_eq!(progress.phase(), Phase::Queued);
        assert!(progress.describe().contains("queued"));
        progress.begin();
        assert!(progress.describe().contains("preparing"));
        let mut watch = Watch(&progress);
        watch.start(3);
        watch.scored();
        assert_eq!(progress.describe(), "while scoring 3 file(s), 1 done");
        let mut watch = Watch(&progress);
        watch.scored();
        watch.scored();
        assert_eq!(progress.phase(), Phase::Finishing);
        assert!(progress.describe().contains("after scoring 3 file(s)"));
    }

    #[test]
    fn timing_covers_the_whole_run_and_the_scoring_part() {
        let progress = Progress::default();
        progress.begin();
        assert_eq!(progress.timing().scoring_ms, None);
        let mut watch = Watch(&progress);
        watch.start(1);
        watch.scored();
        let timing = progress.timing();
        assert!(timing.scoring_ms.is_some());
        assert!(timing.scoring_ms.unwrap() <= timing.elapsed_ms);
    }

    #[test]
    fn zero_files_to_score_moves_straight_on_and_times_no_scoring() {
        let progress = Progress::default();
        progress.begin();
        Watch(&progress).start(0);
        assert_eq!(progress.phase(), Phase::Finishing);
        assert!(progress.describe().contains("after scoring 0 file(s)"));
        assert_eq!(progress.timing().scoring_ms, None);
    }
}
