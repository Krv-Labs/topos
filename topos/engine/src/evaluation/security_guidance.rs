//! Security remediation guidance — one table for prose and operation tokens.
//!
//! Maps each dangerous API in
//! [`crate::functors::probes::cpg::danger`]'s registry to a [`Remediation`]:
//! the imperative prose the suggestion engine ([`crate::evaluation::suggestions`])
//! renders, and the machine-readable operation tokens MCP refactor targets
//! carry. Lookup uses the danger probe's own suffix-aware
//! [`crate::functors::probes::cpg::danger::match_registry_key`] so a
//! qualified or aliased callee (`mypkg.os.system`, `Popen`) resolves to the
//! same guidance the probe flagged it under.
//!
//! `every_registry_entry_has_specific_guidance` (below) guards that every
//! registry entry has a non-default remediation — the registry cannot
//! silently outgrow this table.
//!
//! # Deviation from the Python original
//!
//! `SecurityFinding` here is a lean, evaluation-crate-local mirror of
//! `topos.mcp.schemas.SecurityFinding` (a Pydantic model). `topos.mcp` is
//! the MCP server layer and is out of scope for `topos-core` (no `pyo3`,
//! no wire/serialization concerns) — this crate only needs the finding's
//! *shape*, not its JSON schema or validation.

use crate::functors::probes::cpg::danger::match_registry_key;

/// Actionable SECURE diagnostic for an agent.
///
/// Lean mirror of `topos.mcp.schemas.SecurityFinding` — see the module
/// doc comment's "Deviation from the Python original".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityFinding {
    /// Finding kind, e.g. `"dangerous_call"` or `"taint_flow"`.
    pub kind: String,
    /// 1-based source line.
    pub line: u32,
    /// Source snippet for the finding.
    pub snippet: String,
    /// Detected dangerous callee.
    pub callee: Option<String>,
    /// Taint source snippet (`"taint_flow"` findings only).
    pub source: Option<String>,
    /// Taint sink snippet (`"taint_flow"` findings only).
    pub sink: Option<String>,
}

/// Guidance for one dangerous API: prose advice + operation tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Remediation {
    pub advice: &'static str,
    pub operations: &'static [&'static str],
}

const DYNAMIC_EXEC: &[&str] = &["replace_dynamic_execution", "use_static_dispatch"];
const SHELL: &[&str] = &["remove_shell_execution", "pass_argument_array"];
const DESERIALIZE: &[&str] = &["use_safe_deserializer", "validate_input"];
const DOM: &[&str] = &["sanitize_html", "build_dom_nodes"];
const BOUNDED_COPY: &[&str] = &["use_bounded_copy"];
const UNSAFE: &[&str] = &["encapsulate_unsafe"];

pub const TAINT_OPERATIONS: &[&str] = &["validate_input", "sanitize_before_sink"];
pub const DEFAULT_OPERATIONS: &[&str] = &["replace_dangerous_api", "validate_input"];

/// Keyed by dangerous callee (lowercase; suffix-matched via the danger
/// probe's matcher). Prose preserved verbatim from the original Python
/// suggestion engine.
pub const REMEDIATIONS: &[(&str, Remediation)] = &[
    (
        "eval",
        Remediation {
            advice: "Replace `eval` with `ast.literal_eval` or explicit parsing.",
            operations: DYNAMIC_EXEC,
        },
    ),
    (
        "exec",
        Remediation {
            advice: "Remove `exec`; call the code path directly or dispatch via a map.",
            operations: DYNAMIC_EXEC,
        },
    ),
    (
        "compile",
        Remediation {
            advice: "Avoid dynamic `compile`; use a static, reviewed code path.",
            operations: DYNAMIC_EXEC,
        },
    ),
    (
        "__import__",
        Remediation {
            advice: "Import statically; avoid `__import__` on dynamic names.",
            operations: DYNAMIC_EXEC,
        },
    ),
    (
        "function",
        Remediation {
            advice: "Avoid the `Function` constructor; call a known function directly.",
            operations: DYNAMIC_EXEC,
        },
    ),
    (
        "settimeout",
        Remediation {
            advice: "Pass a function reference to `setTimeout`, never a string.",
            operations: DYNAMIC_EXEC,
        },
    ),
    (
        "setinterval",
        Remediation {
            advice: "Pass a function reference to `setInterval`, never a string.",
            operations: DYNAMIC_EXEC,
        },
    ),
    (
        "pickle.loads",
        Remediation {
            advice: "Use `json` or a schema-validated deserializer instead of `pickle`.",
            operations: DESERIALIZE,
        },
    ),
    (
        "marshal.loads",
        Remediation {
            advice: "Avoid `marshal`; deserialize with `json` or a safe format.",
            operations: DESERIALIZE,
        },
    ),
    (
        "yaml.load",
        Remediation {
            advice: "Use `yaml.safe_load` instead of `yaml.load`.",
            operations: DESERIALIZE,
        },
    ),
    (
        "os.system",
        Remediation {
            advice: "Replace `os.system` with `subprocess.run([...])` (no shell).",
            operations: SHELL,
        },
    ),
    (
        "os.popen",
        Remediation {
            advice: "Replace `os.popen` with `subprocess.run([...], capture_output=True)`.",
            operations: SHELL,
        },
    ),
    (
        "subprocess.call",
        Remediation {
            advice: "Pass an argument list and avoid `shell=True`.",
            operations: SHELL,
        },
    ),
    (
        "subprocess.run",
        Remediation {
            advice: "Pass an argument list and avoid `shell=True`.",
            operations: SHELL,
        },
    ),
    (
        "subprocess.popen",
        Remediation {
            advice: "Pass an argument list and avoid `shell=True`.",
            operations: SHELL,
        },
    ),
    (
        "child_process.exec",
        Remediation {
            advice: "Use `execFile`/`spawn` with an argument array (no shell).",
            operations: SHELL,
        },
    ),
    (
        "system",
        Remediation {
            advice: "Replace `system()` with an `exec*`-family call (no shell).",
            operations: SHELL,
        },
    ),
    (
        "exec.command",
        Remediation {
            advice: "Pass a fixed argument list to `exec.Command`; never build the \
                      command or its args from untrusted input.",
            operations: SHELL,
        },
    ),
    (
        "exec.commandcontext",
        Remediation {
            advice: "Pass a fixed argument list to `exec.CommandContext`; never build \
                      the command or its args from untrusted input.",
            operations: SHELL,
        },
    ),
    (
        "os.startprocess",
        Remediation {
            advice: "Avoid `os.StartProcess` with untrusted paths or args; validate \
                      and pass an explicit argument list.",
            operations: SHELL,
        },
    ),
    (
        "syscall.exec",
        Remediation {
            advice: "Avoid `syscall.Exec`; validate the program path and argument \
                      list before replacing the process image.",
            operations: SHELL,
        },
    ),
    (
        "syscall.forkexec",
        Remediation {
            advice: "Avoid `syscall.ForkExec`; validate the program path and \
                      argument list, or prefer `os/exec` with explicit arguments.",
            operations: SHELL,
        },
    ),
    (
        "innerhtml",
        Remediation {
            advice: "Set text via `textContent`, or sanitize before assigning HTML.",
            operations: DOM,
        },
    ),
    (
        "document.write",
        Remediation {
            advice: "Build DOM nodes instead of `document.write`.",
            operations: DOM,
        },
    ),
    (
        "strcpy",
        Remediation {
            advice: "Use a bounded copy (`strncpy`/`snprintf`).",
            operations: BOUNDED_COPY,
        },
    ),
    (
        "strcat",
        Remediation {
            advice: "Use a bounded concat (`strncat`/`snprintf`).",
            operations: BOUNDED_COPY,
        },
    ),
    (
        "sprintf",
        Remediation {
            advice: "Use `snprintf` with an explicit buffer size.",
            operations: BOUNDED_COPY,
        },
    ),
    (
        "scanf",
        Remediation {
            advice: "Use bounded input (`fgets` + parsing, or width-limited `scanf`).",
            operations: BOUNDED_COPY,
        },
    ),
    (
        "gets",
        Remediation {
            advice: "Replace `gets` with `fgets` and an explicit length.",
            operations: BOUNDED_COPY,
        },
    ),
    (
        "transmute",
        Remediation {
            advice: "Avoid `mem::transmute`; use a safe conversion or `bytemuck`.",
            operations: UNSAFE,
        },
    ),
    (
        "unsafe",
        Remediation {
            advice: "Confine or remove the `unsafe` block; document the invariant it upholds.",
            operations: UNSAFE,
        },
    ),
    (
        "from_raw",
        Remediation {
            advice: "Encapsulate `from_raw` behind a safe wrapper; document pointer ownership.",
            operations: UNSAFE,
        },
    ),
];

fn lookup(key: &str) -> Option<Remediation> {
    REMEDIATIONS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, r)| *r)
}

/// `(advice, operations)` for a security finding.
///
/// Taint flows get flow-specific prose; dangerous calls resolve through
/// the suffix-aware registry matcher; anything unmatched gets generic
/// guidance.
pub fn remediation_for(finding: &SecurityFinding) -> (String, &'static [&'static str]) {
    if finding.kind == "taint_flow" {
        let src = finding.source.as_deref().unwrap_or("untrusted input");
        let sink = finding
            .callee
            .as_deref()
            .or(finding.sink.as_deref())
            .unwrap_or("the dangerous call");
        return (
            format!(
                "Validate/sanitize `{src}` before it reaches `{sink}` (line {}).",
                finding.line
            ),
            TAINT_OPERATIONS,
        );
    }
    let callee = finding.callee.as_deref().unwrap_or("").to_lowercase();
    let key = if callee.is_empty() {
        None
    } else {
        match_registry_key(&callee, REMEDIATIONS.iter().map(|(k, _)| *k))
    };
    if let Some(key) = key {
        if let Some(entry) = lookup(key) {
            return (entry.advice.to_string(), entry.operations);
        }
    }
    (
        format!(
            "Remove or sandbox the dangerous call `{}` (line {}).",
            finding.callee.as_deref().unwrap_or(""),
            finding.line
        ),
        DEFAULT_OPERATIONS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functors::probes::cpg::danger::dangerous_apis;

    fn finding(callee: Option<&str>) -> SecurityFinding {
        SecurityFinding {
            kind: "dangerous_call".to_string(),
            line: 1,
            snippet: format!("{}(x)", callee.unwrap_or("")),
            callee: callee.map(str::to_string),
            source: None,
            sink: None,
        }
    }

    #[test]
    fn every_registry_entry_has_specific_guidance() {
        for language in ["python", "javascript", "typescript", "rust", "cpp", "go"] {
            for api in dangerous_apis(language) {
                let (advice, operations) = remediation_for(&finding(Some(api)));
                assert_ne!(
                    operations, DEFAULT_OPERATIONS,
                    "{language}:{api} fell to default"
                );
                assert!(!advice.contains("Remove or sandbox"), "{language}:{api}");
            }
        }
    }

    #[test]
    fn qualified_callee_suffix_matches() {
        let (_, operations) = remediation_for(&finding(Some("mypkg.os.system")));
        assert_eq!(
            operations,
            &["remove_shell_execution", "pass_argument_array"]
        );
    }

    #[test]
    fn probe_guidance_parity_for_qualified_callees() {
        for language in ["python", "javascript", "typescript", "rust", "cpp", "go"] {
            for api in dangerous_apis(language) {
                let qualified = format!("mypkg.{api}");
                let (_, operations) = remediation_for(&finding(Some(&qualified)));
                assert_ne!(operations, DEFAULT_OPERATIONS, "{qualified}");
            }
        }
    }

    #[test]
    fn subprocess_popen_prefers_longest_key() {
        let key = match_registry_key("subprocess.popen", REMEDIATIONS.iter().map(|(k, _)| *k));
        assert_eq!(key, Some("subprocess.popen"));
    }

    #[test]
    fn deserialization_apis_get_safe_deserializer_ops() {
        for callee in ["pickle.loads", "yaml.load", "marshal.loads"] {
            let (_, operations) = remediation_for(&finding(Some(callee)));
            assert_eq!(
                operations,
                &["use_safe_deserializer", "validate_input"],
                "{callee}"
            );
        }
    }

    #[test]
    fn taint_flow_prose_and_operations() {
        let finding = SecurityFinding {
            kind: "taint_flow".to_string(),
            line: 7,
            snippet: "eval(data)".to_string(),
            callee: Some("eval".to_string()),
            source: Some("request.args".to_string()),
            sink: Some("eval(data)".to_string()),
        };
        let (advice, operations) = remediation_for(&finding);
        assert!(advice.contains("request.args"));
        assert!(advice.contains("line 7"));
        assert_eq!(operations, TAINT_OPERATIONS);
    }

    #[test]
    fn unknown_callee_falls_back_to_default() {
        let (advice, operations) = remediation_for(&finding(Some("totally_benign_call")));
        assert_eq!(operations, DEFAULT_OPERATIONS);
        assert!(advice.contains("Remove or sandbox"));
    }
}
