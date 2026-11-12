//! The graph stage of the search index (docs/archive-design.md 3.4): a plain
//! table of edges, filled when a file is indexed and removed with it. It
//! answers the exact questions that ranking cannot: what replaced a record,
//! which notes link it, which notes cite a reference key.
//!
//! Edge types, from a note: `supersedes`, `superseded_by`, `amends`,
//! `amended_by` and `task` (front matter), `links` (a Markdown link to a file
//! of the project) and `cites` (a citation of a reference key). From a paper:
//! `cites` (the reference list of its `rf.paper-structure/1` sidecar).
//! Belonging is not stored: a unit belongs to its file, and a file to the
//! record whose `README.md` lies in a directory above it.

use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

use regex::Regex;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use sha2::{Digest, Sha256};
use yaml_serde::Value;

use super::{Context, index};
use crate::archive::{front, markdown};
use crate::config::Project;

pub const SCHEMA: &str = "
CREATE TABLE edges(
    -- The note the edge starts at; NULL for an edge between two papers.
    file_id INTEGER REFERENCES files(id),
    -- The file line that holds the link or citation; 1 for front matter.
    line INTEGER NOT NULL,
    kind TEXT NOT NULL,
    -- A record id, a board task id, a path below the project root or a reference key.
    target TEXT NOT NULL,
    -- The reference key of the citing paper, for an edge between two papers.
    source_ref TEXT
);
CREATE INDEX edges_file ON edges(file_id);
CREATE INDEX edges_target ON edges(kind, target COLLATE NOCASE);
CREATE INDEX edges_source ON edges(source_ref) WHERE source_ref IS NOT NULL;
-- The units of a file as they were before its last change, so that an
-- anchor made then still finds its text.
CREATE TABLE old_anchors(
    path TEXT NOT NULL,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    hash TEXT NOT NULL,
    PRIMARY KEY (path, start_line, end_line)
) WITHOUT ROWID;
";

/// The front-matter keys that name another record or a board task.
const FRONT_KEYS: [&str; 5] = [
    "supersedes",
    "superseded_by",
    "amends",
    "amended_by",
    "task",
];

/// The citation forms read without `[lab.search] cites`: `ref:<key>`, and a
/// link to the `#reference-<key>` anchor of a catalogue.
pub const DEFAULT_CITES: [&str; 2] = [
    r"\bref:(?P<key>[A-Za-z0-9][A-Za-z0-9_.-]*[A-Za-z0-9]|[A-Za-z0-9])",
    r"#reference-(?P<key>[A-Za-z0-9_-]+)\)",
];

/// Compiles the citation patterns; each needs a group named `key`.
pub fn cite_patterns(configured: &[String]) -> Result<Vec<Regex>, String> {
    let sources: Vec<&str> = if configured.is_empty() {
        DEFAULT_CITES.to_vec()
    } else {
        configured.iter().map(String::as_str).collect()
    };
    sources
        .into_iter()
        .map(|s| {
            let re = Regex::new(s).map_err(|e| format!("[lab.search] cites: `{s}`: {e}"))?;
            if re.capture_names().flatten().any(|n| n == "key") {
                Ok(re)
            } else {
                Err(format!(
                    "[lab.search] cites: `{s}` needs a group named `key`, as in `(?P<key>\\d+)`"
                ))
            }
        })
        .collect()
}

/// The values of the front-matter keys that make edges: a string, or a list
/// of strings. Any other value is left to the checks.
fn front_edges(bytes: &[u8]) -> Vec<(&'static str, String)> {
    let Some(yaml) = front::split(bytes).yaml else {
        return Vec::new();
    };
    let Ok(Value::Mapping(map)) = yaml_serde::from_slice::<Value>(yaml) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for key in FRONT_KEYS {
        let values: Vec<&Value> = match map.get(key) {
            Some(Value::Sequence(items)) => items.iter().collect(),
            Some(one) => vec![one],
            None => Vec::new(),
        };
        for value in values {
            if let Some(s) = value.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                out.push((key, s.to_string()));
            }
        }
    }
    out
}

fn has_scheme(dest: &str) -> bool {
    dest.split_once(':').is_some_and(|(scheme, _)| {
        scheme.len() > 1
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    })
}

/// The path below the project root that the link `dest` in the file `from`
/// names, without its fragment. `None` for a link to another site, to a
/// place in the same file or to a path outside the project.
fn link_target(from: &str, dest: &str) -> Option<String> {
    let dest = dest.split(['#', '?']).next().unwrap_or("");
    if dest.is_empty() || has_scheme(dest) || dest.starts_with('/') {
        return None;
    }
    let dest = markdown::percent_decode(dest);
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for segment in dest.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/")).filter(|p| !p.is_empty() && p != from)
}

/// Stores the edges that start at the file `path`, whose row is `file`.
pub fn index_edges(
    ctx: &Context,
    tx: &Transaction,
    file: i64,
    path: &str,
    bytes: &[u8],
) -> rusqlite::Result<()> {
    let mut edges: BTreeSet<(usize, &'static str, String)> = BTreeSet::new();
    for (kind, target) in front_edges(bytes) {
        edges.insert((1, kind, target));
    }
    let split = front::split(bytes);
    let body = String::from_utf8_lossy(split.body);
    for link in markdown::parse(&body, split.body_line).links {
        if let Some(target) = link_target(path, &link.dest) {
            edges.insert((link.line, "links", target));
        }
    }
    let mut fence = None;
    for (i, line) in body.split('\n').enumerate() {
        if let Some(open) = fence {
            if markdown::fence_closes(line, open) {
                fence = None;
            }
            continue;
        }
        if let Some(open) = markdown::fence_open(line) {
            fence = Some(open);
            continue;
        }
        for re in &ctx.cites {
            for found in re.captures_iter(line) {
                if let Some(key) = found.name("key") {
                    edges.insert((split.body_line + i, "cites", key.as_str().to_string()));
                }
            }
        }
    }
    let mut insert = tx.prepare_cached(
        "INSERT INTO edges (file_id, line, kind, target) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (line, kind, target) in edges {
        insert.execute(params![file, line as i64, kind, target])?;
    }
    Ok(())
}

/// Removes the edges of a file and keeps its units' anchors, so that an
/// anchor into the text it had still resolves by content hash.
pub fn forget_file(tx: &Transaction, file: i64) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM edges WHERE file_id = ?1", [file])?;
    tx.execute(
        "INSERT OR REPLACE INTO old_anchors (path, start_line, end_line, hash) \
         SELECT f.path, u.start_line, u.end_line, u.hash \
         FROM units u JOIN files f ON f.id = u.file_id WHERE f.id = ?1",
        [file],
    )?;
    Ok(())
}

/// The citation edges between papers, read from the structure sidecars of
/// the registry's texts.
pub struct Papers {
    /// Of the files the edges come from; another value means read them again.
    fingerprint: String,
    /// Citing key, cited key and the line of the reference in the citing text.
    edges: BTreeSet<(String, String, i64)>,
}

fn stat(path: &Path) -> Option<(u64, u128)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Some((meta.len(), mtime))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The registry that `[papers] registry` of the project's `research.toml` names.
fn registry_path(project: &Project) -> Option<std::path::PathBuf> {
    let text = std::fs::read_to_string(project.root.join(crate::config::CONFIG_FILE)).ok()?;
    let config: toml::Value = toml::from_str(&text).ok()?;
    let registry = config.get("papers")?.get("registry")?.as_str()?;
    Some(project.root.join(registry))
}

/// Reads the sidecars when the stored fingerprint is not theirs. `None`
/// means the stored paper edges are current. A sidecar whose text has
/// another sha256 now is out of date and gives no edge.
pub fn papers_changed(ctx: &Context, conn: &Connection) -> rusqlite::Result<Option<Papers>> {
    let stored: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'papers'", [], |r| {
            r.get(0)
        })
        .optional()?;
    // The fingerprint needs only the files' sizes and times; the sidecars
    // are read, and the texts hashed, only when it changed.
    let (fingerprint, texts) = paper_texts(ctx.project);
    if stored.as_deref() == Some(&fingerprint) {
        return Ok(None);
    }
    Ok(Some(Papers {
        edges: read_papers(ctx.project, &texts),
        fingerprint,
    }))
}

/// The fingerprint of the registry and of each text that has a structure
/// sidecar, and those texts as (key, path).
fn paper_texts(project: &Project) -> (String, Vec<(String, String)>) {
    let mut seen = Sha256::new();
    let mut texts = Vec::new();
    let registry = registry_path(project);
    let entries = registry
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    if let Some(path) = &registry {
        seen.update(format!("{:?}\n", stat(path)));
    }
    for line in entries.lines() {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(key) = entry["key"].as_str() else {
            continue;
        };
        for text in entry["texts"].as_array().into_iter().flatten() {
            let Some(rel) = text["path"].as_str() else {
                continue;
            };
            let sidecar = project.root.join(format!("{rel}.structure.json"));
            let Some(sidecar_stat) = stat(&sidecar) else {
                continue;
            };
            let text_stat = stat(&project.root.join(rel));
            seen.update(format!("{rel} {sidecar_stat:?} {text_stat:?}\n"));
            texts.push((key.to_string(), rel.to_string()));
        }
    }
    (hex(&seen.finalize()), texts)
}

/// Citing key, cited key and line, from the sidecars of `texts` whose text
/// still has the sha256 they record.
fn read_papers(project: &Project, texts: &[(String, String)]) -> BTreeSet<(String, String, i64)> {
    let mut edges = BTreeSet::new();
    for (key, rel) in texts {
        let sidecar = project.root.join(format!("{rel}.structure.json"));
        let Some(structure) = std::fs::read(&sidecar)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        else {
            continue;
        };
        let current = std::fs::read(project.root.join(rel))
            .is_ok_and(|b| structure["text"]["sha256"].as_str() == Some(&hex(&Sha256::digest(b))));
        if structure["schema"] != "rf.paper-structure/1" || !current {
            continue;
        }
        for reference in structure["references"]["entries"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(cited) = reference["key"].as_str().filter(|c| c != key) {
                let line = reference["first_line"].as_i64().unwrap_or(1);
                edges.insert((key.clone(), cited.to_string(), line));
            }
        }
    }
    edges
}

/// Replaces the edges between papers.
pub fn write_papers(tx: &Transaction, papers: &Papers) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM edges WHERE file_id IS NULL", [])?;
    let mut insert = tx.prepare_cached(
        "INSERT INTO edges (file_id, line, kind, target, source_ref) VALUES (NULL, ?3, 'cites', ?2, ?1)",
    )?;
    for (citing, cited, line) in &papers.edges {
        insert.execute(params![citing, cited, line])?;
    }
    tx.execute(
        "INSERT OR REPLACE INTO meta VALUES ('papers', ?1)",
        [&papers.fingerprint],
    )?;
    Ok(())
}

/// A file of the index, as a node of the graph.
#[derive(Debug, Clone)]
pub struct FileNode {
    pub id: i64,
    pub path: String,
    pub record_id: Option<String>,
    pub title: Option<String>,
    pub date: Option<String>,
    pub status: Option<String>,
}

const FILE_COLUMNS: &str = "f.id, f.path, f.record_id, f.title, f.date, f.status";

fn file_node(r: &rusqlite::Row) -> rusqlite::Result<FileNode> {
    Ok(FileNode {
        id: r.get(0)?,
        path: r.get(1)?,
        record_id: r.get(2)?,
        title: r.get(3)?,
        date: r.get(4)?,
        status: r.get(5)?,
    })
}

fn files_where(
    conn: &Connection,
    clause: &str,
    param: &dyn rusqlite::ToSql,
) -> rusqlite::Result<Vec<FileNode>> {
    conn.prepare_cached(&format!(
        "SELECT {FILE_COLUMNS} FROM files f WHERE {clause} ORDER BY f.path"
    ))?
    .query_map([param], file_node)?
    .collect()
}

pub fn file_by_path(conn: &Connection, path: &str) -> rusqlite::Result<Option<FileNode>> {
    Ok(files_where(conn, "f.path = ?1", &path)?.pop())
}

pub fn file_by_id(conn: &Connection, id: i64) -> rusqlite::Result<Option<FileNode>> {
    Ok(files_where(conn, "f.id = ?1", &id)?.pop())
}

pub fn files_by_record(conn: &Connection, id: &str) -> rusqlite::Result<Vec<FileNode>> {
    files_where(conn, "f.record_id = ?1 COLLATE NOCASE", &id)
}

/// The link targets that name the file `path`: its path, and its directory
/// when it is that directory's `README.md`.
fn targets_of(path: &str) -> Vec<&str> {
    let mut targets = vec![path];
    targets.extend(path.strip_suffix("/README.md"));
    targets
}

fn quoted(values: &[&str]) -> String {
    values
        .iter()
        .map(|v| format!("'{}'", v.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The file that the path `target` of a link names, when the index holds it.
fn file_of_target(conn: &Connection, target: &str) -> rusqlite::Result<Option<FileNode>> {
    match file_by_path(conn, target)? {
        Some(file) => Ok(Some(file)),
        None => file_by_path(conn, &format!("{target}/README.md")),
    }
}

/// A database error as the message of a command.
pub fn db(e: rusqlite::Error) -> String {
    index::Fault::from(e).into()
}

/// How many other notes link the file `path`.
pub fn linked_by(conn: &Connection, file: i64, path: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        &format!(
            "SELECT count(DISTINCT file_id) FROM edges WHERE kind = 'links' \
             AND target COLLATE NOCASE IN ({}) AND file_id != ?1",
            quoted(&targets_of(path))
        ),
        [file],
        |r| r.get(0),
    )
}

/// The files that the lines `start` to `end` of the file `file` link.
pub fn links_in(
    conn: &Connection,
    file: i64,
    start: i64,
    end: i64,
) -> rusqlite::Result<Vec<String>> {
    conn.prepare_cached(
        "SELECT DISTINCT target FROM edges WHERE file_id = ?1 AND kind = 'links' \
         AND line BETWEEN ?2 AND ?3",
    )?
    .query_map(params![file, start, end], |r| r.get(0))?
    .collect()
}

/// Whether one of the two files names the other's record in a supersession
/// or an amendment: such a pair is two records, never two copies of one fact.
pub fn succession(
    conn: &Connection,
    a: (i64, Option<&str>),
    b: (i64, Option<&str>),
) -> rusqlite::Result<bool> {
    let names = |from: i64, to: Option<&str>| -> rusqlite::Result<bool> {
        let Some(id) = to else {
            return Ok(false);
        };
        conn.prepare_cached(
            "SELECT 1 FROM edges WHERE file_id = ?1 AND target = ?2 COLLATE NOCASE \
             AND kind IN ('supersedes', 'superseded_by', 'amends', 'amended_by') LIMIT 1",
        )?
        .exists(params![from, id])
    };
    Ok(names(a.0, b.1)? || names(b.0, a.1)?)
}

/// The ids of the records whose `supersedes` names the record `id`.
pub fn superseded_by(conn: &Connection, id: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare_cached(
        "SELECT DISTINCT f.record_id FROM edges e JOIN files f ON f.id = e.file_id \
         WHERE e.kind = 'supersedes' AND e.target = ?1 COLLATE NOCASE \
         AND f.record_id IS NOT NULL ORDER BY f.record_id",
    )?
    .query_map([id], |r| r.get(0))?
    .collect()
}

/// The records that replaced the record of `file`: those its `superseded_by`
/// names and those whose `supersedes` names it. An id that no file of the
/// index has is `Err`.
fn successors(
    conn: &Connection,
    file: &FileNode,
) -> rusqlite::Result<Vec<Result<FileNode, String>>> {
    related_records(conn, file, "superseded_by", "supersedes")
}

/// The records on the other end of a relation that either side may state:
/// `out` in this file's front matter, `back` in the other's.
fn related_records(
    conn: &Connection,
    file: &FileNode,
    out: &str,
    back: &str,
) -> rusqlite::Result<Vec<Result<FileNode, String>>> {
    let mut found: Vec<Result<FileNode, String>> = Vec::new();
    let mut seen = HashSet::new();
    let named: Vec<String> = conn
        .prepare_cached(
            "SELECT target FROM edges WHERE file_id = ?1 AND kind = ?2 ORDER BY target",
        )?
        .query_map(params![file.id, out], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for id in named {
        let files = files_by_record(conn, &id)?;
        if files.is_empty() {
            found.push(Err(id));
        }
        for f in files {
            if seen.insert(f.id) {
                found.push(Ok(f));
            }
        }
    }
    if let Some(id) = &file.record_id {
        let naming: Vec<FileNode> = conn
            .prepare_cached(&format!(
                "SELECT DISTINCT {FILE_COLUMNS} FROM edges e JOIN files f ON f.id = e.file_id \
                 WHERE e.kind = ?1 AND e.target = ?2 COLLATE NOCASE ORDER BY f.path"
            ))?
            .query_map(params![back, id], file_node)?
            .collect::<Result<_, _>>()?;
        for f in naming {
            if f.id != file.id && seen.insert(f.id) {
                found.push(Ok(f));
            }
        }
    }
    Ok(found)
}

/// Whether a record that is no longer in force was in force on `date`: each
/// record that replaced it is known and dated later. A record that names no
/// successor, such as a withdrawn one, gives no date to judge by.
pub fn in_force_on(conn: &Connection, file: i64, date: &str) -> rusqlite::Result<bool> {
    let Some(node) = files_where(conn, "f.id = ?1", &file)?.pop() else {
        return Ok(false);
    };
    let successors = successors(conn, &node)?;
    Ok(!successors.is_empty()
        && successors.iter().all(|s| {
            s.as_ref()
                .is_ok_and(|s| s.date.as_deref().is_some_and(|d| d > date))
        }))
}

/// The filters of a query that the graph and the record dates answer.
#[derive(Debug, Default, Clone)]
pub struct Filters {
    /// Only records that existed on this date.
    pub as_of: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    /// Only units that cite this reference key.
    pub cites: Option<String>,
    /// Only units that link this record or path.
    pub links_to: Option<String>,
}

impl Filters {
    pub fn any(&self) -> bool {
        self.as_of.is_some()
            || self.since.is_some()
            || self.until.is_some()
            || self.cites.is_some()
            || self.links_to.is_some()
    }

    /// The filters that select units without any query word.
    pub fn lists(&self) -> bool {
        self.cites.is_some() || self.links_to.is_some()
    }

    fn check(&self) -> Result<(), String> {
        for (name, date) in [
            ("--as-of", &self.as_of),
            ("--since", &self.since),
            ("--until", &self.until),
        ] {
            if let Some(d) = date
                && !front::valid_date(d)
            {
                return Err(format!("{name} takes a date as YYYY-MM-DD, got `{d}`"));
            }
        }
        Ok(())
    }

    /// A `SELECT` of the ids of the units that pass, or `None` without a filter.
    pub fn units_sql(&self, conn: &Connection) -> Result<Option<String>, String> {
        self.check()?;
        if !self.any() {
            return Ok(None);
        }
        let mut sql =
            String::from("SELECT u.id FROM units u JOIN files f ON f.id = u.file_id WHERE 1 = 1");
        // The dates are checked above, so they hold no quote.
        if let Some(d) = &self.as_of {
            let _ = write!(sql, " AND (f.date IS NULL OR f.date <= '{d}')");
        }
        if let Some(d) = &self.since {
            let _ = write!(sql, " AND f.date >= '{d}'");
        }
        if let Some(d) = &self.until {
            let _ = write!(sql, " AND f.date <= '{d}'");
        }
        let in_unit = "e.file_id = u.file_id AND e.line BETWEEN u.start_line AND u.end_line";
        if let Some(key) = &self.cites {
            let key = key.strip_prefix("ref:").unwrap_or(key);
            let _ = write!(
                sql,
                " AND EXISTS (SELECT 1 FROM edges e WHERE {in_unit} AND e.kind = 'cites' \
                 AND e.target = {} COLLATE NOCASE)",
                quoted(&[key])
            );
        }
        if let Some(name) = &self.links_to {
            let files = match resolve(conn, name).map_err(db)? {
                Some(Node::File(file)) => files_by_same_record(conn, file).map_err(db)?,
                _ => {
                    return Err(format!(
                        "--links-to: no record or file `{name}` in the index"
                    ));
                }
            };
            let targets: Vec<&str> = files.iter().flat_map(|f| targets_of(&f.path)).collect();
            let _ = write!(
                sql,
                " AND EXISTS (SELECT 1 FROM edges e WHERE {in_unit} AND e.kind = 'links' \
                 AND e.target COLLATE NOCASE IN ({}))",
                quoted(&targets)
            );
        }
        Ok(Some(sql))
    }
}

/// The file and the others that carry the same record id.
fn files_by_same_record(conn: &Connection, file: FileNode) -> rusqlite::Result<Vec<FileNode>> {
    match &file.record_id {
        Some(id) => files_by_record(conn, id),
        None => Ok(vec![file]),
    }
}

/// What `lab related` and `lab open` are asked about.
pub enum Node {
    File(FileNode),
    /// A paper of the reference registry, by key.
    Paper(String),
    /// A board task, by id.
    Task(String),
}

/// The path of an anchor (`path#L3-L9`, `path:3`, `path:3-9`) or the name as it is.
pub fn path_of(name: &str) -> &str {
    if let Some((path, lines)) = name.rsplit_once("#L")
        && lines.starts_with(|c: char| c.is_ascii_digit())
    {
        return path;
    }
    match name.rsplit_once(':') {
        Some((path, lines))
            if path.contains(['.', '/'])
                && !lines.is_empty()
                && lines.chars().all(|c| c.is_ascii_digit() || c == '-') =>
        {
            path
        }
        _ => name,
    }
}

/// Finds the node that `name` means: a path or an anchor of an indexed
/// file, `ref:<key>`, `task:<id>`, a record id, then a task id or a
/// reference key that some note names.
pub fn resolve(conn: &Connection, name: &str) -> rusqlite::Result<Option<Node>> {
    let name = name.trim();
    if let Some(key) = name.strip_prefix("ref:") {
        return Ok(Some(Node::Paper(key.to_string())));
    }
    if let Some(id) = name.strip_prefix("task:") {
        return Ok(Some(Node::Task(id.to_string())));
    }
    let path = path_of(name).trim_start_matches("./");
    if let Some(file) = file_by_path(conn, path)? {
        return Ok(Some(Node::File(file)));
    }
    if let Some(file) = files_by_record(conn, name)?.into_iter().next() {
        return Ok(Some(Node::File(file)));
    }
    let names = |kind: &str| {
        conn.prepare_cached(
            "SELECT 1 FROM edges WHERE kind = ?1 AND target = ?2 COLLATE NOCASE LIMIT 1",
        )?
        .exists(params![kind, name])
    };
    if names("task")? {
        return Ok(Some(Node::Task(name.to_string())));
    }
    if names("cites")? {
        return Ok(Some(Node::Paper(name.to_string())));
    }
    Ok(None)
}

/// One neighbour of a node.
#[derive(Debug, Serialize)]
pub struct Neighbour {
    /// The record id, `ref:<key>`, the task id, or the path.
    pub node: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The unit that holds the link or the citation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    /// False for an id or a path that names no file of the index.
    pub indexed: bool,
}

impl Neighbour {
    fn file(f: FileNode, anchor: Option<String>) -> Self {
        Self {
            node: f.record_id.clone().unwrap_or_else(|| f.path.clone()),
            id: f.record_id,
            date: f.date,
            status: f.status,
            title: f.title,
            path: Some(f.path),
            anchor,
            indexed: true,
        }
    }

    fn name(node: String, indexed: bool) -> Self {
        Self {
            node,
            id: None,
            date: None,
            status: None,
            title: None,
            path: None,
            anchor: None,
            indexed,
        }
    }

    fn of(found: Result<FileNode, String>) -> Self {
        match found {
            Ok(file) => Self::file(file, None),
            Err(id) => Self::name(id, false),
        }
    }

    fn line(&self) -> String {
        let mut parts = Vec::new();
        if self.path.is_some() {
            parts.push(self.id.clone().unwrap_or_else(|| "-".into()));
            parts.extend(self.date.clone());
            parts.extend(self.status.clone());
            parts.extend(self.anchor.clone().or_else(|| self.path.clone()));
            parts.extend(self.title.clone());
        } else if self.indexed {
            parts.push(self.node.clone());
        } else {
            parts.push(format!("{} (not in the index)", self.node));
        }
        parts.join(" · ")
    }
}

/// The neighbours of one edge type in one direction.
#[derive(Debug, Serialize)]
pub struct Relation {
    /// `superseded by`, `supersedes`, `amended by`, `amends`, `task`, `part of`,
    /// `parts`, `links to`, `linked from`, `cites`, `cited by`, `records`.
    pub name: &'static str,
    /// `out` for an edge that starts at the node, `in` for one that ends at it.
    pub direction: &'static str,
    pub total: usize,
    pub neighbours: Vec<Neighbour>,
}

/// The `--json` output of `lab related`, schema `rf.related/1`.
#[derive(Debug, Serialize)]
pub struct Related {
    pub schema: &'static str,
    pub node: Neighbour,
    pub relations: Vec<Relation>,
}

/// The unit of `file` that holds `line`, as an anchor.
fn anchor_at(conn: &Connection, file: &FileNode, line: i64) -> rusqlite::Result<Option<String>> {
    conn.prepare_cached(
        "SELECT start_line, end_line FROM units WHERE file_id = ?1 AND start_line <= ?2 \
         AND end_line >= ?2 LIMIT 1",
    )?
    .query_row(params![file.id, line], |r| {
        Ok(format!(
            "{}#L{}-L{}",
            file.path,
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?
        ))
    })
    .optional()
}

/// The units that hold an edge of `kind` to one of `targets`, newest file first.
fn units_naming(
    conn: &Connection,
    kind: &str,
    targets: &[&str],
    except: Option<i64>,
) -> rusqlite::Result<Vec<Neighbour>> {
    let rows: Vec<(FileNode, i64)> = conn
        .prepare(&format!(
            "SELECT {FILE_COLUMNS}, e.line FROM edges e JOIN files f ON f.id = e.file_id \
             WHERE e.kind = ?1 AND e.target COLLATE NOCASE IN ({}) \
             ORDER BY f.date IS NULL, f.date DESC, f.path, e.line",
            quoted(targets)
        ))?
        .query_map([kind], |r| Ok((file_node(r)?, r.get(6)?)))?
        .collect::<Result<_, _>>()?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (file, line) in rows {
        if Some(file.id) == except {
            continue;
        }
        let anchor = anchor_at(conn, &file, line)?;
        if seen.insert((file.id, anchor.clone())) {
            out.push(Neighbour::file(file, anchor));
        }
    }
    Ok(out)
}

fn relation(name: &'static str, direction: &'static str, neighbours: Vec<Neighbour>) -> Relation {
    Relation {
        name,
        direction,
        total: neighbours.len(),
        neighbours,
    }
}

fn file_relations(conn: &Connection, file: &FileNode) -> rusqlite::Result<Vec<Relation>> {
    let records = |out: &str, back: &str| -> rusqlite::Result<Vec<Neighbour>> {
        Ok(related_records(conn, file, out, back)?
            .into_iter()
            .map(Neighbour::of)
            .collect())
    };
    let targets = |kind: &str| -> rusqlite::Result<Vec<String>> {
        conn.prepare_cached(
            "SELECT DISTINCT target FROM edges WHERE file_id = ?1 AND kind = ?2 ORDER BY target",
        )?
        .query_map(params![file.id, kind], |r| r.get(0))?
        .collect()
    };
    let mut part_of = Vec::new();
    let mut dir = file.path.as_str();
    while let Some((parent, _)) = dir.rsplit_once('/') {
        dir = parent;
        let readme = format!("{parent}/README.md");
        if readme != file.path
            && let Some(record) = file_by_path(conn, &readme)?.filter(|f| f.record_id.is_some())
        {
            part_of.push(Neighbour::file(record, None));
            break;
        }
    }
    let mut parts = Vec::new();
    if let Some(dir) = file.path.strip_suffix("/README.md")
        && file.record_id.is_some()
    {
        let below = format!("{dir}/");
        parts = conn
            .prepare_cached(&format!(
                "SELECT {FILE_COLUMNS} FROM files f WHERE substr(f.path, 1, length(?1)) = ?1 \
                 AND f.id != ?2 ORDER BY f.path"
            ))?
            .query_map(params![below, file.id], file_node)?
            .map(|f| f.map(|f| Neighbour::file(f, None)))
            .collect::<Result<_, _>>()?;
    }
    let mut links_to = Vec::new();
    for target in targets("links")? {
        links_to.push(match file_of_target(conn, &target)? {
            Some(f) => Neighbour::file(f, None),
            // A file outside the notes scope, such as a data file.
            None => Neighbour::name(target, false),
        });
    }
    Ok(vec![
        relation(
            "superseded by",
            "in",
            records("superseded_by", "supersedes")?,
        ),
        relation("supersedes", "out", records("supersedes", "superseded_by")?),
        relation("amended by", "in", records("amended_by", "amends")?),
        relation("amends", "out", records("amends", "amended_by")?),
        relation(
            "task",
            "out",
            targets("task")?
                .into_iter()
                .map(|t| Neighbour::name(t, true))
                .collect(),
        ),
        relation("part of", "out", part_of),
        relation("parts", "in", parts),
        relation("links to", "out", links_to),
        relation(
            "linked from",
            "in",
            units_naming(conn, "links", &targets_of(&file.path), Some(file.id))?,
        ),
        relation(
            "cites",
            "out",
            targets("cites")?
                .into_iter()
                .map(|k| Neighbour::name(format!("ref:{k}"), true))
                .collect(),
        ),
    ])
}

fn paper_relations(conn: &Connection, key: &str) -> rusqlite::Result<Vec<Relation>> {
    let papers = |sql: &str| -> rusqlite::Result<Vec<Neighbour>> {
        let keys: Vec<String> = conn
            .prepare_cached(sql)?
            .query_map([key], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        Ok(keys
            .into_iter()
            .map(|k| Neighbour::name(format!("ref:{k}"), true))
            .collect())
    };
    Ok(vec![
        relation("cited by", "in", units_naming(conn, "cites", &[key], None)?),
        relation(
            "cites papers",
            "out",
            papers(
                "SELECT DISTINCT target FROM edges WHERE source_ref = ?1 COLLATE NOCASE \
                 AND kind = 'cites' ORDER BY target",
            )?,
        ),
        relation(
            "cited by papers",
            "in",
            papers(
                "SELECT DISTINCT source_ref FROM edges WHERE source_ref IS NOT NULL \
                 AND kind = 'cites' AND target = ?1 COLLATE NOCASE ORDER BY source_ref",
            )?,
        ),
    ])
}

fn task_relations(conn: &Connection, id: &str) -> rusqlite::Result<Vec<Relation>> {
    let records: Vec<Neighbour> = conn
        .prepare_cached(&format!(
            "SELECT DISTINCT {FILE_COLUMNS} FROM edges e JOIN files f ON f.id = e.file_id \
             WHERE e.kind = 'task' AND e.target = ?1 COLLATE NOCASE \
             ORDER BY f.date IS NULL, f.date DESC, f.path"
        ))?
        .query_map([id], file_node)?
        .map(|f| f.map(|f| Neighbour::file(f, None)))
        .collect::<Result<_, _>>()?;
    Ok(vec![relation("records", "in", records)])
}

/// The neighbours of `name`, at most `limit` for each edge type and direction.
pub fn related(conn: &Connection, name: &str, limit: usize) -> Result<Related, String> {
    let node = resolve(conn, name)
        .map_err(index::Fault::from)?
        .ok_or_else(|| {
            format!(
                "`{name}` names no record, file, reference key or board task of the index; \
                 `lab search {name}` looks for it as words"
            )
        })?;
    let (node, relations) = match node {
        Node::File(file) => {
            let relations = file_relations(conn, &file);
            (Neighbour::file(file, None), relations)
        }
        Node::Paper(key) => {
            let relations = paper_relations(conn, &key);
            (Neighbour::name(format!("ref:{key}"), true), relations)
        }
        Node::Task(id) => {
            let relations = task_relations(conn, &id);
            (Neighbour::name(id, true), relations)
        }
    };
    let mut relations = relations.map_err(index::Fault::from)?;
    relations.retain(|r| r.total > 0);
    for r in &mut relations {
        r.neighbours.truncate(limit);
    }
    Ok(Related {
        schema: "rf.related/1",
        node,
        relations,
    })
}

impl Related {
    pub fn text(&self) -> String {
        let mut out = format!("{}\n", self.node.line());
        if self.relations.is_empty() {
            out.push_str("No neighbour in the graph.\n");
        }
        for r in &self.relations {
            let _ = writeln!(out, "{} ({}):", r.name, r.total);
            for n in &r.neighbours {
                let _ = writeln!(out, "  {}", n.line());
            }
            if r.total > r.neighbours.len() {
                let _ = writeln!(
                    out,
                    "  {} more: pass --limit {}.",
                    r.total - r.neighbours.len(),
                    r.total
                );
            }
        }
        out
    }

    /// One line for each relation, for `lab open`: its name and the first
    /// neighbours' anchors, paths or ids.
    pub fn brief(&self, each: usize) -> Vec<String> {
        self.relations
            .iter()
            .map(|r| {
                let mut names: Vec<String> = r
                    .neighbours
                    .iter()
                    .take(each)
                    .map(|n| {
                        let place = n.anchor.as_ref().or(n.path.as_ref());
                        match (&n.id, place) {
                            (Some(id), Some(place)) => format!("{id} {place}"),
                            (None, Some(place)) => place.clone(),
                            _ => n.node.clone(),
                        }
                    })
                    .collect();
                if r.total > names.len() {
                    names.push(format!("{} more", r.total - names.len()));
                }
                format!("{}: {}", r.name, names.join("; "))
            })
            .collect()
    }
}

/// `lab related`.
pub fn run(project: &Project, name: &str, limit: usize, json: bool) -> Result<ExitCode, String> {
    let ctx = Context::new(project, project.lab.search.default_scopes.clone())?;
    let opened = index::open(&ctx, false)?;
    if let Some(why) = &opened.stale {
        eprintln!("lab: partial: the index is stale: {why}");
    }
    let related = related(&opened.conn, name, limit.max(1))?;
    if json {
        println!(
            "{}",
            serde_json::to_string(&related).map_err(|e| e.to_string())?
        );
    } else {
        print!("{}", related.text());
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_a_link_against_its_file_and_drops_other_destinations() {
        let t = |dest| link_target("docs/history/03-x.md", dest);
        assert_eq!(
            t("../decisions/D002-b.md#why").as_deref(),
            Some("docs/decisions/D002-b.md")
        );
        assert_eq!(t("a%20b.md").as_deref(), Some("docs/history/a b.md"));
        assert_eq!(t("../followups/x/").as_deref(), Some("docs/followups/x"));
        assert_eq!(t("#top"), None);
        assert_eq!(t("https://example.org/a.md"), None);
        assert_eq!(t("mailto:a@example.org"), None);
        assert_eq!(t("../../../outside.md"), None);
        assert_eq!(t("03-x.md"), None);
    }

    #[test]
    fn reads_front_matter_edges_as_strings_or_lists_and_skips_other_values() {
        let edges = front_edges(
            b"---\nsupersedes: [D001, D000]\namended_by: D042\ntask: rf-200\namends: 7\n---\nx",
        );
        assert_eq!(
            edges,
            [
                ("supersedes", "D001".to_string()),
                ("supersedes", "D000".to_string()),
                ("amended_by", "D042".to_string()),
                ("task", "rf-200".to_string()),
            ]
        );
        assert!(front_edges(b"# no front matter\n").is_empty());
    }

    #[test]
    fn default_citation_forms_give_the_key() {
        let res = cite_patterns(&[]).unwrap();
        let keys = |line: &str| -> Vec<String> {
            res.iter()
                .flat_map(|re| re.captures_iter(line))
                .map(|c| c["key"].to_string())
                .collect()
        };
        assert_eq!(
            keys("See ref:232. And [7](../references.md#reference-7)."),
            ["232", "7"]
        );
        assert_eq!(keys("ref:smith2024, then prefix:9"), ["smith2024"]);
        assert!(
            cite_patterns(&["\\[(\\d+)\\]".into()])
                .unwrap_err()
                .contains("`key`")
        );
    }

    #[test]
    fn takes_the_path_of_an_anchor() {
        assert_eq!(path_of("docs/a.md#L3-L9"), "docs/a.md");
        assert_eq!(path_of("docs/a.md:12"), "docs/a.md");
        assert_eq!(path_of("docs/a.md:12-20"), "docs/a.md");
        assert_eq!(path_of("docs/a.md"), "docs/a.md");
        assert_eq!(path_of("D002"), "D002");
        assert_eq!(path_of("ref:232"), "ref:232");
    }
}
