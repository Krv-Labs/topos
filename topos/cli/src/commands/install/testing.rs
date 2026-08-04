//! Scratch directories for the install tests, shared so `edits.rs` and
//! `ownership.rs` don't each carry a copy.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

/// A fresh directory per call. The counter keeps concurrently-running tests
/// that pass the same label from colliding.
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
