//! The sources of each scope and the units each source gives.
//!
//! A scope lists its sources, each with a key that is unique in the index and
//! a stamp that changes when the source may have changed. The index loads a
//! source whose stamp differs from the stored one. A listing that could not
//! read everything says so; the index then keeps what it holds of that scope.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::Transaction;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::units::{self, Unit};
use super::{Class, Context, Scope, chat, refs};
use crate::archive::front;
use crate::worktree::Fallback;

/// A source of one scope.
pub struct Doc {
    pub scope: Scope,
    /// Unique in the index: a path, `ref:<key>`, `session:<source>:<id>` or `board:<id>`.
    pub key: String,
    /// Changes when the source may have changed.
    pub stamp: String,
    pub what: What,
}

pub enum What {
    Note,
    Ref(Box<refs::RefDoc>),
    Card(Box<refs::CardDoc>),
    Text(Box<refs::TextDoc>),
    File,
    Session(Box<chat::SessionDoc>),
    Task(Box<Value>),
}

/// The sources of a scope.
#[derive(Default)]
pub struct Listing {
    pub docs: Vec<Doc>,
    /// Why the answer from this scope may be incomplete.
    pub notices: Vec<String>,
    /// What a reader should know that leaves the answer complete, such as
    /// texts read from the main checkout. Only `lab search` prints it.
    pub info: Vec<String>,
    /// false: some sources could not be listed, so stored ones that are not
    /// listed stay in the index.
    pub complete: bool,
}

impl Listing {
    pub fn complete(docs: Vec<Doc>) -> Self {
        Self {
            docs,
            notices: Vec::new(),
            info: Vec::new(),
            complete: true,
        }
    }

    pub fn unavailable(notice: String) -> Self {
        Self {
            docs: Vec::new(),
            notices: vec![notice],
            info: Vec::new(),
            complete: false,
        }
    }
}

/// What the listings of the scopes a query searches said.
#[derive(Default)]
pub struct Said {
    /// Why the answer may be incomplete: `Listing::notices`.
    pub notices: Vec<String>,
    /// `Listing::info`.
    pub info: Vec<String>,
}

impl Said {
    pub fn add(&mut self, listing: &Listing) {
        self.notices.extend(listing.notices.iter().cloned());
        self.info.extend(listing.info.iter().cloned());
    }
}

/// The row of a source in `files`.
#[derive(Debug, Default)]
pub struct Row {
    pub class: Option<Class>,
    pub record_id: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub date: Option<String>,
    pub status: Option<String>,
    pub kind: Option<String>,
    pub task: Option<String>,
    pub superseded_by: Vec<String>,
    /// The registry key the source names.
    pub ref_key: Option<String>,
    /// What anchors name instead of the key, such as the text a card quotes.
    pub anchor: Option<String>,
    /// Scope-specific facts shown with a hit.
    pub info: Option<Value>,
    /// Exact-lookup names besides `record_id`, such as an arXiv id.
    pub aliases: Vec<String>,
    /// The `ids` column of each unit.
    pub ids: String,
}

/// A loaded source.
pub struct Loaded {
    pub row: Row,
    pub units: Vec<Unit>,
    /// Of what the units were made from; the same hash keeps the units.
    pub hash: String,
    /// Derived files that were ignored because they no longer match their text.
    pub stale: Vec<(String, String)>,
    /// The text of a note, for the edges of the graph.
    pub note: Option<Vec<u8>>,
}

impl Loaded {
    /// A source made from several files: its hash is of what it stores, so
    /// any change to the row, the units or the stale derivatives rewrites it.
    pub fn stored(row: Row, units: Vec<Unit>, stale: Vec<(String, String)>) -> Self {
        let hash = sha256(format!("{row:?}\n{units:?}\n{stale:?}").as_bytes());
        Self {
            row,
            units,
            hash,
            stale,
            note: None,
        }
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// `size:mtime_ns` of a file, or `None` when it cannot be read.
pub fn file_stamp(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    meta.is_file().then(|| format!("{}:{mtime}", meta.len()))
}

/// Finds files by paths relative to the project root. In a linked worktree, a
/// file that git ignores and the worktree lacks is the main checkout's.
pub struct Resolver<'a> {
    root: &'a Path,
    fallback: Option<Fallback>,
    main: HashMap<String, PathBuf>,
}

impl<'a> Resolver<'a> {
    pub fn new(root: &'a Path) -> Result<Self, String> {
        Ok(Self {
            root,
            fallback: Fallback::find(root)?,
            main: HashMap::new(),
        })
    }

    /// Looks up the given paths that this checkout lacks in the main
    /// checkout, with one `git check-ignore` for all of them.
    pub fn prepare<'p>(&mut self, rels: impl IntoIterator<Item = &'p str>) -> Result<(), String> {
        let Some(fallback) = &self.fallback else {
            return Ok(());
        };
        let missing: BTreeSet<&str> = rels
            .into_iter()
            .filter(|r| {
                crate::config::below_root(Path::new(r))
                    && !self.main.contains_key(*r)
                    && !self.root.join(r).exists()
            })
            .collect();
        for rel in fallback.resolve(missing)? {
            let path = fallback.main_file(&rel);
            self.main.insert(rel, path);
        }
        Ok(())
    }

    /// The main checkout's project directory, in a linked worktree that
    /// reads git-ignored files from it.
    pub fn main_checkout(&self) -> Option<&Path> {
        self.fallback.as_ref().map(Fallback::main)
    }

    /// The file at `rel`, here or in the main checkout once `prepare` found it there.
    pub fn get(&self, rel: &str) -> Option<PathBuf> {
        let here = self.root.join(rel);
        if here.exists() {
            Some(here)
        } else {
            self.main.get(rel).cloned()
        }
    }
}

/// The sources of `scope`.
pub fn list(ctx: &Context, scope: Scope, resolver: &mut Resolver) -> Result<Listing, String> {
    match scope {
        Scope::Notes => notes(ctx, resolver),
        Scope::Refs => refs::list_refs(ctx, resolver),
        Scope::Texts => refs::list_texts(ctx, resolver),
        Scope::Files => files(ctx, resolver).map(Listing::complete),
        Scope::Sessions => chat::list_sessions(ctx, resolver),
        Scope::Board => Ok(chat::list_board(ctx)),
    }
}

/// The files of the `notes` scope, without generated ones and without the
/// paper texts that the registry lists, which are units of `texts`.
fn notes(ctx: &Context, resolver: &mut Resolver) -> Result<Listing, String> {
    let found = crate::archive::files::walk(&ctx.project.root, &[&ctx.notes])?
        .pop()
        .unwrap_or_default();
    // A registry that cannot be read leaves its texts in `notes` and says
    // so: a query of the notes alone does not fail on it.
    let (texts, notices) = match refs::text_paths(ctx, resolver) {
        Ok(texts) => (texts, Vec::new()),
        Err(e) => (
            HashSet::new(),
            vec![format!("{e}: the paper texts are not left out of notes")],
        ),
    };
    let docs = found
        .into_iter()
        .filter(|p| ctx.class_of(p) != Class::Generated && !texts.contains(p))
        .filter_map(|path| {
            let stamp = file_stamp(&ctx.project.root.join(&path))?;
            Some(Doc {
                scope: Scope::Notes,
                key: path,
                stamp,
                what: What::Note,
            })
        })
        .collect();
    Ok(Listing {
        notices,
        ..Listing::complete(docs)
    })
}

/// The files of the `files` scope: the configured globs, or else the files
/// git tracks that are not Markdown.
fn files(ctx: &Context, resolver: &mut Resolver) -> Result<Vec<Doc>, String> {
    let root = &ctx.project.root;
    let paths: Vec<String> = match &ctx.files {
        Some(set) => crate::archive::files::walk(root, &[set])?
            .pop()
            .unwrap_or_default(),
        None => {
            let out = std::process::Command::new("git")
                .args(["ls-files", "-z"])
                .current_dir(root)
                .output();
            match out {
                Ok(out) if out.status.success() => out
                    .stdout
                    .split(|&b| b == 0)
                    .filter_map(|p| std::str::from_utf8(p).ok())
                    .filter(|p| !p.is_empty() && !p.ends_with(".md"))
                    .map(String::from)
                    .collect(),
                // Not a git checkout, or no git: no tracked files.
                _ => Vec::new(),
            }
        }
    };
    // A file's unit names the record whose note lies in one of its
    // directories, so the stamps of those notes are part of its stamp.
    let mut notes_in: HashMap<String, Vec<String>> = HashMap::new();
    for note in notes(ctx, resolver)?.docs {
        let dir = note
            .key
            .rsplit_once('/')
            .map_or("", |(dir, _)| dir)
            .to_string();
        notes_in
            .entry(dir)
            .or_default()
            .push(format!("{}={}", note.key, note.stamp));
    }
    // A note is a unit of `notes`, never also a file.
    Ok(paths
        .into_iter()
        .filter(|p| !ctx.notes.matches(p) && ctx.class_of(p) != Class::Generated)
        .filter_map(|path| {
            let stamp = file_stamp(&root.join(&path))?;
            let mut notes = Sha256::new();
            for dir in Path::new(&path).ancestors().skip(1) {
                for note in dir
                    .to_str()
                    .and_then(|d| notes_in.get(d))
                    .into_iter()
                    .flatten()
                {
                    notes.update(note.as_bytes());
                    notes.update(b"\n");
                }
            }
            // A short digest keeps the stamp small under many notes.
            let stamp = format!("{stamp}:{}", &hex(&notes.finalize())[..16]);
            Some(Doc {
                scope: Scope::Files,
                key: path,
                stamp,
                what: What::File,
            })
        })
        .collect())
}

/// Loads sources, with what several of them share.
pub struct Loader<'c, 'p> {
    ctx: &'c Context<'p>,
    /// Directory to the record whose note lies in it, for the `files` scope.
    parents: Option<HashMap<String, (String, Option<String>)>>,
}

impl<'c, 'p> Loader<'c, 'p> {
    pub fn new(ctx: &'c Context<'p>) -> Self {
        Self { ctx, parents: None }
    }

    /// The units of `doc`; `None` when it vanished since it was listed.
    pub fn load(&mut self, tx: &Transaction, doc: &Doc) -> Result<Option<Loaded>, String> {
        match &doc.what {
            What::Note => self.note(&doc.key),
            What::Ref(r) => Ok(Some(refs::load_ref(r))),
            What::Card(c) => refs::load_card(c).map(Some),
            What::Text(t) => refs::load_text(t).map(Some),
            What::File => self.file(tx, &doc.key).map(Some),
            What::Session(s) => chat::load_session(s),
            What::Task(t) => Ok(Some(chat::load_task(t))),
        }
    }

    fn note(&self, path: &str) -> Result<Option<Loaded>, String> {
        let ctx = self.ctx;
        let file = ctx.project.root.join(path);
        let bytes = match std::fs::read(&file) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("{}: {e}", file.display())),
        };
        let meta = ctx.spec.meta_of(path, &bytes);
        let (task, kind) = units::task_and_kind(&bytes);
        let text = String::from_utf8_lossy(&bytes);
        let mut sections = units::sections(&text, front::split(&bytes).body_line);
        if sections.is_empty() {
            // A record with front matter or a title only is still found by them.
            sections.push(Unit {
                start: 1,
                end: text.lines().count().max(1),
                heading: String::new(),
                body: String::new(),
            });
        }
        let ids = meta
            .id
            .iter()
            .chain(&meta.aliases)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" ; ");
        Ok(Some(Loaded {
            hash: sha256(&bytes),
            row: Row {
                class: Some(ctx.class_of(path)),
                record_id: meta.id,
                title: meta.title,
                summary: meta.summary,
                date: meta.date,
                status: meta.status,
                kind,
                task,
                superseded_by: meta.superseded_by,
                ids,
                ..Row::default()
            },
            units: sections,
            stale: Vec::new(),
            note: Some(bytes),
        }))
    }

    /// A file of the `files` scope: its name, size and the record whose
    /// directory holds it. Its content is not read.
    fn file(&mut self, tx: &Transaction, path: &str) -> Result<Loaded, String> {
        let parents = match &mut self.parents {
            Some(p) => p,
            none => none.insert(parents(tx).map_err(|e| format!("search index: {e}"))?),
        };
        let size = std::fs::metadata(self.ctx.project.root.join(path)).map_or(0, |m| m.len());
        let parent = Path::new(path)
            .ancestors()
            .skip(1)
            .find_map(|dir| parents.get(dir.to_str()?))
            .cloned();
        let name = path.rsplit('/').next().unwrap_or(path);
        let (parent_id, parent_title) = parent.clone().unzip();
        let parent_title = parent_title.flatten();
        let mut body = format!(
            "{} · {}",
            path.replace(['/', '_', '-', '.'], " "),
            size_text(size)
        );
        if let Some(id) = &parent_id {
            body.push_str(&format!(" · in {id}"));
        }
        Ok(Loaded {
            hash: sha256(format!("{path}\n{size}\n{parent:?}").as_bytes()),
            row: Row {
                class: Some(self.ctx.class_of(path)),
                title: Some(name.to_string()),
                summary: Some(size_text(size)),
                kind: Some("file".into()),
                info: Some(json!({ "size": size, "parent": parent_id })),
                ids: parent_id.clone().unwrap_or_default(),
                ..Row::default()
            },
            units: vec![Unit {
                start: 1,
                end: 1,
                heading: parent_title.unwrap_or_default(),
                body,
            }],
            stale: Vec::new(),
            note: None,
        })
    }
}

/// Each directory that holds a note with a record id, to that record: the
/// README or index note when there is one, else the first by path.
fn parents(tx: &Transaction) -> rusqlite::Result<HashMap<String, (String, Option<String>)>> {
    let mut stmt = tx.prepare(
        "SELECT path, record_id, title FROM files WHERE scope = 'notes' AND record_id IS NOT NULL \
         ORDER BY path",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut map: HashMap<String, (String, Option<String>)> = HashMap::new();
    for row in rows {
        let (path, id, title) = row?;
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", &path));
        let main = matches!(name, "README.md" | "index.md");
        if main || !map.contains_key(dir) {
            map.insert(dir.to_string(), (id, title));
        }
    }
    Ok(map)
}

fn size_text(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / f64::from(1u32 << 30)),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / f64::from(1u32 << 20)),
        b if b >= 1 << 10 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_sizes_in_binary_units() {
        assert_eq!(size_text(12), "12 B");
        assert_eq!(size_text(2048), "2.0 KB");
        assert_eq!(size_text(5 << 20), "5.0 MB");
        assert_eq!(size_text(3 << 30), "3.0 GB");
    }
}
