//! The rules of archive model 2 (`[lab.archive] model = 2`): record metadata,
//! the record status machine, the link between a record and its board task,
//! and pages. See docs/interfaces.md, "Archive model 2".

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::Duration;

use super::files::{self, FileSet};
use super::front::{self, Meta};
use super::rules::{Problem, Target, resolve};
use super::{Record, Spec, Texts, unique_refs};
use crate::config::{ArchiveConfig, CONFIG_FILE, Field, SealMode};
use crate::finding::Severity;

/// The rules an `[[lab.archive.allow]]` entry may name.
pub(super) const ALLOWED: [&str; 7] = [
    "duplicate-id",
    "missing-summary",
    "unindexed-file",
    "no-current-page",
    "historical-link",
    "task-without-record",
    "open-item",
];

/// How long the board export may run.
const BOARD_TIMEOUT: Duration = Duration::from_secs(30);

/// Checks the keys of archive model 2. With `model = 1` none of them may be set.
pub(super) fn validate(cfg: &ArchiveConfig) -> Result<(), String> {
    let err = |m: String| Err(format!("[lab.archive] {m}"));
    let defaults = ArchiveConfig::model_2_defaults();
    if cfg.model == 1 {
        let set: Vec<&str> = [
            ("seal", cfg.seal != SealMode::Manifest),
            ("seal_baseline", cfg.seal_baseline.is_some()),
            ("sealed_statuses", !cfg.sealed_statuses.is_empty()),
            ("terminal_statuses", cfg.terminal_statuses.is_some()),
            ("reason_statuses", cfg.reason_statuses.is_some()),
            ("accepted_edit", !cfg.accepted_edit.is_empty()),
            ("write_indexes", !cfg.write_indexes),
            ("kinds", cfg.kinds != defaults.kinds),
            ("task_from", cfg.task_from.is_some()),
            ("board_export", cfg.board_export.is_some()),
            ("research_tag", cfg.research_tag.is_some()),
            (
                "summary_min_chars",
                cfg.summary_min_chars != defaults.summary_min_chars,
            ),
            ("unindexed_exclude", !cfg.unindexed_exclude.is_empty()),
            ("page_group", !cfg.page_group.is_empty()),
            ("historical_link", cfg.historical_link.is_some()),
            ("allow", !cfg.allow.is_empty()),
            (
                "index `kinds`",
                cfg.index.iter().any(|i| !i.kinds.is_empty()),
            ),
        ]
        .into_iter()
        .filter_map(|(key, set)| set.then_some(key))
        .collect();
        if !set.is_empty() {
            return err(format!(
                "{} need(s) `model = 2` (docs/interfaces.md, \"Archive model 2\")",
                set.join(", ")
            ));
        }
        if cfg.index.iter().any(|i| i.output.is_empty()) {
            return err("every index needs an `output`".into());
        }
        return Ok(());
    }
    if cfg.model != 2 {
        return err(format!("`model` must be 1 or 2, got {}", cfg.model));
    }
    let known =
        |what: &str, statuses: &[String]| match statuses.iter().find(|s| !cfg.statuses.contains(s))
        {
            Some(s) => Err(format!(
                "[lab.archive] {what} status `{s}` is not in `statuses`"
            )),
            None => Ok(()),
        };
    known("sealed", &cfg.sealed_statuses)?;
    known(
        "terminal",
        cfg.terminal_statuses.as_deref().unwrap_or_default(),
    )?;
    known("reason", cfg.reason_statuses.as_deref().unwrap_or_default())?;
    if let Some(h) = &cfg.historical_link {
        known("[lab.archive.historical_link] `from`", &h.from)?;
        known("[lab.archive.historical_link] `to`", &h.to)?;
        if h.marks.iter().any(|m| m.trim().is_empty()) || h.marks.is_empty() {
            return err("[lab.archive.historical_link] `marks` needs at least one word".into());
        }
    }
    match cfg.seal {
        SealMode::Git if cfg.seal_manifest.is_some() => {
            return err(
                "`seal = \"git\"` reads no seal manifest; remove `seal_manifest` (and the \
                 manifest file)"
                    .into(),
            );
        }
        SealMode::Manifest
            if cfg.seal_baseline.is_some()
                || !cfg.sealed_statuses.is_empty()
                || cfg.terminal_statuses.is_some()
                || !cfg.accepted_edit.is_empty() =>
        {
            return err(
                "`seal_baseline`, `sealed_statuses`, `terminal_statuses` and `accepted_edit` \
                 need `seal = \"git\"`"
                    .into(),
            );
        }
        _ => {}
    }
    for a in &cfg.accepted_edit {
        let what = format!("[[lab.archive.accepted_edit]] `{}`", a.path);
        if a.path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        {
            return err(format!(
                "{what}: `path` must be a relative path below the project root"
            ));
        }
        if a.commit.len() < 7 || !a.commit.chars().all(|c| c.is_ascii_hexdigit()) {
            return err(format!(
                "{what}: `commit` must be a commit hash, or at least its first 7 characters"
            ));
        }
        if a.reason.trim().is_empty() {
            return err(format!("{what}: `reason` must say why the edit stays"));
        }
    }
    let unique = |values: &[String]| values.iter().collect::<BTreeSet<_>>().len() == values.len();
    if cfg.kinds.is_empty() || !unique(&cfg.kinds) {
        return err("`kinds` must be a non-empty list without repeats".into());
    }
    for i in &cfg.index {
        if let Some(k) = i.kinds.iter().find(|k| !cfg.kinds.contains(k)) {
            return err(format!("index `{}`: kind `{k}` is not in `kinds`", i.name));
        }
        if i.output.is_empty() && cfg.write_indexes {
            return err(format!(
                "index `{}` needs an `output`, or set `write_indexes = false`",
                i.name
            ));
        }
    }
    if let Some(d) = &cfg.task_from
        && d.parse::<jiff::civil::Date>().is_err()
    {
        return err(format!(
            "`task_from` must be a day as YYYY-MM-DD, got `{d}`"
        ));
    }
    if cfg.board_export.as_ref().is_some_and(|c| c.is_empty()) {
        return err("`board_export` needs a program".into());
    }
    if cfg.research_tag.is_some() && cfg.board_export.is_none() {
        return err("`research_tag` needs `board_export`".into());
    }
    let mut names = BTreeSet::new();
    for g in &cfg.page_group {
        if g.name.trim().is_empty() || !names.insert(&g.name) {
            return err("every page_group needs a non-empty, unique `name`".into());
        }
        if g.include.is_empty() || g.current.is_empty() {
            return err(format!(
                "page_group `{}`: `include` and `current` must not be empty",
                g.name
            ));
        }
    }
    let mut seen = BTreeSet::new();
    for a in &cfg.allow {
        let what = format!("[[lab.archive.allow]] `{}` `{}`", a.rule, a.subject);
        if !ALLOWED.contains(&a.rule.as_str()) {
            return err(format!(
                "{what}: `rule` must be one of: {}",
                ALLOWED.join(", ")
            ));
        }
        if a.subject.trim().is_empty() {
            return err(format!("{what}: `subject` must not be empty"));
        }
        if a.target.is_some() && a.rule != "historical-link" {
            return err(format!("{what}: only `historical-link` takes a `target`"));
        }
        if a.reason.trim().is_empty() {
            return err(format!(
                "{what}: `reason` must say why the finding is accepted"
            ));
        }
        if !seen.insert((&a.rule, &a.subject, &a.target)) {
            return err(format!("{what}: the entry is configured twice"));
        }
    }
    Ok(())
}

pub(super) fn problems(
    spec: &Spec,
    records: &[Record],
    texts: &mut Texts,
    out: &mut Vec<Problem>,
) -> Result<(), String> {
    let unique = unique_refs(&records.iter().collect::<Vec<_>>());
    let valid: Vec<&Record> = unique
        .iter()
        .copied()
        .filter(|r| r.invalid.is_none())
        .collect();
    fields(spec, &valid, out);
    status(spec, &valid, out);
    amendments(&valid, out);
    summaries(spec, records, &valid, out);
    tasks(spec, &valid, out);
    historical_links(spec, &unique, texts, out)?;
    pages(spec, out)?;
    allow(spec, out);
    Ok(())
}

/// The model 2 fields that have the wrong type, and a `kind` that is not allowed.
fn fields(spec: &Spec, records: &[&Record], out: &mut Vec<Problem>) {
    for r in records {
        for e in &r.meta.model_2_errors {
            out.push(Problem::new(
                "front-matter-invalid",
                Severity::Error,
                &r.path,
                e.clone(),
            ));
        }
        if let Some(k) = &r.meta.kind
            && !spec.cfg.kinds.contains(k)
        {
            out.push(Problem::new(
                "front-matter-invalid",
                Severity::Error,
                &r.path,
                format!("`kind` `{k}` is not one of: {}", spec.cfg.kinds.join(", ")),
            ));
        }
    }
}

/// An inactive record names its successor or the reason it was withdrawn.
fn status(spec: &Spec, records: &[&Record], out: &mut Vec<Problem>) {
    let needs_reason = spec
        .cfg
        .reason_statuses
        .as_ref()
        .unwrap_or(&spec.cfg.inactive_statuses);
    for r in records {
        let m = &r.meta;
        if let Some(s) = m.status.as_ref().filter(|s| needs_reason.contains(s))
            && m.superseded_by.is_empty()
            && m.withdrawn_reason.is_none()
        {
            out.push(Problem::new(
                "inactive-without-reason",
                Severity::Error,
                &r.path,
                format!(
                    "has status `{s}` but names no successor and no reason; add \
                     `superseded_by: <id>` or `withdrawn_reason: <why>` to the front matter"
                ),
            ));
        }
    }
}

/// The ids one side of an amendment lists.
type Side = fn(&Meta) -> &Vec<String>;

/// `amends` and `amended_by` name known records and agree with each other.
fn amendments(records: &[&Record], out: &mut Vec<Problem>) {
    let mut by_id: BTreeMap<&str, Vec<&Meta>> = BTreeMap::new();
    for r in records {
        if let Some(id) = &r.meta.id {
            by_id.entry(id).or_default().push(&r.meta);
        }
    }
    let lists = |id: &str, pick: Side, wanted: &str| {
        by_id[id]
            .iter()
            .any(|m| pick(m).iter().any(|x| x == wanted))
    };
    for r in records {
        let m = &r.meta;
        let sides: [(&str, &Vec<String>, Side, &str); 2] = [
            ("amends", &m.amends, |m| &m.amended_by, "amended_by"),
            ("amended_by", &m.amended_by, |m| &m.amends, "amends"),
        ];
        for (field, ids, other_side, other_field) in sides {
            for other in ids {
                if !by_id.contains_key(other.as_str()) {
                    out.push(Problem::new(
                        "unknown-id",
                        Severity::Error,
                        &r.path,
                        format!("`{field}` names {other}, which no indexed record has"),
                    ));
                } else if let Some(me) = &m.id
                    && !lists(other, other_side, me)
                {
                    out.push(Problem::new(
                        "amendment-mismatch",
                        Severity::Warn,
                        &r.path,
                        format!(
                            "`{field}` names {other}, but {other}'s `{other_field}` does not \
                             list {me}"
                        ),
                    ));
                }
            }
        }
    }
}

/// A record has a summary of the configured length. A record whose index
/// requires `summary` and that has none already has a `missing-field` error.
fn summaries(spec: &Spec, all: &[Record], records: &[&Record], out: &mut Vec<Problem>) {
    let min = spec.cfg.summary_min_chars;
    for r in records {
        let message = match &r.meta.summary {
            None => {
                let required = all.iter().any(|x| {
                    x.path == r.path && spec.indexes[x.index].spec.require.contains(&Field::Summary)
                });
                if required {
                    continue;
                }
                "has no `summary`; add one sentence that says what the record found or decided"
                    .to_string()
            }
            Some(s) if s.chars().count() < min => format!(
                "its `summary` is shorter than {min} characters; say what the record found or \
                 decided"
            ),
            Some(_) => continue,
        };
        out.push(Problem::new(
            "missing-summary",
            Severity::Warn,
            &r.path,
            message,
        ));
    }
}

/// One task of the board export.
struct Task {
    id: String,
    done: bool,
    tags: Vec<String>,
}

/// Runs `board_export` and reads its JSON: an array of tasks, or an object
/// with a `tasks` array. The error says why the board gave no task ids.
fn board_tasks(spec: &Spec, command: &[String]) -> Result<Vec<Task>, String> {
    let (program, args) = command.split_first().ok_or("`board_export` is empty")?;
    let mut child = Command::new(spec.project.resolve_program(program))
        .args(args)
        .current_dir(spec.root())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("`{program}` did not start: {e}"))?;
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = Vec::new();
        let read = stdout.read_to_end(&mut text).map(|_| text);
        let _ = tx.send(read);
    });
    let text = match rx.recv_timeout(BOARD_TIMEOUT) {
        Ok(read) => read.map_err(|e| format!("`{program}`: {e}"))?,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "`{program}` did not finish in {} seconds",
                BOARD_TIMEOUT.as_secs()
            ));
        }
    };
    let status = child.wait().map_err(|e| format!("`{program}`: {e}"))?;
    if !status.success() {
        return Err(format!("`{program}` failed ({status})"));
    }
    let value: serde_json::Value = serde_json::from_slice(&text)
        .map_err(|e| format!("`{program}` did not print JSON: {e}"))?;
    let list = value
        .as_array()
        .or_else(|| value.get("tasks").and_then(|t| t.as_array()))
        .ok_or_else(|| format!("`{program}` printed no array of tasks"))?;
    Ok(list
        .iter()
        .filter_map(|t| {
            let strings = |key: &str| {
                t.get(key)
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(String::from))
            };
            Some(Task {
                id: t.get("id")?.as_str()?.to_string(),
                done: t.get("status").and_then(|s| s.as_str()) == Some("done"),
                tags: strings("tags").chain(strings("labels")).collect(),
            })
        })
        .collect())
}

/// A record names its task and kind, and the task is on the board.
fn tasks(spec: &Spec, records: &[&Record], out: &mut Vec<Problem>) {
    let cfg = spec.cfg;
    if let Some(from) = &cfg.task_from {
        for r in records {
            if r.meta.date.as_ref().is_none_or(|d| d < from) {
                continue;
            }
            for (rule, missing, field, hint) in [
                (
                    "missing-task",
                    r.meta.tasks.is_empty(),
                    "task",
                    "the id of the board task the record belongs to".to_string(),
                ),
                (
                    "missing-kind",
                    r.meta.kind.is_none(),
                    "kind",
                    format!("one of: {}", cfg.kinds.join(", ")),
                ),
            ] {
                if missing {
                    out.push(Problem::new(
                        rule,
                        Severity::Error,
                        &r.path,
                        format!(
                            "is dated on or after {from} and has no `{field}`; add `{field}:` \
                             with {hint}"
                        ),
                    ));
                }
            }
        }
    }
    let named: BTreeSet<&str> = records
        .iter()
        .flat_map(|r| r.meta.tasks.iter().map(String::as_str))
        .collect();
    // The state `skipped`: neither an error nor a silent pass.
    let mut skipped = |message: &str, detail: Option<String>| {
        let mut p = Problem::new(
            "task-link-skipped",
            Severity::Info,
            CONFIG_FILE,
            message.into(),
        );
        p.detail = detail;
        out.push(p);
    };
    let Some(command) = &cfg.board_export else {
        if cfg.task_from.is_some() || !named.is_empty() {
            skipped(
                "skipped: no `board_export` is configured, so no `task` value was compared \
                 with the board",
                None,
            );
        }
        return;
    };
    let board = match board_tasks(spec, command) {
        Ok(tasks) => tasks,
        Err(reason) => {
            skipped(
                "skipped: the board export gave no task ids, so no `task` value was compared \
                 with the board",
                Some(reason),
            );
            return;
        }
    };
    let ids: BTreeSet<&str> = board.iter().map(|t| t.id.as_str()).collect();
    for r in records {
        for task in r.meta.tasks.iter().filter(|t| !ids.contains(t.as_str())) {
            out.push(Problem::new(
                "unknown-task",
                Severity::Error,
                &r.path,
                format!(
                    "`task` names {task}, which the board export does not list; correct the id"
                ),
            ));
        }
    }
    if let Some(tag) = &cfg.research_tag {
        for t in board
            .iter()
            .filter(|t| t.done && t.tags.contains(tag) && !named.contains(t.id.as_str()))
        {
            let mut p = Problem::new(
                "task-without-record",
                Severity::Warn,
                CONFIG_FILE,
                format!(
                    "task {} is done and tagged `{tag}`, but no record names it; write the \
                     record, or add `task: {}` to the record that holds its result",
                    t.id, t.id
                ),
            );
            p.subject = Some(t.id.clone());
            out.push(p);
        }
    }
}

/// A link from a page with a current status to a page with a historical
/// status carries a mark on its line.
fn historical_links(
    spec: &Spec,
    records: &[&Record],
    texts: &mut Texts,
    out: &mut Vec<Problem>,
) -> Result<(), String> {
    let Some(rule) = &spec.cfg.historical_link else {
        return Ok(());
    };
    let has = |r: &Record, statuses: &[String]| {
        r.meta.status.as_ref().is_some_and(|s| statuses.contains(s))
    };
    let targets: HashMap<&str, &Record> = records
        .iter()
        .filter(|r| has(r, &rule.to))
        .map(|r| (r.path.as_str(), *r))
        .collect();
    let marks: Vec<String> = rule.marks.iter().map(|m| m.to_lowercase()).collect();
    for r in records.iter().filter(|r| has(r, &rule.from)) {
        let abs = spec.root().join(&r.path);
        let doc = texts.doc(&abs)?;
        let bytes = texts.bytes(&abs)?;
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.lines().collect();
        let dir = r.path.rsplit_once('/').map_or("", |(d, _)| d);
        for link in &doc.links {
            let Target::There { rel: Some(rel), .. } = resolve(spec.root(), dir, &link.dest) else {
                continue;
            };
            let Some(to) = targets.get(rel.as_str()) else {
                continue;
            };
            let line = lines.get(link.line - 1).unwrap_or(&"").to_lowercase();
            if marks.iter().any(|m| line.contains(m)) {
                continue;
            }
            let mut p = Problem::new(
                "historical-link",
                Severity::Warn,
                &r.path,
                format!(
                    "links {} ({}) without a mark; say on the line that it is {}, or link \
                     the page that replaced it",
                    to.meta.id.as_deref().unwrap_or(&to.path),
                    to.meta.status.as_deref().unwrap_or_default(),
                    rule.marks.join(" or ")
                ),
            )
            .at(link.line);
            p.target = Some(link.dest.clone());
            out.push(p);
        }
    }
    Ok(())
}

/// The directory pattern of an index glob with `*.md` as its last part:
/// the Markdown files next to the index's records.
fn siblings(include: &str) -> String {
    match include.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/*.md"),
        None => "*.md".into(),
    }
}

/// Markdown files next to an index's records that no index lists, and page
/// groups without a current page.
fn pages(spec: &Spec, out: &mut Vec<Problem>) -> Result<(), String> {
    let cfg = spec.cfg;
    let err = |what: &str, e: String| format!("[lab.archive] {what}: {e}");
    let near: Vec<String> = cfg
        .index
        .iter()
        .flat_map(|i| i.include.iter().map(|g| siblings(g)))
        .collect();
    let near = FileSet::new(&near, &cfg.unindexed_exclude).map_err(|e| err("index", e))?;
    let groups = cfg
        .page_group
        .iter()
        .map(|g| FileSet::new(&g.include, &g.exclude).map_err(|e| err("page_group", e)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut sets = vec![&near];
    sets.extend(groups.iter());
    let found = files::walk(spec.root(), &sets)?;
    for path in &found[0] {
        // A file that an index excludes was left out on purpose.
        let known = cfg.index.iter().any(|i| i.output == *path)
            || cfg.start.as_deref() == Some(path)
            || spec.indexes.iter().any(|i| i.files.names(path));
        if !known {
            out.push(Problem::new(
                "unindexed-file",
                Severity::Warn,
                path,
                "is next to indexed records, but no index lists or excludes it; add it to an \
                 index's `include` or `exclude`, or to `unindexed_exclude`"
                    .into(),
            ));
        }
    }
    for (g, paths) in cfg.page_group.iter().zip(&found[1..]) {
        let mut current = false;
        for path in paths {
            let bytes =
                std::fs::read(spec.root().join(path)).map_err(|e| format!("{path}: {e}"))?;
            current |= front::status_of(&bytes).is_some_and(|s| g.current.contains(&s));
        }
        if !current {
            let mut p = Problem::new(
                "no-current-page",
                Severity::Warn,
                CONFIG_FILE,
                format!(
                    "page group `{}` has no page with status {}; write the current page, or \
                     set the status of the page that is current",
                    g.name,
                    g.current.join(" or ")
                ),
            );
            p.subject = Some(g.name.clone());
            p.data = Some(serde_json::json!({"pages": paths.len()}));
            out.push(p);
        }
    }
    Ok(())
}

/// Turns each finding that an `[[lab.archive.allow]]` entry names into a
/// note with the entry's reason. An entry that names no finding is reported.
fn allow(spec: &Spec, out: &mut Vec<Problem>) {
    for a in &spec.cfg.allow {
        let mut matched = false;
        for p in out.iter_mut().filter(|p| {
            p.rule == a.rule
                && p.subject_plain() == a.subject
                && a.target
                    .as_ref()
                    .is_none_or(|t| p.target.as_ref() == Some(t))
        }) {
            matched = true;
            p.severity = Severity::Info;
            p.message = format!("{}; accepted by [[lab.archive.allow]]", p.message);
            p.detail = Some(a.reason.clone());
            let mut data = p.data.take().unwrap_or_else(|| serde_json::json!({}));
            data["reason"] = a.reason.clone().into();
            p.data = Some(data);
        }
        if !matched {
            let mut p = Problem::new(
                "stale-allow",
                Severity::Warn,
                CONFIG_FILE,
                format!(
                    "[[lab.archive.allow]] entry for `{}` `{}` matches no finding; remove the \
                     entry",
                    a.rule, a.subject
                ),
            );
            p.subject = Some(format!("{}:{}", a.rule, a.subject));
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::siblings;

    #[test]
    fn sibling_patterns_keep_the_directory_of_the_glob() {
        assert_eq!(siblings("docs/decisions/DR*.md"), "docs/decisions/*.md");
        assert_eq!(
            siblings("docs/followups/*/README.md"),
            "docs/followups/*/*.md"
        );
        assert_eq!(siblings("*.md"), "*.md");
    }
}
