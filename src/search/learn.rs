//! What `lab retro` learns and remembers (docs/state-machines.md 3.4, 5.7):
//! missed queries become aliases, unused thesaurus entries are retired, and
//! each decision persists in `<state_dir>/lab/retro.json`, so a rejected
//! proposal and a dismissed candidate never return.
//!
//! An accepted alias goes into the record's front matter while the record is
//! open. A finished record is never edited: its alias goes to the thesaurus.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use jiff::{SignedDuration, Timestamp};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::archive::Spec;
use crate::archive::files::FileSet;
use crate::archive::front::Meta;
use crate::config::Project;

use super::missed::{Missed, missed};
use super::query::words;
use super::trace::{self, Line};
use super::{Context, index, thesaurus};

const FILE: &str = "lab/retro.json";
const SCHEMA: &str = "rf.retro/1";

/// A recurring finding rule that `lab retro` no longer proposes as a lesson.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dismissed {
    pub tool: String,
    pub rule: String,
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// An accepted alias proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Accepted {
    pub id: String,
    pub query: String,
    /// The record's path.
    pub target: String,
    /// Where the alias went: `front matter` or `thesaurus`.
    pub wrote: String,
    pub at: String,
}

/// A rejected alias proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rejected {
    pub id: String,
    pub query: String,
    #[serde(default)]
    pub target: Option<String>,
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// `<state_dir>/lab/retro.json`, schema `rf.retro/1`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decisions {
    pub schema: String,
    #[serde(default)]
    pub dismissed: Vec<Dismissed>,
    #[serde(default)]
    pub accepted: Vec<Accepted>,
    #[serde(default)]
    pub rejected: Vec<Rejected>,
}

impl Decisions {
    fn file(project: &Project) -> Result<PathBuf, String> {
        trace::shared_state(project, FILE)
    }

    pub fn load(project: &Project) -> Result<Self, String> {
        let file = Self::file(project)?;
        match std::fs::read_to_string(&file) {
            Ok(text) => {
                let d: Self =
                    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
                if d.schema != SCHEMA {
                    return Err(format!(
                        "{}: schema `{}`, expected `{SCHEMA}`",
                        file.display(),
                        d.schema
                    ));
                }
                Ok(d)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                schema: SCHEMA.into(),
                dismissed: Vec::new(),
                accepted: Vec::new(),
                rejected: Vec::new(),
            }),
            Err(e) => Err(format!("{}: {e}", file.display())),
        }
    }

    fn save(&self, project: &Project) -> Result<PathBuf, String> {
        let file = Self::file(project)?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        crate::archive::write_atomically(&file, format!("{text}\n").as_bytes())?;
        Ok(file)
    }

    /// Whether `lab retro` still proposes this rule as a lesson.
    pub fn is_dismissed(&self, tool: &str, rule: &str) -> bool {
        self.dismissed
            .iter()
            .any(|d| d.tool == tool && d.rule == rule)
    }

    /// The missed queries no decision settled yet.
    fn open<'m>(&self, missed: &'m [Missed]) -> Vec<&'m Missed> {
        missed
            .iter()
            .filter(|m| self.accepted.iter().all(|a| a.query != m.query))
            .filter(|m| self.rejected.iter().all(|r| r.id != m.id))
            .collect()
    }
}

/// The start of `since` in the project's time zone.
fn start_of(project: &Project, since: Option<jiff::civil::Date>) -> Option<Timestamp> {
    since.and_then(|d| d.to_zoned(project.tz.clone()).ok().map(|z| z.timestamp()))
}

fn proposals(project: &Project, since: Option<Timestamp>) -> Result<Vec<Missed>, String> {
    let (lines, _) = trace::read(project)?;
    Ok(missed(&lines, since))
}

/// The sections `lab retro` adds: missed queries with their alias proposals,
/// and thesaurus entries that no query used in `[lab.search] review_days`.
pub fn report(project: &Project, since: Option<jiff::civil::Date>) -> Result<String, String> {
    let decisions = Decisions::load(project)?;
    let (lines, bad) = trace::read(project)?;
    let all = missed(&lines, start_of(project, since));
    let open = decisions.open(&all);
    let mut out = String::new();
    let queries = lines
        .iter()
        .filter(|l| matches!(l, Line::Query(q) if q.source == "cli"))
        .count();
    let _ = writeln!(
        out,
        "{queries} search query(ies) in the trace; {} missed query(ies) without a decision.",
        open.len()
    );
    if bad > 0 {
        let _ = writeln!(out, "{bad} trace line(s) did not parse and were skipped.");
    }
    if !open.is_empty() {
        let _ = writeln!(
            out,
            "Missed queries: `lab retro accept <id>` adds the query as an alias of the target; `lab retro reject <id>` hides it for good."
        );
    }
    for m in &open {
        let target = match &m.target {
            Some(t) => format!("target {t}"),
            None => "no target: accept with --record <path or id>".into(),
        };
        let _ = writeln!(
            out,
            "- {} \"{}\": {} miss(es) ({}); {target}",
            m.id,
            m.query,
            m.count,
            m.reasons.join(", ")
        );
    }
    out.push_str(&unused(project, &lines)?);
    Ok(out)
}

/// Active thesaurus entries that no query of the review window used. Only
/// when the trace reaches back to the window's start.
fn unused(project: &Project, lines: &[Line]) -> Result<String, String> {
    let entries = thesaurus::load(project)?;
    let days = project.lab.search.review_days;
    let Some(start) = Timestamp::now()
        .checked_sub(SignedDuration::from_hours(24 * i64::from(days)))
        .ok()
    else {
        return Ok(String::new());
    };
    let queries: Vec<_> = lines
        .iter()
        .filter_map(|l| match l {
            Line::Query(q) => q.at.parse::<Timestamp>().ok().map(|t| (t, q)),
            Line::Open(_) => None,
        })
        .collect();
    if queries.first().is_none_or(|(t, _)| *t > start) {
        return Ok(String::new());
    }
    let stale: Vec<_> = entries
        .iter()
        .filter(|e| !e.retired)
        .filter(|e| {
            let text = e.text();
            !queries
                .iter()
                .filter(|(t, _)| *t >= start)
                .any(|(_, q)| q.expansions.iter().any(|x| x.entry == text))
        })
        .collect();
    let mut out = String::new();
    if !stale.is_empty() {
        let _ = writeln!(
            out,
            "Thesaurus entries no query used in {days} days: `lab retro retire <line>` keeps them but stops applying them."
        );
    }
    for e in stale {
        let _ = writeln!(out, "- line {}: {}", e.line, e.text());
    }
    Ok(out)
}

/// The path of the record with this id, from the search index.
fn record_path(project: &Project, id: &str) -> Result<Option<String>, String> {
    let ctx = Context::new(project)?;
    let opened = index::open(&ctx, false)?;
    opened
        .conn
        .query_row(
            "SELECT path FROM files WHERE record_id = ?1 COLLATE NOCASE ORDER BY path LIMIT 1",
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())
}

/// A finished record is sealed text, or has a status that ends its work.
fn finished(project: &Project, path: &str, meta: &Meta) -> Result<bool, String> {
    let Some(cfg) = project.lab.archive.as_ref() else {
        return Ok(false);
    };
    if FileSet::new(&cfg.sealed, &[])?.matches(path) {
        return Ok(true);
    }
    let ended = cfg
        .sealed_statuses
        .iter()
        .chain(&cfg.inactive_statuses)
        .chain(cfg.terminal_statuses.iter().flatten());
    Ok(meta
        .status
        .as_ref()
        .is_some_and(|s| ended.into_iter().any(|e| e == s)))
}

/// The file with `alias` added to the `aliases` of its front matter, or
/// `None` when the front matter is missing or its `aliases` is a form this
/// does not edit.
fn add_alias(text: &str, alias: &str) -> Option<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if lines.first()?.trim_end() != "---" {
        return None;
    }
    let close = lines.iter().skip(1).position(|l| l.trim_end() == "---")? + 1;
    // A JSON string is a YAML string.
    let quoted = serde_json::to_string(alias).ok()?;
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    match (1..close).find(|&i| lines[i].starts_with("aliases:")) {
        Some(i) => {
            let rest = lines[i]["aliases:".len()..].trim();
            if let Some(inner) = rest.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
                let inner = inner.trim();
                let list = if inner.is_empty() {
                    quoted
                } else {
                    format!("{inner}, {quoted}")
                };
                out[i] = format!("aliases: [{list}]\n");
            } else if rest.is_empty() {
                let mut end = i + 1;
                while end < close && lines[end].trim_start().starts_with("- ") {
                    end += 1;
                }
                let indent: String = if end > i + 1 {
                    lines[i + 1]
                        .chars()
                        .take_while(|c| c.is_whitespace())
                        .collect()
                } else {
                    "  ".into()
                };
                out.insert(end, format!("{indent}- {quoted}\n"));
            } else {
                return None;
            }
        }
        None => out.insert(close, format!("aliases: [{quoted}]\n")),
    }
    Some(out.concat())
}

/// `lab retro accept`: the query becomes an alias of its target record.
pub fn accept(project: &Project, id: &str, record: Option<&str>) -> Result<ExitCode, String> {
    let mut decisions = Decisions::load(project)?;
    let all = proposals(project, None)?;
    let open = decisions.open(&all);
    let proposal = find(&open, id)?;
    let target = match record {
        Some(r) if project.path(r).is_file() => r.to_string(),
        Some(r) => record_path(project, r)?
            .ok_or_else(|| format!("`{r}` is neither a file nor a record id in the index"))?,
        None => proposal.target.clone().ok_or_else(|| {
            format!(
                "proposal {} has no target: give the record with --record <path or id>",
                proposal.id
            )
        })?,
    };
    let file = project.path(&target);
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let spec = Spec::new(project)?;
    let meta = spec.meta_of(&target, text.as_bytes());
    let query = proposal.query.clone();
    let mut wrote = None;
    if !finished(project, &target, &meta)? {
        // Check the edit by reading the record back.
        let edited = add_alias(&text, &query).filter(|edited| {
            let after = spec.meta_of(&target, edited.as_bytes());
            after.id == meta.id && after.aliases.iter().any(|a| words(a) == words(&query))
        });
        match edited {
            Some(edited) => {
                crate::archive::write_atomically(&file, edited.as_bytes())?;
                println!("Added alias \"{query}\" to the front matter of {target}.");
                wrote = Some("front matter");
            }
            None => println!(
                "{target} has no front matter `aliases` list this can edit; the alias goes to the thesaurus."
            ),
        }
    }
    if wrote.is_none() {
        let rid = meta
            .id
            .as_deref()
            .ok_or_else(|| format!("{target} has no record id, so the thesaurus cannot name it"))?;
        let entries = thesaurus::load(project)?;
        if thesaurus::holds(&entries, &query, rid) {
            println!("The thesaurus already holds \"{query} = {rid}\".");
        } else {
            let path = thesaurus::append(project, &[format!("{query} = {rid}")])?;
            println!(
                "Added \"{query} = {rid}\" to {}: {target} is finished, so it is not edited.",
                path.display()
            );
        }
        wrote = Some("thesaurus");
    }
    decisions.accepted.push(Accepted {
        id: proposal.id.clone(),
        query,
        target,
        wrote: wrote.unwrap_or("thesaurus").into(),
        at: crate::finding::now(),
    });
    decisions.save(project)?;
    Ok(ExitCode::SUCCESS)
}

/// The open proposal whose id starts with `id`.
fn find<'m>(open: &[&'m Missed], id: &str) -> Result<&'m Missed, String> {
    let found: Vec<&&Missed> = open.iter().filter(|m| m.id.starts_with(id)).collect();
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(format!(
            "no open proposal `{id}`; `lab retro` lists the open ones"
        )),
        _ => Err(format!("`{id}` names several proposals; give more digits")),
    }
}

/// `lab retro reject`: the proposals are never proposed again.
pub fn reject(project: &Project, ids: &[String], note: Option<&str>) -> Result<ExitCode, String> {
    let mut decisions = Decisions::load(project)?;
    let all = proposals(project, None)?;
    let open = decisions.open(&all);
    let found: Vec<Missed> = ids
        .iter()
        .map(|id| find(&open, id).cloned())
        .collect::<Result<_, _>>()?;
    for m in found {
        println!("Rejected {} \"{}\".", m.id, m.query);
        decisions.rejected.push(Rejected {
            id: m.id,
            query: m.query,
            target: m.target,
            at: crate::finding::now(),
            note: note.map(String::from),
        });
    }
    decisions.save(project)?;
    Ok(ExitCode::SUCCESS)
}

/// `lab retro dismiss tool:rule`: the rule is no longer a lesson candidate.
pub fn dismiss(project: &Project, key: &str, note: Option<&str>) -> Result<ExitCode, String> {
    let (tool, rule) = key
        .split_once(':')
        .filter(|(t, r)| !t.is_empty() && !r.is_empty())
        .ok_or_else(|| {
            format!("give the candidate as `tool:rule`, as `lab retro` prints it; got `{key}`")
        })?;
    let mut decisions = Decisions::load(project)?;
    if decisions.is_dismissed(tool, rule) {
        println!("{tool}:{rule} is dismissed already.");
        return Ok(ExitCode::SUCCESS);
    }
    decisions.dismissed.push(Dismissed {
        tool: tool.into(),
        rule: rule.into(),
        at: crate::finding::now(),
        note: note.map(String::from),
    });
    let file = decisions.save(project)?;
    println!(
        "Dismissed {tool}:{rule}; `lab retro` no longer proposes it ({}).",
        file.display()
    );
    Ok(ExitCode::SUCCESS)
}

/// `lab retro retire <line>`.
pub fn retire(project: &Project, line: usize) -> Result<ExitCode, String> {
    let text = thesaurus::retire(project, line)?;
    println!("Retired thesaurus entry on line {line}: {text}");
    Ok(ExitCode::SUCCESS)
}

/// `lab retro seed`: an entry `alias = id` for each alias of each record
/// that the thesaurus does not hold yet.
pub fn seed(project: &Project) -> Result<ExitCode, String> {
    let ctx = Context::new(project)?;
    let opened = index::open(&ctx, false)?;
    let files: Vec<(String, String)> = {
        let mut stmt = opened
            .conn
            .prepare("SELECT path, record_id FROM files WHERE record_id IS NOT NULL ORDER BY path")
            .map_err(|e| e.to_string())?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?
    };
    let mut entries = thesaurus::load(project)?;
    let mut lines = Vec::new();
    for (path, id) in files {
        let Ok(bytes) = std::fs::read(project.path(&path)) else {
            continue;
        };
        for alias in ctx.spec.meta_of(&path, &bytes).aliases {
            let alias = alias.trim().to_string();
            if alias.is_empty()
                || words(&alias) == words(&id)
                || thesaurus::holds(&entries, &alias, &id)
            {
                continue;
            }
            lines.push(format!("{alias} = {id}"));
            entries.push(thesaurus::Entry {
                terms: vec![words(&alias), words(&id)],
                retired: false,
                line: 0,
            });
        }
    }
    if lines.is_empty() {
        println!("The thesaurus holds every record alias already.");
        return Ok(ExitCode::SUCCESS);
    }
    let file = thesaurus::append(project, &lines)?;
    println!("Added {} entry(ies) to {}.", lines.len(), file.display());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_an_alias_to_each_form_of_list() {
        let flow = "---\nid: D1\naliases: [a]\n---\nbody\n";
        assert_eq!(
            add_alias(flow, "fy shift").unwrap(),
            "---\nid: D1\naliases: [a, \"fy shift\"]\n---\nbody\n"
        );
        let block = "---\nid: D1\naliases:\n  - a\ntitle: T\n---\n";
        assert_eq!(
            add_alias(block, "b").unwrap(),
            "---\nid: D1\naliases:\n  - a\n  - \"b\"\ntitle: T\n---\n"
        );
        let none = "---\nid: D1\n---\n";
        assert_eq!(
            add_alias(none, "b").unwrap(),
            "---\nid: D1\naliases: [\"b\"]\n---\n"
        );
        assert_eq!(add_alias("# no front matter\n", "b"), None);
        assert_eq!(add_alias("---\naliases: one\n---\n", "b"), None);
    }
}
