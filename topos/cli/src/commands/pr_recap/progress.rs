//! What a slow recap shows on stderr before its card: a typewriter over a
//! `graphs N/2` counter while the coupling graphs build, then the scoring
//! bar `evaluate` shares. Each phase fades in, fades out and leaves
//! nothing behind, and one over within [`GRACE`] never draws at all, so a
//! fast recap goes straight to its card.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use console::{Style, Term};
use indicatif::{ProgressBar, ProgressDrawTarget};

use super::ScoreProgress;
use crate::commands::render::{
    bar_line, fade_in, fade_out, open_block, paint, progress_bar, repaint, truncate_right,
    RenderOptions,
};

/// How long a phase runs before it draws.
const GRACE: Duration = Duration::from_millis(200);

/// Whether this run draws transient lines, and how.
#[derive(Clone, Copy)]
pub(super) struct Transient {
    /// stderr is a terminal and the run is not `--json`: a pipe or a CI log
    /// never sees a frame.
    pub(super) shown: bool,
    /// stderr's options. Under `NO_COLOR` the lines draw plain and clear
    /// at once instead of fading.
    pub(super) options: RenderOptions,
}

impl Transient {
    /// `rows` without color, cut to the terminal so none of them wraps: a
    /// wrapped row would throw off the in-place redraw.
    fn plain(self, rows: &[String]) -> Vec<String> {
        let width = self.options.width.saturating_sub(1);
        rows.iter().map(|row| truncate_right(row, width)).collect()
    }
}

/// Wait up to `wait` for the phase to be told to stop; true once it has.
/// Nothing is ever sent: dropping the [`Sender`] is the signal.
fn stopped(stop: &Receiver<()>, wait: Duration) -> bool {
    !matches!(stop.recv_timeout(wait), Err(RecvTimeoutError::Timeout))
}

const GRAPH_FRAMES: [char; 6] = ['⠿', '⠛', '⠹', '⠼', '⠶', '⠦'];
const GRAPH_LINES: &[&str] = &[
    "Checking out base and head side by side...",
    "Tracing who calls whom in both trees",
    "Mapping every import, once",
    "Built once, reused on the next run...",
];
/// Long enough for `graphs 2/2` to register before the lines leave.
const LAST_COUNT_HOLD: Duration = Duration::from_millis(250);

/// The build phase: a dim typewriter over `graphs N/2`, where N counts the
/// sides finished in whichever order they finish.
pub(super) struct GraphProgress {
    done: Arc<AtomicUsize>,
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl GraphProgress {
    pub(super) fn start(transient: Transient) -> Self {
        let done = Arc::new(AtomicUsize::new(0));
        if !transient.shown {
            return Self {
                done,
                stop: None,
                thread: None,
            };
        }
        let (stop, signal) = mpsc::channel();
        let count = Arc::clone(&done);
        let thread = std::thread::spawn(move || graph_loop(&signal, &count, transient));
        Self {
            done,
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    /// One side's graph is built. Called from that side's build thread.
    pub(super) fn side_done(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }

    /// Stop, and wait for the lines to fade out.
    pub(super) fn finish(mut self) {
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn graph_loop(stop: &Receiver<()>, done: &AtomicUsize, transient: Transient) {
    if stopped(stop, GRACE) {
        return;
    }
    let term = Term::stderr();
    let mut typing = Typewriter::new();
    // Cut before painting, like `plain`: a wrapped row breaks the redraw.
    let rows = |typing: &Typewriter, count: usize| {
        let dim = |text: &str| paint(text, Style::new().dim(), transient.options);
        let text = truncate_right(&typing.text(), transient.options.width.saturating_sub(1));
        vec![dim(&text), format!("   {} {count}/2", dim("graphs"))]
    };
    let plain = |typing: &Typewriter, count: usize| {
        transient.plain(&[typing.text(), format!("   graphs {count}/2")])
    };
    open_block(&term, 2);
    fade_in(
        &term,
        &plain(&typing, done.load(Ordering::Relaxed)),
        transient.options.styled,
    );
    loop {
        repaint(&term, &rows(&typing, done.load(Ordering::Relaxed)));
        if stopped(stop, typing.advance()) {
            break;
        }
    }
    let count = done.load(Ordering::Relaxed);
    repaint(&term, &rows(&typing, count));
    if count == 2 {
        std::thread::sleep(LAST_COUNT_HOLD);
    }
    fade_out(&term, &plain(&typing, count), transient.options.styled);
}

/// The working line: the frame turns every tick while the sentence types
/// in, holds for seven seconds, deletes itself one character at a time,
/// waits half a second, and the next sentence types in.
struct Typewriter {
    frame: usize,
    line: usize,
    shown: usize,
    deleting: bool,
    held_since: Instant,
}

impl Typewriter {
    const TICK: Duration = Duration::from_millis(27);
    const HOLD: Duration = Duration::from_secs(7);
    const GAP: Duration = Duration::from_millis(500);

    fn new() -> Self {
        Self {
            frame: 0,
            line: 0,
            shown: 0,
            deleting: false,
            held_since: Instant::now(),
        }
    }

    fn text(&self) -> String {
        let visible: String = GRAPH_LINES[self.line].chars().take(self.shown).collect();
        format!(
            "{}  {visible}",
            GRAPH_FRAMES[self.frame % GRAPH_FRAMES.len()]
        )
    }

    /// Step once, and say how long until the next step.
    fn advance(&mut self) -> Duration {
        let count = GRAPH_LINES[self.line].chars().count();
        self.frame += 1;
        if !self.deleting && self.shown < count {
            self.shown += 1;
            if self.shown == count {
                self.held_since = Instant::now();
            }
        } else if !self.deleting && self.held_since.elapsed() >= Self::HOLD {
            self.deleting = true;
        } else if self.deleting && self.shown > 0 {
            self.shown -= 1;
        } else if self.deleting {
            self.deleting = false;
            self.line = (self.line + 1) % GRAPH_LINES.len();
            return Self::GAP;
        }
        Self::TICK
    }
}

const SCORING: &str = "Scoring";

/// Pass A's bar, e.g. `Scoring ██▓░ 3/13 gates.rs`. indicatif draws it,
/// but only once the grace period is up and the fade-in has played; until
/// then it counts against a hidden target.
pub(super) struct ScoringBar {
    transient: Transient,
    running: Option<Running>,
}

struct Running {
    bar: ProgressBar,
    stop: Sender<()>,
    /// Comes back true once the bar was drawn.
    reveal: JoinHandle<bool>,
}

impl ScoringBar {
    pub(super) fn new(transient: Transient) -> Self {
        Self {
            transient,
            running: None,
        }
    }

    /// Fade the bar out, if it ever drew. Safe to call more than once.
    pub(super) fn finish(&mut self) {
        let Some(Running { bar, stop, reveal }) = self.running.take() else {
            return;
        };
        drop(stop);
        if reveal.join().unwrap_or(false) {
            bar.finish();
            let row = self.transient.plain(&[plain_bar(&bar)]);
            fade_out(&Term::stderr(), &row, self.transient.options.styled);
        }
    }
}

impl ScoreProgress for ScoringBar {
    fn start(&mut self, total: usize) {
        let bar = progress_bar(SCORING, total, !self.transient.shown);
        if bar.is_hidden() {
            return;
        }
        bar.set_draw_target(ProgressDrawTarget::hidden());
        let (stop, signal) = mpsc::channel();
        let reveal = std::thread::spawn({
            let bar = bar.clone();
            let transient = self.transient;
            move || reveal_bar(&bar, &signal, transient)
        });
        self.running = Some(Running { bar, stop, reveal });
    }

    fn scored(&mut self, path: &str) {
        let Some(running) = &self.running else {
            return;
        };
        let name = Path::new(path)
            .file_name()
            .map_or_else(|| path.to_string(), |name| name.to_string_lossy().into());
        running.bar.set_message(name);
        running.bar.inc(1);
        if running.bar.length() == Some(running.bar.position()) {
            self.finish();
        }
    }
}

fn reveal_bar(bar: &ProgressBar, stop: &Receiver<()>, transient: Transient) -> bool {
    if stopped(stop, GRACE) {
        return false;
    }
    let term = Term::stderr();
    let row = transient.plain(&[plain_bar(bar)]);
    fade_in(&term, &row, transient.options.styled);
    // indicatif draws from the cursor, so hand it the start of the row.
    let _ = term.write_str("\r");
    bar.set_draw_target(ProgressDrawTarget::stderr());
    bar.tick();
    true
}

fn plain_bar(bar: &ProgressBar) -> String {
    bar_line(
        SCORING,
        bar.position(),
        bar.length().unwrap_or(0),
        &bar.message(),
    )
}

/// Print the card. On a terminal with color, its title fades in first;
/// the title goes to stdout, so it only fades when stderr is a terminal
/// too, and a pipe gets exactly the lines, with no escapes.
pub(super) fn print_card(lines: &[String], fade: bool) {
    if let (true, Some(title)) = (fade, lines.first()) {
        let term = Term::stdout();
        fade_in(
            &term,
            &[console::strip_ansi_codes(title).into_owned()],
            true,
        );
        let _ = term.write_str("\r\x1b[2K");
    }
    for line in lines {
        println!("{line}");
    }
}
