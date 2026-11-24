//! Repo-relative path helpers. Paths use `/` and carry no leading `./`.

pub(super) fn normalized(path: &str) -> &str {
    path.strip_prefix("./").unwrap_or(path)
}

pub(super) fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

pub(super) fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

pub(super) fn extension(path: &str) -> &str {
    file_name(path).rsplit_once('.').map_or("", |(_, ext)| ext)
}

/// `dir` joined with a relative `path`, with `.` and `..` segments resolved.
pub(super) fn join(dir: &str, path: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if dir.starts_with('/') {
        format!("/{joined}")
    } else {
        joined
    }
}

/// `start` and each of its ancestors up to the repository root (`""`).
pub(super) fn ancestors(start: &str) -> impl Iterator<Item = &str> {
    let mut next = Some(start);
    std::iter::from_fn(move || {
        let dir = next?;
        next = (!dir.is_empty()).then(|| parent(dir));
        Some(dir)
    })
}
