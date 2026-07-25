//! Tiny COMPOSABLE fixture for CI (issue #198).
//! Two files with a CALLS/CONTAINS relationship so GitNexus indexes a real MDG.

mod helper;

pub fn entry() {
    helper::assist(1);
}
