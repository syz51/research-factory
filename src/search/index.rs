//! The search index: one SQLite file per checkout under the project's state
//! directory, a cache that is rebuilt from the sources at any time.
//!
//! States (docs/state-machines.md 3.1): an absent file, one of another schema
//! version or configuration, or a corrupt one is built anew in a temporary
//! file that then replaces it by rename. A current file is refreshed in place
//! for the sources whose stamp and content hash changed. A query builds only
//! the scopes it searches; a scope searched for the first time is added whole.
//! One lock file admits one writer; a reader that finds it held answers from
//! the last committed state and says so.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::{Connection, ErrorCode, OpenFlags, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use super::sources::{self, Doc, Loader, Resolver};
use super::{Context, Scope, graph};

/// Raised when a table, a tokenizer or the way a unit is cut changes.
const SCHEMA_VERSION: &str = "5";
/// How many stale derivatives a notice names.
const STALE_NAMED: usize = 3;

/// BM25 column weights of `units_fts`, in column order: ids, title, summary,
/// heading path, meta (date, status and file path), body. They are the weights
/// that experiment E1 of masters fitted on its judged set (DR045): the title
/// and the heading path count most, the summary and the meta words least.
pub const WEIGHTS: &str = "1.0, 4.0, 0.5, 4.0, 0.5, 1.0";
/// The position of `body` among the columns of `units_fts`.
pub const BODY_COLUMN: usize = 5;

const SCHEMA: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
-- One row per source: a file, a registry entry, a session or a board task.
CREATE TABLE files(
    id INTEGER PRIMARY KEY,
    -- The source's key within its scope: a path, `ref:<key>`,
    -- `session:<source>:<id>` or `board:<id>`. A tracked text is a source of
    -- both `files` and `texts`.
    path TEXT NOT NULL,
    scope TEXT NOT NULL,
    -- What anchors name instead of the key, such as a card's text.
    anchor TEXT,
    stamp TEXT NOT NULL,
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
    superseded_by TEXT NOT NULL,
    -- The registry key the source names.
    ref_key TEXT,
    -- Scope-specific facts shown with a hit, as JSON.
    info TEXT
);
CREATE INDEX files_record ON files(record_id COLLATE NOCASE);
CREATE UNIQUE INDEX files_scope ON files(scope, path);
-- Names an exact lookup finds a source by, besides its record id.
CREATE TABLE aliases(file_id INTEGER NOT NULL REFERENCES files(id), alias TEXT NOT NULL);
CREATE INDEX aliases_alias ON aliases(alias COLLATE NOCASE);
CREATE INDEX aliases_file ON aliases(file_id);
-- Derived files that a source's units ignore because they no longer fit their text.
CREATE TABLE stale(file_id INTEGER NOT NULL REFERENCES files(id), path TEXT NOT NULL, why TEXT NOT NULL);
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
    ids, title, summary, heading, meta, body,
    tokenize = 'porter unicode61 remove_diacritics 2'
);
CREATE VIRTUAL TABLE units_vocab USING fts5vocab(units_fts, 'row');
CREATE VIRTUAL TABLE units_inst USING fts5vocab(units_fts, 'instance');
-- The words of `units_fts`, for a query word that is only part of a word.
CREATE VIRTUAL TABLE words_tri USING fts5(term, tokenize = 'trigram');
";

/// A failure of the index: `Corrupt` is answered by a rebuild.
#[derive(Debug)]
pub enum Fault {
    /// Another process replaced the index file after this one opened it.
    Replaced,
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
            Fault::Replaced => "another process replaced the search index".into(),
        }
    }
}

/// An index ready for a query.
pub struct Opened {
    pub conn: Connection,
    /// Why the index may lack the latest text of some files.
    pub stale: Option<String>,
    /// Why the answer from a scope may be incomplete: a source that could not
    /// be read, a derivative that is stale.
    pub notices: Vec<String>,
}

/// The session messages left out of the `sessions` scope, by rule.
pub fn left_out(conn: &Connection) -> Result<BTreeMap<String, u64>, Fault> {
    let mut stmt =
        conn.prepare("SELECT info FROM files WHERE scope = 'sessions' AND info IS NOT NULL")?;
    let mut total = BTreeMap::new();
    for info in stmt.query_map([], |r| r.get::<_, String>(0))? {
        let info: serde_json::Value = serde_json::from_str(&info?).unwrap_or_default();
        if let Some(rules) = info.get("left_out").and_then(|v| v.as_object()) {
            for (rule, n) in rules {
                *total.entry(rule.clone()).or_default() += n.as_u64().unwrap_or(0);
            }
        }
    }
    Ok(total)
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
    for _ in 0..3 {
        if !force && let Some(mut conn) = open_current(ctx)? {
            match refresh(ctx, &mut conn) {
                Ok((stale, mut notices)) => {
                    notices.extend(stale_notices(ctx, &conn).map_err(String::from)?);
                    return Ok(Opened {
                        conn,
                        stale,
                        notices,
                    });
                }
                // Open the file that replaced it.
                Err(Fault::Replaced) => continue,
                Err(Fault::Corrupt(_)) => force = true,
                Err(Fault::Other(e)) => return Err(e),
            }
        }
        // A build lists every scope; refreshing at once would list them again.
        if let Some(mut notices) = build(ctx, force)?
            && let Some(conn) = open_current(ctx)?
        {
            notices.extend(stale_notices(ctx, &conn).map_err(String::from)?);
            return Ok(Opened {
                conn,
                stale: None,
                notices,
            });
        }
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

/// Builds the index in a new file and renames it over the old one, and
/// returns the notices of its listings. Without `force`, an index that another
/// process built while this one waited for the lock is kept: `None`.
fn build(ctx: &Context, force: bool) -> Result<Option<Vec<String>>, String> {
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
        return Ok(None);
    }
    let path = db_path(ctx);
    let tmp = ctx.dir.join(format!("index.db.tmp-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let built = write_new(ctx, &tmp).and_then(|notices| {
        // A journal that a crashed writer left belongs to the file being replaced.
        let _ = std::fs::remove_file(ctx.dir.join("index.db-journal"));
        std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Some(notices))
    });
    if built.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    built
}

/// Names one build of the index file, so that a writer can tell that the
/// file it opened is still the one at the index path.
fn build_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{}-{nanos}", std::process::id())
}

fn build_of(conn: &Connection) -> rusqlite::Result<String> {
    conn.query_row("SELECT value FROM meta WHERE key = 'build'", [], |r| {
        r.get(0)
    })
}

/// The scopes an index holds.
fn built(conn: &Connection) -> rusqlite::Result<BTreeSet<Scope>> {
    let value: String = conn.query_row("SELECT value FROM meta WHERE key = 'scopes'", [], |r| {
        r.get(0)
    })?;
    Ok(value
        .split(',')
        .filter_map(|s| Scope::ALL.into_iter().find(|c| c.as_str() == s))
        .collect())
}

fn set_built(tx: &Transaction, scopes: &BTreeSet<Scope>) -> rusqlite::Result<()> {
    let names: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();
    tx.execute(
        "INSERT OR REPLACE INTO meta VALUES ('scopes', ?1)",
        [names.join(",")],
    )?;
    Ok(())
}

/// A new index of the scopes the query searches, and the notices of their
/// listings.
fn write_new(ctx: &Context, tmp: &Path) -> Result<Vec<String>, String> {
    let run = || -> Result<Vec<String>, Fault> {
        let mut conn = Connection::open(tmp)?;
        // The file is thrown away on failure, so it needs no journal.
        conn.execute_batch("PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;")?;
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch(graph::SCHEMA)?;
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO meta VALUES ('schema_version', ?1), ('fingerprint', ?2), ('build', ?3)",
            params![SCHEMA_VERSION, ctx.fingerprint, build_id()],
        )?;
        let mut resolver = Resolver::new(&ctx.project.root)?;
        let mut loader = Loader::new(ctx);
        let mut notices = Vec::new();
        for &scope in &ctx.scopes {
            let listing = sources::list(ctx, scope, &mut resolver)?;
            for doc in &listing.docs {
                index_doc(ctx, &tx, &mut loader, doc, None)?;
            }
            notices.extend(listing.notices);
        }
        if let Some(papers) = graph::papers_changed(ctx, &tx)? {
            graph::write_papers(&tx, &papers)?;
        }
        set_built(&tx, &ctx.scopes.iter().copied().collect())?;
        sync_words(&tx)?;
        tx.commit()?;
        Ok(notices)
    };
    run().map_err(String::from)
}

/// A source whose stamp differs from the stored one, or that is new.
struct Change<'d> {
    doc: &'d Doc,
    existing: Option<i64>,
    /// Of a scope the index did not hold: loaded whatever the budget.
    added: bool,
}

/// Brings the index up to the sources of the scopes the query searches:
/// removes the units of sources that are gone, reads the sources whose stamp
/// changed and adds a scope the index lacks. Returns why the index is still
/// stale, when it is, and the notices of the listings.
fn refresh(ctx: &Context, conn: &mut Connection) -> Result<(Option<String>, Vec<String>), Fault> {
    let have = built(conn)?;
    let mut resolver = Resolver::new(&ctx.project.root)?;
    let mut listings = Vec::new();
    let mut notices = Vec::new();
    for &scope in &ctx.scopes {
        let listing = sources::list(ctx, scope, &mut resolver)?;
        notices.extend(listing.notices.iter().cloned());
        listings.push((scope, listing));
    }
    let mut changed: Vec<Change> = Vec::new();
    let mut gone: Vec<i64> = Vec::new();
    {
        let mut stmt = conn.prepare_cached("SELECT path, id, stamp FROM files WHERE scope = ?1")?;
        for (scope, listing) in &listings {
            let added = !have.contains(scope);
            let mut stored: HashMap<String, (i64, String)> = HashMap::new();
            let rows = stmt.query_map([scope.as_str()], |r| {
                Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?)))
            })?;
            for row in rows {
                let (key, file) = row?;
                stored.insert(key, file);
            }
            for doc in &listing.docs {
                match stored.remove(&doc.key) {
                    Some((_, stamp)) if stamp == doc.stamp => {}
                    Some((id, _)) => changed.push(Change {
                        doc,
                        existing: Some(id),
                        added,
                    }),
                    None => changed.push(Change {
                        doc,
                        existing: None,
                        added,
                    }),
                }
            }
            // What is left has no source any more, unless the listing is incomplete.
            if listing.complete {
                gone.extend(stored.into_values().map(|(id, _)| id));
            }
        }
    }
    let new_scopes = ctx.scopes.iter().any(|s| !have.contains(s));
    let papers = graph::papers_changed(ctx, conn)?;
    if changed.is_empty() && gone.is_empty() && !new_scopes && papers.is_none() {
        return Ok((None, notices));
    }
    let pending = changed.len() + gone.len();
    let Some(_lock) = try_lock(ctx)? else {
        let what = match (pending, papers.is_some()) {
            (0, true) => "the changed paper citations are".to_string(),
            (n, false) => format!("{n} changed source(s) are"),
            (n, true) => format!("{n} changed source(s) and the changed paper citations are"),
        };
        return Ok((
            Some(format!(
                "another process is updating the index; {what} not in it yet"
            )),
            notices,
        ));
    };
    // A build may have renamed a new file over the one this connection
    // holds, between its opening and the lock. A write would then go to the
    // unlinked file and leave a journal beside the new one.
    let at_path = Connection::open_with_flags(db_path(ctx), OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|current| build_of(&current));
    if at_path.ok() != Some(build_of(conn)?) {
        return Err(Fault::Replaced);
    }
    let budget = if ctx.budgeted {
        Duration::from_millis(ctx.cfg.refresh_budget_ms)
    } else {
        Duration::MAX
    };
    let started = Instant::now();
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut left = pending;
    if let Some(papers) = &papers {
        graph::write_papers(&tx, papers)?;
    }
    for id in gone {
        remove_file(&tx, id)?;
        left -= 1;
    }
    let mut loader = Loader::new(ctx);
    // Sources of a new scope first, as it is empty until they are in.
    changed.sort_by_key(|c| !c.added);
    for c in &changed {
        if !c.added && started.elapsed() >= budget {
            break;
        }
        index_doc(ctx, &tx, &mut loader, c.doc, c.existing)?;
        left -= 1;
    }
    set_built(&tx, &have.iter().chain(&ctx.scopes).copied().collect())?;
    sync_words(&tx)?;
    tx.commit()?;
    Ok((
        (left > 0).then(|| {
            format!(
                "the refresh stopped at its budget of {} ms; {left} changed source(s) are not in the index yet",
                ctx.cfg.refresh_budget_ms
            )
        }),
        notices,
    ))
}

/// One notice per scope searched that has stale derivatives.
fn stale_notices(ctx: &Context, conn: &Connection) -> Result<Vec<String>, Fault> {
    let mut stmt = conn.prepare_cached(
        "SELECT s.path, s.why FROM stale s JOIN files f ON f.id = s.file_id \
         WHERE f.scope = ?1 ORDER BY s.path",
    )?;
    let mut out = Vec::new();
    for scope in &ctx.searched {
        let rows: Vec<(String, String)> = stmt
            .query_map([scope.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let Some((first, why)) = rows.first() else {
            continue;
        };
        let mut names: Vec<&str> = rows
            .iter()
            .take(STALE_NAMED)
            .map(|r| r.0.as_str())
            .collect();
        if rows.len() > STALE_NAMED {
            names.push("…");
        }
        out.push(format!(
            "scope `{}`: {} stale derivative(s) ignored ({}); {first} was {why}",
            scope.as_str(),
            rows.len(),
            names.join(", ")
        ));
    }
    Ok(out)
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

/// A unit never outlives its source, nor do the edges that start at it.
fn remove_file(tx: &Transaction, file: i64) -> rusqlite::Result<()> {
    graph::forget_file(tx, file)?;
    remove_units(tx, file)?;
    tx.execute("DELETE FROM aliases WHERE file_id = ?1", [file])?;
    tx.execute("DELETE FROM stale WHERE file_id = ?1", [file])?;
    tx.execute("DELETE FROM files WHERE id = ?1", [file])?;
    Ok(())
}

/// Loads one source and replaces its units. `existing` is its row in `files`.
fn index_doc(
    ctx: &Context,
    tx: &Transaction,
    loader: &mut Loader,
    doc: &Doc,
    existing: Option<i64>,
) -> Result<(), Fault> {
    let Some(loaded) = loader.load(tx, doc)? else {
        // Gone since it was listed.
        if let Some(id) = existing {
            remove_file(tx, id)?;
        }
        return Ok(());
    };
    if let Some(id) = existing {
        let same: bool = tx.query_row(
            "SELECT hash = ?2 FROM files WHERE id = ?1",
            params![id, loaded.hash],
            |r| r.get(0),
        )?;
        if same {
            // Touched, not changed.
            tx.execute(
                "UPDATE files SET stamp = ?2 WHERE id = ?1",
                params![id, doc.stamp],
            )?;
            return Ok(());
        }
        remove_file(tx, id)?;
    }
    let row = &loaded.row;
    tx.execute(
        "INSERT INTO files (path, scope, anchor, stamp, hash, class, record_id, title, summary, \
         date, status, kind, task, superseded_by, ref_key, info) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            doc.key,
            doc.scope.as_str(),
            row.anchor,
            doc.stamp,
            loaded.hash,
            row.class.unwrap_or(super::Class::Own).as_str(),
            row.record_id,
            row.title,
            row.summary,
            row.date,
            row.status,
            row.kind,
            row.task,
            row.superseded_by.join("\n"),
            row.ref_key,
            row.info.as_ref().map(|v| v.to_string()),
        ],
    )?;
    let file_id = tx.last_insert_rowid();
    if let Some(bytes) = &loaded.note {
        graph::index_edges(ctx, tx, file_id, &doc.key, bytes)?;
    }
    let mut alias = tx.prepare_cached("INSERT INTO aliases (file_id, alias) VALUES (?1, ?2)")?;
    for a in &row.aliases {
        alias.execute(params![file_id, a])?;
    }
    let mut stale =
        tx.prepare_cached("INSERT INTO stale (file_id, path, why) VALUES (?1, ?2, ?3)")?;
    for (path, why) in &loaded.stale {
        stale.execute(params![file_id, path, why])?;
    }
    let title = row.title.as_deref().unwrap_or("");
    let summary = row.summary.as_deref().unwrap_or("");
    // Date, status and, for notes and files, the path: other keys are not words.
    let path = match doc.scope {
        Scope::Notes | Scope::Files => doc.key.as_str(),
        _ => "",
    };
    let meta = [row.date.as_deref(), row.status.as_deref(), Some(path)]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut unit = tx.prepare_cached(
        "INSERT INTO units (file_id, start_line, end_line, heading, hash) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut fts = tx.prepare_cached(
        "INSERT INTO units_fts (rowid, ids, title, summary, heading, meta, body) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for u in &loaded.units {
        let indexed = format!(
            "{}\n{title}\n{summary}\n{}\n{meta}\n{}",
            row.ids, u.heading, u.body
        );
        let unit_hash = sources::hex(&Sha256::digest(indexed.as_bytes())[..16]);
        unit.execute(params![
            file_id,
            u.start as i64,
            u.end as i64,
            u.heading,
            unit_hash
        ])?;
        let id = tx.last_insert_rowid();
        fts.execute(params![
            id, row.ids, title, summary, u.heading, meta, u.body
        ])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_weights_rank_title_and_heading_first_then_ids_and_body_then_summary_and_meta() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        let names: Vec<String> = conn
            .prepare("SELECT * FROM units_fts")
            .unwrap()
            .column_names()
            .map(String::from)
            .collect();
        assert_eq!(WEIGHTS.split(',').count(), names.len());
        assert_eq!(names[BODY_COLUMN], "body");
        // Row i holds the word in column i alone; every other cell holds one other word.
        for i in 0..names.len() {
            let mut cells = vec!["filler"; names.len()];
            cells[i] = "lynx";
            conn.execute(
                "INSERT INTO units_fts(rowid, ids, title, summary, heading, meta, body) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    i as i64, cells[0], cells[1], cells[2], cells[3], cells[4], cells[5]
                ],
            )
            .unwrap();
        }
        let order: Vec<String> = conn
            .prepare(&format!(
                "SELECT rowid FROM units_fts WHERE units_fts MATCH 'lynx' \
                 AND rank MATCH 'bm25({WEIGHTS})' ORDER BY rank, rowid"
            ))
            .unwrap()
            .query_map([], |r| r.get::<_, i64>(0))
            .unwrap()
            .map(|i| names[usize::try_from(i.unwrap()).unwrap()].clone())
            .collect();
        assert_eq!(
            order,
            ["title", "heading", "ids", "body", "summary", "meta"]
        );
    }
}
