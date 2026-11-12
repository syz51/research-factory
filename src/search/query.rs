//! A query against the index (docs/state-machines.md 3.3): parse, route an
//! exact record id, retrieve by words and by trigrams, apply the status and
//! class priors, fuse the phrasings and group the units by record.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use super::index::{BODY_COLUMN, Fault, WEIGHTS};
use super::{Class, Context};

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
}

impl Term {
    fn text(&self) -> String {
        self.tokens.join(" ")
    }

    /// As an FTS5 string: a word, or a phrase of adjacent words.
    fn fts(&self) -> String {
        format!("\"{}\"", self.text())
    }
}

/// One phrasing of the question.
#[derive(Debug, PartialEq)]
pub struct Phrasing {
    terms: Vec<Term>,
    /// The words as typed, for the exact id lookup.
    raw: Vec<String>,
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Parses one phrasing. Punctuation splits words; `"…"` makes a phrase.
pub fn parse(text: &str) -> Result<Phrasing, String> {
    let mut terms: Vec<Term> = Vec::new();
    let mut add = |tokens: Vec<String>| {
        let term = Term { tokens };
        if !term.tokens.is_empty() && !terms.contains(&term) {
            terms.push(term);
        }
    };
    // Odd pieces of a split at `"` are inside quotes; an unclosed quote runs to the end.
    for (i, piece) in text.split('"').enumerate() {
        if i % 2 == 1 {
            add(words(piece));
        } else {
            for word in words(piece) {
                add(vec![word]);
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
    Ok(Phrasing { terms, raw })
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
    /// `path#Lstart-Lend`: 1-based inclusive lines of the source file.
    pub anchor: String,
    pub path: String,
    pub start: i64,
    pub end: i64,
    pub heading: String,
    pub snippet: String,
    /// The content hash of the unit's indexed text.
    pub hash: String,
    pub record: RecordInfo,
    /// The query terms the unit lacks.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain: Option<Explain>,
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
}

pub struct Options<'a> {
    pub limit: usize,
    /// One hit per record instead of two.
    pub records: bool,
    /// Keep only records with one of these statuses; empty keeps all.
    pub statuses: &'a [String],
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
    statuses: &[String],
) -> Result<(Vec<Candidate>, Vec<usize>), Fault> {
    // Unit id to its BM25 score, trigram score and matched terms.
    let mut found: HashMap<i64, (f64, f64, u64)> = HashMap::new();
    let mut known = Vec::new();
    let mut unknown = Vec::new();
    for (i, term) in phrasing.terms.iter().enumerate() {
        let hit = conn
            .query_row(
                "SELECT 1 FROM units_fts WHERE units_fts MATCH ?1 LIMIT 1",
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
    // The status filter is part of each retrieval, so that it applies before
    // the cut to the best candidates.
    let with_status = |column: &str| {
        if statuses.is_empty() {
            return String::new();
        }
        let list: Vec<String> = statuses
            .iter()
            .map(|s| format!("'{}'", s.replace('\'', "''")))
            .collect();
        format!(
            " AND {column} IN (SELECT u.id FROM units u JOIN files f ON f.id = u.file_id \
             WHERE f.status IN ({}))",
            list.join(", ")
        )
    };
    // Words: the units with every known term, then those with any.
    let ranked = format!(
        "SELECT rowid, rank FROM units_fts WHERE units_fts MATCH ?1 \
         AND rank MATCH 'bm25({WEIGHTS})'{} ORDER BY rank LIMIT {CANDIDATES}",
        with_status("rowid")
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
            with_status("doc")
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
    for word in &phrasing.raw {
        let mut stmt = conn.prepare_cached(
            "SELECT f.id, (SELECT min(u.id) FROM units u WHERE u.file_id = f.id) \
             FROM files f WHERE f.record_id = ?1 COLLATE NOCASE",
        )?;
        // `D002,` and `H158?` name the ids D002 and H158.
        let id = word.trim_matches(|c: char| !c.is_alphanumeric());
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
    let mut load = conn.prepare_cached(
        "SELECT u.file_id, u.start_line, u.end_line, u.heading, u.hash, f.path, f.class, \
         f.record_id, f.title, f.date, f.status, f.kind, f.task, f.superseded_by \
         FROM units u JOIN files f ON f.id = u.file_id WHERE u.id = ?1",
    )?;
    let mut candidates = Vec::new();
    for (unit, (bm25, trigram, matched)) in found {
        let c = load.query_row([unit], |r| {
            let class: String = r.get(6)?;
            let status: Option<String> = r.get(10)?;
            let superseded_by: String = r.get(13)?;
            let inactive = status
                .as_ref()
                .is_some_and(|s| ctx.inactive_statuses.contains(s));
            let prior = Class::parse(&class).map_or(1.0, Class::prior)
                * if inactive { INACTIVE_PRIOR } else { 1.0 };
            let file = r.get(0)?;
            Ok(Candidate {
                unit,
                file,
                phrasing: index,
                exact_id: exact_files.contains(&file),
                bm25,
                trigram,
                matched,
                prior,
                fused: 0.0,
                start: r.get(1)?,
                end: r.get(2)?,
                heading: r.get(3)?,
                hash: r.get(4)?,
                path: r.get(5)?,
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

/// Runs the phrasings and returns the hits to show.
pub fn search(
    conn: &Connection,
    ctx: &Context,
    phrasings: &[Phrasing],
    opts: &Options,
) -> Result<Outcome, Fault> {
    // Unit id to the rank and the candidate of the phrasing that ranks it best.
    let mut best: HashMap<i64, (usize, Candidate)> = HashMap::new();
    let mut unknown: Vec<Unknown> = Vec::new();
    for (i, phrasing) in phrasings.iter().enumerate() {
        let (candidates, absent) = retrieve(conn, ctx, i, phrasing, opts.statuses)?;
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
    let mut ranked: Vec<Candidate> = best.into_values().map(|(_, c)| c).collect();
    ranked.sort_by(|a, b| {
        b.exact_id
            .cmp(&a.exact_id)
            .then(b.fused.total_cmp(&a.fused))
            .then_with(|| (&a.path, a.start).cmp(&(&b.path, b.start)))
    });
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
    let total: usize = groups.iter().map(Vec::len).sum();
    let mut hits = Vec::new();
    for c in groups.into_iter().flatten().take(opts.limit) {
        let phrasing = &phrasings[c.phrasing];
        let held = |i: &usize| c.matched & (1 << i) != 0;
        let (matched, missing): (Vec<usize>, Vec<usize>) =
            (0..phrasing.terms.len()).partition(held);
        let texts = |ids: Vec<usize>| ids.into_iter().map(|i| phrasing.terms[i].text()).collect();
        hits.push(Hit {
            rank: hits.len() + 1,
            anchor: format!("{}#L{}-L{}", c.path, c.start, c.end),
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
            record: c.record,
        });
    }
    Ok(Outcome {
        more: total - hits.len(),
        hits,
        unknown,
    })
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
const STEM_CUT: usize = 5;

/// How far the indexed `stem` is from `word`: the edit distance to the word
/// or to the word without an ending, whichever is less.
fn stem_distance(stem: &[char], word: &[char]) -> usize {
    let row = prefix_distances(stem, word);
    let shortest = word.len().saturating_sub(STEM_CUT).max(4).min(word.len());
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
            1
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
        // A short word keeps all of its characters.
        assert_eq!(d("ca", "cart"), 2);
    }

    #[test]
    fn clips_on_a_character_boundary() {
        assert_eq!(clip("日本語のテキスト", 3), "日本語…");
        assert_eq!(clip("short", 10), "short");
    }
}
