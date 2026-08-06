//! Scratch directories for the install unit tests, shared so each module does
//! not carry its own copy.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

/// A fresh directory per call.
///
/// The counter matters because `cargo test` is threaded: two tests that happen
/// to pass the same label would otherwise share a path and delete each other's
/// fixtures halfway through a run.
pub(crate) fn tmp_dir(label: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "topos-install-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap();
    dir
}
