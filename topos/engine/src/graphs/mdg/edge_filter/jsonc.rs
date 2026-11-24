//! `tsconfig.json` is JSON with comments and trailing commas.

use serde_json::Value;

pub(super) fn parse_jsonc(text: &str) -> Option<Value> {
    serde_json::from_str(&without_trailing_commas(&without_comments(text))).ok()
}

/// Copies `text`, letting `skip` drop or keep each piece outside string
/// literals. `skip` returns how many bytes to drop at the current position.
fn scan(text: &str, skip: impl Fn(&str) -> Option<usize>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let (kept, next) = if c == '"' {
            split_string(rest)
        } else if let Some(dropped) = skip(rest) {
            ("", &rest[dropped.min(rest.len())..])
        } else {
            rest.split_at(c.len_utf8())
        };
        out.push_str(kept);
        rest = next;
    }
    out
}

/// Splits a string literal starting at `text[0]` from what follows it.
fn split_string(text: &str) -> (&str, &str) {
    let mut escaped = false;
    for (i, c) in text.char_indices().skip(1) {
        match (escaped, c) {
            (true, _) => escaped = false,
            (false, '\\') => escaped = true,
            (false, '"') => return text.split_at(i + 1),
            _ => {}
        }
    }
    (text, "")
}

fn without_comments(text: &str) -> String {
    scan(text, |rest| {
        if rest.starts_with("//") {
            return Some(rest.find('\n').unwrap_or(rest.len()));
        }
        rest.strip_prefix("/*")
            .map(|body| body.find("*/").map_or(rest.len(), |end| end + 4))
    })
}

fn without_trailing_commas(text: &str) -> String {
    scan(text, |rest| {
        (rest.starts_with(',') && rest[1..].trim_start().starts_with(['}', ']'])).then_some(1)
    })
}
