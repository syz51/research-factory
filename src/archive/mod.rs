//! `lab archive`: a research archive of Markdown records with optional YAML
//! front matter. It generates the indexes from the front matter, seals
//! append-only files, and checks ids, links, sizes and seals, so agents can
//! find a record by id or a few words instead of reading large files whole.

mod files;
mod front;
mod index;
mod markdown;
mod rules;
mod seal;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;

use regex::Regex;

use crate::config::{ArchiveConfig, IndexSpec, Project};
use crate::finding::Severity;
use files::FileSet;
use front::{Invalid, Meta};
use markdown::Doc;
use rules::{Problem, invalid_problem};
use seal::{Entry, Manifest, State};

/// One Markdown file that an index lists.
#[derive(Debug)]
pub struct Record {
    /// Position of its index in `[[lab.archive.index]]`.
    pub index: usize,
    /// Relative to the project root, `/`-separated.
    pub path: String,
    /// The front matter, with `id`, `title` and `date` derived where absent.
    pub meta: Meta,
    /// Why the front matter is invalid; `meta` then holds only derived values.
    pub invalid: Option<Invalid>,
}

struct Index<'p> {
    spec: &'p IndexSpec,
    files: FileSet,
    id_from: Option<Regex>,
}

/// `[lab.archive]`, validated and compiled.
pub struct Spec<'p> {
    project: &'p Project,
    cfg: &'p ArchiveConfig,
    indexes: Vec<Index<'p>>,
    sealed: FileSet,
    links: FileSet,
    current: FileSet,
    budgets: Vec<FileSet>,
    manifest: PathBuf,
}

impl<'p> Spec<'p> {
    pub fn new(project: &'p Project) -> Result<Self, String> {
        let cfg = project.lab.archive.as_ref().ok_or(
            "no [lab.archive] table in research.toml (`lab init` writes a commented example)",
        )?;
        let err = |what: &str, e: String| format!("[lab.archive] {what}: {e}");
        let unique =
            |values: &[String]| values.iter().collect::<BTreeSet<_>>().len() == values.len();
        if cfg.statuses.is_empty() || !unique(&cfg.statuses) {
            return Err("[lab.archive] `statuses` must be a non-empty list without repeats".into());
        }
        if let Some(s) = cfg
            .inactive_statuses
            .iter()
            .find(|s| !cfg.statuses.contains(s))
        {
            return Err(format!(
                "[lab.archive] inactive status `{s}` is not in `statuses`"
            ));
        }
        let names: Vec<String> = cfg.index.iter().map(|i| i.name.clone()).collect();
        let outputs: Vec<String> = cfg.index.iter().map(|i| i.output.clone()).collect();
        if names.iter().any(|n| n.trim().is_empty()) || !unique(&names) || !unique(&outputs) {
            return Err(
                "[lab.archive] every index needs a non-empty, unique `name` and a unique `output`"
                    .into(),
            );
        }
        let mut indexes = Vec::new();
        for spec in &cfg.index {
            let what = format!("index `{}`", spec.name);
            if spec.include.is_empty() {
                return Err(err(&what, "`include` must not be empty".into()));
            }
            let id_from = spec
                .id_from
                .as_deref()
                .map(|re| {
                    let re = Regex::new(re).map_err(|e| err(&what, format!("`id_from`: {e}")))?;
                    if re.captures_len() != 2 {
                        return Err(err(
                            &what,
                            "`id_from` needs exactly one capture group".into(),
                        ));
                    }
                    Ok(re)
                })
                .transpose()?;
            indexes.push(Index {
                spec,
                files: FileSet::new(&spec.include, &spec.exclude).map_err(|e| err(&what, e))?,
                id_from,
            });
        }
        let set = |what: &str, include: &[String], exclude: &[String]| {
            FileSet::new(include, exclude).map_err(|e| err(what, e))
        };
        Ok(Self {
            project,
            cfg,
            indexes,
            sealed: set("sealed", &cfg.sealed, &[])?,
            links: set("links", &cfg.links, &cfg.links_exclude)?,
            current: set("current_pages", &cfg.current_pages, &[])?,
            budgets: cfg
                .budget
                .iter()
                .map(|b| set("budget", std::slice::from_ref(&b.path), &[]))
                .collect::<Result<_, _>>()?,
            manifest: cfg
                .seal_manifest
                .as_deref()
                .map_or_else(|| project.default_seal_manifest(), |p| project.path(p)),
        })
    }

    fn root(&self) -> &Path {
        &self.project.root
    }

    fn manifest_display(&self) -> String {
        self.manifest
            .strip_prefix(self.root())
            .unwrap_or(&self.manifest)
            .display()
            .to_string()
    }

    /// Walks the tree once for the index sets followed by `extra`.
    fn walk(&self, extra: &[&FileSet]) -> Result<Vec<Vec<String>>, String> {
        let mut sets: Vec<&FileSet> = self.indexes.iter().map(|i| &i.files).collect();
        sets.extend_from_slice(extra);
        files::walk(self.root(), &sets)
    }

    /// The records of every index, from the walk's first `indexes.len()` lists.
    /// An index output is never a record.
    fn records(&self, found: &[Vec<String>], texts: &mut Texts) -> Result<Vec<Record>, String> {
        let outputs: BTreeSet<&str> = self.cfg.index.iter().map(|i| i.output.as_str()).collect();
        let mut records = Vec::new();
        for (i, (ix, paths)) in self.indexes.iter().zip(found).enumerate() {
            for path in paths.iter().filter(|p| !outputs.contains(p.as_str())) {
                let bytes = texts.bytes(&self.root().join(path))?;
                let split = front::split(&bytes);
                let parsed = match (split.yaml, split.unterminated) {
                    (Some(yaml), _) => front::parse(yaml, &self.cfg.statuses),
                    (None, true) => Err(Invalid {
                        line: 1,
                        message: "front matter starts with `---` but no closing `---` line follows"
                            .into(),
                    }),
                    (None, false) => Ok(Meta::default()),
                };
                let (mut meta, invalid) = match parsed {
                    Ok(meta) => (meta, None),
                    Err(e) => (Meta::default(), Some(e)),
                };
                if meta.id.is_none() {
                    meta.id = index::derive_id(ix.spec, ix.id_from.as_ref(), path);
                }
                if meta.title.is_none() {
                    let heading = texts.doc(&self.root().join(path))?.title.clone();
                    meta.title = heading.map(|t| without_id(&t, meta.id.as_deref()).to_string());
                }
                if meta.date.is_none() {
                    meta.date = front::date_in(path);
                }
                records.push(Record {
                    index: i,
                    path: path.clone(),
                    meta,
                    invalid,
                });
            }
        }
        Ok(records)
    }

    /// Index `i`'s records in its order.
    fn rows<'r>(&self, records: &'r [Record], i: usize) -> Vec<&'r Record> {
        let mut rows: Vec<&Record> = records.iter().filter(|r| r.index == i).collect();
        index::sort(&mut rows, self.indexes[i].spec.sort);
        rows
    }

    fn render(&self, records: &[Record], i: usize) -> String {
        index::render(
            self.indexes[i].spec,
            &self.cfg.statuses,
            &self.rows(records, i),
        )
    }

    fn inactive(&self, r: &Record) -> bool {
        r.meta
            .status
            .as_ref()
            .is_some_and(|s| self.cfg.inactive_statuses.contains(s))
    }
}

/// File contents and parsed Markdown, read once per run.
struct Texts {
    bytes: HashMap<PathBuf, Rc<[u8]>>,
    docs: HashMap<PathBuf, Rc<Doc>>,
}

impl Texts {
    fn new() -> Self {
        Self {
            bytes: HashMap::new(),
            docs: HashMap::new(),
        }
    }

    fn bytes(&mut self, path: &Path) -> Result<Rc<[u8]>, String> {
        if let Some(b) = self.bytes.get(path) {
            return Ok(b.clone());
        }
        let b: Rc<[u8]> = std::fs::read(path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .into();
        self.bytes.insert(path.to_path_buf(), b.clone());
        Ok(b)
    }

    /// The Markdown after the front matter, with file line numbers.
    fn doc(&mut self, path: &Path) -> Result<Rc<Doc>, String> {
        if let Some(d) = self.docs.get(path) {
            return Ok(d.clone());
        }
        let bytes = self.bytes(path)?;
        let split = front::split(&bytes);
        let doc = Rc::new(markdown::parse(
            &String::from_utf8_lossy(split.body),
            split.body_line,
        ));
        self.docs.insert(path.to_path_buf(), doc.clone());
        Ok(doc)
    }
}

/// Writes through a temporary file in the same directory, then renames it.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// `Ok(None)` when the file does not exist.
fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// false when a record's front matter is invalid.
fn write_indexes(spec: &Spec, records: &[Record]) -> Result<bool, String> {
    let invalid: Vec<Problem> = records.iter().filter_map(invalid_problem).collect();
    if !invalid.is_empty() {
        for p in &invalid {
            println!("{}", p.text());
        }
        println!("No index written: fix the front matter above, then run this again.");
        return Ok(false);
    }
    for (i, ix) in spec.indexes.iter().enumerate() {
        let text = spec.render(records, i);
        let path = spec.root().join(&ix.spec.output);
        if read_optional(&path)?.as_deref() == Some(text.as_bytes()) {
            println!("unchanged {}", ix.spec.output);
        } else {
            write_atomically(&path, text.as_bytes())?;
            println!("wrote {}", ix.spec.output);
        }
    }
    Ok(true)
}

/// Seals new sealed files and records appends. Returns false when a sealed
/// file was edited or has disappeared.
fn seal_files(spec: &Spec, sealed: &[String]) -> Result<bool, String> {
    let mut manifest = Manifest::load(&spec.manifest)?;
    let mut changed = false;
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for path in sealed {
        let bytes = std::fs::read(spec.root().join(path)).map_err(|e| format!("{path}: {e}"))?;
        let body = front::split(&bytes).body;
        let state = Entry::state(manifest.files.get(path), body);
        let label = match state {
            State::New => "newly sealed",
            State::Appended => "appended",
            State::Unchanged => "unchanged",
            State::Edited => "edited",
        };
        *counts.entry(label).or_default() += 1;
        match state {
            State::New | State::Appended => {
                manifest.files.insert(path.clone(), Entry::of(body));
                changed = true;
                println!(
                    "{} {path}",
                    if state == State::New {
                        "sealed"
                    } else {
                        "appended"
                    }
                );
            }
            State::Unchanged => {}
            State::Edited => println!(
                "edited {path} — the text it was sealed with changed; restore it and append a dated \
                 correction instead, or accept the edit with \
                 `lab archive seal --accept {path} --note \"<why>\"`"
            ),
        }
    }
    let missing: Vec<&String> = manifest
        .files
        .keys()
        .filter(|p| !spec.root().join(p).is_file())
        .collect();
    for p in &missing {
        println!("missing {p} — sealed but no longer exists; restore it (its seal is kept)");
    }
    let summary: Vec<String> = counts.iter().map(|(k, n)| format!("{n} {k}")).collect();
    println!(
        "{} sealed file(s){}{}",
        sealed.len(),
        if summary.is_empty() { "" } else { ": " },
        summary.join(", ")
    );
    if changed {
        manifest.save(&spec.manifest)?;
        println!("wrote {}", spec.manifest_display());
    }
    Ok(!counts.contains_key("edited") && missing.is_empty())
}

fn exit(ok: bool) -> ExitCode {
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// `lab archive index`.
pub fn index(project: &Project) -> Result<ExitCode, String> {
    let spec = Spec::new(project)?;
    let found = spec.walk(&[])?;
    let records = spec.records(&found, &mut Texts::new())?;
    Ok(exit(write_indexes(&spec, &records)?))
}

/// `lab archive seal`, or with `accept` the re-sealing of one edited file.
pub fn seal(project: &Project, accept: Option<(&str, &str)>) -> Result<ExitCode, String> {
    let spec = Spec::new(project)?;
    if let Some((path, note)) = accept {
        return accept_edit(&spec, path, note);
    }
    let found = files::walk(spec.root(), &[&spec.sealed])?;
    Ok(exit(seal_files(&spec, &found[0])?))
}

fn accept_edit(spec: &Spec, path: &str, note: &str) -> Result<ExitCode, String> {
    if note.trim().is_empty() {
        return Err("accepting an edit needs a note saying why".into());
    }
    let mut manifest = Manifest::load(&spec.manifest)?;
    let key = manifest_key(spec, &manifest, path).ok_or_else(|| {
        format!(
            "`{path}` is not in {}; `lab archive seal` seals new files without --accept",
            spec.manifest_display()
        )
    })?;
    let bytes = std::fs::read(spec.root().join(&key)).map_err(|e| format!("{key}: {e}"))?;
    let new = Entry::of(front::split(&bytes).body);
    let old = manifest.files[&key].clone();
    if new == old {
        println!("{key} is unchanged since it was sealed; nothing to accept");
        return Ok(ExitCode::SUCCESS);
    }
    manifest.history.push(seal::Change {
        at: crate::finding::now(),
        path: key.clone(),
        old_sha256: old.sha256,
        new_sha256: new.sha256.clone(),
        note: note.trim().into(),
    });
    manifest.files.insert(key.clone(), new);
    manifest.save(&spec.manifest)?;
    println!(
        "accepted the edit to {key}; recorded in {}",
        spec.manifest_display()
    );
    Ok(ExitCode::SUCCESS)
}

/// The manifest's key for `path`, given relative to the project root or to
/// the current directory.
fn manifest_key(spec: &Spec, manifest: &Manifest, path: &str) -> Option<String> {
    let plain = path.strip_prefix("./").unwrap_or(path);
    if manifest.files.contains_key(plain) {
        return Some(plain.into());
    }
    let abs = std::env::current_dir()
        .ok()?
        .join(path)
        .canonicalize()
        .ok()?;
    let root = spec.root().canonicalize().ok()?;
    let rel = abs.strip_prefix(root).ok()?;
    let key = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    manifest.files.contains_key(&key).then_some(key)
}

/// `lab archive update`: `index`, then `seal`.
pub fn update(project: &Project) -> Result<ExitCode, String> {
    let spec = Spec::new(project)?;
    let mut found = spec.walk(&[&spec.sealed])?;
    let sealed = found.pop().unwrap_or_default();
    let records = spec.records(&found, &mut Texts::new())?;
    let indexed = write_indexes(&spec, &records)?;
    let sealed = seal_files(&spec, &sealed)?;
    Ok(exit(indexed && sealed))
}

/// `lab archive check`: every rule, read-only.
pub fn check(project: &Project, jsonl: bool) -> Result<ExitCode, String> {
    let spec = Spec::new(project)?;
    let problems = rules::problems(&spec)?;
    let errors = problems
        .iter()
        .filter(|p| p.severity == Severity::Error)
        .count();
    if jsonl {
        for p in &problems {
            println!(
                "{}",
                serde_json::to_string(&p.finding()).map_err(|e| e.to_string())?
            );
        }
    } else {
        for p in &problems {
            println!("{}", p.text());
        }
        println!(
            "{errors} error(s), {} warning(s) in the archive.",
            problems.len() - errors
        );
    }
    Ok(exit(errors == 0))
}
/// `lab archive find`.
pub fn find(project: &Project, terms: &[String], limit: usize) -> Result<ExitCode, String> {
    let spec = Spec::new(project)?;
    let found = spec.walk(&[])?;
    let records = spec.records(&found, &mut Texts::new())?;
    let ordered: Vec<&Record> = (0..spec.indexes.len())
        .flat_map(|i| spec.rows(&records, i))
        .collect();
    let terms: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();
    let mut hits = Vec::new();
    for (pos, r) in unique_refs(&ordered).into_iter().enumerate() {
        let m = &r.meta;
        let fields: Vec<String> = [&m.id, &m.title, &m.summary]
            .into_iter()
            .flatten()
            .chain(&m.aliases)
            .chain([&r.path])
            .map(|f| f.to_lowercase())
            .collect();
        if !terms.iter().all(|t| fields.iter().any(|f| f.contains(t))) {
            continue;
        }
        let id = m.id.as_deref().map(str::to_lowercase).unwrap_or_default();
        let rank = if terms.contains(&id) {
            0
        } else if !id.is_empty() && terms.iter().any(|t| id.starts_with(t.as_str())) {
            1
        } else {
            2
        };
        hits.push((rank, pos, r));
    }
    hits.sort_by_key(|&(rank, pos, _)| (rank, pos));
    if hits.is_empty() {
        let indexes: Vec<String> = spec
            .cfg
            .index
            .iter()
            .map(|i| format!("{} ({})", i.name, i.output))
            .collect();
        eprintln!(
            "No record matches all of: {}. Searched ids, titles, summaries, aliases and paths in the indexes {}.",
            terms.join(" "),
            indexes.join(", ")
        );
        return Ok(ExitCode::from(1));
    }
    for (_, _, r) in hits.iter().take(limit) {
        let m = &r.meta;
        let or_dash = |v: &Option<String>| v.clone().unwrap_or_else(|| "-".into());
        println!(
            "{}\t{}\t{}\t{}\t{}",
            or_dash(&m.id),
            or_dash(&m.date),
            or_dash(&m.status),
            r.path,
            or_dash(&m.title)
        );
    }
    if hits.len() > limit {
        eprintln!(
            "{} more match(es); add a word or pass --limit {}",
            hits.len() - limit,
            hits.len()
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn unique_refs<'r>(records: &[&'r Record]) -> Vec<&'r Record> {
    let mut seen = BTreeSet::new();
    records
        .iter()
        .copied()
        .filter(|r| seen.insert(r.path.as_str()))
        .collect()
}

/// `lab archive next`.
pub fn next(project: &Project, prefix: &str, width: Option<usize>) -> Result<ExitCode, String> {
    let spec = Spec::new(project)?;
    let found = spec.walk(&[])?;
    let records = spec.records(&found, &mut Texts::new())?;
    let ids = records.iter().filter_map(|r| r.meta.id.as_deref());
    match index::next_id(ids, prefix, width) {
        Some(id) => {
            println!("{id}");
            Ok(ExitCode::SUCCESS)
        }
        None => {
            eprintln!(
                "No record id starts with `{prefix}` followed by a number. Start a new series with `lab archive next {prefix} --width 3`."
            );
            Ok(ExitCode::from(1))
        }
    }
}

/// A heading-derived title without a leading copy of the record's id, as in
/// `# H158 — Personal details moved`.
fn without_id<'t>(title: &'t str, id: Option<&str>) -> &'t str {
    let Some(rest) = id.and_then(|id| title.strip_prefix(id)) else {
        return title;
    };
    match rest
        .trim_start()
        .strip_prefix(['—', '–', '·', ':', '-'])
        .map(str::trim_start)
    {
        Some(stripped) if !stripped.is_empty() => stripped,
        _ => title,
    }
}

#[cfg(test)]
mod tests {
    use super::without_id;

    #[test]
    fn heading_titles_drop_a_leading_id() {
        assert_eq!(
            without_id("H158 — Personal details moved", Some("H158")),
            "Personal details moved"
        );
        assert_eq!(without_id("H12 · Pilot", Some("H12")), "Pilot");
        assert_eq!(without_id("H1: Scope", Some("H1")), "Scope");
        // Another id, no separator, or nothing after it: unchanged.
        assert_eq!(without_id("H158 — Moved", Some("H15")), "H158 — Moved");
        assert_eq!(without_id("H15 plan", Some("H15")), "H15 plan");
        assert_eq!(without_id("H15 —", Some("H15")), "H15 —");
        assert_eq!(without_id("Initial topic", None), "Initial topic");
    }
}
