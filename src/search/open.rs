//! `lab open`: one unit of the index as text, within a line budget, with the
//! anchors of its neighbours in the graph. An anchor made before its file
//! changed finds its text again by content hash, and the output says so.
//!
//! The text is data: nothing here runs or follows what it says.

use std::fmt::Write as _;
use std::process::ExitCode;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use super::graph::{self, FileNode, db};
use super::{Context, index};
use crate::config::Project;

/// Lines of text `lab open` prints by default.
pub const DEFAULT_LINES: usize = 80;
/// Neighbours named on each relation line.
const NEIGHBOURS_EACH: usize = 3;

/// What an anchor resolved to.
#[derive(Debug, Serialize)]
pub struct Opened {
    pub schema: &'static str,
    /// The anchor of the text printed: `path#Lstart-Lend`.
    pub anchor: String,
    pub path: String,
    pub start: usize,
    pub end: usize,
    /// The unit's lines beyond the budget; `lab open <path>#L<next>-L<end>` reads them.
    pub more_lines: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// How the anchor was found when its lines no longer hold its unit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
    /// The lines, numbered from `start`.
    pub lines: Vec<String>,
    /// One line for each relation: its name and the first neighbours.
    pub neighbours: Vec<String>,
}

impl Opened {
    pub fn text(&self) -> String {
        let mut head = vec![self.anchor.clone()];
        head.extend(self.id.clone());
        head.extend(self.date.clone());
        head.extend(self.status.clone());
        head.extend(self.title.clone());
        let mut out = format!("{}\n", head.join(" · "));
        if let Some(h) = self.heading.as_ref().filter(|h| !h.is_empty()) {
            let _ = writeln!(out, "section: {h}");
        }
        if let Some(n) = &self.notice {
            let _ = writeln!(out, "note: {n}");
        }
        let width = self.end.to_string().len();
        for (i, line) in self.lines.iter().enumerate() {
            let _ = writeln!(out, "{:>width$}  {line}", self.start + i);
        }
        if self.more_lines > 0 {
            let next = self.start + self.lines.len();
            let _ = writeln!(
                out,
                "… {} more line(s): lab open {}#L{next}-L{}",
                self.more_lines,
                self.path,
                next + self.more_lines - 1
            );
        }
        if !self.neighbours.is_empty() {
            out.push_str("neighbours:\n");
            for n in &self.neighbours {
                let _ = writeln!(out, "  {n}");
            }
        }
        out
    }
}

/// A unit of the index.
struct Unit {
    start: usize,
    end: usize,
    heading: String,
}

fn unit_where(
    conn: &Connection,
    clause: &str,
    p: impl rusqlite::Params,
) -> rusqlite::Result<Option<(i64, Unit)>> {
    conn.prepare_cached(&format!(
        "SELECT file_id, start_line, end_line, heading FROM units WHERE {clause} \
         ORDER BY start_line LIMIT 1"
    ))?
    .query_row(p, |r| {
        Ok((
            r.get(0)?,
            Unit {
                start: r.get::<_, i64>(1)? as usize,
                end: r.get::<_, i64>(2)? as usize,
                heading: r.get(3)?,
            },
        ))
    })
    .optional()
}

/// Reads `name`: `path#Lstart-Lend`, `path:line`, `path:start-end`, a path,
/// a record id or the content hash of a hit.
fn lines_of(name: &str) -> Option<(usize, usize)> {
    let tail = name
        .rsplit_once("#L")
        .map(|(_, t)| t.replace("-L", "-"))
        .or_else(|| name.rsplit_once(':').map(|(_, t)| t.to_string()))?;
    let (a, b) = tail.split_once('-').unwrap_or((&tail, &tail));
    let (a, b) = (a.parse().ok()?, b.parse().ok()?);
    (a >= 1 && b >= a).then_some((a, b))
}

fn is_hash(name: &str) -> bool {
    name.len() == 32 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Resolves `name` and reads its text from the file.
pub fn open(ctx: &Context, conn: &Connection, name: &str, budget: usize) -> Result<Opened, String> {
    let name = name.trim();
    let budget = budget.max(1);
    let mut notice = None;
    let (file, unit) = if is_hash(name) {
        let found = unit_where(conn, "hash = ?1", [name.to_ascii_lowercase()]).map_err(db)?;
        let (file_id, unit) =
            found.ok_or_else(|| format!("no unit of the index has the content hash `{name}`"))?;
        (file_by_id(conn, file_id)?, Some(unit))
    } else {
        let path = graph::path_of(name).trim_start_matches("./");
        let file = match graph::file_by_path(conn, path).map_err(db)? {
            Some(f) => f,
            None => graph::files_by_record(conn, name)
                .map_err(db)?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    format!(
                        "`{name}` names no file, anchor, record id or content hash of the index"
                    )
                })?,
        };
        let unit = match lines_of(name).filter(|_| path != name) {
            None => unit_where(conn, "file_id = ?1", [file.id])
                .map_err(db)?
                .map(|(_, u)| u),
            Some((a, b)) if a == b && !name.contains("#L") => {
                // `path:line`: the unit that holds the line.
                unit_where(
                    conn,
                    "file_id = ?1 AND start_line <= ?2 AND end_line >= ?2",
                    params![file.id, a as i64],
                )
                .map_err(db)?
                .map(|(_, u)| u)
                .or(Some(Unit {
                    start: a,
                    end: a,
                    heading: String::new(),
                }))
            }
            Some((a, b)) => {
                let exact = unit_where(
                    conn,
                    "file_id = ?1 AND start_line = ?2 AND end_line = ?3",
                    params![file.id, a as i64, b as i64],
                )
                .map_err(db)?;
                match exact {
                    Some((_, u)) => Some(u),
                    None => {
                        let (unit, why) = relocate(conn, &file, a, b)?;
                        notice = why;
                        unit
                    }
                }
            }
        };
        (file, unit)
    };
    let unit = unit.unwrap_or(Unit {
        start: 1,
        end: usize::MAX,
        heading: String::new(),
    });
    let text = std::fs::read(ctx.project.root.join(&file.path))
        .map_err(|e| format!("{}: {e}", file.path))?;
    let text = String::from_utf8_lossy(&text);
    let all: Vec<&str> = text.lines().collect();
    if unit.start > all.len() {
        return Err(format!(
            "{} has {} line(s); line {} is past its end",
            file.path,
            all.len(),
            unit.start
        ));
    }
    let end = unit.end.min(all.len());
    let shown = (end - unit.start + 1).min(budget);
    let lines: Vec<String> = all[unit.start - 1..unit.start - 1 + shown]
        .iter()
        .map(|l| l.to_string())
        .collect();
    let related = graph::related(conn, &file.path, NEIGHBOURS_EACH)?;
    Ok(Opened {
        schema: "rf.open/1",
        anchor: format!("{}#L{}-L{end}", file.path, unit.start),
        path: file.path,
        start: unit.start,
        end,
        more_lines: end - unit.start + 1 - shown,
        heading: Some(unit.heading),
        id: file.record_id,
        title: file.title,
        date: file.date,
        status: file.status,
        notice,
        lines,
        neighbours: related.brief(NEIGHBOURS_EACH),
    })
}

fn file_by_id(conn: &Connection, id: i64) -> Result<FileNode, String> {
    graph::file_by_id(conn, id)
        .map_err(db)?
        .ok_or_else(|| "the index lost a file of its own unit".to_string())
}

/// Finds the text that the anchor `a`-`b` held before its file changed: the
/// unit with the content hash that the anchor's unit had then.
fn relocate(
    conn: &Connection,
    file: &FileNode,
    a: usize,
    b: usize,
) -> Result<(Option<Unit>, Option<String>), String> {
    let old: Option<String> = conn
        .query_row(
            "SELECT hash FROM old_anchors WHERE path = ?1 AND start_line = ?2 AND end_line = ?3",
            params![file.path, a as i64, b as i64],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?;
    let fallback = Unit {
        start: a,
        end: b,
        heading: String::new(),
    };
    let Some(hash) = old else {
        // A range that was never a unit: print it as asked.
        return Ok((Some(fallback), None));
    };
    match unit_where(conn, "file_id = ?1 AND hash = ?2", params![file.id, hash]).map_err(db)? {
        Some((_, u)) => {
            let why = format!(
                "the file changed; the text of L{a}-L{b} moved to L{}-L{} (same content hash)",
                u.start, u.end
            );
            Ok((Some(u), Some(why)))
        }
        None => Ok((
            Some(fallback),
            Some(format!(
                "the file changed and the text of L{a}-L{b} is gone; these are its lines now"
            )),
        )),
    }
}

/// `lab open`.
pub fn run(project: &Project, name: &str, lines: usize, json: bool) -> Result<ExitCode, String> {
    let ctx = Context::new(project, project.lab.search.default_scopes.clone())?;
    let opened = index::open(&ctx, false)?;
    if let Some(why) = &opened.stale {
        eprintln!("lab: partial: the index is stale: {why}");
    }
    let unit = open(&ctx, &opened.conn, name, lines)?;
    super::trace::record_open(project, &unit.anchor);
    if json {
        println!(
            "{}",
            serde_json::to_string(&unit).map_err(|e| e.to_string())?
        );
    } else {
        print!("{}", unit.text());
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_lines_of_an_anchor() {
        assert_eq!(lines_of("docs/a.md#L3-L9"), Some((3, 9)));
        assert_eq!(lines_of("docs/a.md:12"), Some((12, 12)));
        assert_eq!(lines_of("docs/a.md:12-20"), Some((12, 20)));
        assert_eq!(lines_of("docs/a.md"), None);
        assert_eq!(lines_of("docs/a.md#L9-L3"), None);
        assert!(is_hash("0123456789abcdef0123456789abcdef"));
        assert!(!is_hash("DR041"));
    }
}
