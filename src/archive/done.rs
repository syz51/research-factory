//! `lab archive done`, the one gate for "this record is complete", and the
//! read-only views `lab archive log` and `lab archive index --print`.

use std::process::ExitCode;

use super::rules::{Problem, invalid_problem};
use super::{Record, Spec, Texts, exit, files, gitseal, index, seal_files, unique_refs};
use crate::config::{Project, SealMode};
use crate::finding::Severity;

/// The states a record passes on its way to done, in order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    Links,
    Task,
    Sources,
    Sealed,
}

impl Stage {
    const ALL: [Self; 4] = [Self::Links, Self::Task, Self::Sources, Self::Sealed];

    fn name(self) -> &'static str {
        match self {
            Self::Links => "links and metadata",
            Self::Task => "task named and known",
            Self::Sources => "sources cited",
            Self::Sealed => "sealed",
        }
    }
}

/// The stage of an archive rule and how to fix its finding.
fn fix(rule: &str) -> Option<(Stage, &'static str)> {
    use Stage::{Links, Sealed, Task};
    Some(match rule {
        "front-matter-invalid" => (Links, "correct the front matter at the line shown"),
        "missing-field" => (Links, "add the field to the record's front matter"),
        "duplicate-id" => (
            Links,
            "give one of the records a new id: `lab archive renumber <path> <id from lab archive next>`",
        ),
        "id-taken-on-default-branch" => (
            Links,
            "take a new id from `lab archive next <prefix>` and run `lab archive renumber` before the merge",
        ),
        "unknown-id" => (
            Links,
            "correct the id, or add the record that the field names",
        ),
        "supersession-mismatch" | "amendment-mismatch" => (
            Links,
            "name each record in the other one's field, so both sides agree",
        ),
        "superseded-but-active" => (
            Links,
            "set the superseded record's status to an inactive one",
        ),
        "inactive-without-reason" => (
            Links,
            "add `superseded_by: <id>` or `withdrawn_reason: <why>` to the front matter",
        ),
        "broken-link" | "broken-anchor" => (
            Links,
            "correct the link target; in sealed text, restore the target or add a \
             [[lab.archive.kept_link]] entry with a reason",
        ),
        "superseded-link" | "historical-link" => (
            Links,
            "link the record that replaced it, or say on the line that the target is historical",
        ),
        "stale-kept-link" | "stale-allow" => (Links, "remove the entry from research.toml"),
        "untracked-link" => (
            Links,
            "commit the target in the same commit as the link, or have git ignore it on purpose",
        ),
        "stale-derivative" => (
            Stage::Sources,
            "rebuild the structure file or paper card from the current text with `papers`, or delete it",
        ),
        "kept-link" => (
            Links,
            "nothing: a [[lab.archive.kept_link]] entry keeps this link on purpose",
        ),
        "archived-link" => (
            Links,
            "nothing: the target moved out of the project, and the offloaded list names it",
        ),
        "index-stale" | "index-missing" => (Links, "run `lab archive index`"),
        "similar-record" => (
            Links,
            "extend or link the record on the same topic, or keep both on purpose",
        ),
        "similar-skipped" => (
            Links,
            "nothing: the duplicate check could not search; `lab archive similar <file>` says why",
        ),
        "index-leftover" => (
            Links,
            "delete the file; `lab archive index --print <name>` prints the index",
        ),
        "size-budget" => (
            Links,
            "shorten the file, or move detail to a record and link it",
        ),
        "missing-summary" => (
            Links,
            "add a `summary` that says what the record found or decided",
        ),
        "unindexed-file" => (
            Links,
            "add the file to an index's `include` or `exclude`, or to `unindexed_exclude`",
        ),
        "no-current-page" => (
            Links,
            "set `status: current` on the page that is current, or write that page",
        ),
        "missing-task" => (
            Task,
            "add `task: <board task id>` to the record's front matter",
        ),
        "missing-kind" => (Task, "add `kind:` to the record's front matter"),
        "unknown-task" => (Task, "correct the `task` id; the board does not list it"),
        "open-item" => (
            Task,
            "add the id of the board task that holds the item, or `no action: <reason>`",
        ),
        "task-without-record" => (
            Task,
            "write the record of the task, or add `task: <id>` to the record that holds its result",
        ),
        "task-link-skipped" => (
            Task,
            "set `board_export` in [lab.archive], or make the board reachable, then run this again",
        ),
        "sealed-edited" => (
            Sealed,
            "restore the sealed text and append a dated correction; or record the edit in \
             [[lab.archive.accepted_edit]] (git seal) or with `lab archive seal --accept` (manifest)",
        ),
        "sealed-missing" => (
            Sealed,
            "restore the file; with the git seal, a removal that must stay needs a \
             [[lab.archive.accepted_edit]] entry",
        ),
        "status-reversed" => (
            Sealed,
            "restore the status; a superseded or withdrawn record stays so, and a new record \
             carries the change",
        ),
        "seal-unverified" => (
            Sealed,
            "run lab in a git repository, or set `seal = \"manifest\"`",
        ),
        "unsealed" => (Sealed, "run `lab archive seal`"),
        _ => return None,
    })
}

/// One finding of the gate.
struct Line {
    stage: Stage,
    error: bool,
    text: String,
    fix: String,
}

fn line(tool: &str, rule: &str, severity: Severity, text: String) -> Line {
    let (stage, fix) = match fix(rule).filter(|_| tool == "lab") {
        Some((stage, fix)) => (stage, fix.to_string()),
        None => (
            Stage::Sources,
            format!("correct what the message names, then run `{tool}` again"),
        ),
    };
    Line {
        stage,
        error: severity == Severity::Error,
        text,
        fix,
    }
}

/// The findings that are not in the baseline, from the configured checks, or
/// from the archive rules alone when no check is configured.
fn findings(project: &Project, spec: &Spec) -> Result<Vec<Line>, String> {
    if project.lab.checks.is_empty() {
        println!(
            "No [[lab.checks]] are configured: only the archive rules run, without a baseline."
        );
        return Ok(super::problems(project, spec)?
            .iter()
            .map(|p| line("lab", p.rule, p.severity, p.text()))
            .collect());
    }
    let report = crate::check::run(project, &[])?;
    let mut lines = Vec::new();
    for c in &report.run.header.checks {
        if !c.ran() {
            lines.push(Line {
                stage: Stage::Sources,
                error: true,
                text: format!("error check `{}` did not run ({})", c.name, c.status),
                fix: format!(
                    "run `lab check --only {} --verbose` and correct the command",
                    c.name
                ),
            });
        }
    }
    for i in &report.run.items {
        // Without a baseline for its check, every finding counts as new.
        let new = report
            .delta
            .get(&i.check)
            .and_then(Option::as_ref)
            .is_none_or(|d| d.new.contains(&i.fingerprint));
        if new {
            let f = &i.finding;
            let text = format!(
                "{} {} {} — {}",
                f.severity.as_str(),
                f.rule,
                f.subject,
                f.message
            );
            lines.push(line(&f.tool, &f.rule, f.severity, text));
        }
    }
    Ok(lines)
}

/// Checks the front matter and writes the index files, or names where the
/// indexes print when none is written. false when front matter is invalid.
fn refresh_indexes(spec: &Spec, records: &[Record]) -> Result<bool, String> {
    if spec.cfg.write_indexes {
        return super::write_indexes(spec, records);
    }
    let invalid: Vec<Problem> = records.iter().filter_map(invalid_problem).collect();
    for p in &invalid {
        println!("{}", p.text());
        println!("  fix: correct the front matter at the line shown");
    }
    if invalid.is_empty() {
        println!(
            "indexes: none written (`write_indexes = false`); print one with \
             `lab archive index --print <name>`"
        );
    }
    Ok(invalid.is_empty())
}

/// `lab archive done`.
pub fn done(project: &Project) -> Result<ExitCode, String> {
    let spec = Spec::new(project)?;
    let stop = |stage: &str, fix: &str| {
        println!("Stopped at `{stage}`. {fix}, then run `lab archive done` again.");
        Ok(ExitCode::from(1))
    };
    let mut found = spec.walk(&[&spec.sealed])?;
    let sealed = found.pop().unwrap_or_default();
    let records = spec.records(&found, &mut Texts::new())?;
    if !refresh_indexes(&spec, &records)? {
        return stop("written", "Correct the front matter above");
    }
    if spec.cfg.seal == SealMode::Manifest && !seal_files(&spec, &sealed)? {
        return stop("sealed", "Follow the lines above");
    }
    match crate::search::refresh(project) {
        Ok((files, units)) => {
            println!("search index: up to date, {units} unit(s) of {files} file(s)");
        }
        Err(e) => {
            println!("search index: {e}");
            return stop("written", "Correct the search index error above");
        }
    }
    let lines = findings(project, &spec)?;
    for stage in Stage::ALL {
        let here: Vec<&Line> = lines.iter().filter(|l| l.stage == stage).collect();
        for l in &here {
            println!("{}", l.text);
            println!("  fix: {}", l.fix);
        }
        let errors = here.iter().filter(|l| l.error).count();
        if errors > 0 {
            return stop(
                stage.name(),
                &format!("Fix the {errors} new error(s) above"),
            );
        }
    }
    println!(
        "done: links resolve, task named and known, sources cited, sealed{}",
        if lines.is_empty() {
            "."
        } else {
            "; the lines above are not errors."
        }
    );
    Ok(ExitCode::SUCCESS)
}

/// `lab archive index --print <name>`: the index on stdout, with links
/// relative to the project root; nothing is written.
pub fn print_index(project: &Project, name: &str) -> Result<ExitCode, String> {
    let spec = Spec::new(project)?;
    let Some(i) = spec.cfg.index.iter().position(|i| i.name == name) else {
        let names: Vec<&str> = spec.cfg.index.iter().map(|i| i.name.as_str()).collect();
        return Err(format!(
            "no index is named `{name}`; the indexes are: {}",
            names.join(", ")
        ));
    };
    let found = spec.walk(&[])?;
    let records = spec.records(&found, &mut Texts::new())?;
    let invalid: Vec<Problem> = records.iter().filter_map(invalid_problem).collect();
    for p in &invalid {
        eprintln!("{}", p.text());
    }
    let mut at_root = spec.indexes[i].spec.clone();
    at_root.output.clear();
    print!(
        "{}",
        index::render(&at_root, &spec.cfg.statuses, &spec.rows(&records, i))
    );
    Ok(exit(invalid.is_empty()))
}

/// `lab archive log`: the records in date order, or with `record` the
/// commits that changed one record.
pub fn log(
    project: &Project,
    since: Option<&str>,
    record: Option<&str>,
) -> Result<ExitCode, String> {
    let spec = Spec::new(project)?;
    if let Some(d) = since
        && d.parse::<jiff::civil::Date>().is_err()
    {
        return Err(format!("--since must be a day as YYYY-MM-DD, got `{d}`"));
    }
    let found = files::walk(
        spec.root(),
        &spec.indexes.iter().map(|i| &i.files).collect::<Vec<_>>(),
    )?;
    let records = spec.records(&found, &mut Texts::new())?;
    let mut rows = unique_refs(&records.iter().collect::<Vec<_>>());
    if let Some(wanted) = record {
        let plain = wanted.strip_prefix("./").unwrap_or(wanted);
        let Some(r) = rows
            .iter()
            .find(|r| r.path == plain || r.meta.id.as_deref() == Some(wanted))
        else {
            return Err(format!("no indexed record has the id or path `{wanted}`"));
        };
        print!("{}", gitseal::history(spec.root(), &r.path)?);
        return Ok(ExitCode::SUCCESS);
    }
    let undated = rows.iter().filter(|r| r.meta.date.is_none()).count();
    rows.retain(|r| {
        r.meta
            .date
            .as_deref()
            .is_some_and(|d| since.is_none_or(|s| d >= s))
    });
    index::sort(&mut rows, crate::config::SortOrder::Date);
    for r in &rows {
        let m = &r.meta;
        let summary = m.summary.as_deref().or(m.title.as_deref()).unwrap_or("-");
        println!(
            "{}\t{}\t{}\t{}\t{}",
            m.date.as_deref().unwrap_or("-"),
            m.kind.as_deref().unwrap_or("-"),
            m.id.as_deref().unwrap_or("-"),
            r.path,
            summary.split_whitespace().collect::<Vec<_>>().join(" ")
        );
    }
    if undated > 0 {
        eprintln!("{undated} record(s) have no date and are not in the timeline.");
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every archive rule has a stage and a fix in `lab archive done`; the
    /// rules of `lab check` itself (`check-*`) report under `sources cited`.
    #[test]
    fn every_archive_rule_has_a_fix() {
        for r in crate::rules::RULES {
            assert_eq!(fix(r.id).is_some(), !r.id.starts_with("check-"), "{}", r.id);
        }
    }
}
