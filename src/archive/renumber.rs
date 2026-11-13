//! `lab archive renumber`: gives a record a new id. It renames the file,
//! changes the id in the record's front matter and title heading, the id
//! lists of the other records and the links to the file, and lists the lines
//! it leaves for a person: links in sealed text and plain mentions of the id.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::Path;
use std::process::ExitCode;

use super::rules::{Seals, Target, line_ends_by, resolve};
use super::seal::Manifest;
use super::{Record, Spec, Texts, front, gitseal, index, markdown, reserve};
use crate::config::{Project, SealMode};

/// The front-matter fields whose values are record ids.
const ID_FIELDS: [&str; 5] = [
    "supersedes",
    "superseded_by",
    "decisions",
    "amends",
    "amended_by",
];

/// A character that continues an id or a file name, so `D042-a` and `D042.md`
/// are not the id `D042`.
fn id_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/')
}

/// A character that continues a word, for mentions in prose.
fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The byte offsets of `id` in `line` with no `edge` character on either side.
fn tokens(line: &str, id: &str, edge: fn(char) -> bool) -> Vec<usize> {
    line.match_indices(id)
        .map(|(i, _)| i)
        .filter(|&i| {
            !line[..i].chars().next_back().is_some_and(edge)
                && !line[i + id.len()..].chars().next().is_some_and(edge)
        })
        .collect()
}

fn replace_tokens(line: &str, old: &str, new: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut last = 0;
    for i in tokens(line, old, id_char) {
        out.push_str(&line[last..i]);
        out.push_str(new);
        last = i + old.len();
    }
    out.push_str(&line[last..]);
    out
}

/// The record that `old` names: a record path, else the one record with that id.
fn pick<'r>(records: &'r [Record], old: &str) -> Result<&'r Record, String> {
    if let Some(r) = records.iter().find(|r| r.path == old) {
        return Ok(r);
    }
    let by_path: BTreeMap<&str, &Record> = records
        .iter()
        .filter(|r| r.meta.id.as_deref() == Some(old))
        .map(|r| (r.path.as_str(), r))
        .collect();
    match by_path.len() {
        0 => Err(format!("no record has the id or path `{old}`")),
        1 => Ok(by_path.into_values().next().expect("one record")),
        _ => Err(format!(
            "{} records have the id {old}: {}; name the one to renumber by its path",
            by_path.len(),
            by_path.into_keys().collect::<Vec<_>>().join(", ")
        )),
    }
}

/// The rest of a file name after the id at its start: `-mine.md` for
/// `D003-mine.md`, but nothing for `D0031.md`, whose name holds another id.
fn after_id<'a>(name: &'a str, id: &str) -> Option<&'a str> {
    name.strip_prefix(id)
        .filter(|rest| !rest.starts_with(|c: char| c.is_alphanumeric()))
}

/// Whether line `line` (from 1) of `file` lies in sealed text. The end of
/// the sealed text is looked up once per file, since a git seal reads git.
fn sealed_line(
    seals: &Seals,
    ends: &mut BTreeMap<String, Option<usize>>,
    file: &str,
    bytes: &[u8],
    line: usize,
) -> Result<bool, String> {
    let end = match ends.get(file) {
        Some(end) => *end,
        None => {
            let end = seals.sealed_end(file, bytes)?;
            ends.insert(file.to_string(), end);
            end
        }
    };
    Ok(end.is_some_and(|end| line_ends_by(bytes, line, end)))
}

/// A destination written as `raw` that resolves to the renamed record, with
/// its file name and, when the title heading changed, its fragment changed.
/// `None` when the file name is written in a way this cannot change.
fn new_dest(
    raw: &str,
    names: (&str, &str),
    ids: (&str, &str),
    slugs: (&str, &str),
) -> Option<String> {
    let (path, fragment) = match raw.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (raw, None),
    };
    let (path, query) = match path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path, None),
    };
    let start = path.rfind('/').map_or(0, |i| i + 1);
    let segment = &path[start..];
    let segment = if names.0 == names.1 || path.is_empty() {
        segment.to_string()
    } else if let Some(rest) = after_id(segment, ids.0) {
        format!("{}{rest}", ids.1)
    } else if markdown::percent_decode(segment) == names.0 {
        names.1.to_string()
    } else {
        return None;
    };
    let mut out = format!("{}{segment}", &path[..start]);
    if let Some(q) = query {
        out.push('?');
        out.push_str(q);
    }
    if let Some(f) = fragment {
        out.push('#');
        if slugs.0 != slugs.1 && markdown::percent_decode(f).to_lowercase() == slugs.0 {
            out.push_str(slugs.1);
        } else {
            out.push_str(f);
        }
    }
    Some(out)
}

/// A new destination for the bytes `span` of line `line` (from 0).
struct Edit {
    line: usize,
    span: Range<usize>,
    dest: String,
}

/// The files to change, as lines, with what changed and what is left.
struct Plan {
    lines: BTreeMap<String, Vec<String>>,
    changed: BTreeSet<String>,
    log: Vec<String>,
    by_hand: Vec<String>,
}

impl Plan {
    fn lines(&mut self, root: &Path, path: &str) -> Result<Option<&mut Vec<String>>, String> {
        if !self.lines.contains_key(path) {
            let bytes = std::fs::read(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
            let Ok(text) = String::from_utf8(bytes) else {
                return Ok(None);
            };
            self.lines.insert(
                path.to_string(),
                text.split('\n').map(String::from).collect(),
            );
        }
        Ok(self.lines.get_mut(path))
    }
}

/// Rewrites the id lists of the front matter in `lines` (file lines from 0)
/// whose YAML ends before `body_line`. Only lines under an id field change.
fn rewrite_lists(lines: &mut [String], body_line: usize, old: &str, new: &str) -> Vec<usize> {
    let mut changed = Vec::new();
    let mut field = String::new();
    for (i, line) in lines
        .iter_mut()
        .enumerate()
        .take(body_line.saturating_sub(2))
        .skip(1)
    {
        if !line.starts_with([' ', '\t', '-', '#']) && !line.trim().is_empty() {
            field = line.split(':').next().unwrap_or("").to_string();
        }
        if ID_FIELDS.contains(&field.as_str()) {
            let value_from = if line.starts_with(field.as_str()) {
                field.len()
            } else {
                0
            };
            let next = format!(
                "{}{}",
                &line[..value_from],
                replace_tokens(&line[value_from..], old, new)
            );
            if next != *line {
                *line = next;
                changed.push(i);
            }
        }
    }
    changed
}

/// Renames `from` to `to`, both relative to `root`: with `git mv` when git
/// tracks it, so the rename is staged and the new file is no untracked one.
fn rename(root: &Path, from: &str, to: &str) -> Result<(), String> {
    if !crate::worktree::tracks(root, from)? {
        return std::fs::rename(root.join(from), root.join(to))
            .map_err(|e| format!("{from} -> {to}: {e}"));
    }
    let out = std::process::Command::new("git")
        .args(["mv", "--", from, to])
        .current_dir(root)
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| format!("git mv: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git mv {from} {to}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

pub fn renumber(
    project: &Project,
    config: Option<&Path>,
    old: &str,
    new: &str,
    dry_run: bool,
) -> Result<ExitCode, String> {
    let spec = Spec::new(project)?;
    let root = spec.root();
    let found = spec.walk(&[&spec.sealed, &spec.links, &spec.current])?;
    let (by_index, rest) = found.split_at(spec.indexes.len());
    let (sealed, links, current) = (&rest[0], &rest[1], &rest[2]);
    let mut texts = Texts::new();
    let records = spec.records(by_index, &mut texts)?;
    let record = pick(&records, old)?;
    let path = record.path.as_str();
    let old_id = record
        .meta
        .id
        .clone()
        .ok_or_else(|| format!("{path} has no id"))?;
    if new.is_empty() || new.contains(|c: char| c.is_whitespace() || c == '/' || c == '#') {
        return Err(format!("`{new}` is not an id"));
    }
    if new == old_id {
        return Err(format!("{path} already has the id {new}"));
    }
    if let Some(r) = records.iter().find(|r| r.meta.id.as_deref() == Some(new)) {
        return Err(format!(
            "{new} is taken: {} has it; take a free id from `lab archive next`",
            r.path
        ));
    }
    if reserve::elsewhere(&spec, config)?.iter().any(|i| i == new) {
        return Err(format!(
            "{new} is taken on the default branch or in another worktree; take a free id from `lab archive next`"
        ));
    }
    if let Some(e) = &record.invalid {
        return Err(format!(
            "{path}:{}: {}; correct the front matter first",
            e.line, e.message
        ));
    }
    let manifest = match spec.cfg.seal {
        SealMode::Git => None,
        _ => Some(Manifest::load(&spec.manifest)?),
    };
    let is_sealed = match &manifest {
        None => gitseal::AtRev::new(&spec, "HEAD").sealed(path)?.is_some(),
        Some(m) => m.files.contains_key(path),
    };
    if is_sealed {
        return Err(format!(
            "{path} is sealed, and a new name or heading would break its seal; renumber the \
             other record with the id {old_id} instead, or rename this one by hand under the \
             seal's rules"
        ));
    }
    let bytes = texts.bytes(&root.join(path))?;
    let split = front::split(&bytes);
    let explicit = split
        .yaml
        .and_then(|y| front::parse(y, &spec.cfg.statuses).ok())
        .is_some_and(|m| m.id.is_some());
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    let new_name = match after_id(name, &old_id) {
        Some(rest) => format!("{new}{rest}"),
        None if explicit => name.to_string(),
        None => {
            return Err(format!(
                "{path}: the id {old_id} comes from the path, but the file name does not start \
                 with it; rename the record by hand"
            ));
        }
    };
    let new_path = if dir.is_empty() {
        new_name.clone()
    } else {
        format!("{dir}/{new_name}")
    };
    if new_path != path {
        if root.join(&new_path).exists() {
            return Err(format!("{new_path} exists already"));
        }
        let ix = &spec.indexes[record.index];
        if !ix.files.matches(&new_path) {
            return Err(format!(
                "{new_path} would not be in index `{}`; choose an id that its `include` matches",
                ix.spec.name
            ));
        }
        if !explicit
            && index::derive_id(ix.spec, ix.id_from.as_ref(), &new_path).as_deref() != Some(new)
        {
            return Err(format!(
                "index `{}` would not derive the id {new} from {new_path}; set `id: {new}` in \
                 the front matter, or rename the record by hand",
                ix.spec.name
            ));
        }
    }

    let mut plan = Plan {
        lines: BTreeMap::new(),
        changed: BTreeSet::new(),
        log: Vec::new(),
        by_hand: Vec::new(),
    };
    let Some(own) = plan.lines(root, path)? else {
        return Err(format!("{path} is not UTF-8"));
    };
    // The title heading: the first level-1 heading of the body.
    let mut heading = None;
    let mut fence = None;
    for (i, line) in own.iter().enumerate().skip(split.body_line - 1) {
        let line = line.strip_suffix('\r').unwrap_or(line);
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
        if let Some((1, text)) = markdown::atx(line) {
            if tokens(text, &old_id, id_char).first() == Some(&0) {
                heading = Some((i, text.to_string()));
            }
            break;
        }
    }
    let slugs = match &heading {
        Some((_, text)) => (
            markdown::slug(&markdown::heading_text(text)),
            markdown::slug(&markdown::heading_text(&format!(
                "{new}{}",
                &text[old_id.len()..]
            ))),
        ),
        None => (String::new(), String::new()),
    };

    // Links to the record, and to its title heading from inside it.
    let seals = match &manifest {
        None => Seals::git(&spec),
        Some(m) => Seals::Manifest {
            sealed: sealed.iter().map(String::as_str).collect(),
            manifest: m,
        },
    };
    let mut ends = BTreeMap::new();
    let outputs: BTreeSet<&str> = spec.cfg.index.iter().map(|i| i.output.as_str()).collect();
    let files: BTreeSet<&str> = links
        .iter()
        .chain(current)
        .map(String::as_str)
        .chain(records.iter().map(|r| r.path.as_str()))
        .filter(|f| !outputs.contains(f))
        .collect();
    let mut edits: BTreeMap<&str, Vec<Edit>> = BTreeMap::new();
    for &file in &files {
        let doc = texts.doc(&root.join(file))?;
        let file_dir = file.rsplit_once('/').map_or("", |(d, _)| d);
        for link in &doc.links {
            let to_record = match resolve(root, file_dir, &link.dest) {
                Target::There { rel: Some(r), .. } => r == path,
                Target::Here(f) => {
                    file == path && !slugs.0.is_empty() && f.to_lowercase() == slugs.0
                }
                _ => false,
            };
            if !to_record {
                continue;
            }
            let at = format!("{file}:{}", link.line);
            let bytes = texts.bytes(&root.join(file))?;
            if sealed_line(&seals, &mut ends, file, &bytes, link.line)? {
                plan.by_hand.push(format!(
                    "{at}: sealed text links to `{}`; keep it with a [[lab.archive.kept_link]] \
                     entry",
                    link.dest
                ));
                continue;
            }
            let Some(lines) = plan.lines(root, file)? else {
                plan.by_hand.push(format!(
                    "{at}: links to `{}`, in a file that is not UTF-8",
                    link.dest
                ));
                continue;
            };
            let raw = lines[link.line - 1][link.span.clone()].to_string();
            match new_dest(
                &raw,
                (name, &new_name),
                (&old_id, new),
                (&slugs.0, &slugs.1),
            ) {
                Some(dest) if dest != raw => {
                    plan.log.push(format!("{at}: link `{raw}` -> `{dest}`"));
                    edits.entry(file).or_default().push(Edit {
                        line: link.line - 1,
                        span: link.span.clone(),
                        dest,
                    });
                }
                Some(_) => {}
                None => plan
                    .by_hand
                    .push(format!("{at}: links to `{raw}`, which this cannot change")),
            }
        }
    }
    for (file, mut list) in edits {
        // From the end of each line, so each span still holds.
        list.sort_by_key(|e| (e.line, std::cmp::Reverse(e.span.start)));
        let lines = plan.lines.get_mut(file).expect("read above");
        for e in list {
            lines[e.line].replace_range(e.span, &e.dest);
        }
        plan.changed.insert(file.to_string());
    }

    // The record's own id and heading.
    let own = plan.lines.get_mut(path).expect("read above");
    if explicit {
        for (i, line) in own
            .iter_mut()
            .enumerate()
            .take(split.body_line.saturating_sub(2))
            .skip(1)
        {
            if let Some(value) = line.strip_prefix("id:") {
                let next = format!("id:{}", replace_tokens(value, &old_id, new));
                if next != *line {
                    *line = next;
                    plan.log
                        .push(format!("{path}:{}: id {old_id} -> {new}", i + 1));
                }
            }
        }
    }
    if explicit {
        let text = own.join("\n");
        let id = front::split(text.as_bytes())
            .yaml
            .and_then(|y| front::parse(y, &spec.cfg.statuses).ok())
            .and_then(|m| m.id);
        if id.as_deref() != Some(new) {
            return Err(format!(
                "{path}: its front matter gives the id in a form this cannot change; change \
                 it to `id: {new}` by hand, then rename the file"
            ));
        }
    }
    if let Some((i, _)) = &heading {
        let line = &mut own[*i];
        let at = line
            .find(old_id.as_str())
            .expect("the heading starts with the id");
        line.replace_range(at..at + old_id.len(), new);
        plan.log.push(format!(
            "{path}:{}: heading `{}`",
            i + 1,
            line.trim_end_matches('\r')
        ));
    }
    plan.changed.insert(path.to_string());

    // The id lists of the other records, when the id is not shared here.
    let holders: BTreeSet<&str> = records
        .iter()
        .filter(|r| r.meta.id.as_deref() == Some(old_id.as_str()))
        .map(|r| r.path.as_str())
        .collect();
    let mut listed = BTreeSet::new();
    for r in &records {
        let m = &r.meta;
        let names = [
            &m.supersedes,
            &m.superseded_by,
            &m.decisions,
            &m.amends,
            &m.amended_by,
        ]
        .iter()
        .any(|list| list.contains(&old_id));
        if !names || r.path == path || !listed.insert(r.path.as_str()) {
            continue;
        }
        if holders.len() > 1 {
            plan.by_hand.push(format!(
                "{}: its front matter names {old_id}, which {} records have; change it by hand \
                 if it means {path}",
                r.path,
                holders.len()
            ));
            continue;
        }
        let bytes = texts.bytes(&root.join(&r.path))?;
        let body_line = front::split(&bytes).body_line;
        let Some(lines) = plan.lines(root, &r.path)? else {
            plan.by_hand.push(format!(
                "{}: its front matter names {old_id}, in a file that is not UTF-8",
                r.path
            ));
            continue;
        };
        let before = lines.clone();
        let changed = rewrite_lists(lines, body_line, &old_id, new);
        // Kept only when the front matter still parses and no id field names the old id.
        let text = lines.join("\n");
        let ok = front::split(text.as_bytes())
            .yaml
            .and_then(|y| front::parse(y, &spec.cfg.statuses).ok())
            .is_some_and(|m| {
                [
                    m.supersedes,
                    m.superseded_by,
                    m.decisions,
                    m.amends,
                    m.amended_by,
                ]
                .iter()
                .all(|list| !list.contains(&old_id))
            });
        if ok {
            for i in changed {
                plan.log.push(format!(
                    "{}:{}: front matter {old_id} -> {new}",
                    r.path,
                    i + 1
                ));
            }
            plan.changed.insert(r.path.clone());
        } else {
            *lines = before;
            plan.by_hand.push(format!(
                "{}: its front matter names {old_id} in a form this cannot change",
                r.path
            ));
        }
    }

    // Mentions of the old id that are left, in the files the links came from.
    for &file in &files {
        if file != path && holders.contains(file) {
            continue;
        }
        let shown = if file == path {
            new_path.as_str()
        } else {
            file
        };
        let Some(lines) = plan.lines(root, file)? else {
            continue;
        };
        let found: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| !tokens(line, &old_id, word_char).is_empty())
            .map(|(i, _)| i + 1)
            .collect();
        // Sealed text keeps its words: nobody changes it.
        let bytes = texts.bytes(&root.join(file))?;
        for line in found {
            if !sealed_line(&seals, &mut ends, file, &bytes, line)? {
                plan.by_hand
                    .push(format!("{shown}:{line}: mentions {old_id}"));
            }
        }
    }

    // Config entries that name the record by its path stop matching.
    if new_path != path {
        for a in spec.cfg.allow.iter().filter(|a| a.subject == path) {
            plan.by_hand.push(format!(
                "the config: [[lab.archive.allow]] entry for `{}` has the subject {path}; \
                 change it to {new_path}",
                a.rule
            ));
        }
        if spec.cfg.kept_link.iter().any(|k| k.file == path) {
            plan.by_hand.push(format!(
                "the config: [[lab.archive.kept_link]] entries have the file {path}; change it \
                 to {new_path}"
            ));
        }
    }

    if new_path != path {
        println!("renamed {path} -> {new_path}");
    }
    // The record's own lines under the name it gets.
    let own = format!("{path}:");
    for line in &plan.log {
        match line.strip_prefix(&own) {
            Some(rest) => println!("changed {new_path}:{rest}"),
            None => println!("changed {line}"),
        }
    }
    for line in &plan.by_hand {
        println!("by hand {line}");
    }
    if dry_run {
        println!("Dry run: nothing written.");
        return Ok(ExitCode::SUCCESS);
    }
    // The rename first: when it fails, nothing links to the new path yet.
    if new_path != path {
        rename(root, path, &new_path)?;
    }
    for file in &plan.changed {
        let text = plan.lines[file.as_str()].join("\n");
        if file == path {
            super::write_atomically(&root.join(&new_path), text.as_bytes())?;
        } else {
            super::write_atomically(&root.join(file), text.as_bytes())?;
        }
    }
    if spec.cfg.write_indexes {
        let found = spec.walk(&[])?;
        let records = spec.records(&found, &mut Texts::new())?;
        super::write_indexes(&spec, &records)?;
    }
    println!(
        "Renumbered {old_id} to {new}: {} file(s) changed, {} line(s) to check by hand.",
        plan.changed.len(),
        plan.by_hand.len()
    );
    Ok(ExitCode::SUCCESS)
}
