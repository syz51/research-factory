//! `lab brief`: the short text an agent reads at the start of a session.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::check::Run;
use crate::finding::Severity;
use crate::lessons::{Kind, Lesson, Status};

/// Adopted lessons older than this without a `verified` decision are listed
/// for evaluation.
pub const EVALUATE_AFTER_DAYS: i32 = 14;

pub fn render(
    project: &str,
    today: &str,
    lessons: &[Lesson],
    last: Option<&Run>,
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
        out.push_str("None adopted yet.\n");
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
    match last {
        None => out.push_str("No `lab check` run recorded.\n"),
        Some(run) => {
            let mut by_sev: BTreeMap<Severity, usize> = BTreeMap::new();
            let mut by_rule: BTreeMap<(String, String), usize> = BTreeMap::new();
            for item in &run.items {
                *by_sev.entry(item.finding.severity).or_default() += 1;
                if item.finding.severity == Severity::Error {
                    *by_rule
                        .entry((item.finding.tool.clone(), item.finding.rule.clone()))
                        .or_default() += 1;
                }
            }
            let failed: Vec<&str> = run
                .header
                .checks
                .iter()
                .filter(|c| !c.ran())
                .map(|c| c.name.as_str())
                .collect();
            let _ = writeln!(
                out,
                "{}: {} errors, {} warnings, {} info across {} checks.",
                run.header.at,
                by_sev.get(&Severity::Error).unwrap_or(&0),
                by_sev.get(&Severity::Warn).unwrap_or(&0),
                by_sev.get(&Severity::Info).unwrap_or(&0),
                run.header.checks.len()
            );
            if !failed.is_empty() {
                let _ = writeln!(out, "Checks that failed to run: {}.", failed.join(", "));
            }
            let mut top: Vec<_> = by_rule.into_iter().collect();
            top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            for ((tool, rule), n) in top.into_iter().take(5) {
                let _ = writeln!(out, "- {n} × {tool}:{rule}");
            }
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
                "- {candidates} recurring finding rule(s) have no lesson (run `lab retro`)."
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
        let text = render("demo", "2026-11-07", &lessons, None, &[], 2);
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
        let lit = render("demo", "2026-11-07", &lessons, None, &["lit".into()], 0);
        assert!(lit.contains("rule a") && !lit.contains("(`b`)") && !lit.contains("Maintenance"));
        assert!(
            !lit.contains("Learned from failures"),
            "an empty group has no heading:\n{lit}"
        );
    }
}
