//! SHA-256 of a file, and the `checksums.txt` line format.
//!
//! Lives here because `sha2` and `hex` are already dependencies of this crate,
//! and the CLI crate does not carry a crypto dependency. The release's
//! `checksums.txt` is the only place a topos binary's integrity is
//! established, so the verification code sits next to the release lookup it
//! belongs with rather than inside a command.

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256 of a file's contents, read in chunks so a large
/// binary does not have to fit in memory.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => hasher.update(&buffer[..count]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        }
    }
    Ok(hex::encode(hasher.finalize()))
}

/// The expected hash for `asset` from a `checksums.txt` body.
///
/// Matched on the **file name**, not the line order: the release publishes one
/// line per platform, and a body whose first line happened to be some other
/// platform's must not validate this one. A malformed entry is `None` rather
/// than a truncated hash, because a 8-character "hash" that compares unequal
/// would report a corruption that is not there.
pub fn expected(body: &str, asset: &str) -> Option<String> {
    body.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?;
        (name == asset && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The NIST vector for "abc", to prove the digest itself is right rather
    /// than merely self-consistent.
    #[test]
    fn the_digest_matches_a_known_vector() {
        let path = std::env::temp_dir().join(format!("topos-sha-{}", std::process::id()));
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_digest_reads_a_file_larger_than_one_chunk() {
        let path = std::env::temp_dir().join(format!("topos-sha-big-{}", std::process::id()));
        // Three chunks' worth, so the read loop has to iterate.
        std::fs::write(&path, vec![b'a'; 200_000]).unwrap();
        let digest = sha256_file(&path).unwrap();
        assert_eq!(digest.len(), 64);
        std::fs::remove_file(&path).ok();
    }

    /// A real SHA-256 is 64 hex characters. Built by repetition rather than
    /// written out, because a hand-typed 64-char literal is exactly how the
    /// first version of this test shipped 60-character "hashes" and asserted
    /// they were found.
    fn fake_hash(seed: &str) -> String {
        seed.repeat(64 / seed.len())
    }

    #[test]
    fn the_checksum_lookup_is_by_name_not_by_line_order() {
        let mac = fake_hash("ab");
        let linux = fake_hash("cd");
        let body = format!("{mac}  topos-macos-arm64\n{linux}  topos-linux-amd64\n");
        assert_eq!(
            expected(&body, "topos-linux-amd64").as_deref(),
            Some(linux.as_str())
        );
        assert_eq!(
            expected(&body, "topos-macos-arm64").as_deref(),
            Some(mac.as_str())
        );
        assert!(expected(&body, "topos-linux-arm64").is_none());
    }

    #[test]
    fn a_malformed_entry_is_rejected_rather_than_trusted() {
        assert!(expected("deadbeef  topos-linux-amd64", "topos-linux-amd64").is_none());
        assert!(expected("AAAA  topos-linux-amd64", "topos-linux-amd64").is_none());
        assert!(expected(
            "not-a-hash-at-all-zzzz  topos-linux-amd64",
            "topos-linux-amd64"
        )
        .is_none());
        assert!(expected("", "topos-linux-amd64").is_none());
    }

    #[test]
    fn hashes_normalize_to_lowercase_so_comparison_is_case_insensitive() {
        let body = format!("{}  topos-linux-amd64", "AB".repeat(32));
        assert_eq!(
            expected(&body, "topos-linux-amd64").as_deref(),
            Some("ab".repeat(32).as_str())
        );
    }

    /// A 60-character hash is not a SHA-256, and accepting one would mean a
    /// truncated `checksums.txt` silently validates against a short prefix.
    #[test]
    fn a_wrong_length_hash_is_not_a_checksum() {
        let body = format!("{}  topos-linux-amd64", "ab".repeat(30));
        assert_eq!(expected(&body, "topos-linux-amd64"), None);
    }
}
