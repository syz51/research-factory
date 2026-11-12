//! Scopes `refs` and `texts`: the reference registry (`rf.reference/1`), the
//! extracted texts it lists, their structure files (`rf.paper-structure/1`)
//! and paper cards (`rf.paper-card/1`).
//!
//! A structure file or card records the sha256 of the text it was made from.
//! When the text has changed since, the derivative is stale: it is ignored
//! and reported (docs/state-machines.md 4.1).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use super::passages::{Span, windows};
use super::sources::{Doc, Listing, Loaded, Resolver, Row, What, file_stamp, sha256};
use super::units::Unit;
use super::{Class, Context, Scope};

/// The file beside a text that gives its sections.
pub const STRUCTURE_SUFFIX: &str = ".structure.json";

/// One text of an entry.
#[derive(Debug, Clone)]
struct TextRef {
    path: String,
    /// `None` or `plain` for plain PDF text; any other value names a
    /// structured extraction, which is preferred.
    extraction: Option<String>,
}

/// One registry entry, as far as search needs it.
#[derive(Debug, Clone)]
pub struct Entry {
    key: String,
    title: String,
    authors: Vec<String>,
    year: Option<String>,
    venue: Option<String>,
    depth: Option<String>,
    summary: Option<String>,
    arxiv: Option<String>,
    doi: Option<String>,
    texts: Vec<TextRef>,
    duplicate_of: Option<String>,
    /// The catalogue line: a path relative to the project root and a 1-based line.
    source: Option<(String, usize)>,
    /// The JSON line, for the stamp.
    raw: String,
    /// Short names from the names of its text files; see `short_names`.
    short: Vec<String>,
}

/// A string, or a number written as one.
fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

impl Entry {
    fn parse(line: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(line).ok()?;
        let str_at = |ptr: &str| v.pointer(ptr).and_then(text_of);
        let strings = |key: &str| -> Vec<String> {
            v.get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(text_of)
                .collect()
        };
        Some(Self {
            key: str_at("/key")?,
            title: str_at("/title").unwrap_or_default(),
            authors: strings("authors"),
            year: str_at("/year"),
            venue: str_at("/venue"),
            depth: str_at("/depth"),
            summary: str_at("/abstract"),
            arxiv: str_at("/ids/arxiv"),
            doi: str_at("/ids/doi"),
            texts: v
                .get("texts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|t| {
                    Some(TextRef {
                        path: t.get("path").and_then(text_of)?,
                        extraction: t.get("extraction").and_then(text_of),
                    })
                })
                .collect(),
            duplicate_of: str_at("/duplicate_of"),
            source: str_at("/source/path")
                .zip(v.pointer("/source/line").and_then(Value::as_u64))
                .map(|(p, l)| (p, l as usize)),
            raw: line.to_string(),
            short: Vec::new(),
        })
    }

    /// The names an exact lookup finds the entry by.
    fn aliases(&self) -> Vec<String> {
        let mut out = vec![format!("ref:{}", self.key)];
        out.extend(self.arxiv.as_deref().map(normal_arxiv));
        out.extend(self.doi.as_deref().map(normal_doi));
        for s in &self.short {
            if !out.contains(s) {
                out.push(s.clone());
            }
        }
        out
    }
}

/// Generic names of a text file, which name no paper.
const GENERIC_STEMS: &[&str] = &[
    "article", "content", "document", "full", "fulltext", "index", "main", "paper", "pdf",
    "source", "text",
];

/// A text file named by a word, such as `hirec.txt`, carries the project's
/// short name of the paper. A name that holds no letter, the key, a generic
/// word, or that another entry's text shares names no paper; nor does a name
/// in a directory named by the key, such as `texts/12/paper.txt`.
fn short_names(entries: &mut [Entry]) {
    let stem = |path: &str| {
        Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_lowercase)
    };
    let mut owners: HashMap<String, Vec<&str>> = HashMap::new();
    for e in entries.iter() {
        for t in &e.texts {
            if let Some(s) = stem(&t.path) {
                let keys = owners.entry(s).or_default();
                if !keys.contains(&e.key.as_str()) {
                    keys.push(&e.key);
                }
            }
        }
    }
    let shared: Vec<String> = owners
        .into_iter()
        .filter(|(_, keys)| keys.len() > 1)
        .map(|(s, _)| s)
        .collect();
    for e in entries.iter_mut() {
        let key = e.key.to_lowercase();
        let mut short = Vec::new();
        for t in &e.texts {
            let in_key_dir = Path::new(&t.path).parent().is_some_and(|d| {
                d.iter()
                    .any(|c| c.to_str().map(str::to_lowercase) == Some(key.clone()))
            });
            let Some(s) = stem(&t.path) else { continue };
            if s.chars().any(char::is_alphabetic)
                && s != key
                && !in_key_dir
                && !GENERIC_STEMS.contains(&s.as_str())
                && !shared.contains(&s)
                && !short.contains(&s)
            {
                short.push(s);
            }
        }
        e.short = short;
    }
}

/// An arXiv id without its `arXiv:` prefix and version, lowercased.
pub fn normal_arxiv(id: &str) -> String {
    let id = id.trim().to_lowercase();
    let id = id.strip_prefix("arxiv:").unwrap_or(&id).to_string();
    match id.rsplit_once('v') {
        Some((head, v)) if !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()) => {
            head.to_string()
        }
        _ => id,
    }
}

/// A DOI without its resolver prefix, lowercased.
pub fn normal_doi(doi: &str) -> String {
    let doi = doi.trim().to_lowercase();
    ["https://doi.org/", "http://doi.org/", "doi:"]
        .iter()
        .find_map(|p| doi.strip_prefix(p))
        .unwrap_or(&doi)
        .to_string()
}

/// The registry: its entries, and each entry's place by key.
struct Registry {
    entries: Vec<Entry>,
    by_key: HashMap<String, usize>,
    /// Lines that are not entries.
    bad: usize,
}

impl Registry {
    /// `None` when the project has no registry file.
    fn read(ctx: &Context, resolver: &mut Resolver) -> Result<Option<Self>, String> {
        resolver.prepare([ctx.registry.as_str()])?;
        let Some(path) = resolver.get(&ctx.registry) else {
            return Ok(None);
        };
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut entries = Vec::new();
        let mut bad = 0;
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            match Entry::parse(line) {
                Some(e) => entries.push(e),
                None => bad += 1,
            }
        }
        short_names(&mut entries);
        let by_key = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.key.clone(), i))
            .collect();
        Ok(Some(Self {
            entries,
            by_key,
            bad,
        }))
    }

    /// The entry that `key` resolves to through `duplicate_of`: the last one
    /// of the chain that exists. A chain that loops resolves to `key`.
    fn root(&self, key: &str) -> usize {
        let Some(&start) = self.by_key.get(key) else {
            return usize::MAX;
        };
        let mut at = start;
        for _ in 0..self.entries.len() {
            match self.entries[at]
                .duplicate_of
                .as_ref()
                .and_then(|d| self.by_key.get(d))
            {
                Some(&next) if next == start => return start,
                Some(&next) => at = next,
                None => return at,
            }
        }
        start
    }

    /// Each entry that is no duplicate, with the duplicates that resolve to it.
    fn groups(&self) -> Vec<(usize, Vec<usize>)> {
        let mut dups: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut roots = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            let root = self.root(&e.key);
            if root == i {
                roots.push(i);
            } else {
                dups.entry(root).or_default().push(i);
            }
        }
        roots
            .into_iter()
            .map(|r| (r, dups.remove(&r).unwrap_or_default()))
            .collect()
    }

    fn notices(&self, ctx: &Context) -> Vec<String> {
        (self.bad > 0)
            .then(|| {
                format!(
                    "{}: {} line(s) are not registry entries and are left out",
                    ctx.registry, self.bad
                )
            })
            .into_iter()
            .collect()
    }
}

/// The catalogue lines that the entries name, read once per file.
fn catalogue(
    entries: &[&Entry],
    resolver: &mut Resolver,
) -> Result<HashMap<(String, usize), String>, String> {
    let mut paths: Vec<&str> = entries
        .iter()
        .filter_map(|e| e.source.as_ref().map(|s| s.0.as_str()))
        .collect();
    paths.sort_unstable();
    paths.dedup();
    resolver.prepare(paths.iter().copied())?;
    let mut lines = HashMap::new();
    for path in paths {
        let Some(file) = resolver.get(path) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(&file) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let all: Vec<&str> = text.lines().collect();
        for e in entries {
            if let Some((p, n)) = &e.source
                && p == path
                && let Some(line) = n.checked_sub(1).and_then(|i| all.get(i))
            {
                lines.insert((p.clone(), *n), line.trim().to_string());
            }
        }
    }
    Ok(lines)
}

/// The text to index of an entry and its duplicates: the last one with a
/// structured extraction that exists, else the first plain one that exists.
fn preferred<'e>(
    texts: impl Iterator<Item = &'e TextRef>,
    resolver: &Resolver,
) -> Option<(&'e TextRef, PathBuf)> {
    let all: Vec<&TextRef> = texts.collect();
    let structured = |t: &TextRef| t.extraction.as_deref().is_some_and(|x| x != "plain");
    let first = all.iter().rev().filter(|t| structured(t));
    let then = all.iter().filter(|t| !structured(t));
    first
        .chain(then)
        .find_map(|t| resolver.get(&t.path).map(|p| (*t, p)))
}

/// A `refs` source: an entry and its duplicates.
pub struct RefDoc {
    root: Entry,
    dups: Vec<Entry>,
    /// The catalogue line of each entry, the root's first.
    lines: Vec<Option<String>>,
    /// The text the entry's passages come from, when one is here.
    text: Option<String>,
}

pub fn list_refs(ctx: &Context, resolver: &mut Resolver) -> Result<Listing, String> {
    let Some(reg) = Registry::read(ctx, resolver)? else {
        // Cards are listed under their own key, and their notices kept.
        let (cards, notices) = cards(ctx, None, resolver)?;
        return Ok(Listing::complete(cards).with_notices(notices));
    };
    let all: Vec<&Entry> = reg.entries.iter().collect();
    let lines = catalogue(&all, resolver)?;
    resolver.prepare(
        reg.entries
            .iter()
            .flat_map(|e| e.texts.iter().map(|t| t.path.as_str())),
    )?;
    let mut docs = Vec::new();
    for (root, dups) in reg.groups() {
        let members: Vec<&Entry> = std::iter::once(root)
            .chain(dups)
            .map(|i| &reg.entries[i])
            .collect();
        let text =
            preferred(members.iter().flat_map(|e| &e.texts), resolver).map(|(t, _)| t.path.clone());
        let entry_lines: Vec<Option<String>> = members
            .iter()
            .map(|e| e.source.as_ref().and_then(|s| lines.get(s)).cloned())
            .collect();
        let mut stamp = String::new();
        for (e, line) in members.iter().zip(&entry_lines) {
            stamp.push_str(&e.raw);
            stamp.push('\n');
            stamp.push_str(line.as_deref().unwrap_or(""));
            stamp.push('\n');
        }
        stamp.push_str(text.as_deref().unwrap_or(""));
        docs.push(Doc {
            scope: Scope::Refs,
            key: format!("ref:{}", members[0].key),
            stamp: sha256(stamp.as_bytes()),
            what: What::Ref(Box::new(RefDoc {
                root: members[0].clone(),
                dups: members[1..].iter().map(|e| (*e).clone()).collect(),
                lines: entry_lines,
                text,
            })),
        });
    }
    let (cards, notices) = cards(ctx, Some(&reg), resolver)?;
    docs.extend(cards);
    Ok(Listing::complete(docs).with_notices([reg.notices(ctx), notices].concat()))
}

impl Listing {
    fn with_notices(mut self, notices: Vec<String>) -> Self {
        self.notices.extend(notices);
        self
    }
}

pub fn load_ref(r: &RefDoc) -> Loaded {
    let e = &r.root;
    let mut body = e.summary.clone().unwrap_or_default();
    if let Some(Some(line)) = r.lines.first() {
        body.push('\n');
        body.push_str(line);
    }
    let line_no = |e: &Entry| e.source.as_ref().map_or(1, |s| s.1);
    let mut units = vec![Unit {
        start: line_no(e),
        end: line_no(e),
        heading: String::new(),
        body: body.trim().to_string(),
    }];
    for (d, line) in r.dups.iter().zip(r.lines.iter().skip(1)) {
        units.push(Unit {
            start: line_no(d),
            end: line_no(d),
            heading: format!("duplicate ref:{}", d.key),
            body: [
                d.title.clone(),
                d.authors.join(", "),
                line.clone().unwrap_or_default(),
            ]
            .join("\n")
            .trim()
            .to_string(),
        });
    }
    let aliases: Vec<String> = std::iter::once(e)
        .chain(&r.dups)
        .flat_map(Entry::aliases)
        .collect();
    let summary = [
        e.authors.join(", "),
        e.year.clone().unwrap_or_default(),
        e.venue.clone().unwrap_or_default(),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    Loaded::stored(
        Row {
            class: Some(Class::Paper),
            record_id: Some(format!("ref:{}", e.key)),
            title: Some(e.title.clone()),
            summary: Some(summary),
            date: e.year.clone(),
            kind: e.depth.clone(),
            ref_key: Some(e.key.clone()),
            info: Some(json!({
                "key": e.key,
                "year": e.year,
                "first_author": e.authors.first(),
                "depth": e.depth,
                "venue": e.venue,
                "text": r.text,
                "duplicates": r.dups.iter().map(|d| &d.key).collect::<Vec<_>>(),
            })),
            ids: aliases.join(" ; "),
            aliases,
            ..Row::default()
        },
        units,
        Vec::new(),
    )
}

/// `rf.paper-structure/1`, as far as search needs it.
#[derive(Deserialize)]
struct Structure {
    schema: String,
    text: StructureText,
    sections: Vec<Section>,
}

#[derive(Deserialize)]
struct StructureText {
    sha256: String,
    /// The text's line count, as `str::lines` counts them.
    #[serde(default)]
    lines: Option<usize>,
}

#[derive(Deserialize)]
struct Section {
    kind: String,
    heading: Option<String>,
    first_line: usize,
    last_line: usize,
}

/// The sections of a structure file, by whether it was made from the text
/// that has sha256 `text_sha` now.
enum Sections {
    Current(Vec<Section>),
    /// Made from an earlier text: why, and the structure as it was.
    Stale(String, Structure),
}

/// The sections of a structure file, or why it is unreadable.
fn structure(file: &std::path::Path, text_sha: &str) -> Result<Sections, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("unreadable: {e}"))?;
    let s: Structure =
        serde_json::from_slice(&bytes).map_err(|e| format!("not a structure file: {e}"))?;
    if !crate::finding::version_1(&s.schema, "rf.paper-structure/") {
        return Err(format!("schema `{}` is not rf.paper-structure/1", s.schema));
    }
    if !s.text.sha256.eq_ignore_ascii_case(text_sha) {
        return Ok(Sections::Stale(stale_why(&s.text.sha256, text_sha), s));
    }
    Ok(Sections::Current(s.sections))
}

/// The sections of a structure file made from an earlier text, found again
/// in `text`, so that an edit moves the passages of the other sections and
/// leaves their text as it was. A section starts at the line that equals its
/// heading, up to spaces, nearest to where the edits so far moved it; one
/// whose heading is gone moves with the section before it. A section that
/// ran to the next one still does, and the last one ends with the text.
fn relocate(old: Structure, text: &str) -> Vec<Section> {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let lines: Vec<&str> = text.lines().collect();
    // The lines of the text that equal an old heading.
    let mut at: HashMap<String, Vec<usize>> = old
        .sections
        .iter()
        .filter_map(|s| s.heading.as_deref().map(norm))
        .filter(|h| !h.is_empty())
        .map(|h| (h, Vec::new()))
        .collect();
    for (i, line) in lines.iter().enumerate() {
        if let Some(found) = at.get_mut(&norm(line)) {
            found.push(i + 1);
        }
    }
    // How far the edits so far moved the text, and the first line the next
    // section may start on.
    let mut shift = 0isize;
    let mut floor = 1;
    let mut firsts = Vec::with_capacity(old.sections.len());
    for s in &old.sections {
        let expected = s.first_line as isize + shift;
        let found = s
            .heading
            .as_deref()
            .and_then(|h| at.get(&norm(h)))
            .and_then(|at| {
                at.iter()
                    .copied()
                    .filter(|&n| n >= floor)
                    .min_by_key(|&n| (n as isize - expected).abs())
            });
        let first = match found {
            Some(n) => n,
            // Past the end of a shorter text, the section is empty.
            None => expected.min(lines.len() as isize + 1).max(floor as isize) as usize,
        };
        shift = first as isize - s.first_line as isize;
        firsts.push(first);
        floor = first + 1;
    }
    let old_end = old
        .text
        .lines
        .or_else(|| old.sections.last().map(|s| s.last_line))
        .unwrap_or(0);
    let mut out = Vec::with_capacity(old.sections.len());
    let mut sections = old.sections.into_iter().zip(&firsts).peekable();
    while let Some((s, &first)) = sections.next() {
        let moved = (s.last_line as isize + first as isize - s.first_line as isize).max(0) as usize;
        let last = match sections.peek() {
            Some((next, next_first)) if s.last_line + 1 >= next.first_line => *next_first - 1,
            Some((_, next_first)) => moved.min(*next_first - 1),
            None if s.last_line >= old_end => lines.len(),
            None => moved.min(lines.len()),
        };
        out.push(Section {
            first_line: first,
            last_line: last,
            ..s
        });
    }
    out
}

fn stale_why(recorded: &str, actual: &str) -> String {
    let short = |s: &str| s.chars().take(12).collect::<String>();
    format!(
        "made from text sha256 {}…, but the text now has {}…; it is ignored until it is made again",
        short(recorded),
        short(actual)
    )
}

/// Every path that an entry of the registry names as a text, preferred or
/// not, in the form of a walked path (`./a//b.md` is `a/b.md`). The `notes`
/// scope leaves these files out: `texts` holds one text of each paper, and
/// another extraction of it as a note would bring the duplicate back.
pub fn text_paths(ctx: &Context, resolver: &mut Resolver) -> Result<HashSet<String>, String> {
    Ok(Registry::read(ctx, resolver)?
        .map(|reg| {
            reg.entries
                .iter()
                .flat_map(|e| &e.texts)
                .map(|t| {
                    Path::new(&t.path)
                        .components()
                        .filter(|c| !matches!(c, std::path::Component::CurDir))
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/")
                })
                .collect()
        })
        .unwrap_or_default())
}

/// A `texts` source: the preferred text of an entry.
pub struct TextDoc {
    key: String,
    title: String,
    year: Option<String>,
    path: String,
    extraction: Option<String>,
    file: PathBuf,
    sidecar: Option<(String, PathBuf)>,
}

pub fn list_texts(ctx: &Context, resolver: &mut Resolver) -> Result<Listing, String> {
    let Some(reg) = Registry::read(ctx, resolver)? else {
        return Ok(Listing::complete(Vec::new()));
    };
    let sidecars: Vec<String> = reg
        .entries
        .iter()
        .flat_map(|e| &e.texts)
        .map(|t| format!("{}{STRUCTURE_SUFFIX}", t.path))
        .collect();
    resolver.prepare(
        reg.entries
            .iter()
            .flat_map(|e| e.texts.iter().map(|t| t.path.as_str()))
            .chain(sidecars.iter().map(String::as_str)),
    )?;
    let mut docs = Vec::new();
    // Entries that are not marked duplicates may still share one text: the
    // first of them owns it.
    let mut seen = std::collections::HashSet::new();
    for (root, dups) in reg.groups() {
        let e = &reg.entries[root];
        let texts = std::iter::once(root)
            .chain(dups)
            .flat_map(|i| &reg.entries[i].texts);
        let Some((t, file)) = preferred(texts, resolver) else {
            continue;
        };
        if !seen.insert(t.path.clone()) {
            continue;
        }
        let side_rel = format!("{}{STRUCTURE_SUFFIX}", t.path);
        let sidecar = resolver.get(&side_rel).map(|p| (side_rel, p));
        let Some(text_stamp) = file_stamp(&file) else {
            continue;
        };
        let stamp = format!(
            "{text_stamp}|{}|{}|{}|{:?}",
            sidecar
                .as_ref()
                .and_then(|(_, p)| file_stamp(p))
                .unwrap_or_default(),
            e.key,
            e.title,
            e.year
        );
        docs.push(Doc {
            scope: Scope::Texts,
            key: t.path.clone(),
            stamp,
            what: What::Text(Box::new(TextDoc {
                key: e.key.clone(),
                title: e.title.clone(),
                year: e.year.clone(),
                path: t.path.clone(),
                extraction: t.extraction.clone(),
                file,
                sidecar,
            })),
        });
    }
    Ok(Listing::complete(docs).with_notices(reg.notices(ctx)))
}

/// The passages of a text: within the sections of its structure file
/// without the reference list, or else plain windows of the whole text.
pub fn load_text(t: &TextDoc) -> Result<Loaded, String> {
    let bytes = std::fs::read(&t.file).map_err(|e| format!("{}: {e}", t.file.display()))?;
    let sha = sha256(&bytes);
    let text = String::from_utf8_lossy(&bytes).replace('\0', " ");
    let mut stale = Vec::new();
    let mut used = "none";
    let mut sections = None;
    if let Some((rel, file)) = &t.sidecar {
        match structure(file, &sha) {
            Ok(Sections::Current(current)) => {
                used = "used";
                sections = Some(current);
            }
            // A stale structure file is still the best guess at the sections.
            Ok(Sections::Stale(why, old)) => {
                used = "stale";
                stale.push((rel.clone(), why));
                sections = Some(relocate(old, &text));
            }
            Err(why) => {
                used = "stale";
                stale.push((rel.clone(), why));
            }
        }
    }
    let spans = sections.map(|sections| {
        sections
            .into_iter()
            .filter(|s| s.kind != "references")
            .map(|s| Span {
                first: s.first_line,
                last: s.last_line,
                heading: s.heading.filter(|h| !h.trim().is_empty()).unwrap_or(s.kind),
            })
            .collect::<Vec<_>>()
    });
    let spans = spans.unwrap_or_else(|| {
        vec![Span {
            first: 1,
            last: text.split('\n').count(),
            heading: String::new(),
        }]
    });
    let units = windows(&text, &spans);
    Ok(Loaded::stored(
        Row {
            class: Some(Class::Paper),
            anchor: Some(t.path.clone()),
            title: Some(t.title.clone()),
            summary: t.year.clone(),
            date: t.year.clone(),
            kind: t.extraction.clone(),
            ref_key: Some(t.key.clone()),
            info: Some(json!({
                "key": t.key,
                "year": t.year,
                "extraction": t.extraction,
                "structure": used,
            })),
            ..Row::default()
        },
        units,
        stale,
    ))
}

/// `rf.paper-card/1`, as far as search needs it.
#[derive(Deserialize)]
struct Card {
    schema: String,
    key: Value,
    text: CardText,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    items: Vec<CardItem>,
}

#[derive(Deserialize)]
struct CardText {
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
struct CardItem {
    field: String,
    value: Value,
    #[serde(default)]
    quote: Option<String>,
    #[serde(default)]
    lines: Option<(usize, usize)>,
}

/// A card of the `refs` scope.
pub struct CardDoc {
    path: String,
    card: Card,
    /// The registry key it resolves to, and that entry's title.
    key: String,
    title: Option<String>,
    text: Option<PathBuf>,
}

/// The cards under the configured globs, and why some are left out.
fn cards(
    ctx: &Context,
    reg: Option<&Registry>,
    resolver: &mut Resolver,
) -> Result<(Vec<Doc>, Vec<String>), String> {
    let found = crate::archive::files::walk(&ctx.project.root, &[&ctx.cards])?
        .pop()
        .unwrap_or_default();
    let mut read = Vec::new();
    let mut notices = Vec::new();
    for path in found {
        let file = ctx.project.root.join(&path);
        let parsed = std::fs::read(&file)
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_slice::<Card>(&b).map_err(|e| e.to_string()));
        match parsed {
            Ok(card) if crate::finding::version_1(&card.schema, "rf.paper-card/") => {
                read.push((path, card));
            }
            Ok(card) => notices.push(format!(
                "{path}: schema `{}` is not rf.paper-card/1; the card is left out",
                card.schema
            )),
            Err(e) => notices.push(format!("{path}: not a paper card ({e}); it is left out")),
        }
    }
    resolver.prepare(read.iter().map(|(_, c)| c.text.path.as_str()))?;
    let docs = read
        .into_iter()
        .filter_map(|(path, card)| {
            let key = text_of(&card.key)?;
            let (key, title) = match reg {
                Some(reg) => match reg.entries.get(reg.root(&key)) {
                    Some(e) => (e.key.clone(), Some(e.title.clone())),
                    None => (key, None),
                },
                None => (key, None),
            };
            let text = resolver.get(&card.text.path);
            let stamp = format!(
                "{}|{}|{key}|{title:?}",
                file_stamp(&ctx.project.root.join(&path))?,
                text.as_deref().and_then(file_stamp).unwrap_or_default()
            );
            Some(Doc {
                scope: Scope::Refs,
                key: path.clone(),
                stamp,
                what: What::Card(Box::new(CardDoc {
                    path,
                    card,
                    key,
                    title,
                    text,
                })),
            })
        })
        .collect();
    Ok((docs, notices))
}

/// Why a card no longer fits its text, if it does not.
fn card_stale(c: &CardDoc) -> Result<Option<String>, String> {
    let Some(file) = &c.text else {
        return Ok(Some(format!(
            "its text {} is not there; it is ignored",
            c.card.text.path
        )));
    };
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let sha = sha256(&bytes);
    Ok((!c.card.text.sha256.eq_ignore_ascii_case(&sha))
        .then(|| stale_why(&c.card.text.sha256, &sha)))
}

/// A card's items as units at their quotes' lines; none when it is stale.
pub fn load_card(c: &CardDoc) -> Result<Loaded, String> {
    let stale = card_stale(c)?;
    let units = if stale.is_some() {
        Vec::new()
    } else {
        c.card
            .items
            .iter()
            .map(|item| {
                let value = match &item.value {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                let (start, end) = item.lines.unwrap_or((1, 1));
                Unit {
                    start,
                    end: end.max(start),
                    heading: format!("card: {}", item.field),
                    body: [value, item.quote.clone().unwrap_or_default()]
                        .join("\n")
                        .trim()
                        .to_string(),
                }
            })
            .collect()
    };
    Ok(Loaded::stored(
        Row {
            class: Some(Class::Generated),
            title: c.title.clone(),
            kind: Some("card".into()),
            ref_key: Some(c.key.clone()),
            anchor: Some(c.card.text.path.clone()),
            info: Some(json!({ "key": c.key, "card": c.path, "model": c.card.model })),
            ..Row::default()
        },
        units,
        stale.map(|why| (c.path.clone(), why)).into_iter().collect(),
    ))
}

/// Every structure file and card that no longer fits its text, with why:
/// what `lab archive check` reports as `stale-derivative`.
pub fn stale(ctx: &Context) -> Result<Vec<(String, String)>, String> {
    let mut resolver = Resolver::new(&ctx.project.root)?;
    let mut out = Vec::new();
    if let Some(reg) = Registry::read(ctx, &mut resolver)? {
        let texts: Vec<&TextRef> = reg.entries.iter().flat_map(|e| &e.texts).collect();
        let sidecars: Vec<String> = texts
            .iter()
            .map(|t| format!("{}{STRUCTURE_SUFFIX}", t.path))
            .collect();
        resolver.prepare(
            texts
                .iter()
                .map(|t| t.path.as_str())
                .chain(sidecars.iter().map(String::as_str)),
        )?;
        for (t, side_rel) in texts.iter().zip(&sidecars) {
            let (Some(file), Some(side)) = (resolver.get(&t.path), resolver.get(side_rel)) else {
                continue;
            };
            let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            if let Err(why) | Ok(Sections::Stale(why, _)) = structure(&side, &sha256(&bytes)) {
                out.push((side_rel.clone(), why));
            }
        }
        let (cards, _) = cards(ctx, Some(&reg), &mut resolver)?;
        for doc in cards {
            if let What::Card(c) = &doc.what
                && let Some(why) = card_stale(c)?
            {
                out.push((c.path.clone(), why));
            }
        }
    } else {
        let (cards, _) = cards(ctx, None, &mut resolver)?;
        for doc in cards {
            if let What::Card(c) = &doc.what
                && let Some(why) = card_stale(c)?
            {
                out.push((c.path.clone(), why));
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A structure file of `text` with `sections` of (kind, heading, first, last).
    fn structure_of(text: &str, sections: &[(&str, Option<&str>, usize, usize)]) -> Structure {
        serde_json::from_value(json!({
            "schema": "rf.paper-structure/1",
            "text": {"sha256": sha256(text.as_bytes()), "lines": text.lines().count()},
            "sections": sections.iter().map(|(kind, heading, first, last)| json!({
                "kind": kind, "heading": heading, "first_line": first, "last_line": last,
            })).collect::<Vec<_>>(),
        }))
        .unwrap()
    }

    fn ranges(sections: &[Section]) -> Vec<(usize, usize)> {
        sections
            .iter()
            .map(|s| (s.first_line, s.last_line))
            .collect()
    }

    /// The passages of `text` within `sections`, as (heading, text).
    fn passages(text: &str, sections: Vec<Section>) -> Vec<(String, String)> {
        let spans: Vec<Span> = sections
            .into_iter()
            .map(|s| Span {
                first: s.first_line,
                last: s.last_line,
                heading: s.heading.unwrap_or(s.kind),
            })
            .collect();
        windows(text, &spans)
            .into_iter()
            .map(|u| (u.heading, u.body))
            .collect()
    }

    #[test]
    fn a_stale_structure_files_sections_are_found_again_by_their_headings() {
        // Lines: 1 title, 2-3 contents (a heading's twin), 4-30 Method, 31-60 Results.
        let words = |w: &str, n: usize| -> String {
            (0..n)
                .map(|i| format!("{w}{i} ").repeat(30) + "\n")
                .collect()
        };
        let text = format!(
            "Title\nContents\n2  Results\n# Method\n{}2 Results\n{}",
            words("m", 26),
            words("r", 29)
        );
        let sections = [
            ("front", None, 1, 3),
            ("body", Some("# Method"), 4, 30),
            ("body", Some("2 Results"), 31, 60),
        ];
        let before = passages(&text, structure_of(&text, &sections).sections);
        // Four lines in the front section leave every passage of the later
        // sections as it was.
        let edited = text.replacen("Title\n", "Title\nA\n\nB\n\n", 1);
        let found = relocate(structure_of(&text, &sections), &edited);
        assert_eq!(ranges(&found), [(1, 7), (8, 34), (35, 64)]);
        let after = passages(&edited, found);
        assert!(before.len() == after.len() && before[1..] == after[1..]);
        // A heading that only changed its spaces is still found.
        let spaced = edited.replacen("\n2 Results\n", "\n2   Results\n", 1);
        let found = relocate(structure_of(&text, &sections), &spaced);
        assert_eq!(ranges(&found), [(1, 7), (8, 34), (35, 64)]);
        // A heading that is gone moves with the section before it, and a gap
        // between sections stays one.
        let gone = text.replacen("# Method\n", "", 1);
        let gap = [
            ("front", None, 1, 2),
            ("body", Some("# Method"), 4, 30),
            ("body", Some("2 Results"), 31, 60),
        ];
        let found = relocate(structure_of(&text, &gap), &gone);
        assert_eq!(ranges(&found), [(1, 2), (4, 29), (30, 59)]);
        // A text that got shorter or empty leaves the sections past its end
        // empty, and their passages none.
        for short in ["", "Title\n"] {
            let found = relocate(structure_of(&text, &sections), short);
            assert!(
                found.iter().all(|s| s.first_line > 0),
                "{:?}",
                ranges(&found)
            );
            assert!(passages(short, found).len() <= 1);
        }
    }

    #[test]
    fn normalises_arxiv_ids_and_dois() {
        assert_eq!(normal_arxiv("arXiv:2401.01234v3"), "2401.01234");
        assert_eq!(normal_arxiv("2401.01234"), "2401.01234");
        assert_eq!(normal_arxiv("cs/0101001v1"), "cs/0101001");
        assert_eq!(normal_doi("https://doi.org/10.1145/ABC"), "10.1145/abc");
        assert_eq!(normal_doi("doi:10.1/x"), "10.1/x");
    }

    #[test]
    fn reads_an_entry_with_null_and_number_fields() {
        let e = Entry::parse(
            r#"{"key":7,"title":"T","authors":null,"year":2024,"ids":{"arxiv":"2401.1v2","doi":null},"texts":[{"path":"t/hirec.md","extraction":"docling"},{"path":"t/7.txt"},{"path":"t/hirec.txt"}],"duplicate_of":null,"source":{"path":"refs.md","line":3}}"#,
        )
        .unwrap();
        assert_eq!((e.key.as_str(), e.year.as_deref()), ("7", Some("2024")));
        assert!(e.authors.is_empty());
        assert_eq!(e.aliases(), ["ref:7", "2401.1"]);
        assert_eq!(e.texts[0].extraction.as_deref(), Some("docling"));
        assert_eq!(e.source, Some(("refs.md".into(), 3)));
        assert!(Entry::parse(r#"{"title":"no key"}"#).is_none());
    }

    #[test]
    fn a_text_named_by_a_word_of_its_own_is_a_short_name() {
        let line = |k: &str, texts: &[&str]| {
            let texts: Vec<String> = texts
                .iter()
                .map(|t| format!(r#"{{"path":"{t}"}}"#))
                .collect();
            format!(
                r#"{{"key":"{k}","title":"t","texts":[{}]}}"#,
                texts.join(",")
            )
        };
        let mut entries: Vec<Entry> = [
            line("7", &["t/hirec.md", "t/7.txt", "t/hirec.txt"]),
            line("8", &["t/8/notes.txt", "t/paper.txt"]),
            line("9", &["t/shared.txt"]),
            line("10", &["t/shared.txt"]),
        ]
        .iter()
        .map(|l| Entry::parse(l).unwrap())
        .collect();
        short_names(&mut entries);
        let short: Vec<&[String]> = entries.iter().map(|e| e.short.as_slice()).collect();
        // Once for two files of one name; none named by the key, in its
        // directory, generic or shared with another entry.
        assert_eq!(short, [&["hirec".to_string()][..], &[], &[], &[]]);
    }

    #[test]
    fn a_duplicate_chain_resolves_to_its_end_and_a_loop_to_itself() {
        let line = |k: &str, d: Option<&str>| {
            format!(
                r#"{{"key":"{k}","title":"t{k}","duplicate_of":{}}}"#,
                d.map_or("null".into(), |d| format!("\"{d}\""))
            )
        };
        let entries: Vec<Entry> = [
            line("1", Some("2")),
            line("2", Some("3")),
            line("3", None),
            line("4", Some("5")),
            line("5", Some("4")),
            line("6", Some("missing")),
        ]
        .iter()
        .map(|l| Entry::parse(l).unwrap())
        .collect();
        let by_key = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.key.clone(), i))
            .collect();
        let reg = Registry {
            entries,
            by_key,
            bad: 0,
        };
        let key = |k: &str| reg.entries[reg.root(k)].key.as_str();
        assert_eq!(key("1"), "3");
        assert_eq!(key("4"), "4");
        assert_eq!(key("6"), "6");
        let groups = reg.groups();
        assert!(groups.contains(&(2, vec![0, 1])));
    }
}
