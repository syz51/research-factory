//! A query against the index (docs/state-machines.md 3.3): parse, route an
//! exact record id, retrieve by words and by trigrams, apply the status and
//! class priors, fuse the phrasings and group the units by record.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use super::index::{BODY_COLUMN, Fault, WEIGHTS};
use super::refs::{normal_arxiv, normal_doi};
use super::thesaurus::Entry;
use super::{Class, Context, Scope, graph};

/// Candidates taken from each retrieval of one phrasing.
const CANDIDATES: usize = 100;
/// A query with more terms is refused.
const MAX_TERMS: usize = 32;
/// The constant of reciprocal rank fusion.
const RRF_K: f64 = 60.0;
const SNIPPET_CHARS: usize = 160;
/// A unit of a record that is no longer in force scores this share.
const INACTIVE_PRIOR: f64 = 0.25;
/// A query word that is part of indexed words stands for at most this many of them.
const PART_OF: usize = 20;
/// The share of its BM25 score that a match on a part of a word keeps.
const PART_WEIGHT: f64 = 0.5;

/// One word, or the words of one quoted phrase, lowercased.
#[derive(Debug, PartialEq)]
pub struct Term {
    tokens: Vec<String>,
    /// Equivalent terms from the thesaurus: the term matches a unit that
    /// holds the term or one of these.
    alts: Vec<Vec<String>>,
}

impl Term {
    fn text(&self) -> String {
        self.tokens.join(" ")
    }

    /// As an FTS5 string: a word, or a phrase of adjacent words; with
    /// thesaurus terms, any of them.
    fn fts(&self) -> String {
        let quoted = |tokens: &Vec<String>| format!("\"{}\"", tokens.join(" "));
        if self.alts.is_empty() {
            return quoted(&self.tokens);
        }
        let any: Vec<String> = std::iter::once(&self.tokens)
            .chain(&self.alts)
            .map(quoted)
            .collect();
        format!("({})", any.join(" OR "))
    }
}

/// One phrasing of the question.
#[derive(Debug, PartialEq)]
pub struct Phrasing {
    terms: Vec<Term>,
    /// The words as typed, for the exact id lookup.
    raw: Vec<String>,
    /// Every word in order, for the thesaurus.
    seq: Vec<String>,
}

/// One thesaurus entry that a query used.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct Expansion {
    /// The term of the query.
    pub term: String,
    /// The equivalent terms that were added.
    pub added: Vec<String>,
    /// The entry, as `a = b`.
    pub entry: String,
}

impl Phrasing {
    /// Adds the equivalent terms of each active thesaurus entry of which the
    /// phrasing holds a term, and returns the entries used. A word of the
    /// entry's term then matches a unit that holds any term of the entry.
    pub fn expand(&mut self, entries: &[Entry]) -> Vec<Expansion> {
        let mut used = Vec::new();
        for entry in entries.iter().filter(|e| !e.retired) {
            let held = entry.terms.iter().position(|term| {
                self.seq
                    .windows(term.len())
                    .any(|window| window == term.as_slice())
            });
            let Some(held) = held else { continue };
            let own = &entry.terms[held];
            let others: Vec<&Vec<String>> = entry
                .terms
                .iter()
                .enumerate()
                .filter(|(i, other)| *i != held && !contains(&self.seq, other))
                .map(|(_, other)| other)
                .collect();
            let mut applied = false;
            for term in &mut self.terms {
                if !term.tokens.iter().all(|w| own.contains(w)) {
                    continue;
                }
                for other in &others {
                    if !term.alts.contains(other) {
                        term.alts.push((*other).clone());
                    }
                    applied = true;
                }
            }
            if !applied {
                continue;
            }
            // A one-word term may be a record id.
            for other in &others {
                if let [word] = other.as_slice() {
                    self.raw.push(word.clone());
                }
            }
            used.push(Expansion {
                term: own.join(" "),
                added: others.iter().map(|o| o.join(" ")).collect(),
                entry: entry.text(),
            });
        }
        used
    }
}

fn contains(seq: &[String], term: &[String]) -> bool {
    seq.windows(term.len()).any(|window| window == term)
}

pub fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// A decimal number as the words the index holds, without a fraction of only
/// zeros: `47.0%` is `47`, `0.50` is `0 50`. `None` for other words.
fn number(word: &str) -> Option<Vec<String>> {
    let core = word.trim_matches(|c: char| !c.is_ascii_digit());
    let (int, frac) = core.split_once('.')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let rest = word.replacen(core, "", 1);
    if !digits(int) || !digits(frac) || rest.chars().any(char::is_alphanumeric) {
        return None;
    }
    // A fraction of only zeros adds nothing (`47.0` is `47`); any other is
    // kept as written, since the index holds the text's tokens as written.
    let frac = if frac.bytes().all(|b| b == b'0') {
        ""
    } else {
        frac
    };
    Some(
        [int, frac]
            .into_iter()
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect(),
    )
}

/// Parses one phrasing. Punctuation splits words; `"…"` makes a phrase; a
/// decimal number is one term.
pub fn parse(text: &str) -> Result<Phrasing, String> {
    let mut terms: Vec<Term> = Vec::new();
    let mut add = |tokens: Vec<String>| {
        let term = Term {
            tokens,
            alts: Vec::new(),
        };
        if !term.tokens.is_empty() && !terms.contains(&term) {
            terms.push(term);
        }
    };
    // Odd pieces of a split at `"` are inside quotes; an unclosed quote runs to the end.
    for (i, piece) in text.split('"').enumerate() {
        if i % 2 == 1 {
            add(words(piece));
        } else {
            for chunk in piece.split_whitespace() {
                match number(chunk) {
                    Some(tokens) => add(tokens),
                    None => words(chunk).into_iter().for_each(|w| add(vec![w])),
                }
            }
        }
    }
    if terms.is_empty() {
        return Err(format!("bad query: `{text}` holds no word"));
    }
    if terms.len() > MAX_TERMS {
        return Err(format!(
            "bad query: {} terms; a query takes at most {MAX_TERMS}",
            terms.len()
        ));
    }
    let raw = text
        .split_whitespace()
        .map(|w| w.trim_matches('"').to_string())
        .filter(|w| !w.is_empty())
        .collect();
    Ok(Phrasing {
        terms,
        raw,
        seq: words(text),
    })
}

/// The record a hit belongs to.
#[derive(Debug, Clone, Serialize)]
pub struct RecordInfo {
    pub id: Option<String>,
    pub title: Option<String>,
    pub date: Option<String>,
    pub status: Option<String>,
    /// The source class: own, paper, copied, chat or historical.
    pub class: String,
    pub kind: Option<String>,
    pub task: Option<String>,
    /// The record's status is one of `[lab.archive] inactive_statuses`.
    pub inactive: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub superseded_by: Vec<String>,
}

/// The stage scores of one hit, for `--explain`.
#[derive(Debug, Clone, Serialize)]
pub struct Explain {
    /// The query names the record's id.
    pub exact_id: bool,
    /// The weighted BM25 score of the word index; 0 when the words did not retrieve the unit.
    pub bm25: f64,
    /// The BM25 score of the trigram index; 0 when it did not retrieve the unit.
    pub trigram: f64,
    /// The factor for the record's status and source class.
    pub prior: f64,
    /// `(bm25 + trigram) * prior`.
    pub score: f64,
    /// The sum over the phrasings of `1 / (60 + rank)`.
    pub fused: f64,
    /// The query terms the unit holds, of the phrasing that ranked it best.
    pub matched: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Hit {
    pub rank: usize,
    /// `path#Lstart-Lend`: 1-based inclusive lines of the source file;
    /// `ref:<key>` for a registry entry; `board:<id>` or `board:<id>#note-<n>`
    /// for a board task; `transcript#L<n>` for a session message.
    pub anchor: String,
    pub scope: Scope,
    /// The source's key: a path, `ref:<key>`, `session:<source>:<id>` or `board:<id>`.
    pub path: String,
    pub start: i64,
    pub end: i64,
    pub heading: String,
    pub snippet: String,
    /// The content hash of the unit's indexed text.
    pub hash: String,
    pub record: RecordInfo,
    /// The registry key of a `refs` or `texts` hit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Scope-specific facts: year, first author, depth and text of an entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<serde_json::Value>,
    /// The query terms the unit lacks.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain: Option<Explain>,
    /// How many other notes link the hit's file.
    pub linked_by: i64,
    /// Units of other notes that hold the same fact and link this record:
    /// a log entry or a topic page. `--all` shows them as hits of their own.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub also_in: Vec<AlsoIn>,
}

/// A unit folded under the hit of the record it links.
#[derive(Debug, Serialize)]
pub struct AlsoIn {
    pub anchor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// A query word that no unit holds.
#[derive(Debug, Serialize, PartialEq)]
pub struct Unknown {
    pub word: String,
    /// The nearest indexed words.
    pub nearest: Vec<String>,
}

pub struct Outcome {
    pub hits: Vec<Hit>,
    /// Hits below the limit.
    pub more: usize,
    pub unknown: Vec<Unknown>,
    /// The best units of the lexical stage before grouping: anchor and fused score.
    pub candidates: Vec<(String, f64)>,
}

/// Units of the lexical stage that the trace keeps.
const TRACED: usize = 10;

/// How much each term of the phrasing tells units apart: `ln(1 + units /
/// units that hold it)`, with the term's text. A term in no unit weighs most.
pub fn weights(conn: &Connection, phrasing: &Phrasing) -> Result<Vec<(String, f64)>, Fault> {
    let units: i64 = conn.query_row("SELECT count(*) FROM units", [], |r| r.get(0))?;
    let mut count = conn.prepare("SELECT count(*) FROM units_fts WHERE units_fts MATCH ?1")?;
    phrasing
        .terms
        .iter()
        .map(|term| {
            let holders: i64 = count.query_row([term.fts()], |r| r.get(0))?;
            let weight = (1.0 + units as f64 / holders.max(1) as f64).ln();
            Ok((term.text(), weight))
        })
        .collect()
}

pub struct Options<'a> {
    pub limit: usize,
    /// One hit per record instead of two.
    pub records: bool,
    /// Keep only records with one of these statuses; empty keeps all.
    pub statuses: &'a [String],
    /// A `SELECT` of the ids of the units the graph and date filters keep.
    pub filter: Option<&'a str>,
    /// Rank by the records in force on this date.
    pub as_of: Option<&'a str>,
    /// Fold a unit that links a record under that record's hit.
    pub fold: bool,
}

/// A unit that one phrasing retrieved.
struct Candidate {
    unit: i64,
    file: i64,
    phrasing: usize,
    exact_id: bool,
    bm25: f64,
    trigram: f64,
    /// Bit `i` is set when the unit holds term `i` of the phrasing.
    matched: u64,
    prior: f64,
    fused: f64,
    start: i64,
    end: i64,
    heading: String,
    hash: String,
    path: String,
    scope: Scope,
    anchor: Option<String>,
    key: Option<String>,
    info: Option<serde_json::Value>,
    record: RecordInfo,
}

impl Candidate {
    fn score(&self) -> f64 {
        (self.bm25 + self.trigram) * self.prior
    }
}

fn id_list(ids: impl Iterator<Item = i64>) -> String {
    ids.map(|i| i.to_string()).collect::<Vec<_>>().join(",")
}

/// The units one phrasing retrieves, best first, and the terms no unit holds.
fn retrieve(
    conn: &Connection,
    ctx: &Context,
    index: usize,
    phrasing: &Phrasing,
    opts: &Options,
    scopes: Option<&str>,
    allowed: Option<&HashSet<i64>>,
) -> Result<(Vec<Candidate>, Vec<usize>), Fault> {
    let statuses = opts.statuses;
    // The status and scope filters are part of each retrieval, so that they
    // apply before the cut to the best candidates.
    let mut conds = Vec::new();
    if !statuses.is_empty() {
        let list: Vec<String> = statuses
            .iter()
            .map(|s| format!("'{}'", s.replace('\'', "''")))
            .collect();
        conds.push(format!("f.status IN ({})", list.join(", ")));
    }
    if let Some(list) = scopes {
        conds.push(format!("f.scope IN ({list})"));
    }
    // The unary `+` keeps SQLite from handing a list to FTS5 as a rowid
    // constraint, which runs the whole match once per listed unit.
    let filter = |column: &str| {
        let mut out = String::new();
        if !conds.is_empty() {
            out = format!(
                " AND +{column} IN (SELECT u.id FROM units u JOIN files f ON f.id = u.file_id \
                 WHERE {})",
                conds.join(" AND ")
            );
        }
        if let Some(sql) = opts.filter {
            out.push_str(&format!(" AND +{column} IN ({sql})"));
        }
        out
    };
    let in_scope = scopes.map_or(String::new(), |list| format!(" AND f.scope IN ({list})"));
    // Unit id to its BM25 score, trigram score and matched terms.
    let mut found: HashMap<i64, (f64, f64, u64)> = HashMap::new();
    let mut known = Vec::new();
    let mut unknown = Vec::new();
    for (i, term) in phrasing.terms.iter().enumerate() {
        let hit = conn
            .query_row(
                &format!(
                    "SELECT 1 FROM units_fts WHERE units_fts MATCH ?1{} LIMIT 1",
                    filter("rowid")
                ),
                [term.fts()],
                |_| Ok(()),
            )
            .optional()?;
        if hit.is_some() {
            known.push(term.fts());
        } else {
            unknown.push(i);
        }
    }
    // Words: the units with every known term, then those with any.
    let ranked = format!(
        "SELECT rowid, rank FROM units_fts WHERE units_fts MATCH ?1 \
         AND rank MATCH 'bm25({WEIGHTS})'{} ORDER BY rank LIMIT {CANDIDATES}",
        filter("rowid")
    );
    let mut lexical = conn.prepare(&ranked)?;
    let mut joins = vec![" OR "];
    if known.len() > 1 {
        joins.insert(0, " AND ");
    }
    for join in joins.into_iter().filter(|_| !known.is_empty()) {
        let rows = lexical.query_map([known.join(join)], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
        })?;
        for row in rows {
            let (unit, rank) = row?;
            // FTS5 ranks are negative, the best the lowest.
            found.entry(unit).or_default().0 = -rank;
        }
    }
    // Trigrams: a term the word index lacks may be part of an indexed word.
    for &i in &unknown {
        let text = phrasing.terms[i].text();
        let words: Vec<String> = if text.chars().count() >= 3 {
            conn.prepare(&format!(
                "SELECT term FROM words_tri WHERE words_tri MATCH ?1 ORDER BY rank LIMIT {PART_OF}"
            ))?
            .query_map([format!("\"{text}\"")], |r| r.get(0))?
            .collect::<Result<_, _>>()?
        } else if !text.is_ascii() {
            // Too short for a trigram, such as a two-character Chinese or
            // Japanese word: scan the words.
            conn.prepare(&format!(
                "SELECT term FROM units_vocab WHERE instr(term, ?1) > 0 ORDER BY doc DESC LIMIT {PART_OF}"
            ))?
            .query_map([&text], |r| r.get(0))?
            .collect::<Result<_, _>>()?
        } else {
            Vec::new()
        };
        // The words are stems as the index holds them. A `MATCH` would stem
        // them a second time, so read their units from the vocabulary.
        let mut holders = conn.prepare_cached(&format!(
            "SELECT doc, count(*) FROM units_inst WHERE term = ?1{} \
             GROUP BY doc ORDER BY 2 DESC LIMIT {CANDIDATES}",
            filter("doc")
        ))?;
        for word in &words {
            let rows =
                holders.query_map([word], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
            for row in rows {
                let (unit, count) = row?;
                let entry = found.entry(unit).or_default();
                // A part of a word counts less than the word.
                entry.1 += PART_WEIGHT * (1.0 + (count as f64).ln());
                entry.2 |= 1 << i;
            }
        }
    }
    // An exact record id: every retrieved unit of the record, or its first unit.
    let mut exact_files = HashSet::new();
    let mut names: Vec<String> = Vec::new();
    for word in &phrasing.raw {
        // `D002,` and `H158?` name the ids D002 and H158.
        let id = word.trim_matches(|c: char| !c.is_alphanumeric());
        names.extend([id.to_string(), normal_arxiv(id), normal_doi(id)]);
        // A registry key alone: `232` names `ref:232`.
        if phrasing.raw.len() == 1 && !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) {
            names.push(format!("ref:{id}"));
        }
    }
    names.sort();
    names.dedup();
    for id in names.iter().filter(|n| !n.is_empty()) {
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT f.id, (SELECT min(u.id) FROM units u WHERE u.file_id = f.id) \
             FROM files f WHERE (f.record_id = ?1 COLLATE NOCASE \
             OR f.id IN (SELECT file_id FROM aliases WHERE alias = ?1 COLLATE NOCASE)){in_scope}"
        ))?;
        let rows = stmt.query_map([id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))
        })?;
        for row in rows {
            let (file, first) = row?;
            exact_files.insert(file);
            if let Some(first) = first {
                found.entry(first).or_default();
            }
        }
    }
    // Which known terms each candidate holds.
    let units = id_list(found.keys().copied());
    for (i, term) in phrasing.terms.iter().enumerate() {
        if unknown.contains(&i) {
            continue;
        }
        let mut stmt = conn.prepare(&format!(
            "SELECT rowid FROM units_fts WHERE units_fts MATCH ?1 AND rowid IN ({units})"
        ))?;
        let rows = stmt.query_map([term.fts()], |r| r.get::<_, i64>(0))?;
        for row in rows {
            if let Some(entry) = found.get_mut(&row?) {
                entry.2 |= 1 << i;
            }
        }
    }
    let mut candidates = Vec::new();
    for (unit, (bm25, trigram, matched)) in found {
        if allowed.is_some_and(|a| !a.contains(&unit)) {
            continue;
        }
        let mut c = load(conn, ctx, unit, opts.as_of)?;
        c.phrasing = index;
        c.exact_id = exact_files.contains(&c.file);
        c.bm25 = bm25;
        c.trigram = trigram;
        c.matched = matched;
        let wanted = statuses.is_empty()
            || c.record
                .status
                .as_ref()
                .is_some_and(|s| statuses.contains(s));
        if wanted {
            candidates.push(c);
        }
    }
    // The exact id first, then the units with every term, then by score.
    let all = (1u64 << phrasing.terms.len()) - 1;
    candidates.sort_by(|a, b| {
        (b.exact_id, b.matched == all)
            .cmp(&(a.exact_id, a.matched == all))
            .then(b.score().total_cmp(&a.score()))
            .then_with(|| (&a.path, a.start).cmp(&(&b.path, b.start)))
    });
    Ok((candidates, unknown))
}

/// Reads one unit and its record. With `as_of`, a record that is no longer
/// in force ranks as in force when it was on that date.
fn load(
    conn: &Connection,
    ctx: &Context,
    unit: i64,
    as_of: Option<&str>,
) -> Result<Candidate, Fault> {
    let mut c = conn
        .prepare_cached(
            "SELECT u.file_id, u.start_line, u.end_line, u.heading, u.hash, f.path, f.class, \
             f.record_id, f.title, f.date, f.status, f.kind, f.task, f.superseded_by, \
             f.scope, f.anchor, f.ref_key, f.info \
             FROM units u JOIN files f ON f.id = u.file_id WHERE u.id = ?1",
        )?
        .query_row([unit], |r| {
            let class: String = r.get(6)?;
            let status: Option<String> = r.get(10)?;
            let superseded_by: String = r.get(13)?;
            let inactive = status
                .as_ref()
                .is_some_and(|s| ctx.inactive_statuses.contains(s));
            Ok(Candidate {
                unit,
                file: r.get(0)?,
                phrasing: 0,
                exact_id: false,
                bm25: 0.0,
                trigram: 0.0,
                matched: 0,
                prior: 1.0,
                fused: 0.0,
                start: r.get(1)?,
                end: r.get(2)?,
                heading: r.get(3)?,
                hash: r.get(4)?,
                path: r.get(5)?,
                scope: Scope::parse(&r.get::<_, String>(14)?).unwrap_or(Scope::Notes),
                anchor: r.get(15)?,
                key: r.get(16)?,
                info: r
                    .get::<_, Option<String>>(17)?
                    .and_then(|i| serde_json::from_str(&i).ok()),
                record: RecordInfo {
                    id: r.get(7)?,
                    title: r.get(8)?,
                    date: r.get(9)?,
                    status,
                    class,
                    kind: r.get(11)?,
                    task: r.get(12)?,
                    inactive,
                    superseded_by: superseded_by.lines().map(String::from).collect(),
                },
            })
        })?;
    if c.record.inactive
        && let Some(id) = &c.record.id
    {
        // A successor may name the record in its `supersedes` only.
        for later in graph::superseded_by(conn, id)? {
            if !c
                .record
                .superseded_by
                .iter()
                .any(|s| s.eq_ignore_ascii_case(&later))
            {
                c.record.superseded_by.push(later);
            }
        }
    }
    if let Some(date) = as_of
        && c.record.inactive
        && graph::in_force_on(conn, c.file, date)?
    {
        c.record.inactive = false;
    }
    if let Some(date) = as_of {
        // A successor dated after the date did not exist then.
        let mut kept = Vec::new();
        for id in std::mem::take(&mut c.record.superseded_by) {
            let files = graph::files_by_record(conn, &id)?;
            let later = !files.is_empty()
                && files
                    .iter()
                    .all(|f| f.date.as_deref().is_some_and(|d| d > date));
            if !later {
                kept.push(id);
            }
        }
        c.record.superseded_by = kept;
    }
    c.prior = Class::parse(&c.record.class).map_or(1.0, Class::prior)
        * if c.record.inactive {
            INACTIVE_PRIOR
        } else {
            1.0
        };
    Ok(c)
}

/// The units the filter keeps, newest record first, for a query without words.
fn listed(
    conn: &Connection,
    ctx: &Context,
    opts: &Options,
    filter: &str,
) -> Result<Vec<Candidate>, Fault> {
    let units: Vec<i64> = conn
        .prepare(&format!(
            "SELECT u.id FROM units u JOIN files f ON f.id = u.file_id WHERE u.id IN ({filter}) \
             ORDER BY f.date IS NULL, f.date DESC, f.path, u.start_line"
        ))?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for unit in units {
        let c = load(conn, ctx, unit, opts.as_of)?;
        if opts.statuses.is_empty()
            || c.record
                .status
                .as_ref()
                .is_some_and(|s| opts.statuses.contains(s))
        {
            out.push(c);
        }
    }
    Ok(out)
}

/// The letters an id starts with: `DR` of `DR041`, `H` of `H158`.
fn id_prefix(id: Option<&str>) -> Option<String> {
    id.map(|id| {
        id.chars()
            .take_while(|c| c.is_alphabetic())
            .collect::<String>()
            .to_lowercase()
    })
}

/// A group of a record's units, and the groups folded under it.
type Folded = (Vec<Candidate>, Vec<Candidate>);

/// Folds each group whose best unit links the record of another group under
/// that record, which takes the better place of the two. Two records of one
/// series (`DR040` and `DR041`) and a pair in a supersession or amendment stay
/// apart; a record that holds folds is not folded itself.
fn fold(conn: &Connection, groups: Vec<Vec<Candidate>>) -> Result<Vec<Folded>, Fault> {
    let mut at: HashMap<&str, usize> = HashMap::new();
    for (i, g) in groups.iter().enumerate() {
        at.entry(g[0].path.as_str()).or_insert(i);
        if let Some(dir) = g[0].path.strip_suffix("/README.md") {
            at.entry(dir).or_insert(i);
        }
    }
    let mut into: Vec<Option<usize>> = vec![None; groups.len()];
    for (i, g) in groups.iter().enumerate() {
        let h = &g[0];
        for target in graph::links_in(conn, h.file, h.start, h.end)? {
            let Some(&j) = at.get(target.as_str()) else {
                continue;
            };
            let r = &groups[j][0];
            if j == i || r.file == h.file || r.record.id.is_none() {
                continue;
            }
            let (hid, rid) = (h.record.id.as_deref(), r.record.id.as_deref());
            if hid.is_some() && id_prefix(hid) == id_prefix(rid) {
                continue;
            }
            if graph::succession(conn, (h.file, hid), (r.file, rid))? {
                continue;
            }
            into[i] = Some(j);
            break;
        }
    }
    // A group that receives a fold keeps its place.
    let receivers: HashSet<usize> = into.iter().flatten().copied().collect();
    for (i, slot) in into.iter_mut().enumerate() {
        if receivers.contains(&i) {
            *slot = None;
        }
    }
    let mut folded: Vec<Vec<Candidate>> = (0..groups.len()).map(|_| Vec::new()).collect();
    let mut slots: Vec<Option<Vec<Candidate>>> = groups.into_iter().map(Some).collect();
    for (i, target) in into.iter().enumerate() {
        if let Some(j) = *target
            && let Some(mut g) = slots[i].take()
        {
            folded[j].push(g.swap_remove(0));
        }
    }
    let mut order = Vec::new();
    let mut done = HashSet::new();
    for (i, target) in into.iter().enumerate() {
        let owner = target.unwrap_or(i);
        if done.insert(owner)
            && let Some(g) = slots[owner].take()
        {
            order.push((g, std::mem::take(&mut folded[owner])));
        }
    }
    Ok(order)
}

/// Runs the phrasings and returns the hits to show.
pub fn search(
    conn: &Connection,
    ctx: &Context,
    phrasings: &[Phrasing],
    opts: &Options,
) -> Result<Outcome, Fault> {
    let allowed: Option<HashSet<i64>> = match opts.filter {
        Some(sql) => Some(
            conn.prepare(sql)?
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?,
        ),
        None => None,
    };
    // Unit id to the rank and the candidate of the phrasing that ranks it best.
    let mut best: HashMap<i64, (usize, Candidate)> = HashMap::new();
    let mut unknown: Vec<Unknown> = Vec::new();
    let scopes = scope_filter(conn, ctx)?;
    for (i, phrasing) in phrasings.iter().enumerate() {
        let (candidates, absent) = retrieve(
            conn,
            ctx,
            i,
            phrasing,
            opts,
            scopes.as_deref(),
            allowed.as_ref(),
        )?;
        for (rank, mut c) in candidates.into_iter().enumerate() {
            let share = 1.0 / (RRF_K + rank as f64 + 1.0);
            // The phrasing that ranks a unit best describes it; every phrasing adds its share.
            match best.get_mut(&c.unit) {
                Some((seen_rank, seen)) => {
                    c.fused = seen.fused + share;
                    c.exact_id |= seen.exact_id;
                    if rank < *seen_rank {
                        *seen_rank = rank;
                        *seen = c;
                    } else {
                        seen.fused = c.fused;
                        seen.exact_id = c.exact_id;
                    }
                }
                None => {
                    c.fused = share;
                    best.insert(c.unit, (rank, c));
                }
            }
        }
        for i in absent {
            let term = &phrasing.terms[i];
            let word = term.text();
            if unknown.iter().all(|u| u.word != word) {
                let nearest = match term.tokens.as_slice() {
                    [one] => nearest(conn, one)?,
                    _ => Vec::new(),
                };
                unknown.push(Unknown { word, nearest });
            }
        }
    }
    let ranked: Vec<Candidate> = match opts.filter {
        Some(filter) if phrasings.is_empty() => listed(conn, ctx, opts, filter)?,
        _ => {
            let mut ranked: Vec<Candidate> = best.into_values().map(|(_, c)| c).collect();
            ranked.sort_by(|a, b| {
                b.exact_id
                    .cmp(&a.exact_id)
                    .then(b.fused.total_cmp(&a.fused))
                    .then_with(|| (&a.path, a.start).cmp(&(&b.path, b.start)))
            });
            ranked
        }
    };
    let candidates = ranked
        .iter()
        .take(TRACED)
        .map(|c| (format!("{}#L{}-L{}", c.path, c.start, c.end), c.fused))
        .collect();
    // Group by record: its units follow its best one.
    let per_record = if opts.records { 1 } else { 2 };
    let mut groups: Vec<Vec<Candidate>> = Vec::new();
    let mut place: HashMap<i64, usize> = HashMap::new();
    for c in ranked {
        let i = *place.entry(c.file).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        if groups[i].len() < per_record {
            groups[i].push(c);
        }
    }
    below_successors(&mut groups);
    let groups = if opts.fold {
        fold(conn, groups)?
    } else {
        groups.into_iter().map(|g| (g, Vec::new())).collect()
    };
    let total: usize = groups.iter().map(|(g, _)| g.len()).sum();
    let no_words = Phrasing {
        terms: Vec::new(),
        raw: Vec::new(),
        seq: Vec::new(),
    };
    let mut hits = Vec::new();
    let units = groups.into_iter().flat_map(|(g, also)| {
        let mut also = Some(also);
        g.into_iter()
            .map(move |c| (c, also.take().unwrap_or_default()))
    });
    for (c, also) in units.take(opts.limit) {
        let phrasing = phrasings.get(c.phrasing).unwrap_or(&no_words);
        let held = |i: &usize| c.matched & (1 << i) != 0;
        let (matched, missing): (Vec<usize>, Vec<usize>) =
            (0..phrasing.terms.len()).partition(held);
        let texts = |ids: Vec<usize>| ids.into_iter().map(|i| phrasing.terms[i].text()).collect();
        let linked_by = graph::linked_by(conn, c.file, &c.path)?;
        hits.push(Hit {
            rank: hits.len() + 1,
            anchor: anchor(&c),
            snippet: snippet(conn, c.unit, phrasing)?,
            explain: Some(Explain {
                exact_id: c.exact_id,
                bm25: c.bm25,
                trigram: c.trigram,
                prior: c.prior,
                score: c.score(),
                fused: c.fused,
                matched: texts(matched),
            }),
            // A record asked for by id is the answer whatever words it holds.
            missing: if c.exact_id {
                Vec::new()
            } else {
                texts(missing)
            },
            path: c.path,
            start: c.start,
            end: c.end,
            heading: c.heading,
            hash: c.hash,
            linked_by,
            also_in: also
                .into_iter()
                .map(|a| AlsoIn {
                    anchor: anchor(&a),
                    id: a.record.id,
                    title: a.record.title,
                })
                .collect(),
            scope: c.scope,
            key: c.key,
            info: c.info,
            record: c.record,
        });
    }
    Ok(Outcome {
        more: total - hits.len(),
        hits,
        unknown,
        candidates,
    })
}

/// The SQL list of the scopes searched, when the index holds others too.
fn scope_filter(conn: &Connection, ctx: &Context) -> Result<Option<String>, Fault> {
    let held: Vec<String> = conn
        .prepare_cached("SELECT DISTINCT scope FROM files")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let wanted: Vec<&str> = ctx.searched.iter().map(|s| s.as_str()).collect();
    if held.iter().all(|h| wanted.contains(&h.as_str())) {
        return Ok(None);
    }
    Ok(Some(
        wanted
            .iter()
            .map(|s| format!("'{s}'"))
            .collect::<Vec<_>>()
            .join(", "),
    ))
}

/// Where a hit points (see `Hit::anchor`).
fn anchor(c: &Candidate) -> String {
    let base = c.anchor.as_deref().unwrap_or(&c.path);
    match c.scope {
        Scope::Refs if c.anchor.is_none() => c.path.clone(),
        Scope::Files => c.path.clone(),
        Scope::Board if c.start == 0 => c.path.clone(),
        Scope::Board => format!("{}#note-{}", c.path, c.start),
        Scope::Sessions => format!("{base}#L{}", c.start),
        _ => format!("{base}#L{}-L{}", c.start, c.end),
    }
}

/// The head line of a registry entry hit: key, year, first author, title,
/// depth and the local text.
pub fn ref_line(hit: &Hit) -> Option<String> {
    let info = hit.info.as_ref()?;
    if hit.scope != Scope::Refs || info.get("card").is_some() {
        return None;
    }
    let field = |k: &str| match info.get(k)? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Null => None,
        other => Some(other.to_string()),
    };
    let mut parts = vec![hit.path.clone()];
    parts.extend(field("year"));
    parts.extend(field("first_author"));
    parts.extend(hit.record.title.clone());
    parts.extend(field("depth"));
    parts.push(field("text").unwrap_or_else(|| "no local text".into()));
    Some(parts.join(" · "))
}

/// Moves each record that is no longer in force below the successor it names,
/// when both are hits and the query did not ask for it by id.
fn below_successors(groups: &mut Vec<Vec<Candidate>>) {
    // A chain of n records needs fewer than n * n moves; the bound also ends
    // records that name each other.
    for _ in 0..groups.len() * groups.len() {
        let moved = (0..groups.len()).find_map(|i| {
            let old = &groups[i][0];
            if old.exact_id || !old.record.inactive {
                return None;
            }
            let successor = groups.iter().rposition(|g| {
                g[0].record.id.as_ref().is_some_and(|id| {
                    old.record
                        .superseded_by
                        .iter()
                        .any(|s| s.eq_ignore_ascii_case(id))
                })
            })?;
            (successor > i).then_some((i, successor))
        });
        let Some((from, to)) = moved else { return };
        let group = groups.remove(from);
        groups.insert(to, group);
    }
}

fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clip(text: &str, chars: usize) -> String {
    match text.char_indices().nth(chars) {
        Some((end, _)) => format!("{}…", text[..end].trim_end()),
        None => text.to_string(),
    }
}

/// One passage of the unit's body around the query's words.
fn snippet(conn: &Connection, unit: i64, phrasing: &Phrasing) -> Result<String, Fault> {
    if phrasing.terms.is_empty() {
        let body: String =
            conn.query_row("SELECT body FROM units_fts WHERE rowid = ?1", [unit], |r| {
                r.get(0)
            })?;
        return Ok(clip(&flat(&body), SNIPPET_CHARS));
    }
    let any = phrasing
        .terms
        .iter()
        .map(Term::fts)
        .collect::<Vec<_>>()
        .join(" OR ");
    let by_words: Option<String> = conn
        .query_row(
            &format!(
                "SELECT snippet(units_fts, {BODY_COLUMN}, '', '', '…', 24) FROM units_fts \
                 WHERE units_fts MATCH ?1 AND rowid = ?2"
            ),
            params![any, unit],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(text) = by_words {
        return Ok(clip(&flat(&text), SNIPPET_CHARS));
    }
    // A unit that the trigrams or the id found: the text around the first term.
    let body: String =
        conn.query_row("SELECT body FROM units_fts WHERE rowid = ?1", [unit], |r| {
            r.get(0)
        })?;
    let body = flat(&body);
    let lower = body.to_lowercase();
    let at = phrasing
        .terms
        .iter()
        .filter_map(|t| lower.find(&t.text()))
        .min()
        // Offsets carry over only when lowercasing kept every length.
        .filter(|_| lower.len() == body.len())
        .unwrap_or(0);
    let mut from = at.saturating_sub(40);
    while !body.is_char_boundary(from) {
        from -= 1;
    }
    let lead = if from > 0 { "…" } else { "" };
    Ok(format!("{lead}{}", clip(&body[from..], SNIPPET_CHARS)))
}

/// The edit distance from `a` to each prefix of `b`, the empty one first.
fn prefix_distances(a: &[char], b: &[char]) -> Vec<usize> {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != cb))
                .min(above + 1)
                .min(row[j] + 1);
            diagonal = above;
        }
    }
    row
}

/// A word stem lacks at most this many of its word's last characters.
const STEM_CUT: usize = 3;

/// How far the indexed `stem` is from `word`: the edit distance to the word
/// or to the word without an ending, whichever is less.
fn stem_distance(stem: &[char], word: &[char]) -> usize {
    let row = prefix_distances(stem, word);
    // A short word has a short ending: at most a quarter of it.
    let shortest = word.len() - STEM_CUT.min(word.len() / 4);
    row[shortest..].iter().copied().min().unwrap_or(usize::MAX)
}

/// Up to three indexed words near `word`: those it starts, then those within
/// a small edit distance, the more frequent first.
fn nearest(conn: &Connection, word: &str) -> Result<Vec<String>, Fault> {
    let chars: Vec<char> = word.chars().collect();
    if chars.len() < 3 {
        return Ok(Vec::new());
    }
    let reach = if chars.len() <= 4 { 1 } else { 2 };
    let mut near: Vec<(usize, i64, String)> = Vec::new();
    let mut stmt = conn.prepare("SELECT term, doc FROM units_vocab")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let term = row.get_ref(0)?.as_str().map_err(rusqlite::Error::from)?;
        let distance = if term.starts_with(word) {
            0
        } else if term.len().abs_diff(word.len()) <= reach * 4 {
            stem_distance(&term.chars().collect::<Vec<_>>(), &chars)
        } else {
            continue;
        };
        if distance <= reach {
            near.push((distance, row.get(1)?, term.to_string()));
        }
    }
    near.sort_by(|a, b| (a.0, -a.1, &a.2).cmp(&(b.0, -b.1, &b.2)));
    near.truncate(3);
    // The index holds word stems; show a word as the text writes it.
    let mut words = Vec::new();
    for (_, _, stem) in near {
        let marked: Option<String> = conn
            .query_row(
                "SELECT snippet(units_fts, -1, char(1), char(2), '', 1) FROM units_fts \
                 WHERE units_fts MATCH ?1 LIMIT 1",
                [format!("\"{stem}\"")],
                |r| r.get(0),
            )
            .optional()?;
        let written = marked.as_deref().and_then(|m| {
            let (_, rest) = m.split_once('\u{1}')?;
            let (word, _) = rest.split_once('\u{2}')?;
            Some(word.to_lowercase())
        });
        let word = written.unwrap_or(stem);
        if !words.contains(&word) {
            words.push(word);
        }
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(words: &[&str]) -> Term {
        Term {
            tokens: words.iter().map(|w| w.to_string()).collect(),
            alts: Vec::new(),
        }
    }

    #[test]
    fn parses_words_phrases_and_punctuation() {
        let p = parse("Haiku/Opus \"fiscal year\" DR041 haiku").unwrap();
        assert_eq!(
            p.terms,
            [
                term(&["haiku"]),
                term(&["opus"]),
                term(&["fiscal", "year"]),
                term(&["dr041"]),
            ]
        );
        assert_eq!(p.raw, ["Haiku/Opus", "fiscal", "year", "DR041", "haiku"]);
        assert_eq!(p.terms[2].fts(), "\"fiscal year\"");
        // An unclosed quote runs to the end.
        assert_eq!(parse("\"a b").unwrap().terms, [term(&["a", "b"])]);
    }

    #[test]
    fn a_decimal_number_is_one_term_without_a_zero_fraction() {
        assert_eq!(parse("47.0%").unwrap().terms, [term(&["47"])]);
        assert_eq!(parse("47.00").unwrap().terms, [term(&["47"])]);
        assert_eq!(parse("(0.50)").unwrap().terms, [term(&["0", "50"])]);
        assert_eq!(parse("47").unwrap().terms, [term(&["47"])]);
        assert_eq!(parse("v1.2").unwrap().terms, [term(&["v1"]), term(&["2"])]);
    }

    #[test]
    fn refuses_a_query_without_a_word_or_with_too_many() {
        assert!(parse(" -- \"\" ").unwrap_err().starts_with("bad query"));
        let many: Vec<String> = (0..=MAX_TERMS).map(|i| format!("w{i}")).collect();
        assert!(parse(&many.join(" ")).unwrap_err().contains("at most"));
    }

    #[test]
    fn measures_the_distance_from_a_stem_to_a_word_with_or_without_its_ending() {
        let d = |stem: &str, word: &str| {
            stem_distance(
                &stem.chars().collect::<Vec<_>>(),
                &word.chars().collect::<Vec<_>>(),
            )
        };
        assert_eq!(d("fiscal", "fiskal"), 1);
        assert_eq!(d("calendar", "calender"), 1);
        assert_eq!(d("sitting", "kitten"), 3);
        // `retriev` is the stem of `retriever`.
        assert_eq!(d("retriev", "retriever"), 0);
        assert_eq!(d("retriev", "retreiver"), 2);
        // A short word loses at most one character.
        assert_eq!(d("call", "calend"), 2);
        assert_eq!(d("ca", "car"), 1);
    }

    #[test]
    fn clips_on_a_character_boundary() {
        assert_eq!(clip("日本語のテキスト", 3), "日本語…");
        assert_eq!(clip("short", 10), "short");
    }
}
