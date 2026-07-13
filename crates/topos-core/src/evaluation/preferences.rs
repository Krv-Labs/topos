//! The three quality generators of `G_qual`, and (eventually) full
//! strict-ordering preferences over them.
//!
//! Only [`Generator`] is ported so far — it's a dependency of
//! `policies::{base,calibration}`. The rest of Python's
//! `preferences.py` (`UserPreferences`, the relaxation walk, two-stage
//! IDEAL/pairwise-meet targeting described in `AGENTS.md`) is real
//! remaining work within issue #144, not yet started.

/// The three quality generators of `G_qual`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Generator {
    Simple,
    Composable,
    Secure,
}

impl Generator {
    pub const ALL: [Generator; 3] = [Generator::Simple, Generator::Composable, Generator::Secure];

    pub fn as_str(self) -> &'static str {
        match self {
            Generator::Simple => "simple",
            Generator::Composable => "composable",
            Generator::Secure => "secure",
        }
    }
}
