//! Every rule a `lab` check reports a finding under, with its default
//! severity. docs/interfaces.md, "Rule table", lists the same rules; a test
//! keeps the two equal, and debug builds check each finding against this table.

use crate::finding::Severity;

/// One rule of `lab archive check` or `lab check`.
pub struct Rule {
    pub id: &'static str,
    /// The severity a finding of this rule has unless an allow entry or a
    /// kept-link entry lowers it to info.
    pub severity: Severity,
    /// The project's config sets the severity, such as `size-budget`.
    pub configurable: bool,
}

const fn rule(id: &'static str, severity: Severity) -> Rule {
    Rule {
        id,
        severity,
        configurable: false,
    }
}

use Severity::{Error, Info, Warn};

/// The rules, sorted by id.
pub const RULES: &[Rule] = &[
    rule("amendment-mismatch", Warn),
    rule("archived-link", Info),
    rule("broken-anchor", Error),
    rule("broken-link", Error),
    rule("check-exit-mismatch", Warn),
    rule("check-failed", Error),
    rule("check-output-invalid", Error),
    rule("duplicate-id", Error),
    rule("front-matter-invalid", Error),
    rule("historical-link", Warn),
    rule("id-taken-on-default-branch", Warn),
    rule("ignored-link", Warn),
    rule("inactive-without-reason", Error),
    rule("index-leftover", Warn),
    rule("index-missing", Error),
    rule("index-stale", Error),
    rule("kept-link", Info),
    rule("missing-field", Error),
    rule("missing-kind", Error),
    rule("missing-summary", Warn),
    rule("missing-task", Error),
    rule("no-current-page", Warn),
    rule("open-item", Error),
    rule("seal-unverified", Error),
    rule("sealed-edited", Error),
    rule("sealed-missing", Error),
    rule("similar-record", Warn),
    rule("similar-skipped", Info),
    Rule {
        id: "size-budget",
        severity: Warn,
        configurable: true,
    },
    rule("stale-allow", Warn),
    rule("stale-derivative", Warn),
    rule("stale-kept-link", Warn),
    rule("status-reversed", Error),
    rule("superseded-but-active", Warn),
    rule("superseded-link", Warn),
    rule("supersession-mismatch", Warn),
    rule("task-link-skipped", Info),
    rule("task-without-record", Warn),
    rule("unindexed-file", Warn),
    rule("unknown-id", Error),
    rule("unknown-task", Error),
    rule("unsealed", Warn),
    rule("untracked-link", Warn),
];

pub fn get(id: &str) -> Option<&'static Rule> {
    RULES
        .binary_search_by(|r| r.id.cmp(id))
        .ok()
        .map(|i| &RULES[i])
}

/// Debug builds: panics when `id` is not in [`RULES`], or when `severity` is
/// neither the rule's default nor info (an allow or kept-link entry) and the
/// rule's severity is not configurable.
pub fn debug_check(id: &str, severity: Severity) {
    if cfg!(debug_assertions) {
        let Some(r) = get(id) else {
            panic!("rule {id:?} is not in src/rules.rs RULES");
        };
        assert!(
            r.configurable || severity == r.severity || severity == Info,
            "rule {id:?} emitted as {}, but its default is {}",
            severity.as_str(),
            r.severity.as_str()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_are_sorted_and_unique() {
        for w in RULES.windows(2) {
            assert!(w[0].id < w[1].id, "{} before {}", w[0].id, w[1].id);
        }
    }

    /// The table in docs/interfaces.md, "Rule table", names the same rules
    /// with the same default severities.
    #[test]
    fn docs_table_matches() {
        let doc = include_str!("../docs/interfaces.md");
        let start = doc
            .find("### Rule table")
            .expect("docs/interfaces.md has a \"### Rule table\" section");
        let rows: Vec<(String, String)> = doc[start..]
            .lines()
            .skip_while(|l| !l.starts_with('|'))
            .take_while(|l| l.starts_with('|'))
            .skip(2)
            .map(|l| {
                let cells: Vec<&str> = l.trim_matches('|').split('|').map(str::trim).collect();
                (cells[0].trim_matches('`').to_string(), cells[1].to_string())
            })
            .collect();
        let want: Vec<(String, String)> = RULES
            .iter()
            .map(|r| {
                let sev = if r.configurable {
                    format!("{} (configurable)", r.severity.as_str())
                } else {
                    r.severity.as_str().to_string()
                };
                (r.id.to_string(), sev)
            })
            .collect();
        assert_eq!(rows, want);
    }

    #[test]
    #[should_panic(expected = "not in src/rules.rs")]
    #[cfg(debug_assertions)]
    fn unknown_rule_panics() {
        debug_check("no-such-rule", Error);
    }

    #[test]
    #[should_panic(expected = "its default is warn")]
    #[cfg(debug_assertions)]
    fn wrong_severity_panics() {
        debug_check("unsealed", Error);
    }
}
