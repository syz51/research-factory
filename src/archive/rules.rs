//! The rules of `lab archive check`. Each problem becomes an `rf.finding/1`
//! whose subject and message stay the same while the problem does, so its
//! fingerprint survives unrelated edits.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::front::{self, Meta};
use super::markdown;
use super::seal::{Entry, Manifest, State};
use super::{Record, Spec, Texts, read_optional};
use crate::config::{KeptLinkSpec, OffloadedSpec};
use crate::finding::{Evidence, Finding, Severity};

/// A problem `lab archive check` reports, before it becomes an `rf.finding/1`.
#[derive(Debug)]
pub(super) struct Problem {
    rule: &'static str,
    pub(super) severity: Severity,
    path: String,
    line: Option<usize>,
    /// The subject when it is not `path[:line]`, such as a duplicated id.
    subject: Option<String>,
    message: String,
    evidence: Vec<String>,
    data: Option<serde_json::Value>,
    /// Shown after the message in text output only, so it can change
    /// without changing the finding's fingerprint.
    detail: Option<String>,
}

impl Problem {
    fn new(rule: &'static str, severity: Severity, path: &str, message: String) -> Self {
        Self {
            rule,
            severity,
            path: path.into(),
            line: None,
            subject: None,
            message,
            evidence: Vec::new(),
            data: None,
            detail: None,
        }
    }

    fn at(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }

    fn subject(&self) -> String {
        match (&self.subject, self.line) {
            (Some(s), _) => s.clone(),
            (None, Some(l)) => format!("{}:{l}", self.path),
            (None, None) => self.path.clone(),
        }
    }

    pub(super) fn text(&self) -> String {
        let detail = self
            .detail
            .as_ref()
            .map_or_else(String::new, |d| format!(" ({d})"));
        format!(
            "{} {} {} — {}{detail}",
            self.severity.as_str(),
            self.rule,
            self.subject(),
            self.message
        )
    }

    pub(super) fn finding(&self) -> Finding {
        let mut f = Finding::lab(
            self.rule,
            self.severity,
            &self.subject(),
            self.message.clone(),
        );
        f.evidence = self
            .evidence
            .iter()
            .map(|p| Evidence {
                path: p.clone(),
                locator: None,
            })
            .collect();
        f.data = self.data.clone();
        f
    }
}

pub(super) fn invalid_problem(r: &Record) -> Option<Problem> {
    let inv = r.invalid.as_ref()?;
    Some(
        Problem::new(
            "front-matter-invalid",
            Severity::Error,
            &r.path,
            inv.message.clone(),
        )
        .at(inv.line),
    )
}

pub(super) fn problems(spec: &Spec) -> Result<Vec<Problem>, String> {
    let mut extra = vec![&spec.sealed, &spec.links, &spec.current];
    extra.extend(spec.budgets.iter());
    let found = spec.walk(&extra)?;
    let (by_index, rest) = found.split_at(spec.indexes.len());
    let (sealed, links, current, budgets) = (&rest[0], &rest[1], &rest[2], &rest[3..]);
    let mut texts = Texts::new();
    let records = spec.records(by_index, &mut texts)?;
    let mut out = Vec::new();
    record_problems(spec, &records, &mut out);
    index_problems(spec, &records, &mut out)?;
    for (b, paths) in spec.cfg.budget.iter().zip(budgets) {
        for path in paths {
            let bytes = std::fs::metadata(spec.root().join(path))
                .map_err(|e| format!("{path}: {e}"))?
                .len();
            if bytes > b.max_bytes {
                let mut p = Problem::new(
                    "size-budget",
                    b.severity,
                    path,
                    format!(
                        "is larger than its budget of {} bytes; move detail into linked records",
                        b.max_bytes
                    ),
                );
                p.data = Some(serde_json::json!({"bytes": bytes, "max_bytes": b.max_bytes}));
                out.push(p);
            }
        }
    }
    let manifest = Manifest::load(&spec.manifest)?;
    let seals = Seals {
        sealed: sealed.iter().map(String::as_str).collect(),
        manifest: &manifest,
    };
    link_problems(spec, &records, links, current, &seals, &mut texts, &mut out)?;
    seal_problems(spec, sealed, &manifest, &mut out)?;
    out.sort_by(|a, b| {
        (&a.path, a.line, a.rule, &a.message).cmp(&(&b.path, b.line, b.rule, &b.message))
    });
    Ok(out)
}

/// Each record once, by path, in index order.
fn unique(records: &[Record]) -> Vec<&Record> {
    super::unique_refs(&records.iter().collect::<Vec<_>>())
}

fn record_problems(spec: &Spec, records: &[Record], out: &mut Vec<Problem>) {
    let mut missing = BTreeSet::new();
    for r in records {
        for field in &spec.indexes[r.index].spec.require {
            let m = &r.meta;
            let value = match field {
                crate::config::Field::Id => &m.id,
                crate::config::Field::Title => &m.title,
                crate::config::Field::Date => &m.date,
                crate::config::Field::Status => &m.status,
                crate::config::Field::Summary => &m.summary,
            };
            if value.is_none() && r.invalid.is_none() && missing.insert((&r.path, field.as_str())) {
                out.push(Problem::new(
                    "missing-field",
                    Severity::Error,
                    &r.path,
                    format!(
                        "has no `{}`, which index `{}` requires; add it to the front matter",
                        field.as_str(),
                        spec.indexes[r.index].spec.name
                    ),
                ));
            }
        }
    }
    let records = unique(records);
    out.extend(records.iter().filter_map(|r| invalid_problem(r)));
    let mut by_id: BTreeMap<&str, Vec<&Record>> = BTreeMap::new();
    for r in &records {
        if let Some(id) = &r.meta.id {
            by_id.entry(id).or_default().push(r);
        }
    }
    for (id, rs) in by_id.iter().filter(|(_, rs)| rs.len() > 1) {
        let paths: Vec<String> = rs.iter().map(|r| r.path.clone()).collect();
        let mut p = Problem::new(
            "duplicate-id",
            Severity::Error,
            &paths[0],
            format!(
                "{} records have id {id}: {}; give each its own id",
                paths.len(),
                paths.join(", ")
            ),
        );
        p.subject = Some((*id).into());
        p.evidence = paths;
        out.push(p);
    }
    let lists = |id: &str, pick: fn(&Meta) -> &Vec<String>, wanted: &str| {
        by_id
            .get(id)
            .is_some_and(|rs| rs.iter().any(|r| pick(&r.meta).iter().any(|x| x == wanted)))
    };
    for r in &records {
        let m = &r.meta;
        for (field, ids) in [
            ("supersedes", &m.supersedes),
            ("superseded_by", &m.superseded_by),
            ("decisions", &m.decisions),
        ] {
            for id in ids.iter().filter(|id| !by_id.contains_key(id.as_str())) {
                out.push(Problem::new(
                    "unknown-id",
                    Severity::Error,
                    &r.path,
                    format!("`{field}` names {id}, which no indexed record has"),
                ));
            }
        }
        if let Some(me) = &m.id {
            for other in m
                .supersedes
                .iter()
                .filter(|o| by_id.contains_key(o.as_str()))
            {
                if !lists(other, |m| &m.superseded_by, me) {
                    out.push(Problem::new(
                        "supersession-mismatch",
                        Severity::Warn,
                        &r.path,
                        format!(
                            "supersedes {other}, but {other}'s `superseded_by` does not list {me}"
                        ),
                    ));
                }
            }
            for other in m
                .superseded_by
                .iter()
                .filter(|o| by_id.contains_key(o.as_str()))
            {
                if !lists(other, |m| &m.supersedes, me) {
                    out.push(Problem::new(
                        "supersession-mismatch",
                        Severity::Warn,
                        &r.path,
                        format!("is superseded by {other}, but {other}'s `supersedes` does not list {me}"),
                    ));
                }
            }
        }
        if !m.superseded_by.is_empty() && !spec.inactive(r) {
            out.push(Problem::new(
                "superseded-but-active",
                Severity::Warn,
                &r.path,
                format!(
                    "is superseded by {} but its status is {}; set it to one of: {}",
                    m.superseded_by.join(", "),
                    m.status
                        .as_deref()
                        .map_or_else(|| "missing".into(), |s| format!("`{s}`")),
                    spec.cfg.inactive_statuses.join(", ")
                ),
            ));
        }
    }
}

fn index_problems(spec: &Spec, records: &[Record], out: &mut Vec<Problem>) -> Result<(), String> {
    for (i, ix) in spec.indexes.iter().enumerate() {
        // `lab archive index` would refuse to write it, so there is nothing to compare.
        if records.iter().any(|r| r.index == i && r.invalid.is_some()) {
            continue;
        }
        let output = &ix.spec.output;
        let (rule, message) = match read_optional(&spec.root().join(output))? {
            None => ("index-missing", "does not exist; run `lab archive update`"),
            Some(b) if b != spec.render(records, i).as_bytes() => (
                "index-stale",
                "differs from what its records generate; run `lab archive update` (never edit an index by hand)",
            ),
            Some(_) => continue,
        };
        out.push(Problem::new(rule, Severity::Error, output, message.into()));
    }
    Ok(())
}

/// Where a link points.
#[derive(Debug, PartialEq)]
enum Target {
    /// A URL, or nothing to check.
    Skip,
    /// `#fragment` in the same file.
    Here(String),
    /// A file or directory; `rel` when it lies inside the project.
    There {
        abs: PathBuf,
        rel: Option<String>,
        fragment: Option<String>,
    },
}

fn has_scheme(dest: &str) -> bool {
    dest.split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
    })
}

/// `dir/path` with `.` and `..` resolved; `None` when it leaves the root.
fn normalize(dir: &str, path: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

/// Resolves a link destination written in a file in directory `dir`.
fn resolve(root: &Path, dir: &str, dest: &str) -> Target {
    let dest = dest.trim();
    if dest.is_empty() || has_scheme(dest) {
        return Target::Skip;
    }
    let (path, fragment) = match dest.split_once('#') {
        Some((p, f)) => (
            p,
            Some(markdown::percent_decode(f)).filter(|f| !f.is_empty()),
        ),
        None => (dest, None),
    };
    let path = path.split_once('?').map_or(path, |(p, _)| p);
    if path.is_empty() {
        return fragment.map_or(Target::Skip, Target::Here);
    }
    let path = markdown::percent_decode(path);
    if let Some(stripped) = path.strip_prefix('/') {
        let abs = PathBuf::from(&path);
        if abs.exists() {
            let rel = abs
                .strip_prefix(root)
                .ok()
                .map(|r| r.to_string_lossy().replace('\\', "/"));
            return Target::There { abs, rel, fragment };
        }
        // Not a file system path: try it relative to the project root, as GitHub does.
        let rel = normalize("", stripped);
        let abs = root.join(stripped);
        return Target::There { abs, rel, fragment };
    }
    let rel = normalize(dir, &path);
    let abs = match &rel {
        Some(r) => root.join(r),
        None => root.join(dir).join(&path),
    };
    Target::There { abs, rel, fragment }
}

/// Directory listings, to check that a path exists with exactly the spelling a
/// link uses. On a case-insensitive file system (macOS by default)
/// `Path::exists` accepts `TOPIC.md` for `topic.md`, a link that breaks on
/// GitHub and Linux.
#[derive(Default)]
struct Listings(HashMap<PathBuf, Option<BTreeSet<String>>>);

impl Listings {
    /// Whether `rel`, relative to `root`, exists. A component that the file
    /// system finds but that differs from the listed name only in case is
    /// missing; another difference (such as Unicode normalisation) is accepted.
    fn exists(&mut self, root: &Path, rel: &str) -> bool {
        let mut dir = root.to_path_buf();
        for part in rel.split('/').filter(|p| !p.is_empty()) {
            let names = self.0.entry(dir.clone()).or_insert_with(|| {
                std::fs::read_dir(&dir).ok().map(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
            });
            let Some(names) = names else {
                // Unlisted directory: fall back to the file system's answer.
                return root.join(rel).exists();
            };
            if !names.contains(part) {
                let lower = part.to_lowercase();
                if !dir.join(part).exists() || names.iter().any(|n| n.to_lowercase() == lower) {
                    return false;
                }
            }
            dir.push(part);
        }
        true
    }
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| x.eq_ignore_ascii_case("md") || x.eq_ignore_ascii_case("markdown"))
}

/// Words that mark a link to an inactive record as deliberate.
const HISTORY_WORDS: [&str; 4] = ["superseded", "historical", "rejected", "withdrawn"];

/// One `[[lab.archive.offloaded]]` list: the moved paths, every directory
/// that holds one, and how many links point into them.
struct Offloaded<'a> {
    spec: &'a OffloadedSpec,
    paths: HashSet<String>,
    dirs: HashSet<String>,
    links: usize,
}

impl<'a> Offloaded<'a> {
    /// Reads the list: one path per line, relative to the project root. A
    /// missing or unreadable list is an error, since its links would
    /// otherwise all be broken.
    fn load(root: &Path, spec: &'a OffloadedSpec) -> Result<Self, String> {
        let path = root.join(&spec.list);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("[[lab.archive.offloaded]] list {}: {e}", path.display()))?;
        let mut paths = HashSet::new();
        let mut dirs = HashSet::new();
        for (n, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // Written like a link from the root: `/`, `./` and `..` resolve
            // the same way, and a path that leaves the root is an error.
            let p = normalize("", line).filter(|p| !p.is_empty()).ok_or_else(|| {
                format!(
                    "[[lab.archive.offloaded]] list {}:{}: `{line}` is not a path below the project root",
                    path.display(),
                    n + 1
                )
            })?;
            for (i, _) in p.match_indices('/') {
                dirs.insert(p[..i].to_string());
            }
            paths.insert(p);
        }
        Ok(Self {
            spec,
            paths,
            dirs,
            links: 0,
        })
    }

    /// Whether `rel`, a missing link target, is a moved path, lies in a moved
    /// directory, or is a directory that held a moved path.
    fn holds(&self, rel: &str) -> bool {
        self.dirs.contains(rel)
            || std::iter::once(rel)
                .chain(rel.match_indices('/').map(|(i, _)| &rel[..i]))
                .any(|p| self.paths.contains(p))
    }
}

/// The append-only files and their seals.
struct Seals<'a> {
    /// The files that match `[lab.archive] sealed`.
    sealed: BTreeSet<&'a str>,
    manifest: &'a Manifest,
}

impl Seals<'_> {
    /// Whether file line `line` (from 1) of `file`, whose content is `bytes`,
    /// lies wholly in the text its seal covers: the file is append-only, its
    /// seal holds, and the line ends within the sealed bytes of the body.
    fn covers(&self, file: &str, bytes: &[u8], line: usize) -> bool {
        if !self.sealed.contains(file) {
            return false;
        }
        let entry = self.manifest.files.get(file);
        let body = front::sealed_body(bytes);
        if !matches!(
            Entry::state(entry, body),
            State::Unchanged | State::Appended
        ) {
            return false;
        }
        let Some(sealed) = entry.and_then(|e| usize::try_from(e.bytes).ok()) else {
            return false;
        };
        // The body is the end of the file, so the sealed span starts where it does.
        let sealed_end = bytes.len() - body.len() + sealed;
        let mut start = 0;
        for _ in 1..line {
            match bytes[start..].iter().position(|&b| b == b'\n') {
                Some(i) => start += i + 1,
                None => return false,
            }
        }
        let end = bytes[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |i| start + i);
        end <= sealed_end
    }
}

/// One `[[lab.archive.kept_link]]` entry and whether a missing link matched it.
struct Kept<'a> {
    spec: &'a KeptLinkSpec,
    matched: bool,
}

/// The link targets, relative to the root, that a linked git worktree lacks
/// but its main checkout has, where git ignores them in the worktree: data
/// and downloads exist only in the checkout that made them. Each such target
/// is only checked to exist in the main checkout; its anchors are not checked.
fn in_main_checkout<'f>(
    root: &Path,
    files: impl Iterator<Item = &'f &'f str>,
    texts: &mut Texts,
    listings: &mut Listings,
) -> Result<BTreeSet<String>, String> {
    let Some(fallback) = crate::worktree::Fallback::find(root)? else {
        return Ok(BTreeSet::new());
    };
    let mut missing = BTreeSet::new();
    for file in files {
        let doc = texts.doc(&root.join(file))?;
        let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
        for link in &doc.links {
            if let Target::There { rel: Some(r), .. } = resolve(root, dir, &link.dest)
                && !r.is_empty()
                && !listings.exists(root, &r)
            {
                missing.insert(r);
            }
        }
    }
    fallback.resolve(missing.iter().map(String::as_str))
}

fn link_problems(
    spec: &Spec,
    records: &[Record],
    links: &[String],
    current: &[String],
    seals: &Seals,
    texts: &mut Texts,
    out: &mut Vec<Problem>,
) -> Result<(), String> {
    let root = spec.root();
    let by_path: HashMap<&str, &Record> = unique(records)
        .into_iter()
        .map(|r| (r.path.as_str(), r))
        .collect();
    let checked: BTreeSet<&str> = links.iter().map(String::as_str).collect();
    let current: BTreeSet<&str> = current.iter().map(String::as_str).collect();
    let mut exists: HashMap<PathBuf, bool> = HashMap::new();
    let mut listings = Listings::default();
    let mut offloaded = spec
        .cfg
        .offloaded
        .iter()
        .map(|o| Offloaded::load(root, o))
        .collect::<Result<Vec<_>, _>>()?;
    let mut kept: Vec<Kept> = spec
        .cfg
        .kept_link
        .iter()
        .map(|spec| Kept {
            spec,
            matched: false,
        })
        .collect();
    let in_main = in_main_checkout(root, checked.union(&current), texts, &mut listings)?;
    for file in checked.union(&current) {
        let abs_file = root.join(file);
        let doc = texts.doc(&abs_file)?;
        let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
        let check = checked.contains(file);
        for link in &doc.links {
            let problem =
                |rule, message| Problem::new(rule, Severity::Error, file, message).at(link.line);
            match resolve(root, dir, &link.dest) {
                Target::Skip => {}
                Target::Here(fragment) => {
                    if check && !doc.anchors.contains(&fragment.to_lowercase()) {
                        out.push(problem(
                            "broken-anchor",
                            format!("links to `#{fragment}`, which matches no heading or anchor in this file"),
                        ));
                    }
                }
                Target::There { abs, rel, fragment } => {
                    let found = *exists.entry(abs.clone()).or_insert_with(|| match &rel {
                        Some(r) => listings.exists(root, r) || in_main.contains(r),
                        None => abs.exists(),
                    });
                    if !found {
                        if !check {
                            continue;
                        }
                        // A link to a file moved out of the project is counted
                        // per list, not reported one by one.
                        if let Some(o) = rel
                            .as_deref()
                            .and_then(|r| offloaded.iter_mut().find(|o| o.holds(r)))
                        {
                            o.links += 1;
                            continue;
                        }
                        let message = format!("links to `{}`, which does not exist", link.dest);
                        let Some(k) = kept
                            .iter_mut()
                            .find(|k| k.spec.file == *file && k.spec.target == link.dest)
                        else {
                            out.push(problem("broken-link", message));
                            continue;
                        };
                        k.matched = true;
                        if seals.covers(file, &texts.bytes(&abs_file)?, link.line) {
                            let mut p = problem(
                                "kept-link",
                                format!(
                                    "{message}; it is kept in sealed text by [[lab.archive.kept_link]]"
                                ),
                            );
                            p.severity = Severity::Info;
                            p.data = Some(serde_json::json!({
                                "target": k.spec.target,
                                "reason": k.spec.reason,
                            }));
                            p.detail = Some(k.spec.reason.clone());
                            out.push(p);
                        } else {
                            out.push(problem(
                                "broken-link",
                                format!(
                                    "{message}; [[lab.archive.kept_link]] keeps a link only in the \
                                     sealed text of an append-only file, and this line is not sealed"
                                ),
                            ));
                        }
                        continue;
                    }
                    // Anchors are checked only in Markdown files inside the project.
                    if check
                        && let Some(fragment) = &fragment
                        && rel.is_some()
                        && is_markdown(&abs)
                        && abs.is_file()
                        && !texts.doc(&abs)?.anchors.contains(&fragment.to_lowercase())
                    {
                        out.push(problem(
                            "broken-anchor",
                            format!(
                                "links to `{}`, but the target has no heading or anchor `#{fragment}`",
                                link.dest
                            ),
                        ));
                    }
                    if current.contains(file)
                        && let Some(r) = rel.as_deref().and_then(|r| by_path.get(r))
                        && spec.inactive(r)
                    {
                        let bytes = texts.bytes(&abs_file)?;
                        let text = String::from_utf8_lossy(&bytes);
                        let line = text.lines().nth(link.line - 1).unwrap_or("").to_lowercase();
                        let marked = HISTORY_WORDS.iter().any(|w| line.contains(w))
                            || spec
                                .cfg
                                .inactive_statuses
                                .iter()
                                .any(|s| line.contains(&s.to_lowercase()));
                        if !marked {
                            let mut p = problem(
                                "superseded-link",
                                format!(
                                    "links {} ({}) as if it were current; link the record that replaced it, or say on the line that it is superseded or historical",
                                    r.meta.id.as_deref().unwrap_or(&r.path),
                                    r.meta.status.as_deref().unwrap_or_default()
                                ),
                            );
                            p.severity = Severity::Warn;
                            out.push(p);
                        }
                    }
                }
            }
        }
    }
    for k in kept.iter().filter(|k| !k.matched) {
        out.push(Problem::new(
            "stale-kept-link",
            Severity::Warn,
            &k.spec.file,
            format!(
                "[[lab.archive.kept_link]] entry for `{}` matches no missing link in this file; \
                 remove the entry",
                k.spec.target
            ),
        ));
    }
    // The count is in `data` and the text output only, so the finding keeps
    // its fingerprint while links come and go.
    for o in offloaded {
        let message = if o.links == 0 {
            format!(
                "no link points to the files this list records as moved to {}; \
                 the list and its entry can go",
                o.spec.location
            )
        } else {
            format!(
                "links point to files this list records as moved to {}",
                o.spec.location
            )
        };
        let mut p = Problem::new("archived-link", Severity::Info, &o.spec.list, message);
        p.data = Some(serde_json::json!({"links": o.links, "location": o.spec.location}));
        p.detail = Some(format!("{} link(s)", o.links));
        out.push(p);
    }
    Ok(())
}

fn seal_problems(
    spec: &Spec,
    sealed: &[String],
    manifest: &Manifest,
    out: &mut Vec<Problem>,
) -> Result<(), String> {
    for path in sealed {
        let bytes = std::fs::read(spec.root().join(path)).map_err(|e| format!("{path}: {e}"))?;
        match Entry::state(manifest.files.get(path), front::sealed_body(&bytes)) {
            State::New => out.push(Problem::new(
                "unsealed",
                Severity::Warn,
                path,
                "is append-only but not sealed yet; run `lab archive update`".into(),
            )),
            State::Edited => out.push(Problem::new(
                "sealed-edited",
                Severity::Error,
                path,
                "was edited after it was sealed; restore the sealed text and append a dated correction, \
                 or accept the edit with `lab archive seal --accept <path> --note \"<why>\"`"
                    .into(),
            )),
            State::Unchanged | State::Appended => {}
        }
    }
    for path in manifest.files.keys() {
        if !spec.root().join(path).is_file() {
            out.push(Problem::new(
                "sealed-missing",
                Severity::Error,
                path,
                format!(
                    "is sealed in {} but no longer exists; restore it",
                    spec.manifest_display()
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A project whose `docs/a.md` links `links`, with one offloaded list.
    fn offloaded_project(dir: &Path, links: &[&str]) -> crate::config::Project {
        std::fs::write(
            dir.join("research.toml"),
            "[lab.archive]\nlinks = [\"docs/*.md\"]\n\
             [[lab.archive.offloaded]]\nlist = \"lists/moved.txt\"\nlocation = \"cloud:archive/day/proj/\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::create_dir_all(dir.join("lists")).unwrap();
        let body: Vec<String> = links.iter().map(|l| format!("[x]({l})\n")).collect();
        std::fs::write(dir.join("docs/a.md"), body.join("\n")).unwrap();
        crate::config::Project::discover(dir, None).unwrap()
    }

    fn summary(problems: &[Problem]) -> Vec<String> {
        problems
            .iter()
            .map(|p| format!("{} {} {}", p.severity.as_str(), p.rule, p.subject()))
            .collect()
    }

    #[test]
    fn links_to_offloaded_files_and_their_directories_are_counted_not_broken() {
        let dir = tempfile::tempdir().unwrap();
        let p = offloaded_project(
            dir.path(),
            &[
                "../lab-rf86-data/run1/out.csv",
                "../lab-rf86-data/run1",
                "/lab-rf86-data/run1/log.txt",
                "../lab-rf86-data",
                "../lab-rf86-data/run2/out.csv",
                "../lab-rf86-data/run3/sub/x.csv",
            ],
        );
        std::fs::write(
            dir.path().join("lists/moved.txt"),
            "\u{feff}lab-rf86-data/run1/out.csv\r\n./lab-rf86-data/run1/log.txt\n\n\
             /lab-rf86-data//run3/\n",
        )
        .unwrap();
        let problems = problems(&Spec::new(&p).unwrap()).unwrap();
        // The listed file, its directory, a listed path written from the
        // root, an ancestor directory and a file in a listed directory are
        // archived; the typo is broken.
        assert_eq!(
            summary(&problems),
            [
                "error broken-link docs/a.md:9",
                "info archived-link lists/moved.txt"
            ]
        );
        let info = &problems[1];
        assert_eq!(
            info.message,
            "links point to files this list records as moved to cloud:archive/day/proj/"
        );
        assert!(info.text().ends_with("(5 link(s))"), "{}", info.text());
        assert_eq!(
            info.data,
            Some(serde_json::json!({"links": 5, "location": "cloud:archive/day/proj/"}))
        );
    }

    #[test]
    fn a_missing_offloaded_list_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = offloaded_project(dir.path(), &["../data/run1/out.csv"]);
        let err = problems(&Spec::new(&p).unwrap()).unwrap_err();
        assert!(
            err.contains("[[lab.archive.offloaded]] list") && err.contains("moved.txt"),
            "{err}"
        );
        // An unreadable list, here not UTF-8, is an error too.
        std::fs::write(dir.path().join("lists/moved.txt"), [0xff, 0xfe, b'\n']).unwrap();
        assert!(problems(&Spec::new(&p).unwrap()).is_err());
        // An empty list is read, and every missing target stays broken.
        // A line that leaves the root is an error naming the line.
        std::fs::write(dir.path().join("lists/moved.txt"), "a.txt\n../up.txt\n").unwrap();
        let err = problems(&Spec::new(&p).unwrap()).unwrap_err();
        assert!(err.contains("moved.txt:2"), "{err}");
        // An empty list is read, every missing target stays broken, and the
        // note says the list serves no link.
        std::fs::write(dir.path().join("lists/moved.txt"), "").unwrap();
        let problems = problems(&Spec::new(&p).unwrap()).unwrap();
        assert_eq!(
            summary(&problems),
            [
                "error broken-link docs/a.md:1",
                "info archived-link lists/moved.txt"
            ]
        );
        assert!(problems[1].message.starts_with("no link points"));
    }

    #[test]
    fn offloaded_lists_need_a_relative_path_a_location_and_one_entry() {
        let dir = tempfile::tempdir().unwrap();
        let entry = |list: &str, location: &str| {
            format!("[[lab.archive.offloaded]]\nlist = \"{list}\"\nlocation = \"{location}\"\n")
        };
        for entries in [
            entry("../up.txt", "cloud:x/"),
            entry("/abs.txt", "cloud:x/"),
            entry("./l.txt", "cloud:x/"),
            entry("l.txt", " "),
            entry("l.txt", "cloud:x/") + &entry("l.txt", "cloud:y/"),
        ] {
            std::fs::write(
                dir.path().join("research.toml"),
                format!("[lab.archive]\n{entries}"),
            )
            .unwrap();
            let p = crate::config::Project::discover(dir.path(), None).unwrap();
            let err = Spec::new(&p).err().unwrap();
            assert!(
                err.contains("[[lab.archive.offloaded]]"),
                "{entries}: {err}"
            );
        }
        std::fs::write(
            dir.path().join("research.toml"),
            format!("[lab.archive]\n{}", entry("l.txt", "cloud:x/")),
        )
        .unwrap();
        let p = crate::config::Project::discover(dir.path(), None).unwrap();
        assert!(Spec::new(&p).is_ok());
    }

    #[test]
    fn resolves_links_relative_to_the_file() {
        let root = Path::new("/p");
        let there = |rel: &str, fragment: Option<&str>| Target::There {
            abs: root.join(rel),
            rel: Some(rel.into()),
            fragment: fragment.map(Into::into),
        };
        assert_eq!(
            resolve(root, "docs/a", "../b/c.md#Sec%20One"),
            there("docs/b/c.md", Some("Sec One"))
        );
        assert_eq!(
            resolve(root, "docs", "x%20y.md?raw=1"),
            there("docs/x y.md", None)
        );
        assert_eq!(resolve(root, "docs", "#top"), Target::Here("top".into()));
        for skipped in [
            "https://x.org/a.md",
            "mailto:a@b.c",
            "file:///x",
            "",
            "#",
            "  ",
        ] {
            assert_eq!(resolve(root, "docs", skipped), Target::Skip, "{skipped}");
        }
        assert!(matches!(
            resolve(root, "docs", "../../outside.md"),
            Target::There { rel: None, .. }
        ));
        assert_eq!(
            resolve(root, "docs", "/no/such/dir/a.md"),
            there("no/such/dir/a.md", None)
        );
    }
}
