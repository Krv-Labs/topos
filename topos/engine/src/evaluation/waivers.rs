//! Waivers — `[[pr_recap.waive]]`, the PR gates' acknowledged findings.
//!
//! A waiver names one gate and a path glob, and says why: a finding of
//! that gate under that path stops counting toward `pr-recap`'s readiness
//! and exit code. Like the SECURE allowlist
//! ([`crate::evaluation::suppression`]) it is disclosed, never silent: a
//! waived finding keeps its severity and stays in the report beside its
//! reason, and a waiver that matched nothing or has expired is reported
//! too, so a stale one gets noticed and removed.
//!
//! Every waiver **requires a non-empty `reason`** and a `path`; an entry
//! without one, naming an unknown gate, or carrying a malformed `expires`
//! is dropped and recorded in [`crate::config::PrGateConfig::warnings`].

use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::{AllowEntry, GateId};

/// One `[[pr_recap.waive]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiver {
    /// `pattern` is the gate key, `scope` the path glob.
    pub entry: AllowEntry,
    /// Last day the waiver applies, `YYYY-MM-DD`, inclusive.
    pub expires: Option<String>,
}

impl Waiver {
    /// The gate key, e.g. `pillar_lost`.
    pub fn gate(&self) -> &str {
        &self.entry.pattern
    }

    /// The path glob.
    pub fn path(&self) -> &str {
        &self.entry.scope
    }

    pub fn reason(&self) -> &str {
        &self.entry.reason
    }

    /// Whether the waiver still applies on `today` (`YYYY-MM-DD`). Both
    /// dates are zero-padded, so comparing the text compares the dates.
    pub fn is_active(&self, today: &str) -> bool {
        self.expires
            .as_deref()
            .is_none_or(|expires| today <= expires)
    }

    /// Whether a finding of `gate` at `path` falls under this waiver,
    /// expired or not.
    pub fn covers(&self, gate: &str, path: &str) -> bool {
        self.gate() == gate && self.entry.matches_path(path)
    }
}

/// What became of one waiver on one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaiverStatus {
    /// Active and matched at least one finding.
    Used,
    /// Active and matched nothing: probably safe to delete.
    Unused,
    /// Past its `expires` date, so it waived nothing.
    Expired,
}

impl WaiverStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            WaiverStatus::Used => "used",
            WaiverStatus::Unused => "unused",
            WaiverStatus::Expired => "expired",
        }
    }
}

/// One waiver's result, in configuration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaiverOutcome {
    /// Findings the waiver covers. For an expired waiver these are the
    /// findings it would have waived, which is what makes it worth
    /// renewing or fixing.
    pub matched: usize,
    pub status: WaiverStatus,
}

/// Match findings, given as `(gate key, path)`, against `waivers`.
///
/// Returns, per finding, the index of the first active waiver covering it
/// (`None` when it still counts), and one [`WaiverOutcome`] per waiver.
pub fn apply<'a>(
    waivers: &[Waiver],
    today: &str,
    findings: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> (Vec<Option<usize>>, Vec<WaiverOutcome>) {
    let mut matched = vec![0usize; waivers.len()];
    let waived_by = findings
        .into_iter()
        .map(|(gate, path)| {
            let mut first_active = None;
            for (index, waiver) in waivers.iter().enumerate() {
                if !waiver.covers(gate, path) {
                    continue;
                }
                let active = waiver.is_active(today);
                // An expired waiver counts what it would have covered; an
                // active one counts only the findings it actually waived.
                if !active || first_active.is_none() {
                    matched[index] += 1;
                }
                if active && first_active.is_none() {
                    first_active = Some(index);
                }
            }
            first_active
        })
        .collect();
    let outcomes = waivers
        .iter()
        .zip(matched)
        .map(|(waiver, matched)| WaiverOutcome {
            matched,
            status: if !waiver.is_active(today) {
                WaiverStatus::Expired
            } else if matched == 0 {
                WaiverStatus::Unused
            } else {
                WaiverStatus::Used
            },
        })
        .collect();
    (waived_by, outcomes)
}

/// Parse the `waive` array of tables. Invalid entries are dropped and
/// recorded in `warnings`, numbered from 1 as they appear in the file.
pub fn parse_waivers(value: &toml::Value, warnings: &mut Vec<String>) -> Vec<Waiver> {
    let Some(entries) = value.as_array() else {
        warnings.push(
            "pr_recap.waive: expected an array of tables ([[pr_recap.waive]]), ignored".to_string(),
        );
        return Vec::new();
    };
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, raw)| {
            let name = format!("pr_recap.waive #{}", index + 1);
            let parsed = parse_waiver(raw, &name, warnings);
            if let Err(problem) = &parsed {
                warnings.push(format!("{name}: {problem}, ignored"));
            }
            parsed.ok()
        })
        .collect()
}

fn parse_waiver(
    raw: &toml::Value,
    name: &str,
    warnings: &mut Vec<String>,
) -> Result<Waiver, String> {
    let table = raw.as_table().ok_or("expected a table")?;
    for key in table.keys() {
        if !matches!(key.as_str(), "gate" | "path" | "reason" | "expires") {
            warnings.push(format!("{name}.{key}: unknown setting, ignored"));
        }
    }
    let gate = text(table.get("gate")).ok_or("missing gate")?;
    if GateId::parse(&gate).is_none() {
        return Err(format!("unknown gate {gate:?}"));
    }
    let path = text(table.get("path")).ok_or("missing path")?;
    // reason is mandatory anti-gaming friction, as for `[[secure.allow]]`.
    let reason = text(table.get("reason")).ok_or("missing reason")?;
    let expires = match table.get("expires") {
        None => None,
        Some(value) => Some(
            date_text(value).ok_or_else(|| format!("expires {value} is not a YYYY-MM-DD date"))?,
        ),
    };
    Ok(Waiver {
        entry: AllowEntry::new(gate, reason).with_scope(path),
        expires,
    })
}

fn text(value: Option<&toml::Value>) -> Option<String> {
    let text = value?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// `expires` as a zero-padded `YYYY-MM-DD`: a quoted string or a bare
/// TOML date (`expires = 2026-12-31` parses as one).
fn date_text(value: &toml::Value) -> Option<String> {
    let text = match value {
        toml::Value::String(text) => text.trim().to_string(),
        toml::Value::Datetime(datetime) if datetime.time.is_none() => datetime.to_string(),
        _ => return None,
    };
    is_valid_date(&text).then_some(text)
}

fn is_valid_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let [year, month, day] = parts.as_slice() else {
        return false;
    };
    let digits =
        |part: &str, len: usize| part.len() == len && part.bytes().all(|b| b.is_ascii_digit());
    if !(digits(year, 4) && digits(month, 2) && digits(day, 2)) {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        year.parse::<u32>(),
        month.parse::<u32>(),
        day.parse::<u32>(),
    ) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days_in_month).contains(&day)
}

/// Today's UTC date, `YYYY-MM-DD`, from the system clock.
pub fn today_utc() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    civil_date(days)
}

/// The proleptic Gregorian date `days` after 1970-01-01, `YYYY-MM-DD`
/// (Howard Hinnant's `civil_from_days`).
pub fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waivers(text: &str) -> (Vec<Waiver>, Vec<String>) {
        let table: toml::Table = text.parse().unwrap();
        let mut warnings = Vec::new();
        let parsed = parse_waivers(&table["pr_recap"]["waive"], &mut warnings);
        (parsed, warnings)
    }

    #[test]
    fn valid_entries_parse_with_either_date_form() {
        let (parsed, warnings) = waivers(
            "[[pr_recap.waive]]\ngate = \"pillar_lost\"\npath = \"src/legacy/**\"\nreason = \"being rewritten\"\nexpires = \"2026-12-31\"\n\n[[pr_recap.waive]]\ngate = \"cosmetic\"\npath = \"src/gen/*\"\nreason = \"generated\"\nexpires = 2027-01-15\n\n[[pr_recap.waive]]\ngate = \"score_drop\"\npath = \"**\"\nreason = \"known\"\n",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].gate(), "pillar_lost");
        assert_eq!(parsed[0].path(), "src/legacy/**");
        assert_eq!(parsed[0].reason(), "being rewritten");
        assert_eq!(parsed[0].expires.as_deref(), Some("2026-12-31"));
        assert_eq!(parsed[1].expires.as_deref(), Some("2027-01-15"));
        assert_eq!(parsed[2].expires, None);
    }

    #[test]
    fn invalid_entries_are_dropped_and_reported() {
        let (parsed, warnings) = waivers(
            "[[pr_recap.waive]]\ngate = \"pillar_lost\"\npath = \"a/**\"\n\n[[pr_recap.waive]]\ngate = \"pilar_lost\"\npath = \"a/**\"\nreason = \"x\"\n\n[[pr_recap.waive]]\ngate = \"cosmetic\"\npath = \"a/**\"\nreason = \"x\"\nexpires = \"2026-02-30\"\n\n[[pr_recap.waive]]\ngate = \"cosmetic\"\nreason = \"x\"\n\n[[pr_recap.waive]]\ngate = \"cosmetic\"\npath = \"a/**\"\nreason = \"  \"\n",
        );
        assert!(parsed.is_empty(), "{parsed:?}");
        let joined = warnings.join("\n");
        for needle in [
            "pr_recap.waive #1: missing reason",
            "pr_recap.waive #2: unknown gate \"pilar_lost\"",
            "pr_recap.waive #3: expires \"2026-02-30\" is not a YYYY-MM-DD date",
            "pr_recap.waive #4: missing path",
            "pr_recap.waive #5: missing reason",
        ] {
            assert!(joined.contains(needle), "missing {needle}: {joined}");
        }
    }

    #[test]
    fn a_waiver_is_active_through_its_expiry_day() {
        let waiver = Waiver {
            entry: AllowEntry::new("pillar_lost", "x").with_scope("src/**"),
            expires: Some("2026-09-22".to_string()),
        };
        assert!(waiver.is_active("2026-09-21"));
        assert!(waiver.is_active("2026-09-22"));
        assert!(!waiver.is_active("2026-09-23"));
        assert!(waiver.covers("pillar_lost", "src/a/b.py"));
        assert!(!waiver.covers("score_drop", "src/a/b.py"));
        assert!(!waiver.covers("pillar_lost", "lib/b.py"));
    }

    #[test]
    fn apply_reports_used_unused_and_expired_waivers() {
        let waiver = |gate: &str, path: &str, expires: Option<&str>| Waiver {
            entry: AllowEntry::new(gate, "x").with_scope(path),
            expires: expires.map(str::to_string),
        };
        let waivers = [
            waiver("pillar_lost", "src/**", None),
            waiver("cosmetic", "docs/**", None),
            waiver("score_drop", "src/**", Some("2026-01-01")),
        ];
        let findings = [
            ("pillar_lost", "src/a.py"),
            ("score_drop", "src/a.py"),
            ("pillar_lost", "lib/b.py"),
        ];
        let (waived_by, outcomes) = apply(&waivers, "2026-09-22", findings);
        assert_eq!(waived_by, [Some(0), None, None]);
        let statuses: Vec<_> = outcomes.iter().map(|o| (o.status, o.matched)).collect();
        assert_eq!(
            statuses,
            [
                (WaiverStatus::Used, 1),
                (WaiverStatus::Unused, 0),
                (WaiverStatus::Expired, 1),
            ]
        );
    }

    #[test]
    fn civil_dates_match_known_days() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(11_016), "2000-02-29");
        assert_eq!(civil_date(20_718), "2026-09-22");
        assert_eq!(civil_date(-1), "1969-12-31");
        assert_eq!(today_utc().len(), 10);
    }
}
