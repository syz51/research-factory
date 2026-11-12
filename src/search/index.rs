//! The search index: one SQLite file per checkout under the project's state
//! directory, a cache that is rebuilt from the sources at any time.
//!
//! States (docs/state-machines.md 3.1): an absent file, one of another schema
//! version or configuration, or a corrupt one is built anew in a temporary
//! file that then replaces it by rename. A current file is refreshed in place
//! for the sources whose size, modification time and content hash changed.
//! One lock file admits one writer; a reader that finds it held answers from
//! the last committed state and says so.

use std::collections::HashMap;
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::{Connection, ErrorCode, OpenFlags, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use super::Context;
use super::units;
use crate::archive::front;

/// Raised when a table, a tokenizer or the way a unit is cut changes.
const SCHEMA_VERSION: &str = "1";

/// BM25 column weights of `units_fts`, in column order: id and aliases, then
/// title, then summary, heading path and file path, then body.
pub const WEIGHTS: &str = "10.0, 5.0, 2.0, 2.0, 2.0, 1.0";
/// The position of `body` among the columns of `units_fts`.
pub const BODY_COLUMN: usize = 5;

const SCHEMA: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE files(
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    size INTEGER NOT NULL,
    mtime_ns INTEGER NOT NULL,
    hash TEXT NOT NULL,
    class TEXT NOT NULL,
    record_id TEXT,
    title TEXT,
    summary TEXT,
    date TEXT,
    status TEXT,
    kind TEXT,
    task TEXT,
    -- Record ids, one per line.
    superseded_by TEXT NOT NULL
);
CREATE INDEX files_record ON files(record_id COLLATE NOCASE);
CREATE TABLE units(
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files(id),
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    heading TEXT NOT NULL,
    -- Of the unit's indexed text; a later dense stage keys its vectors by it.
    hash TEXT NOT NULL
);
CREATE INDEX units_file ON units(file_id);
CREATE VIRTUAL TABLE units_fts USING fts5(
    ids, title, summary, heading, path, body,
    tokenize = 'porter unicode61 remove_diacritics 2'
);
CREATE VIRTUAL TABLE units_vocab USING fts5vocab(units_fts, 'row');
-- The words of `units_fts`, for a query word that is only part of a word.
CREATE VIRTUAL TABLE words_tri USING fts5(term, tokenize = 'trigram');
";

/// A failure of the index: `Corrupt` is answered by a rebuild.
#[derive(Debug)]
pub enum Fault {
    Corrupt(String),
    Other(String),
}

impl From<rusqlite::Error> for Fault {
    fn from(e: rusqlite::Error) -> Self {
        match e.sqlite_error_code() {
            Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => {
                Self::Corrupt(e.to_string())
            }
            _ => Self::Other(format!("search index: {e}")),
        }
    }
}

impl From<String> for Fault {
    fn from(e: String) -> Self {
        Self::Other(e)
    }
}

impl From<Fault> for String {
    fn from(f: Fault) -> Self {
        match f {
            Fault::Corrupt(e) => format!("search index is corrupt: {e}"),
            Fault::Other(e) => e,
        }
    }
}

/// A source file of the `notes` scope as the walk found it.
struct Source {
    path: String,
    size: i64,
    mtime_ns: i64,
}

/// An index ready for a query.
pub struct Opened {
    pub conn: Connection,
    /// Why the index may lack the latest text of some files.
    pub stale: Option<String>,
}

/// How many files and units the index holds.
pub fn counts(conn: &Connection) -> Result<(i64, i64), Fault> {
    let count =
        |table: &str| conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0));
    Ok((count("files")?, count("units")?))
}

fn db_path(ctx: &Context) -> PathBuf {
    ctx.dir.join("index.db")
}

/// Opens the index, building or refreshing it first as its state requires.
/// `rebuild` builds it anew whatever its state.
pub fn open(ctx: &Context, rebuild: bool) -> Result<Opened, String> {
    std::fs::create_dir_all(&ctx.dir).map_err(|e| format!("{}: {e}", ctx.dir.display()))?;
    // The directory ignores itself, so the cache stays out of git in a
    // project that does not ignore its state directory.
    let ignore = ctx.dir.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(&ignore, "*\n").map_err(|e| format!("{}: {e}", ignore.display()))?;
    }
    let mut force = rebuild;
    for _ in 0..2 {
        if !force && let Some(mut conn) = open_current(ctx)? {
            match refresh(ctx, &mut conn) {
                Ok(stale) => return Ok(Opened { conn, stale }),
                Err(Fault::Corrupt(_)) => force = true,
                Err(Fault::Other(e)) => return Err(e),
            }
        }
        build(ctx, force)?;
        force = false;
    }
    Err(format!(
        "search index {} is unusable after a rebuild",
        db_path(ctx).display()
    ))
}

/// The index file when it exists, reads as a database and has this schema
/// version and configuration; `None` means it must be built.
fn open_current(ctx: &Context) -> Result<Option<Connection>, String> {
    let path = db_path(ctx);
    if !path.exists() {
        return Ok(None);
    }
    let read = || -> rusqlite::Result<Connection> {
        let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        conn.busy_timeout(Duration::from_millis(ctx.cfg.lock_timeout_ms))?;
        let value = |key: &str| {
            conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| {
                r.get::<_, String>(0)
            })
        };
        let current =
            value("schema_version")? == SCHEMA_VERSION && value("fingerprint")? == ctx.fingerprint;
        if current {
            Ok(conn)
        } else {
            Err(rusqlite::Error::QueryReturnedNoRows)
        }
    };
    match read() {
        Ok(conn) => Ok(Some(conn)),
        Err(e) if e.sqlite_error_code() == Some(ErrorCode::DatabaseBusy) => Err(format!(
            "search index {}: lock timeout: {e}",
            path.display()
        )),
        // Not a database, no `meta` table, another version: build it anew.
        Err(_) => Ok(None),
    }
}

/// The write lock, when no other process holds it.
fn try_lock(ctx: &Context) -> Result<Option<File>, String> {
    let path = ctx.dir.join("lock");
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(format!("{}: {e}", path.display())),
    }
}

/// Builds the index in a new file and renames it over the old one. Without
/// `force`, an index that another process built while this one waited for the
/// lock is kept.
fn build(ctx: &Context, force: bool) -> Result<(), String> {
    let waited = Instant::now();
    let _lock = loop {
        if let Some(lock) = try_lock(ctx)? {
            break lock;
        }
        if waited.elapsed() >= Duration::from_millis(ctx.cfg.lock_timeout_ms) {
            return Err(format!(
                "search index: lock timeout: another process held {} for {} ms",
                ctx.dir.join("lock").display(),
                ctx.cfg.lock_timeout_ms
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if !force && open_current(ctx)?.is_some() {
        return Ok(());
    }
    let path = db_path(ctx);
    let tmp = ctx.dir.join(format!("index.db.tmp-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let built = write_new(ctx, &tmp).and_then(|()| {
        // A journal that a crashed writer left belongs to the file being replaced.
        let _ = std::fs::remove_file(ctx.dir.join("index.db-journal"));
        std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
    });
    if built.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    built
}

fn write_new(ctx: &Context, tmp: &Path) -> Result<(), String> {
    let run = || -> Result<(), Fault> {
        let mut conn = Connection::open(tmp)?;
        // The file is thrown away on failure, so it needs no journal.
        conn.execute_batch("PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;")?;
        conn.execute_batch(SCHEMA)?;
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO meta VALUES ('schema_version', ?1), ('fingerprint', ?2)",
            params![SCHEMA_VERSION, ctx.fingerprint],
        )?;
        for source in sources(ctx)? {
            index_file(ctx, &tx, &source, None)?;
        }
        sync_words(&tx)?;
        tx.commit()?;
        Ok(())
    };
    run().map_err(String::from)
}

/// The files of the `notes` scope, without generated ones.
fn sources(ctx: &Context) -> Result<Vec<Source>, String> {
    let found = crate::archive::files::walk(&ctx.project.root, &[&ctx.notes])?
        .pop()
        .unwrap_or_default();
    let mut sources = Vec::new();
    for path in found {
        if ctx.class_of(&path) == super::Class::Generated {
            continue;
        }
        let file = ctx.project.root.join(&path);
        let meta = match std::fs::metadata(&file) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{}: {e}", file.display())),
        };
        let mtime_ns = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX));
        sources.push(Source {
            path,
            size: i64::try_from(meta.len()).unwrap_or(i64::MAX),
            mtime_ns,
        });
    }
    Ok(sources)
}

/// Brings the index up to the sources: removes the units of deleted files and
/// reads the files whose size or modification time changed. Returns why the
/// index is still stale, when it is.
fn refresh(ctx: &Context, conn: &mut Connection) -> Result<Option<String>, Fault> {
    let mut stored: HashMap<String, (i64, i64, i64)> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT path, id, size, mtime_ns FROM files")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))?;
        for row in rows {
            let (path, file) = row?;
            stored.insert(path, file);
        }
    }
    let mut changed = Vec::new();
    for source in sources(ctx)? {
        match stored.remove(&source.path) {
            Some((_, size, mtime_ns)) if size == source.size && mtime_ns == source.mtime_ns => {}
            Some((id, ..)) => changed.push((source, Some(id))),
            None => changed.push((source, None)),
        }
    }
    // What is left in `stored` has no source any more.
    if changed.is_empty() && stored.is_empty() {
        return Ok(None);
    }
    let pending = changed.len() + stored.len();
    let Some(_lock) = try_lock(ctx)? else {
        return Ok(Some(format!(
            "another process is updating the index; {pending} changed file(s) are not in it yet"
        )));
    };
    let budget = Duration::from_millis(ctx.cfg.refresh_budget_ms);
    let started = Instant::now();
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut left = pending;
    for (id, ..) in stored.values() {
        remove_file(&tx, *id)?;
        left -= 1;
    }
    for (source, id) in &changed {
        if started.elapsed() >= budget {
            break;
        }
        index_file(ctx, &tx, source, *id)?;
        left -= 1;
    }
    sync_words(&tx)?;
    tx.commit()?;
    Ok((left > 0).then(|| {
        format!(
            "the refresh stopped at its budget of {} ms; {left} changed file(s) are not in the index yet",
            ctx.cfg.refresh_budget_ms
        )
    }))
}

/// Brings the trigram table of words in line with the words of the units.
fn sync_words(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "DELETE FROM words_tri WHERE term NOT IN (SELECT term FROM units_vocab);
         INSERT INTO words_tri (term) SELECT term FROM units_vocab
             WHERE term NOT IN (SELECT term FROM words_tri);",
    )
}

fn remove_units(tx: &Transaction, file: i64) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM units_fts WHERE rowid IN (SELECT id FROM units WHERE file_id = ?1)",
        [file],
    )?;
    tx.execute("DELETE FROM units WHERE file_id = ?1", [file])?;
    Ok(())
}

/// A unit never outlives its source.
fn remove_file(tx: &Transaction, file: i64) -> rusqlite::Result<()> {
    remove_units(tx, file)?;
    tx.execute("DELETE FROM files WHERE id = ?1", [file])?;
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Reads one source and replaces its units. `existing` is its row in `files`.
fn index_file(
    ctx: &Context,
    tx: &Transaction,
    source: &Source,
    existing: Option<i64>,
) -> Result<(), Fault> {
    let file = ctx.project.root.join(&source.path);
    let bytes = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        // Deleted since the walk.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(id) = existing {
                remove_file(tx, id)?;
            }
            return Ok(());
        }
        Err(e) => return Err(format!("{}: {e}", file.display()).into()),
    };
    let hash = hex(&Sha256::digest(&bytes));
    if let Some(id) = existing {
        let same: bool = tx.query_row(
            "SELECT hash = ?2 FROM files WHERE id = ?1",
            params![id, hash],
            |r| r.get(0),
        )?;
        if same {
            // Touched, not changed.
            tx.execute(
                "UPDATE files SET size = ?2, mtime_ns = ?3 WHERE id = ?1",
                params![id, source.size, source.mtime_ns],
            )?;
            return Ok(());
        }
        remove_file(tx, id)?;
    }
    let meta = ctx.spec.meta_of(&source.path, &bytes);
    let (task, kind) = units::task_and_kind(&bytes);
    tx.execute(
        "INSERT INTO files (path, size, mtime_ns, hash, class, record_id, title, summary, date, \
         status, kind, task, superseded_by) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            source.path,
            source.size,
            source.mtime_ns,
            hash,
            ctx.class_of(&source.path).as_str(),
            meta.id,
            meta.title,
            meta.summary,
            meta.date,
            meta.status,
            kind,
            task,
            meta.superseded_by.join("\n"),
        ],
    )?;
    let file_id = tx.last_insert_rowid();
    let ids = meta
        .id
        .iter()
        .chain(&meta.aliases)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" ; ");
    let title = meta.title.as_deref().unwrap_or("");
    let summary = meta.summary.as_deref().unwrap_or("");
    let text = String::from_utf8_lossy(&bytes);
    let mut unit = tx.prepare_cached(
        "INSERT INTO units (file_id, start_line, end_line, heading, hash) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut fts = tx.prepare_cached(
        "INSERT INTO units_fts (rowid, ids, title, summary, heading, path, body) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    let mut sections = units::sections(&text, front::split(&bytes).body_line);
    if sections.is_empty() {
        // A record with front matter or a title only is still found by them.
        sections.push(units::Unit {
            start: 1,
            end: text.lines().count().max(1),
            heading: String::new(),
            body: String::new(),
        });
    }
    for u in sections {
        let indexed = format!("{ids}\n{title}\n{}\n{}", u.heading, u.body);
        let unit_hash = hex(&Sha256::digest(indexed.as_bytes())[..16]);
        unit.execute(params![
            file_id,
            u.start as i64,
            u.end as i64,
            u.heading,
            unit_hash
        ])?;
        let id = tx.last_insert_rowid();
        fts.execute(params![
            id,
            ids,
            title,
            summary,
            u.heading,
            source.path,
            u.body
        ])?;
    }
    Ok(())
}
