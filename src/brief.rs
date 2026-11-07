//! `lab brief`: the short text an agent reads at the start of a session.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::check::{CheckOutcome, Run};
use crate::finding::Severity;
use crate::lessons::{Kind, Lesson, Status};

/// Adopted lessons older than this without a `verified` decision are listed
/// for evaluation.
pub const EVALUATE_AFTER_DAYS: i32 = 14;

/// `runs` are the stored check runs, oldest first, and `checks` the configured
/// check names; `candidates` counts recurring rules without a lesson across
/// the whole project.
pub fn render(
    project: &str,
    today: &str,
    lessons: &[Lesson],
    runs: &[Run],
    checks: &[String],
    tags: &[String],
    candidates: usize,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {project}: research brief ({today})\n");

    let active: Vec<&Lesson> = lessons
        .iter()
        .filter(|l| l.meta.status.active())
        .filter(|l| tags.is_empty() || l.meta.tags.iter().any(|t| tags.contains(t)))
        .collect();
    let _ = writeln!(out, "## Rules from lessons ({})\n", active.len());
    if active.is_empty() {
        if !tags.is_empty() && lessons.iter().any(|l| l.meta.status.active()) {
            let _ = writeln!(out, "None adopted with tag {}.", tags.join(", "));
        } else {
            out.push_str("None adopted yet.\n");
        }
    } else {
        out.push_str("Follow every rule below; its group says only where it came from.\n");
    }
    let (mut failures, mut worked) = (Vec::new(), Vec::new());
    for l in &active {
        match l.meta.kind {
            Kind::Bad => failures.push(*l),
            Kind::Good => worked.push(*l),
        }
    }
    for (heading, rules) in [
        ("Learned from failures", failures),
        ("Practices that worked", worked),
    ] {
        if rules.is_empty() {
            continue;
        }
        let _ = writeln!(out, "\n{heading}:\n");
        for l in rules {
            let _ = writeln!(out, "- {} (`{}`)", l.meta.rule.trim(), l.meta.id);
        }
    }
    out.push_str("\n## Last check\n\n");
    // Each check's latest run: `lab check --only` stores only the checks it ran.
    let latest: Vec<(&Run, &CheckOutcome)> = checks
        .iter()
        .filter_map(|name| {
            runs.iter().rev().find_map(|r| {
                r.header
                    .checks
                    .iter()
                    .find(|c| &c.name == name)
                    .map(|c| (r, c))
            })
        })
        .collect();
    if latest.is_empty() {
        out.push_str("No `lab check` run recorded.\n");
    } else {
        let mut by_sev: BTreeMap<Severity, usize> = BTreeMap::new();
        let mut by_rule: BTreeMap<(String, String), usize> = BTreeMap::new();
        for (run, check) in &latest {
            for item in run.items.iter().filter(|i| i.check == check.name) {
                *by_sev.entry(item.finding.severity).or_default() += 1;
                if item.finding.severity == Severity::Error {
                    *by_rule
                        .entry((item.finding.tool.clone(), item.finding.rule.clone()))
                        .or_default() += 1;
                }
            }
        }
        let at: BTreeSet<&str> = latest.iter().map(|(r, _)| r.header.at.as_str()).collect();
        let when = match (at.first(), at.last()) {
            (Some(a), Some(b)) if a != b => format!("{a} to {b} (each check's latest run)"),
            (Some(a), _) => (*a).to_string(),
            _ => unreachable!("latest is not empty"),
        };
        let _ = writeln!(
            out,
            "{when}: {} errors, {} warnings, {} info across {} checks.",
            by_sev.get(&Severity::Error).unwrap_or(&0),
            by_sev.get(&Severity::Warn).unwrap_or(&0),
            by_sev.get(&Severity::Info).unwrap_or(&0),
            latest.len()
        );
        let failed: Vec<&str> = latest
            .iter()
            .filter(|(_, c)| !c.ran())
            .map(|(_, c)| c.name.as_str())
            .collect();
        if !failed.is_empty() {
            let _ = writeln!(out, "Checks that failed to run: {}.", failed.join(", "));
        }
        let never: Vec<&str> = checks
            .iter()
            .filter(|n| !latest.iter().any(|(_, c)| &c.name == *n))
            .map(String::as_str)
            .collect();
        if !never.is_empty() {
            let _ = writeln!(out, "Never run: {}.", never.join(", "));
        }
        let mut top: Vec<_> = by_rule.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        for ((tool, rule), n) in top.into_iter().take(5) {
            let _ = writeln!(out, "- {n} × {tool}:{rule}");
        }
    }

    let due: Vec<&Lesson> = lessons
        .iter()
        .filter(|l| l.meta.status == Status::Adopted)
        .filter(|l| tags.is_empty() || l.meta.tags.iter().any(|t| tags.contains(t)))
        .filter(|l| {
            l.meta
                .adopted
                .as_deref()
                .and_then(|a| a.parse::<jiff::civil::Date>().ok())
                .zip(today.parse::<jiff::civil::Date>().ok())
                .is_some_and(|(a, t)| (t - a).get_days() >= EVALUATE_AFTER_DAYS)
        })
        .collect();
    if !due.is_empty() || candidates > 0 {
        out.push_str("\n## Maintenance\n\n");
        if !due.is_empty() {
            let ids: Vec<&str> = due.iter().map(|l| l.meta.id.as_str()).collect();
            let _ = writeln!(
                out,
                "- Adopted over {EVALUATE_AFTER_DAYS} days, not yet verified: {} (run `lab evaluate`).",
                ids.join(", ")
            );
        }
        if candidates > 0 {
            let _ = writeln!(
                out,
                "- {candidates} recurring finding rule(s) have no lesson{} (run `lab retro`).",
                if tags.is_empty() {
                    ""
                } else {
                    " across the project, whatever their tags"
                }
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lessons::Meta;

    fn lesson(
        id: &str,
        kind: Kind,
        status: Status,
        adopted: Option<&str>,
        tags: &[&str],
    ) -> Lesson {
        Lesson {
            path: format!("{id}.md").into(),
            meta: Meta {
                id: id.into(),
                title: "t".into(),
                kind,
                status,
                scope: "workflow".into(),
                tags: tags.iter().map(|t| t.to_string()).collect(),
                rule: format!("rule {id}"),
                metric: None,
                created: "2026-10-01".into(),
                updated: "2026-10-01".into(),
                adopted: adopted.map(Into::into),
                evidence: vec![],
            },
            body: String::new(),
        }
    }

    #[test]
    fn lists_active_rules_and_due_evaluations() {
        let lessons = vec![
            lesson(
                "a",
                Kind::Good,
                Status::Adopted,
                Some("2026-11-06"),
                &["lit"],
            ),
            lesson(
                "b",
                Kind::Bad,
                Status::Adopted,
                Some("2026-10-01"),
                &["exp"],
            ),
            lesson("c", Kind::Bad, Status::Observed, None, &["lit"]),
        ];
        let text = render("demo", "2026-11-07", &lessons, &[], &[], &[], 2);
        let failures = text
            .find("Learned from failures:\n\n- rule b (`b`)")
            .unwrap();
        let worked = text
            .find("Practices that worked:\n\n- rule a (`a`)")
            .unwrap();
        assert!(
            failures < worked,
            "lessons from failures come first:\n{text}"
        );
        assert!(text.contains("Follow every rule below"));
        assert!(
            !text.contains("[avoid]") && !text.contains("[keep]"),
            "a rule is never shown inverted:\n{text}"
        );
        assert!(!text.contains("rule c"));
        assert!(text.contains("not yet verified: b"));
        assert!(text.contains("2 recurring finding rule(s)"));
        let lit = render("demo", "2026-11-07", &lessons, &[], &[], &["lit".into()], 0);
        assert!(lit.contains("rule a") && !lit.contains("(`b`)") && !lit.contains("Maintenance"));
        assert!(
            !lit.contains("Learned from failures"),
            "an empty group has no heading:\n{lit}"
        );
    }

    fn run(at: &str, checks: &[(&str, &str)], errors: &[(&str, &str)]) -> Run {
        use crate::check::{Item, RunHeader};
        use crate::finding::Finding;
        Run {
            path: format!("{at}.jsonl").into(),
            header: RunHeader {
                schema: "rf.labcheck/1".into(),
                at: at.into(),
                checks: checks
                    .iter()
                    .map(|(name, status)| CheckOutcome {
                        name: (*name).into(),
                        command: vec![],
                        exit: Some(0),
                        status: (*status).into(),
                        findings: 0,
                        unparsed: 0,
                        duration_ms: 0,
                    })
                    .collect(),
            },
            items: errors
                .iter()
                .enumerate()
                .map(|(i, (check, rule))| Item {
                    schema: "rf.labcheck.item/1".into(),
                    check: (*check).into(),
                    fingerprint: format!("{i}"),
                    finding: Finding::lab(rule, Severity::Error, "s", "m".into()),
                })
                .collect(),
        }
    }

    #[test]
    fn last_check_uses_each_checks_latest_run() {
        let checks: Vec<String> = ["references", "claims", "sources"].map(Into::into).to_vec();
        let runs = [
            run(
                "2026-11-06T00:00:00Z",
                &[("references", "ok"), ("claims", "ok")],
                &[("references", "r1"), ("claims", "c1"), ("claims", "c1")],
            ),
            // `lab check --only claims` found nothing.
            run("2026-11-07T00:00:00Z", &[("claims", "ok")], &[]),
        ];
        let text = render("demo", "2026-11-07", &[], &runs, &checks, &[], 0);
        assert!(
            text.contains("2026-11-06T00:00:00Z to 2026-11-07T00:00:00Z (each check's latest run): 1 errors, 0 warnings, 0 info across 2 checks."),
            "{text}"
        );
        assert!(text.contains("- 1 × lab:r1"), "{text}");
        assert!(!text.contains("lab:c1"), "{text}");
        assert!(text.contains("Never run: sources."), "{text}");
    }

    #[test]
    fn tag_scoped_brief_says_what_the_tags_left_out() {
        let lessons = vec![lesson(
            "a",
            Kind::Good,
            Status::Adopted,
            Some("2026-11-06"),
            &["lit"],
        )];
        let exp = render("demo", "2026-11-07", &lessons, &[], &[], &["exp".into()], 3);
        assert!(exp.contains("None adopted with tag exp."), "{exp}");
        assert!(!exp.contains("None adopted yet."), "{exp}");
        assert!(
            exp.contains("have no lesson across the project, whatever their tags"),
            "{exp}"
        );
        let all = render("demo", "2026-11-07", &lessons, &[], &[], &[], 3);
        assert!(all.contains("- 3 recurring finding rule(s) have no lesson (run `lab retro`)."));
    }
}
