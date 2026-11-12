//! The git seal (`[lab.archive] seal = "git"`): git history proves that sealed
//! text did not change, so no manifest is written or read.
//!
//! A file is sealed from the first commit in which it matches `sealed` or its
//! status is one of `sealed_statuses`. In each later commit, in the staged
//! change and in the working tree, its body must start with the body of the
//! version before; front matter may change. A status in `terminal_statuses`
//! never changes. An `[[lab.archive.accepted_edit]]` entry that names the path
//! and the commit is the only way to keep an edit.
//!
//! The check reads each commit once: it stores the last commit whose history
//! was clean in `<state_dir>/lab/archive-git-seal.json` and starts there next
//! time. A missing, unreadable or unfitting cache means a full check.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde::{Deserialize, Serialize};

use super::rules::Problem;
use super::{Record, Spec, front, seal};
use crate::finding::Severity;

const SCHEMA: &str = "rf.archive-git-seal/1";

#[derive(Serialize, Deserialize)]
struct Cache {
    schema: String,
    /// Every commit up to this one was checked and clean.
    commit: String,
    /// Hash of the settings the check depends on.
    key: String,
    /// Records sealed by status up to `commit`.
    status_sealed: BTreeSet<String>,
}

fn command(root: &Path) -> Command {
    let mut c = Command::new("git");
    c.current_dir(root).env("LC_ALL", "C");
    c
}

/// Runs git in `root`, with `input` on stdin.
fn run(root: &Path, args: &[&str], input: &[u8]) -> Result<std::process::Output, String> {
    let mut child = command(root)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git: {e}"))?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let input = input.to_vec();
    // Written from another thread, so a full stdout pipe cannot block both sides.
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let out = child
        .wait_with_output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    // git may exit before it reads all of its input.
    let _ = writer.join().expect("the writer thread does not panic");
    Ok(out)
}

/// The stdout of a git command that must succeed.
fn output(root: &Path, args: &[&str], input: &[u8]) -> Result<Vec<u8>, String> {
    let out = run(root, args, input)?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// The commit `name` resolves to, `None` when it names no commit.
fn commit(root: &Path, name: &str) -> Result<Option<String>, String> {
    let out = run(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{name}^{{commit}}"),
        ],
        b"",
    )?;
    Ok(out
        .status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()))
}

/// The project root relative to the top of the work tree (empty, or ending
/// in `/`); `None` outside a git work tree or without git.
fn prefix(root: &Path) -> Option<String> {
    let out = run(root, &["rev-parse", "--show-prefix"], b"").ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The record ids in the tree of `rev`, derived as `Spec::records` derives
/// the work tree's: one record per index that lists a file. Empty outside a
/// git work tree; `None` when `rev` names no commit, such as a remote branch
/// that a prune removed.
pub(super) fn ids_at(spec: &Spec, rev: &str) -> Result<Option<Vec<String>>, String> {
    let root = spec.root();
    let Some(prefix) = prefix(root) else {
        return Ok(Some(Vec::new()));
    };
    let Some(rev) = commit(root, rev)? else {
        return Ok(None);
    };
    let outputs: BTreeSet<&str> = spec.cfg.index.iter().map(|i| i.output.as_str()).collect();
    let tree = output(
        root,
        &["ls-tree", "-r", "-z", "--name-only", "--full-tree", &rev],
        b"",
    )?;
    let mut blobs = Blobs::open(root)?;
    let mut ids = Vec::new();
    for full in tree.split(|&b| b == 0).filter(|p| !p.is_empty()) {
        let full = String::from_utf8_lossy(full);
        let Some(path) = full.strip_prefix(prefix.as_str()) else {
            continue;
        };
        if outputs.contains(path) || !spec.indexes.iter().any(|i| i.files.matches(path)) {
            continue;
        }
        if let Some(bytes) = blobs.get(&format!("{rev}:{full}"))? {
            ids.extend(blob_ids(spec, path, &bytes)?);
        }
    }
    Ok(Some(ids))
}

/// The ids that the indexes listing `path` derive for a file holding `bytes`.
fn blob_ids(spec: &Spec, path: &str, bytes: &[u8]) -> Result<Vec<String>, String> {
    let mut ids = Vec::new();
    for ix in spec.indexes.iter().filter(|i| i.files.matches(path)) {
        let (meta, _) = spec.meta(Some(ix), path, bytes, || Ok(None))?;
        ids.extend(meta.id);
    }
    Ok(ids)
}

/// `id-taken-on-default-branch`: a record of the work tree whose id a record
/// added on the default branch since the merge base has, at a path the work
/// tree lacks. After a rebase or merge both would be there, a `duplicate-id`.
/// A record that had the id at the merge base is left alone: the default
/// branch moved or copied it. Nothing outside git, without a default branch
/// or a common commit, or when `HEAD` contains the default branch.
pub(super) fn taken_on_default(spec: &Spec, records: &[&Record]) -> Result<Vec<Problem>, String> {
    let root = spec.root();
    let Some(prefix) = prefix(root) else {
        return Ok(Vec::new());
    };
    let Some(branch) = crate::worktree::default_branch(root)? else {
        return Ok(Vec::new());
    };
    let Some(tip) = commit(root, &branch)? else {
        eprintln!("note: rule id-taken-on-default-branch skipped: `{branch}` names no commit");
        return Ok(Vec::new());
    };
    let Some(base) = crate::worktree::merge_base(root, &tip)? else {
        return Ok(Vec::new());
    };
    if base == tip {
        return Ok(Vec::new());
    }
    let added = output(
        root,
        &[
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            "--diff-filter=A",
            "--relative",
            &base,
            &tip,
            "--",
        ],
        b"",
    )?;
    let outputs: BTreeSet<&str> = spec.cfg.index.iter().map(|i| i.output.as_str()).collect();
    let mut blobs = Blobs::open(root)?;
    // Each id of a record added there, with its path.
    let mut theirs: Vec<(String, String)> = Vec::new();
    for path in added.split(|&b| b == 0).filter(|p| !p.is_empty()) {
        let path = String::from_utf8_lossy(path).into_owned();
        if outputs.contains(path.as_str())
            || !spec.indexes.iter().any(|i| i.files.matches(&path))
            || root.join(&path).exists()
        {
            continue;
        }
        if let Some(bytes) = blobs.get(&format!("{tip}:{prefix}{path}"))? {
            for id in blob_ids(spec, &path, &bytes)? {
                theirs.push((id, path.clone()));
            }
        }
    }
    let mut out = Vec::new();
    for r in records {
        let Some(id) = r.meta.id.as_deref() else {
            continue;
        };
        let Some((_, other)) = theirs.iter().find(|(i, _)| i == id) else {
            continue;
        };
        let at_base = match blobs.get(&format!("{base}:{prefix}{}", r.path))? {
            Some(bytes) => blob_ids(spec, &r.path, &bytes)?,
            None => Vec::new(),
        };
        if at_base.iter().any(|i| i == id) {
            continue;
        }
        let mut p = Problem::new(
            "id-taken-on-default-branch",
            Severity::Warn,
            &r.path,
            format!(
                "has id {id}, which {other} also has on {branch}; take a new id from \
                 `lab archive next <prefix>` and give it to this record before the merge \
                 (`lab archive renumber {} <new id>` does it when the record is not sealed)",
                r.path
            ),
        );
        p.data = Some(serde_json::json!({"id": id, "other": other, "branch": branch}));
        out.push(p);
    }
    Ok(out)
}

/// Reads blobs through one `git cat-file --batch` process.
struct Blobs {
    child: Child,
    stdin: ChildStdin,
    out: BufReader<ChildStdout>,
}

impl Blobs {
    fn open(root: &Path) -> Result<Self, String> {
        let mut child = command(root)
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("git cat-file: {e}"))?;
        Ok(Self {
            stdin: child.stdin.take().expect("stdin is piped"),
            out: BufReader::new(child.stdout.take().expect("stdout is piped")),
            child,
        })
    }

    /// The object `name` (a hash, or `HEAD:path`); `None` when it does not exist.
    fn get(&mut self, name: &str) -> Result<Option<Vec<u8>>, String> {
        let err = |e: std::io::Error| format!("git cat-file {name}: {e}");
        writeln!(self.stdin, "{name}").map_err(err)?;
        self.stdin.flush().map_err(err)?;
        let mut header = String::new();
        self.out.read_line(&mut header).map_err(err)?;
        let header = header.trim_end();
        if header.ends_with(" missing") {
            return Ok(None);
        }
        let size: usize = header
            .rsplit(' ')
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("git cat-file {name}: unexpected answer `{header}`"))?;
        // The content is followed by one newline.
        let mut bytes = vec![0; size + 1];
        self.out.read_exact(&mut bytes).map_err(err)?;
        bytes.pop();
        Ok(Some(bytes))
    }
}

impl Drop for Blobs {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One changed file of `git diff-tree` or `git diff-index` raw output.
struct Change {
    /// The commit that made the change; empty for the index and the work tree.
    commit: String,
    status: u8,
    old: String,
    /// All zeros when the new content is only in the work tree.
    new: String,
    /// Relative to the project root.
    path: String,
}

/// Parses `--raw -z` output: commit hashes, and `:modes hashes status` each
/// followed by its path. Paths outside `prefix` are dropped.
fn changes(raw: &[u8], prefix: &str) -> Result<Vec<Change>, String> {
    let mut out = Vec::new();
    let mut commit = String::new();
    let mut tokens = raw.split(|&b| b == 0).filter(|t| !t.is_empty());
    while let Some(token) = tokens.next() {
        let token = String::from_utf8_lossy(token);
        let Some(entry) = token.strip_prefix(':') else {
            commit = token.into_owned();
            continue;
        };
        let fields: Vec<&str> = entry.split(' ').collect();
        let (Some(path), [_, _, old, new, status]) = (tokens.next(), fields.as_slice()) else {
            return Err(format!("git printed an unexpected change line: `{token}`"));
        };
        let path = String::from_utf8_lossy(path);
        if let Some(rel) = path.strip_prefix(prefix) {
            out.push(Change {
                commit: commit.clone(),
                status: status.as_bytes()[0],
                old: (*old).into(),
                new: (*new).into(),
                path: rel.into(),
            });
        }
    }
    Ok(out)
}

/// Where a change was made, and how its findings name it.
#[derive(Clone, Copy, PartialEq)]
enum Place {
    Commit,
    Staged,
    WorkTree,
}

struct Checker<'s, 'p> {
    spec: &'s Spec<'p>,
    blobs: Blobs,
    terminal: Vec<String>,
    /// Records sealed by status in a version checked so far: they stay
    /// sealed when a later commit changes the status.
    status_sealed: BTreeSet<String>,
    /// `(rule, path)` of each finding, so the work tree does not repeat the
    /// staged change.
    seen: BTreeSet<(&'static str, String)>,
    out: Vec<Problem>,
}

impl Checker<'_, '_> {
    /// Whether an `[[lab.archive.accepted_edit]]` entry keeps the edit of
    /// `path` in `commit`: it names the commit, or `commit` is a merge that
    /// brings in the commit it names (a merge is compared with its first
    /// parent, so it shows the edit again).
    fn accepted(&self, path: &str, commit: &str) -> Result<bool, String> {
        let root = self.spec.root();
        let mut named = Vec::new();
        for a in self
            .spec
            .cfg
            .accepted_edit
            .iter()
            .filter(|a| a.path == path)
        {
            let wanted = a.commit.to_ascii_lowercase();
            if commit.starts_with(&wanted) {
                return Ok(true);
            }
            named.push(wanted);
        }
        if named.is_empty()
            || !run(
                root,
                &["rev-parse", "-q", "--verify", &format!("{commit}^2")],
                b"",
            )?
            .status
            .success()
        {
            return Ok(false);
        }
        let first = format!("{commit}^1");
        let ancestor = |a: &str, b: &str| -> Result<bool, String> {
            Ok(run(root, &["merge-base", "--is-ancestor", a, b], b"")?
                .status
                .success())
        };
        for c in &named {
            if ancestor(c, commit)? && !ancestor(c, &first)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn check(&mut self, c: &Change, place: Place) -> Result<(), String> {
        let spec = self.spec;
        let cfg = spec.cfg;
        let glob = spec.sealed.matches(&c.path);
        let record = spec.indexes.iter().any(|i| i.files.matches(&c.path));
        // An added file has no version before it.
        if !(glob || record) || c.status == b'A' {
            return Ok(());
        }
        let Some(old) = self.blobs.get(&c.old)? else {
            return Err(format!("git has no object {} for {}", c.old, c.path));
        };
        let old_status = front::status_of(&old);
        if old_status
            .as_ref()
            .is_some_and(|s| cfg.sealed_statuses.contains(s))
        {
            self.status_sealed.insert(c.path.clone());
        }
        let sealed = glob || self.status_sealed.contains(&c.path);
        if place == Place::Commit && self.accepted(&c.path, &c.commit)? {
            return Ok(());
        }
        let at = match place {
            Place::Commit => format!("commit {}", &c.commit[..c.commit.len().min(10)]),
            Place::Staged => "the staged change".into(),
            Place::WorkTree => "the working tree".into(),
        };
        let way_out = match place {
            Place::Commit => {
                "accept the edit with a [[lab.archive.accepted_edit]] entry that names this \
                 path, the commit and a reason"
            }
            _ => "commit the edit and accept it with a [[lab.archive.accepted_edit]] entry",
        };
        let mut report = |rule: &'static str, message: String| {
            if place == Place::Commit || self.seen.insert((rule, c.path.clone())) {
                self.out
                    .push(Problem::new(rule, Severity::Error, &c.path, message));
            }
        };
        if c.status == b'D' {
            if sealed {
                report(
                    "sealed-missing",
                    format!("was sealed in git and deleted in {at}; restore it, or {way_out}"),
                );
            }
            return Ok(());
        }
        let new = if c.new.bytes().all(|b| b == b'0') {
            std::fs::read(spec.root().join(&c.path)).map_err(|e| format!("{}: {e}", c.path))?
        } else {
            self.blobs
                .get(&c.new)?
                .ok_or_else(|| format!("git has no object {} for {}", c.new, c.path))?
        };
        if sealed && !front::sealed_body(&new).starts_with(front::sealed_body(&old)) {
            report(
                "sealed-edited",
                format!(
                    "its sealed text was changed in {at}; restore the text and append a dated \
                     correction, or {way_out}"
                ),
            );
        }
        if record
            && let Some(was) = old_status.filter(|s| self.terminal.contains(s))
            && front::status_of(&new).as_ref() != Some(&was)
        {
            report(
                "status-reversed",
                format!(
                    "its status `{was}` was changed in {at}; `{was}` is final: restore it and \
                     write a new record, or {way_out}"
                ),
            );
        }
        Ok(())
    }
}

/// The statuses that never change: `terminal_statuses`, by default
/// `superseded` and `withdrawn` where the project has them.
fn terminal(spec: &Spec) -> Vec<String> {
    spec.cfg.terminal_statuses.clone().unwrap_or_else(|| {
        ["superseded", "withdrawn"]
            .into_iter()
            .filter(|s| spec.cfg.statuses.iter().any(|x| x == s))
            .map(String::from)
            .collect()
    })
}

/// A hash of every setting that decides what the history check reports.
fn key(spec: &Spec, prefix: &str, baseline: Option<&str>) -> String {
    let cfg = spec.cfg;
    let indexes: Vec<_> = cfg.index.iter().map(|i| (&i.include, &i.exclude)).collect();
    let accepted: Vec<_> = cfg
        .accepted_edit
        .iter()
        .map(|a| (&a.path, &a.commit))
        .collect();
    seal::sha256(
        format!(
            "{:?}",
            (
                prefix,
                baseline,
                &cfg.sealed,
                &cfg.sealed_statuses,
                terminal(spec),
                indexes,
                accepted
            )
        )
        .as_bytes(),
    )
}

/// The problems of the git seal: in the commits after the baseline, in the
/// staged change and in the working tree.
pub(super) fn problems(spec: &Spec) -> Result<Vec<Problem>, String> {
    let root = spec.root();
    let Some(prefix) = prefix(root) else {
        return Ok(vec![Problem::new(
            "seal-unverified",
            Severity::Error,
            crate::config::CONFIG_FILE,
            "[lab.archive] seal = \"git\" needs a git repository, and the project is not in \
             one (or git is not installed); nothing proves that sealed text is unchanged"
                .into(),
        )]);
    };
    // Without a commit, no file is sealed yet.
    let Some(head) = commit(root, "HEAD")? else {
        return Ok(Vec::new());
    };
    let baseline = match &spec.cfg.seal_baseline {
        None => None,
        Some(b) => Some(commit(root, b)?.ok_or_else(|| {
            format!("[lab.archive] seal_baseline `{b}` is not a commit in this repository")
        })?),
    };
    let key = key(spec, &prefix, baseline.as_deref());
    let cache_path = spec.project.git_seal_cache();
    let cached = std::fs::read(&cache_path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Cache>(&b).ok())
        .filter(|c| c.schema == SCHEMA && c.key == key);
    let (verified, status_sealed) = match cached {
        Some(c)
            if run(
                root,
                &["merge-base", "--is-ancestor", &c.commit, &head],
                b"",
            )?
            .status
            .success() =>
        {
            (Some(c.commit), c.status_sealed)
        }
        _ => (None, BTreeSet::new()),
    };
    let mut checker = Checker {
        spec,
        blobs: Blobs::open(root)?,
        terminal: terminal(spec),
        status_sealed,
        seen: BTreeSet::new(),
        out: Vec::new(),
    };
    if verified.as_deref() != Some(head.as_str()) {
        let range = match verified.as_ref().or(baseline.as_ref()) {
            Some(from) => format!("{from}..{head}"),
            None => head.clone(),
        };
        let commits = output(
            root,
            &["rev-list", "--reverse", "--topo-order", &range],
            b"",
        )?;
        let raw = output(
            root,
            &[
                "diff-tree",
                "--stdin",
                // A merge is compared with its first parent: the line it joins.
                "--diff-merges=first-parent",
                "-r",
                "--raw",
                "--no-abbrev",
                "--no-renames",
                "-z",
            ],
            &commits,
        )?;
        for c in changes(&raw, &prefix)? {
            checker.check(&c, Place::Commit)?;
        }
        if checker.out.is_empty() {
            let cache = Cache {
                schema: SCHEMA.into(),
                commit: head,
                key,
                status_sealed: checker.status_sealed.clone(),
            };
            let text = serde_json::to_string_pretty(&cache).map_err(|e| e.to_string())? + "\n";
            super::write_atomically(&cache_path, text.as_bytes())?;
        }
    }
    let diff = ["--raw", "--no-abbrev", "--no-renames", "-z", "HEAD"];
    for (place, cached) in [(Place::Staged, true), (Place::WorkTree, false)] {
        let mut args = vec!["diff-index"];
        if cached {
            args.push("--cached");
        }
        args.extend(diff);
        for c in changes(&output(root, &args, b"")?, &prefix)? {
            checker.check(&c, place)?;
        }
    }
    Ok(std::mem::take(&mut checker.out))
}

/// How many bytes of `body`, the body of `file` now, are sealed: the length
/// of its body at `HEAD`, when the file is sealed there and `body` still
/// starts with that text.
pub(super) fn sealed_len(spec: &Spec, file: &str, body: &[u8]) -> Result<Option<usize>, String> {
    let Some(at_head) = sealed_at_head(spec, file)? else {
        return Ok(None);
    };
    let head = front::sealed_body(&at_head);
    Ok(body.starts_with(head).then_some(head.len()))
}

/// The content of `file` at `HEAD` when it is sealed there.
pub(super) fn sealed_at_head(spec: &Spec, file: &str) -> Result<Option<Vec<u8>>, String> {
    let root = spec.root();
    let Some(prefix) = prefix(root) else {
        return Ok(None);
    };
    let out = run(
        root,
        &["cat-file", "blob", &format!("HEAD:{prefix}{file}")],
        b"",
    )?;
    if !out.status.success() {
        return Ok(None);
    }
    let sealed = spec.sealed.matches(file)
        || front::status_of(&out.stdout).is_some_and(|s| spec.cfg.sealed_statuses.contains(&s))
        || status_sealed_by(spec, "HEAD", file)?;
    Ok(sealed.then_some(out.stdout))
}

/// Whether `file`, a record relative to the project root, had a status in
/// `sealed_statuses` in a version up to `rev`, as the history check reads
/// them: the version at `seal_baseline`, when set and an ancestor of `rev`,
/// and each version a later commit made. A record sealed by status stays
/// sealed when a later commit changes its status; a file in no index is
/// never sealed by status.
pub(super) fn status_sealed_by(spec: &Spec, rev: &str, file: &str) -> Result<bool, String> {
    let cfg = spec.cfg;
    let root = spec.root();
    if cfg.sealed_statuses.is_empty() || !spec.indexes.iter().any(|i| i.files.matches(file)) {
        return Ok(false);
    }
    let Some(prefix) = prefix(root) else {
        return Ok(false);
    };
    let path = format!("{prefix}{file}");
    let baseline = match &cfg.seal_baseline {
        None => None,
        Some(b) => Some(commit(root, b)?.ok_or_else(|| {
            format!("[lab.archive] seal_baseline `{b}` is not a commit in this repository")
        })?),
    };
    // A baseline that `rev` does not contain bounds nothing before `rev`.
    let baseline = match baseline {
        Some(b)
            if run(root, &["merge-base", "--is-ancestor", &b, rev], b"")?
                .status
                .success() =>
        {
            Some(b)
        }
        _ => None,
    };
    let range = match &baseline {
        Some(b) => format!("{b}..{rev}"),
        None => rev.to_string(),
    };
    let log = output(
        root,
        &["log", "--full-history", "--format=%H", &range, "--", &path],
        b"",
    )?;
    let mut blobs = Blobs::open(root)?;
    for c in String::from_utf8_lossy(&log)
        .split_whitespace()
        .chain(baseline.as_deref())
    {
        if blobs
            .get(&format!("{c}:{path}"))?
            .and_then(|b| front::status_of(&b))
            .is_some_and(|s| cfg.sealed_statuses.contains(&s))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `git log` of one file, oldest change last: for `lab archive log <record>`.
pub(super) fn history(root: &Path, path: &str) -> Result<String, String> {
    let out = output(
        root,
        &[
            "log",
            "--follow",
            "--date=short",
            "--format=%h %ad %s",
            "--",
            path,
        ],
        b"",
    )?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_raw_changes_and_keeps_only_the_project() {
        let raw = b"aaaa\0:100644 100644 1111 2222 M\0proj/a.md\0:100644 000000 3333 0000 D\0other/b.md\0bbbb\0:000000 100644 0000 4444 A\0proj/c d.md\0";
        let got = changes(raw, "proj/").unwrap();
        let summary: Vec<_> = got
            .iter()
            .map(|c| (c.commit.as_str(), c.status, c.path.as_str()))
            .collect();
        assert_eq!(summary, [("aaaa", b'M', "a.md"), ("bbbb", b'A', "c d.md")]);
        assert_eq!((got[0].old.as_str(), got[0].new.as_str()), ("1111", "2222"));
        assert!(changes(b":100644 M\0a.md\0", "").is_err());
    }
}
