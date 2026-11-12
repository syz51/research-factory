//! Scopes `sessions` and `board`: the student's messages in agent sessions
//! (the session list of `agentprof scan`, `rf.session/1`) and the tasks and
//! notes of the work board (the configured board export command).
//!
//! Both are class `chat`. A board that cannot be read, or a cached export
//! older than `board_max_age`, makes the answer partial, never an error
//! (docs/state-machines.md 3.6 and 7.2).

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::sources::{Doc, Listing, Loaded, Resolver, Row, What, file_stamp, sha256};
use super::units::Unit;
use super::{Class, Context, Scope, board};

/// A message longer than this many characters is cut.
const MESSAGE_CHARS: usize = 8000;

/// A session of the `sessions` scope.
pub struct SessionDoc {
    title: Option<String>,
    start: Option<String>,
    /// The transcript path as the session list gives it.
    listed: String,
    file: PathBuf,
}

/// `~/x` as a path below the home directory.
fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .map_or_else(|| PathBuf::from(path), |home| Path::new(&home).join(rest)),
        None => PathBuf::from(path),
    }
}

pub fn list_sessions(ctx: &Context, resolver: &mut Resolver) -> Result<Listing, String> {
    let Some(rel) = &ctx.sessions else {
        return Ok(Listing::unavailable(
            "scope `sessions`: no session list is configured ([lab] agent_sessions or agent_findings)"
                .into(),
        ));
    };
    resolver.prepare([rel.as_str()])?;
    let Some(path) = resolver.get(rel) else {
        return Ok(Listing::unavailable(format!(
            "scope `sessions`: {rel} does not exist yet (run `agentprof scan`)"
        )));
    };
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut docs = Vec::new();
    // A list that names a session twice indexes it once.
    let mut seen = HashSet::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let s = |p: &str| v.pointer(p).and_then(Value::as_str).map(String::from);
        // Subagent sessions hold the agent's prompts, not the student's words.
        if s("/role").as_deref() == Some("subagent")
            || s("/transcript/status").as_deref() != Some("present")
        {
            continue;
        }
        let (Some(source), Some(id), Some(listed)) =
            (s("/source"), s("/id"), s("/transcript/path"))
        else {
            continue;
        };
        let key = format!("session:{source}:{id}");
        let file = expand(&listed);
        let Some(stamp) = file_stamp(&file) else {
            continue;
        };
        if !seen.insert(key.clone()) {
            continue;
        }
        docs.push(Doc {
            scope: Scope::Sessions,
            key,
            stamp,
            what: What::Session(Box::new(SessionDoc {
                title: s("/title"),
                start: s("/start"),
                listed,
                file,
            })),
        });
    }
    Ok(Listing::complete(docs))
}

/// The text a person typed in one transcript record, if it holds any:
/// Claude Code `user` records that are not meta or sidechain, and Codex
/// `response_item` user messages. Tool results and injected context are not.
fn user_text(v: &Value) -> Option<String> {
    let blocks = |content: &Value, kind: &str| -> Option<String> {
        match content {
            Value::String(s) => Some(s.clone()),
            Value::Array(items) => {
                let texts: Vec<&str> = items
                    .iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some(kind))
                    .filter_map(|b| b.get("text").and_then(Value::as_str))
                    .collect();
                (!texts.is_empty()).then(|| texts.join("\n"))
            }
            _ => None,
        }
    };
    let flag = |k: &str| v.get(k).and_then(Value::as_bool) == Some(true);
    let text = match v.get("type").and_then(Value::as_str) {
        Some("user") if !flag("isMeta") && !flag("isSidechain") => {
            blocks(v.pointer("/message/content")?, "text")?
        }
        Some("response_item")
            if v.pointer("/payload/type").and_then(Value::as_str) == Some("message")
                && v.pointer("/payload/role").and_then(Value::as_str) == Some("user") =>
        {
            blocks(v.pointer("/payload/content")?, "input_text")?
        }
        _ => return None,
    };
    let text = text.trim();
    let injected = ["<", "# AGENTS.md", "[Request interrupted", "Caveat:"];
    if text.is_empty() || injected.iter().any(|p| text.starts_with(p)) {
        return None;
    }
    Some(match text.char_indices().nth(MESSAGE_CHARS) {
        Some((cut, _)) => text[..cut].to_string(),
        None => text.to_string(),
    })
}

/// Openings of user-role messages that an agent or a tool wrote, by rule.
const LEFT_OUT: &[(&str, &[&str])] = &[
    (
        "compaction",
        &["This session is being continued from a previous conversation"],
    ),
    (
        "brief",
        &[
            "Act as the ",
            "Goal: do board task",
            "You own board task",
            "Work board task",
        ],
    ),
    (
        "wake-up",
        &["the user answered ask", "Continue where you left off"],
    ),
];
/// A message of fewer words is too short to find.
const MIN_WORDS: usize = 3;

/// The rule that leaves a message out of the scope, if one does.
fn left_out(text: &str) -> Option<&'static str> {
    LEFT_OUT
        .iter()
        .find(|(_, openings)| openings.iter().any(|o| text.starts_with(o)))
        .map(|(rule, _)| *rule)
        .or_else(|| (text.split_whitespace().count() < MIN_WORDS).then_some("short"))
}

/// One unit per message the student typed, at its transcript line. The
/// messages left out are counted by rule in the row's `info.left_out`.
pub fn load_session(s: &SessionDoc) -> Result<Option<Loaded>, String> {
    let bytes = match std::fs::read(&s.file) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", s.file.display())),
    };
    let text = String::from_utf8_lossy(&bytes);
    let date = s
        .start
        .as_deref()
        .map(|d| d.chars().take(10).collect::<String>());
    let heading = [date.clone(), s.title.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let mut dropped: BTreeMap<&str, usize> = BTreeMap::new();
    let units: Vec<Unit> = text
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let body = user_text(&serde_json::from_str(line).ok()?)?;
            if let Some(rule) = left_out(&body) {
                *dropped.entry(rule).or_default() += 1;
                return None;
            }
            Some(Unit {
                start: i + 1,
                end: i + 1,
                heading: heading.clone(),
                body,
            })
        })
        .collect();
    let hash = sha256(
        units
            .iter()
            .map(|u| format!("{}\n{}\n", u.start, u.body))
            .chain([heading.clone()])
            .collect::<String>()
            .as_bytes(),
    );
    Ok(Some(Loaded {
        row: Row {
            class: Some(Class::Chat),
            title: s.title.clone(),
            date,
            kind: Some("session".into()),
            anchor: Some(s.listed.clone()),
            info: (!dropped.is_empty()).then(|| json!({ "left_out": dropped })),
            ..Row::default()
        },
        units,
        hash,
        stale: Vec::new(),
        note: None,
    }))
}

/// The board's tasks from the cached export, or a notice why the board could
/// not be read. A query never waits for the export: a cache older than
/// `board_max_age`, or none, starts a detached refresh (board.rs).
pub fn list_board(ctx: &Context) -> Listing {
    let Some(command) = &ctx.board_export else {
        return Listing::unavailable(
            "scope `board`: no board export command is configured ([lab.search] board_export \
             or [lab.archive] board_export)"
                .into(),
        );
    };
    let max_age = board::max_age(ctx.project);
    let cache = board::read(&ctx.dir, command);
    if let Some(c) = &cache
        && c.age < max_age
    {
        return Listing::complete(board_docs(&c.tasks));
    }
    let refresh = match board::start(ctx.project, &ctx.dir) {
        Ok(board::Refresh::Started) => "a refresh started".to_string(),
        Ok(board::Refresh::Running) => "a refresh is running".to_string(),
        Ok(board::Refresh::Waiting) => format!(
            "the next refresh starts after {} from the last failure",
            board::age_text(board::RETRY_AFTER)
        ),
        Err(e) => format!("a refresh did not start: {e}"),
    };
    let failed = board::last_error(&ctx.dir)
        .map(|(e, ago)| {
            format!(
                "; the last refresh failed {} ago: {e}",
                board::age_text(ago)
            )
        })
        .unwrap_or_default();
    match cache {
        Some(c) => {
            let mut listing = Listing::complete(board_docs(&c.tasks));
            listing.notices.push(format!(
                "scope `board`: the board export is {} old, older than board_max_age ({}); \
                 the answer uses it and {refresh}{failed}",
                board::age_text(c.age),
                board::age_text(max_age),
            ));
            listing
        }
        None => Listing::unavailable(format!(
            "scope `board`: no board export is cached yet; {refresh} \
             (`lab search --refresh-board` waits for it){failed}"
        )),
    }
}

/// A doc for each task; an export that names a task twice indexes it once.
fn board_docs(tasks: &[Value]) -> Vec<Doc> {
    let mut seen = HashSet::new();
    tasks
        .iter()
        .filter_map(|t| {
            let id = t.get("id")?.as_str()?.to_string();
            seen.insert(id.clone()).then_some(())?;
            Some(Doc {
                scope: Scope::Board,
                key: format!("board:{id}"),
                stamp: sha256(t.to_string().as_bytes()),
                what: What::Task(Box::new(t.clone())),
            })
        })
        .collect()
}

/// A task: one unit of its title, body, acceptance and summary, and one per
/// note, numbered from 1 in the order the export gives them.
pub fn load_task(t: &Value) -> Loaded {
    let s = |k: &str| {
        t.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let id = s("id").unwrap_or_default().to_string();
    let title = s("title").map(String::from);
    let main = [s("title"), s("body"), s("acceptance"), s("summary")]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    let mut units = vec![Unit {
        start: 0,
        end: 0,
        heading: String::new(),
        body: main,
    }];
    let notes = t
        .get("notes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for (n, note) in notes.enumerate() {
        let field = |k: &str| note.get(k).and_then(Value::as_str).unwrap_or_default();
        let text = match note {
            Value::String(s) => s.as_str(),
            _ => field("text"),
        };
        if text.trim().is_empty() {
            continue;
        }
        let date: String = field("at").chars().take(10).collect();
        units.push(Unit {
            start: n + 1,
            end: n + 1,
            heading: [field("kind"), &date]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · "),
            body: text.trim().to_string(),
        });
    }
    Loaded {
        row: Row {
            class: Some(Class::Chat),
            record_id: Some(id.clone()),
            title,
            summary: s("status").map(String::from),
            date: s("updated_at")
                .or(s("created_at"))
                .map(|d| d.chars().take(10).collect()),
            status: None,
            kind: Some("task".into()),
            info: Some(json!({ "status": s("status") })),
            ids: id,
            ..Row::default()
        },
        units,
        hash: sha256(t.to_string().as_bytes()),
        stale: Vec::new(),
        note: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_typed_messages_and_skips_tool_results_and_injected_text() {
        let text = |line: &str| user_text(&serde_json::from_str(line).unwrap());
        assert_eq!(
            text(r#"{"type":"user","message":{"content":"Why does HiREC win?"}}"#).as_deref(),
            Some("Why does HiREC win?")
        );
        assert_eq!(
            text(r#"{"type":"user","message":{"content":[{"type":"text","text":"a"},{"type":"tool_result","content":"x"}]}}"#).as_deref(),
            Some("a")
        );
        assert!(
            text(r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"x"}]}}"#)
                .is_none()
        );
        assert!(text(r#"{"type":"user","isMeta":true,"message":{"content":"m"}}"#).is_none());
        assert!(
            text(r#"{"type":"user","message":{"content":"<command-name>/x</command-name>"}}"#)
                .is_none()
        );
        assert!(text(r#"{"type":"assistant","message":{"content":"no"}}"#).is_none());
        assert_eq!(
            text(r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"codex q"}]}}"#).as_deref(),
            Some("codex q")
        );
    }

    #[test]
    fn injected_and_short_messages_are_left_out_by_rule() {
        assert_eq!(
            left_out("This session is being continued from a previous conversation that ran out"),
            Some("compaction")
        );
        assert_eq!(
            left_out("Act as the research sub-agent for task 12."),
            Some("brief")
        );
        assert_eq!(left_out("the user answered ask 4: use B"), Some("wake-up"));
        assert_eq!(left_out("yes please"), Some("short"));
        assert_eq!(left_out("Why does HiREC win?"), None);
    }

    #[test]
    fn a_task_gives_a_unit_and_one_per_note() {
        let t: Value = serde_json::from_str(
            r#"{"id":"rf-1","title":"T","body":"B","status":"done","notes":[{"kind":"decision","at":"2026-11-01T00:00:00Z","text":"keep X"},{"text":""}]}"#,
        )
        .unwrap();
        let l = load_task(&t);
        assert_eq!(l.row.record_id.as_deref(), Some("rf-1"));
        let seen: Vec<(usize, &str, &str)> = l
            .units
            .iter()
            .map(|u| (u.start, u.heading.as_str(), u.body.as_str()))
            .collect();
        assert_eq!(
            seen,
            [(0, "", "T\nB"), (1, "decision · 2026-11-01", "keep X")]
        );
    }
}
