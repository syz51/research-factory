//! The query trace (docs/state-machines.md 3.3, 3.4): one `rf.search-query/1`
//! line per query, and one `rf.search-open/1` line per opened hit, appended
//! to `<state_dir>/lab/search-queries.jsonl`. In a linked git worktree the
//! file is the main checkout's, when git ignores it, so every checkout adds
//! to one trace. Writing the trace never fails a query: a failure is a
//! notice on stderr.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Project;
use crate::worktree;

use super::query::Expansion;

/// The file, relative to the state directory.
const FILE: &str = "lab/search-queries.jsonl";
pub const QUERY_SCHEMA: &str = "rf.search-query/1";
pub const OPEN_SCHEMA: &str = "rf.search-open/1";

/// One unit a stage ranked, with its score.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scored {
    pub id: String,
    pub score: f64,
}

/// One `rf.search-query/1` line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryLine {
    pub schema: String,
    /// RFC 3339 UTC.
    pub at: String,
    /// `LAB_SESSION`, else `CLAUDE_CODE_SESSION_ID`; null outside a session.
    pub session: Option<String>,
    /// `cli` for `lab search` and `lab archive find`, `run` for a topics
    /// file, `similar` for the duplicate check.
    pub source: String,
    /// The phrasings.
    pub query: Vec<String>,
    pub scopes: Vec<String>,
    /// `hits`, `partial`, `zero` or `error`.
    pub state: String,
    /// The stages that ran.
    pub stages: Vec<String>,
    /// Configured stages that are not built.
    #[serde(default)]
    pub missing_stages: Vec<String>,
    /// The thesaurus entries the query used.
    #[serde(default)]
    pub expansions: Vec<Expansion>,
    /// Per stage, the best units before grouping by record.
    #[serde(default)]
    pub candidates: BTreeMap<String, Vec<Scored>>,
    pub hit_count: usize,
    /// Hits that hold every term of the query.
    pub complete: usize,
    /// The anchors of the hits, best first.
    pub hits: Vec<String>,
    /// Query words no unit holds.
    #[serde(default)]
    pub unknown: Vec<String>,
    /// Why the answer may be incomplete.
    #[serde(default)]
    pub notices: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub latency_ms: u64,
}

/// One `rf.search-open/1` line: a session read a hit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenLine {
    pub schema: String,
    pub at: String,
    pub session: Option<String>,
    /// `path#Lstart-Lend`, or a path.
    pub anchor: String,
}

/// A line of the trace.
#[derive(Debug, Clone)]
pub enum Line {
    Query(Box<QueryLine>),
    Open(OpenLine),
}

impl Line {
    pub fn at(&self) -> &str {
        match self {
            Self::Query(q) => &q.at,
            Self::Open(o) => &o.at,
        }
    }

    pub fn session(&self) -> Option<&str> {
        match self {
            Self::Query(q) => q.session.as_deref(),
            Self::Open(o) => o.session.as_deref(),
        }
    }
}

/// The session the query runs in.
pub fn session() -> Option<String> {
    ["LAB_SESSION", "CLAUDE_CODE_SESSION_ID"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .find(|v| !v.trim().is_empty())
}

/// A file of the state directory that every checkout of the project
/// shares: the main checkout's in a linked worktree where git ignores it.
pub fn shared_state(project: &Project, rel: &str) -> Result<PathBuf, String> {
    let local = project.state_dir.join(rel);
    let Ok(below) = project.state_dir.strip_prefix(&project.root) else {
        return Ok(local);
    };
    let Some(fallback) = worktree::Fallback::find(&project.root)? else {
        return Ok(local);
    };
    let rel = below.join(rel);
    let rel = rel.to_string_lossy().replace('\\', "/");
    Ok(fallback.shared(&rel)?.unwrap_or(local))
}

/// The trace file.
pub fn path(project: &Project) -> Result<PathBuf, String> {
    shared_state(project, FILE)
}

fn append(file: &Path, line: &str) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .map_err(|e| format!("{}: {e}", file.display()))?;
    // One write per line, so lines of concurrent queries do not interleave.
    f.write_all(format!("{line}\n").as_bytes())
        .map_err(|e| format!("{}: {e}", file.display()))
}

fn write(project: &Project, line: Result<String, serde_json::Error>) {
    let written = line
        .map_err(|e| e.to_string())
        .and_then(|line| append(&path(project)?, &line));
    if let Err(e) = written {
        eprintln!("lab: the query was not written to the search trace: {e}");
    }
}

/// Appends a query line; a failure is only a notice.
pub fn record(project: &Project, line: &QueryLine) {
    write(project, serde_json::to_string(line));
}

/// Appends an open line for `anchor`: for a command that reads a hit, such
/// as `lab open`. A failure is only a notice.
#[allow(dead_code)]
pub fn record_open(project: &Project, anchor: &str) {
    let line = OpenLine {
        schema: OPEN_SCHEMA.into(),
        at: crate::finding::now(),
        session: session(),
        anchor: anchor.into(),
    };
    write(project, serde_json::to_string(&line));
}

/// Every line of the trace that parses; an absent trace has none. Lines of
/// other schemas and lines that do not parse are skipped and counted.
pub fn read(project: &Project) -> Result<(Vec<Line>, usize), String> {
    let file = path(project)?;
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(e) => return Err(format!("{}: {e}", file.display())),
    };
    let mut lines = Vec::new();
    let mut bad = 0;
    for raw in text.lines().filter(|l| !l.trim().is_empty()) {
        let value: Result<serde_json::Value, _> = serde_json::from_str(raw);
        let line = match value
            .as_ref()
            .ok()
            .and_then(|v| v.get("schema"))
            .and_then(|s| s.as_str())
        {
            Some(QUERY_SCHEMA) => serde_json::from_str(raw)
                .ok()
                .map(|q| Line::Query(Box::new(q))),
            Some(OPEN_SCHEMA) => serde_json::from_str(raw).ok().map(Line::Open),
            _ => None,
        };
        match line {
            Some(line) => lines.push(line),
            None => bad += 1,
        }
    }
    Ok((lines, bad))
}
