//! End-to-end: archive model 2 (`[lab.archive] model = 2`): the git seal, the
//! record status machine, the metadata and task rules, number reservation,
//! and `lab archive done`, `log` and `index --print`.

use std::path::Path;
use std::process::{Command, Output};

const LAB: &str = env!("CARGO_BIN_EXE_lab");

fn run(program: &str, dir: &Path, args: &[&str]) -> Output {
    Command::new(program)
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("run")
}

fn lab(dir: &Path, args: &[&str]) -> Output {
    run(LAB, dir, args)
}

fn git(dir: &Path, args: &[&str]) -> String {
    let mut all = vec!["-c", "user.name=t", "-c", "user.email=t@example.com"];
    all.extend_from_slice(args);
    let out = run("git", dir, &all);
    assert!(out.status.success(), "git {args:?}: {}", stderr(&out));
    stdout(&out).trim().to_string()
}

fn commit(dir: &Path, message: &str) -> String {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", message]);
    git(dir, &["rev-parse", "HEAD"])
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

fn append(dir: &Path, rel: &str, text: &str) {
    write(dir, rel, &(read(dir, rel) + text));
}

const CONFIG: &str = r#"[project]
name = "demo"

[lab.archive]
model = 2
seal = "git"
write_indexes = false
statuses = ["proposed", "accepted", "superseded", "withdrawn", "current", "historical"]
inactive_statuses = ["superseded", "withdrawn"]
sealed = ["docs/log/*.md"]
sealed_statuses = ["accepted", "superseded", "withdrawn"]
links = ["docs/**/*.md"]
summary_min_chars = 20

[[lab.archive.index]]
name = "decisions"
title = "Decisions"
include = ["docs/decisions/D*.md"]
id_from = '^(D\d+)-'
require = ["id", "title", "status"]
"#;

const D001: &str = "docs/decisions/D001-first.md";
const D002: &str = "docs/decisions/D002-second.md";

fn record(status: &str, date: &str, extra: &str, title: &str) -> String {
    format!(
        "---\nstatus: {status}\ndate: {date}\nsummary: A summary that is long enough for the rule.\n{extra}---\n# {title}\n\nLine one.\nLine two.\n"
    )
}

/// A git repository with two accepted decisions and a month log, committed.
fn archive(d: &Path, config: &str) {
    git(d, &["init", "-q", "-b", "main"]);
    write(d, ".gitignore", ".research/\n");
    write(d, "research.toml", config);
    write(
        d,
        D001,
        &record(
            "accepted",
            "2026-10-01",
            "kind: decision\ntask: T-1\n",
            "First",
        ),
    );
    write(
        d,
        D002,
        &record(
            "accepted",
            "2026-10-05",
            "kind: result\ntask: T-2\n",
            "Second",
        ),
    );
    write(d, "docs/log/2026-10.md", "# October\n\n- began.\n");
    commit(d, "start");
}

fn check(d: &Path) -> (Option<i32>, String) {
    let out = lab(d, &["archive", "check"]);
    assert!(stderr(&out).is_empty(), "{}", stderr(&out));
    (out.status.code(), stdout(&out))
}

/// `severity rule subject` of each finding that is not a note.
fn rules(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.starts_with("error ") || l.starts_with("warn "))
        .map(|l| l.split(' ').take(3).collect::<Vec<_>>().join(" "))
        .collect()
}

fn clean(d: &Path) {
    let (code, text) = check(d);
    assert_eq!((code, rules(&text)), (Some(0), Vec::new()), "{text}");
}

#[test]
fn git_seal_allows_growth_at_the_end_and_refuses_an_edit() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    archive(d, CONFIG);
    clean(d);
    assert!(lab(d, &["archive", "update"]).status.success());
    assert!(lab(d, &["archive", "seal"]).status.success());
    // The only state is the ignored cache: no manifest, no index file.
    assert_eq!(git(d, &["status", "--porcelain"]), "");
    assert!(!d.join(".research/lab/archive-seal.json").exists());
    assert!(d.join(".research/lab/archive-git-seal.json").exists());

    // Lines at the end and a front matter change are not edits: in the working
    // tree, staged, and committed.
    append(d, D001, "\nCorrection of 2026-10-09: line two holds.\n");
    append(d, "docs/log/2026-10.md", "- went on.\n");
    clean(d);
    git(d, &["add", "-A"]);
    clean(d);
    commit(d, "append");
    write(
        d,
        D001,
        &read(d, D001).replace("status: accepted\n", "status: accepted\naliases: [one]\n"),
    );
    commit(d, "front matter");
    clean(d);

    // An earlier line changed: in the working tree, staged, then committed.
    write(d, D001, &read(d, D001).replace("Line one.", "Line 1."));
    let (code, text) = check(d);
    assert_eq!(code, Some(1), "{text}");
    assert_eq!(rules(&text), [format!("error sealed-edited {D001}")]);
    assert!(text.contains("in the working tree"), "{text}");
    git(d, &["add", "-A"]);
    let (_, text) = check(d);
    assert_eq!(rules(&text), [format!("error sealed-edited {D001}")]);
    assert!(text.contains("in the staged change"), "{text}");
    let edit = commit(d, "edit");
    let (code, text) = check(d);
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains(&format!("in commit {}", &edit[..10])),
        "{text}"
    );
    assert!(!lab(d, &["archive", "seal"]).status.success());

    // A missing cache and a wrong cache both mean a full check, never a pass.
    let cache = ".research/lab/archive-git-seal.json";
    write(
        d,
        cache,
        &format!(r#"{{"schema":"rf.archive-git-seal/1","commit":"{edit}","key":"other"}}"#),
    );
    assert_eq!(check(d).0, Some(1));
    write(d, cache, "not json");
    assert_eq!(check(d).0, Some(1));

    // The only way out is an accepted edit with a reason.
    let entry = |commit: &str, reason: &str| {
        format!(
            "{CONFIG}\n[[lab.archive.accepted_edit]]\npath = \"{D001}\"\ncommit = \"{commit}\"\nreason = \"{reason}\"\n"
        )
    };
    write(d, "research.toml", &entry(&edit, ""));
    let out = lab(d, &["archive", "check"]);
    assert!(
        stderr(&out).contains("`reason` must say why"),
        "{}",
        stderr(&out)
    );
    write(
        d,
        "research.toml",
        &entry(&edit[..7], "The number was wrong."),
    );
    commit(d, "accept");
    clean(d);
    assert!(read(d, cache).contains(&git(d, &["rev-parse", "HEAD"])));
    // With the verified commit cached, the check still sees a new edit.
    write(
        d,
        "docs/log/2026-10.md",
        "# October\n\n- began late.\n- went on.\n",
    );
    commit(d, "edit the log");
    let (code, text) = check(d);
    assert_eq!(code, Some(1), "{text}");
    assert_eq!(rules(&text), ["error sealed-edited docs/log/2026-10.md"]);
}

#[test]
fn a_record_is_sealed_from_the_first_commit_with_a_sealed_status() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    archive(d, CONFIG);
    let d3 = "docs/decisions/D003-third.md";
    write(d, d3, &record("proposed", "2026-10-07", "", "Third"));
    commit(d, "draft");
    // A draft is free to edit.
    write(d, d3, &read(d, d3).replace("Line one.", "Line 1."));
    commit(d, "edit the draft");
    write(d, d3, &read(d, d3).replace("proposed", "accepted"));
    commit(d, "accept");
    clean(d);
    write(d, d3, &read(d, d3).replace("Line 1.", "Line one again."));
    commit(d, "edit the accepted record");
    let (code, text) = check(d);
    assert_eq!(code, Some(1), "{text}");
    assert_eq!(rules(&text), [format!("error sealed-edited {d3}")]);
}

#[test]
fn the_status_machine_needs_a_reason_and_keeps_terminal_statuses() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    archive(d, CONFIG);

    // Inactive without a successor or a reason.
    let front = |from: &str, to: &str| write(d, D001, &read(d, D001).replace(from, to));
    front("status: accepted\n", "status: withdrawn\n");
    let (code, text) = check(d);
    assert_eq!(code, Some(1), "{text}");
    assert_eq!(
        rules(&text),
        [format!("error inactive-without-reason {D001}")]
    );
    front(
        "status: withdrawn\n",
        "status: withdrawn\nwithdrawn_reason: The data was wrong.\n",
    );
    clean(d);
    commit(d, "withdraw");

    // `withdrawn` is terminal: a move out of it is an error.
    front("status: withdrawn\n", "status: accepted\n");
    commit(d, "revive");
    let (code, text) = check(d);
    assert_eq!(code, Some(1), "{text}");
    assert_eq!(rules(&text), [format!("error status-reversed {D001}")]);
    front("status: accepted\n", "status: withdrawn\n");
    commit(d, "restore");
    // The reversal stays in history until it is accepted.
    assert_eq!(check(d).0, Some(1));

    // A record whose index requires a status and that has none.
    front("status: withdrawn\n", "");
    let (_, text) = check(d);
    assert!(
        rules(&text).contains(&format!("error missing-field {D001}")),
        "{text}"
    );
}

#[test]
fn amendments_agree_on_both_sides_and_keep_the_record_in_force() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    archive(d, CONFIG);
    write(
        d,
        D002,
        &read(d, D002).replace("kind: result\n", "kind: result\namends: [D001]\n"),
    );
    let (code, text) = check(d);
    assert_eq!(code, Some(0), "{text}");
    assert_eq!(rules(&text), [format!("warn amendment-mismatch {D002}")]);
    write(
        d,
        D001,
        &read(d, D001).replace("kind: decision\n", "kind: decision\namended_by: D002\n"),
    );
    // D001 stays accepted: an amended record is in force.
    clean(d);
    write(
        d,
        D002,
        &read(d, D002).replace("amends: [D001]", "amends: [D009]"),
    );
    let (code, text) = check(d);
    assert_eq!(code, Some(1), "{text}");
    assert_eq!(
        rules(&text),
        [
            format!("warn amendment-mismatch {D001}"),
            format!("error unknown-id {D002}")
        ]
    );
}

#[test]
fn the_git_seal_works_in_a_linked_worktree_and_across_a_merge() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("main");
    std::fs::create_dir(&d).unwrap();
    archive(&d, CONFIG);
    let wt = dir.path().join("wt");
    git(
        &d,
        &["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap()],
    );
    clean(&wt);
    assert!(wt.join(".research/lab/archive-git-seal.json").exists());

    // Both branches add a line to the same sealed log; the merge keeps both.
    let log = "docs/log/2026-10.md";
    append(&wt, log, "- side.\n");
    commit(&wt, "side");
    append(&d, log, "- main.\n");
    commit(&d, "main");
    let merge = run(
        "git",
        &d,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "merge",
            "-q",
            "side",
        ],
    );
    assert!(!merge.status.success(), "both sides changed the same lines");
    write(&d, log, "# October\n\n- began.\n- main.\n- side.\n");
    commit(&d, "merge");
    clean(&d);

    // An edit in the linked worktree is found there.
    write(&wt, D001, &read(&wt, D001).replace("Line two.", "Line 2."));
    let (code, text) = check(&wt);
    assert_eq!(code, Some(1), "{text}");
    assert_eq!(rules(&text), [format!("error sealed-edited {D001}")]);
}

#[test]
fn git_seal_outside_a_repository_is_an_error_not_a_pass() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(d, "research.toml", CONFIG);
    write(d, D001, &record("accepted", "2026-10-01", "", "First"));
    let out = Command::new(LAB)
        .args(["archive", "check"])
        .current_dir(d)
        .env("GIT_CEILING_DIRECTORIES", d.parent().unwrap())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        rules(&stdout(&out)),
        ["error seal-unverified research.toml"]
    );
}

const MODEL_1: &str = r#"[project]
name = "demo"

[lab.archive]
seal_manifest = "docs/.seal.json"
sealed = ["docs/log/*.md"]

[[lab.archive.index]]
name = "decisions"
output = "docs/decisions/INDEX.md"
title = "Decisions"
include = ["docs/decisions/D*.md"]
"#;

#[test]
fn update_seals_nothing_when_the_index_step_fails() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(d, "research.toml", MODEL_1);
    write(d, "docs/log/2026-10.md", "# October\n");
    write(d, D001, "---\nstatus: [\n---\n# First\n");
    let out = lab(d, &["archive", "update"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("Nothing sealed"), "{}", stdout(&out));
    assert!(!d.join("docs/.seal.json").exists());
    assert!(!d.join("docs/decisions/INDEX.md").exists());

    write(d, D001, "---\nstatus: accepted\n---\n# First\n");
    assert!(lab(d, &["archive", "update"]).status.success());
    assert!(d.join("docs/.seal.json").exists());
}

#[test]
fn model_2_keys_need_model_2() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(
        d,
        "research.toml",
        &MODEL_1.replace(
            "sealed = [",
            "seal = \"git\"\ntask_from = \"2026-01-01\"\nsealed = [",
        ),
    );
    let out = lab(d, &["archive", "check"]);
    assert_eq!(out.status.code(), Some(2), "{}", stdout(&out));
    assert!(
        stderr(&out).contains("seal, task_from need(s) `model = 2`"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn next_reserves_a_number_across_worktrees() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("main");
    std::fs::create_dir(&d).unwrap();
    archive(&d, CONFIG);
    let wt = dir.path().join("wt");
    git(
        &d,
        &["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap()],
    );
    let next = |dir: &Path, args: &[&str]| {
        let mut all = vec!["archive", "next", "D"];
        all.extend_from_slice(args);
        let out = lab(dir, &all);
        assert!(out.status.success(), "{}", stderr(&out));
        stdout(&out).trim().to_string()
    };
    assert_eq!(next(&d, &[]), "D003");
    assert_eq!(next(&wt, &[]), "D004");
    assert_eq!(next(&d, &["--peek"]), "D005");
    assert_eq!(next(&d, &["--peek"]), "D005");
    assert_eq!(next(&wt, &["--reserve"]), "D005");
    // A record that only the main checkout has counts in the worktree too.
    write(&d, "docs/decisions/D009-ninth.md", "# Ninth\n");
    assert_eq!(next(&wt, &[]), "D010");
    // The reservation is in the main checkout's ignored state.
    assert!(d.join(".research/lab/archive-reserved.json").exists());
    assert!(!wt.join(".research/lab/archive-reserved.json").exists());
    assert_eq!(git(&d, &["status", "--porcelain", "--", ".research"]), "");

    // Model 1 reserves only when asked.
    let old = tempfile::tempdir().unwrap();
    let o = old.path();
    write(o, "research.toml", MODEL_1);
    write(o, D001, "# First\n");
    let ids = |args: &[&str]| stdout(&lab(o, args)).trim().to_string();
    assert_eq!(ids(&["archive", "next", "D"]), "D002");
    assert_eq!(ids(&["archive", "next", "D"]), "D002");
    assert_eq!(ids(&["archive", "next", "D", "--reserve"]), "D002");
    assert_eq!(ids(&["archive", "next", "D", "--reserve"]), "D003");
}

#[test]
fn check_warns_of_an_id_that_the_default_branch_added_at_another_path() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("main");
    std::fs::create_dir(&d).unwrap();
    archive(&d, CONFIG);
    let new = |title: &str| {
        record(
            "proposed",
            "2026-11-12",
            "kind: decision\ntask: T-3\n",
            title,
        )
    };
    write(&d, "docs/decisions/D005-draft.md", &new("Draft"));
    commit(&d, "draft");
    let wt = dir.path().join("wt");
    git(
        &d,
        &["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap()],
    );
    // The branch writes D003 and D004 and keeps D005 where it was.
    write(&wt, "docs/decisions/D003-mine.md", &new("Mine"));
    write(&wt, "docs/decisions/D004-shared.md", &new("Shared"));
    commit(&wt, "side records");
    clean(&wt);
    // The default branch adds its own D003, the same D004, and moves D005.
    write(&d, "docs/decisions/D003-theirs.md", &new("Theirs"));
    write(&d, "docs/decisions/D004-shared.md", &new("Shared"));
    git(
        &d,
        &[
            "mv",
            "docs/decisions/D005-draft.md",
            "docs/decisions/D005-moved.md",
        ],
    );
    commit(&d, "main records");
    clean(&d);
    let (code, text) = check(&wt);
    assert_eq!(code, Some(0), "{text}");
    assert_eq!(
        rules(&text),
        ["warn id-taken-on-default-branch docs/decisions/D003-mine.md"],
        "{text}"
    );
    assert!(
        text.contains("docs/decisions/D003-theirs.md also has on main"),
        "{text}"
    );
    // The warning is what `lab check --new-only` gates in the change.
    let out = lab(&wt, &["archive", "check", "--format", "jsonl"]);
    let line = stdout(&out)
        .lines()
        .find(|l| l.contains("id-taken-on-default-branch"))
        .map(String::from)
        .unwrap_or_default();
    assert!(
        line.contains(r#""subject":"docs/decisions/D003-mine.md""#),
        "{line}"
    );
    // After the merge both records are there: a duplicate-id error.
    git(&wt, &["merge", "-q", "--no-edit", "main"]);
    let (code, text) = check(&wt);
    assert_eq!(code, Some(1), "{text}");
    assert_eq!(rules(&text), ["error duplicate-id D003"], "{text}");
}

#[test]
fn next_counts_the_default_branch_and_the_other_worktrees() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("main");
    write(&d, ".gitignore", ".research/\n");
    write(&d, "research.toml", MODEL_1);
    write(&d, D001, "# First\n");
    git(&d, &["init", "-q", "-b", "main"]);
    commit(&d, "start");
    let add = |name: &str| {
        let wt = dir.path().join(name);
        git(
            &d,
            &["worktree", "add", "-q", "-b", name, wt.to_str().unwrap()],
        );
        wt
    };
    let wt = add("wt");
    let next = |dir: &Path, args: &[&str]| {
        let mut all = vec!["archive", "next", "D"];
        all.extend_from_slice(args);
        let out = lab(dir, &all);
        assert!(out.status.success(), "{}", stderr(&out));
        (stdout(&out).trim().to_string(), stderr(&out))
    };
    assert_eq!(next(&wt, &[]).0, "D002");
    // A record merged into the default branch counts, though neither work
    // tree has it.
    write(&d, "docs/decisions/D007-merged.md", "# Merged\n");
    commit(&d, "merge D007");
    git(&d, &["switch", "-q", "-c", "old", "HEAD~1"]);
    assert!(!d.join("docs/decisions/D007-merged.md").exists());
    assert_eq!(next(&wt, &[]).0, "D008");
    assert_eq!(next(&wt, &["--peek"]).0, "D008");
    // So does a record that another worktree has not committed yet.
    let other = add("other");
    write(&other, "docs/decisions/D011-draft.md", "# Draft\n");
    assert_eq!(next(&wt, &[]).0, "D012");
    assert_eq!(next(&wt, &["--peek"]).0, "D012");
    assert_eq!(next(&d, &[]).0, "D012");
    // A worktree whose project does not load is skipped with a note.
    let broken = add("broken");
    write(&broken, "research.toml", "[lab.archive\n");
    write(&broken, "docs/decisions/D020-lost.md", "# Lost\n");
    let (id, note) = next(&wt, &[]);
    assert_eq!(id, "D012");
    assert!(
        note.contains("note: the records of") && note.contains("broken"),
        "{note}"
    );
}

#[test]
fn next_counts_elsewhere_for_a_project_below_the_top_and_skips_a_dangling_default() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("main");
    let p = d.join("sub");
    write(&d, ".gitignore", ".research/\n");
    write(&p, "research.toml", MODEL_1);
    write(&p, D001, "# First\n");
    // A file of the same name above the project is no record of it.
    write(&d, "docs/decisions/D050-outside.md", "# Outside\n");
    git(&d, &["init", "-q", "-b", "main"]);
    commit(&d, "start");
    let wt = dir.path().join("wt");
    git(
        &d,
        &["worktree", "add", "-q", "-b", "wt", wt.to_str().unwrap()],
    );
    let w = wt.join("sub");
    let next = |dir: &Path| {
        let out = lab(dir, &["archive", "next", "D"]);
        assert!(out.status.success(), "{}", stderr(&out));
        (stdout(&out).trim().to_string(), stderr(&out))
    };
    assert_eq!(next(&w).0, "D002");
    write(&p, "docs/decisions/D004-merged.md", "# Merged\n");
    commit(&d, "merge D004");
    git(&d, &["switch", "-q", "-c", "old", "HEAD~1"]);
    assert_eq!(next(&w).0, "D005", "from the default branch");
    write(&p, "docs/decisions/D008-draft.md", "# Draft\n");
    assert_eq!(next(&w).0, "D009", "from the main checkout's work tree");
    // `origin/HEAD` that names a pruned branch is skipped with a note.
    git(
        &d,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/gone",
        ],
    );
    let (id, note) = next(&w);
    assert_eq!(id, "D009");
    assert!(
        note.contains("the default branch are not counted") && note.contains("origin/gone"),
        "{note}"
    );
}

#[test]
fn metadata_rules_report_ids_summaries_stray_files_and_page_groups() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let config = CONFIG.replace(
        "summary_min_chars = 20\n",
        "summary_min_chars = 20\nunindexed_exclude = [\"docs/decisions/README.md\"]\n",
    ) + "\n[[lab.archive.index]]\nname = \"topics\"\ntitle = \"Topics\"\ninclude = [\"docs/topics/*.md\"]\n\n[[lab.archive.page_group]]\nname = \"retrieval\"\ninclude = [\"docs/topics/retrieval-*.md\"]\n";
    archive(d, &config);
    write(d, "docs/decisions/README.md", "# How to write a decision\n");
    write(d, "docs/decisions/notes.md", "# Stray notes\n");
    write(
        d,
        "docs/decisions/D001-again.md",
        &record("accepted", "2026-10-02", "", "First again"),
    );
    write(
        d,
        "docs/decisions/D004-none.md",
        "---\nstatus: accepted\n---\n# No summary\n",
    );
    write(
        d,
        "docs/decisions/D005-short.md",
        "---\nstatus: accepted\nsummary: Too short.\n---\n# Short summary\n",
    );
    write(
        d,
        "docs/topics/retrieval-2025.md",
        "---\nstatus: historical\nsummary: What retrieval looked like in the year 2025.\n---\n# Retrieval 2025\n",
    );
    let (code, text) = check(d);
    assert_eq!(code, Some(1), "{text}");
    assert_eq!(
        rules(&text),
        [
            "error duplicate-id D001",
            "warn missing-summary docs/decisions/D004-none.md",
            "warn missing-summary docs/decisions/D005-short.md",
            "warn unindexed-file docs/decisions/notes.md",
            "warn no-current-page retrieval",
        ]
    );
    let double = text.lines().find(|l| l.contains("duplicate-id")).unwrap();
    assert!(
        double.contains("docs/decisions/D001-again.md, docs/decisions/D001-first.md"),
        "{double}"
    );

    // An allow entry with a reason silences one finding and no other.
    let allow = |rule: &str, subject: &str, reason: &str| {
        format!(
            "\n[[lab.archive.allow]]\nrule = \"{rule}\"\nsubject = \"{subject}\"\nreason = \"{reason}\"\n"
        )
    };
    write(
        d,
        "research.toml",
        &(config.clone() + &allow("missing-summary", "docs/decisions/D004-none.md", "")),
    );
    assert!(stderr(&lab(d, &["archive", "check"])).contains("`reason` must say why"));
    let allowed = config.clone()
        + &allow(
            "missing-summary",
            "docs/decisions/D004-none.md",
            "A stub that the owner keeps.",
        )
        + &allow("duplicate-id", "D001", "Both files are cited from outside.")
        + &allow(
            "unindexed-file",
            "docs/decisions/gone.md",
            "No longer there.",
        );
    write(d, "research.toml", &allowed);
    let (code, text) = check(d);
    assert_eq!(code, Some(0), "{text}");
    assert_eq!(
        rules(&text),
        [
            "warn missing-summary docs/decisions/D005-short.md",
            "warn unindexed-file docs/decisions/notes.md",
            "warn no-current-page retrieval",
            "warn stale-allow unindexed-file:docs/decisions/gone.md",
        ]
    );
    assert!(
        text.contains("info missing-summary docs/decisions/D004-none.md")
            && text.contains("(A stub that the owner keeps.)"),
        "{text}"
    );

    // A page with the current status ends `no-current-page`.
    write(
        d,
        "docs/topics/retrieval-2026.md",
        "---\nstatus: current\nsummary: What retrieval looks like in the year 2026.\n---\n# Retrieval 2026\n",
    );
    let (_, text) = check(d);
    assert!(!text.contains("no-current-page"), "{text}");
}

#[test]
fn a_current_page_marks_its_links_to_historical_pages() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let config = CONFIG.to_string()
        + "\n[[lab.archive.index]]\nname = \"topics\"\ntitle = \"Topics\"\ninclude = [\"docs/topics/*.md\"]\n\n[lab.archive.historical_link]\nmarks = [\"historical\", \"superseded\"]\n";
    archive(d, &config);
    let page = |status: &str, body: &str| {
        format!(
            "---\nstatus: {status}\nsummary: A page about retrieval that is long enough.\n---\n# Page\n\n{body}"
        )
    };
    write(d, "docs/topics/old.md", &page("historical", "Old text.\n"));
    write(
        d,
        "docs/topics/older.md",
        &page("historical", "See [old](old.md).\n"),
    );
    write(
        d,
        "docs/topics/now.md",
        &page(
            "current",
            "See [the old page](old.md).\nHistorical: [the old page](old.md).\nAlso [older](older.md).\n",
        ),
    );
    // Committed, so that no link points to an untracked file.
    commit(d, "pages");
    let (code, text) = check(d);
    assert_eq!(code, Some(0), "{text}");
    assert_eq!(
        rules(&text),
        [
            "warn historical-link docs/topics/now.md:7",
            "warn historical-link docs/topics/now.md:9",
        ]
    );
    write(
        d,
        "research.toml",
        &(config
            + "\n[[lab.archive.allow]]\nrule = \"historical-link\"\nsubject = \"docs/topics/now.md\"\ntarget = \"older.md\"\nreason = \"The page lists its own sources.\"\n"),
    );
    let (_, text) = check(d);
    assert_eq!(rules(&text), ["warn historical-link docs/topics/now.md:7"]);
}

#[test]
fn records_name_a_task_that_the_board_knows() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let with = |extra: &str| CONFIG.replace("summary_min_chars = 20\n", extra);
    archive(d, &with("task_from = \"2026-10-03\"\n"));
    // D001 is older than the start date; D003 is newer and names nothing.
    write(
        d,
        D001,
        &read(d, D001).replace("kind: decision\ntask: T-1\n", ""),
    );
    write(
        d,
        "docs/decisions/D003-third.md",
        &record("proposed", "2026-10-07", "", "Third"),
    );
    let (code, text) = check(d);
    assert_eq!(code, Some(1), "{text}");
    assert_eq!(
        rules(&text),
        [
            "error missing-kind docs/decisions/D003-third.md",
            "error missing-task docs/decisions/D003-third.md",
        ]
    );
    // No board export is configured: the state is `skipped`, and it is printed.
    assert!(
        text.contains("info task-link-skipped research.toml — skipped: no `board_export`"),
        "{text}"
    );
    write(
        d,
        "docs/decisions/D003-third.md",
        &record(
            "proposed",
            "2026-10-07",
            "kind: memo\ntask: [T-2, T-9]\n",
            "Third",
        ),
    );
    let (_, text) = check(d);
    assert_eq!(
        rules(&text),
        ["error front-matter-invalid docs/decisions/D003-third.md"]
    );
    assert!(
        text.contains("`kind` `memo` is not one of: result, decision"),
        "{text}"
    );
    write(
        d,
        "docs/decisions/D003-third.md",
        &record(
            "proposed",
            "2026-10-07",
            "kind: survey\ntask: [T-2, T-9]\n",
            "Third",
        ),
    );

    // The board export gives the known task ids.
    write(
        d,
        "board.json",
        r#"[{"id":"T-1","status":"done","tags":["research"]},{"id":"T-2","status":"doing"},{"id":"T-3","status":"done","tags":["research"]},{"id":"T-4","status":"done"}]"#,
    );
    write(
        d,
        "research.toml",
        &with("board_export = [\"cat\", \"board.json\"]\nresearch_tag = \"research\"\n"),
    );
    let (code, text) = check(d);
    assert_eq!(code, Some(1), "{text}");
    assert_eq!(
        rules(&text),
        [
            "error unknown-task docs/decisions/D003-third.md",
            "warn task-without-record T-1",
            "warn task-without-record T-3",
        ]
    );
    assert!(!text.contains("task-link-skipped"), "{text}");

    // An unreachable board is `skipped`: neither an error nor a silent pass.
    for command in [
        "[\"false\"]",
        "[\"no-such-board-program\"]",
        "[\"echo\", \"busy\"]",
    ] {
        write(
            d,
            "research.toml",
            &with(&format!("board_export = {command}\n")),
        );
        let (code, text) = check(d);
        assert_eq!((code, rules(&text)), (Some(0), Vec::new()), "{text}");
        assert!(
            text.contains("info task-link-skipped research.toml — skipped: the board export"),
            "{text}"
        );
    }

    // `research_parents`: a done task at or below a listed task needs a
    // record, unless it changed before `task_from`. U-5 and U-6 form a
    // `parent` cycle, which must end.
    write(
        d,
        "board.json",
        r#"[{"id":"T-2","status":"doing"},{"id":"T-9","status":"doing"},
            {"id":"U-0","status":"done","updated_at":"2026-10-03T00:00:00Z"},
            {"id":"U-1","status":"done","parent":"U-0","updated_at":"2026-11-01T09:00:00Z"},
            {"id":"U-2","status":"done","parent":"U-1"},
            {"id":"U-3","status":"done","parent":"U-0","updated_at":"2026-10-02T23:59:59Z"},
            {"id":"U-4","status":"done","updated_at":"2026-11-01T09:00:00Z"},
            {"id":"U-5","status":"done","parent":"U-6"},
            {"id":"U-6","status":"done","parent":"U-5"},
            {"id":"U-7","status":"doing","parent":"U-0"}]"#,
    );
    write(
        d,
        "research.toml",
        &with(
            "task_from = \"2026-10-03\"\nboard_export = [\"cat\", \"board.json\"]\n\
             research_parents = [\"U-0\"]\n",
        ),
    );
    let (code, text) = check(d);
    assert_eq!(code, Some(0), "{text}");
    assert_eq!(
        rules(&text),
        [
            "warn task-without-record U-0",
            "warn task-without-record U-1",
            "warn task-without-record U-2",
        ]
    );
    assert!(
        text.contains("task U-2 is done and below U-0, which `research_parents` lists"),
        "{text}"
    );
    assert!(
        text.contains("task U-0 is done and listed in `research_parents`"),
        "{text}"
    );

    // Without `board_export` the setting is a config error.
    write(d, "research.toml", &with("research_parents = [\"U-0\"]\n"));
    let out = lab(d, &["archive", "check"]);
    assert_eq!(out.status.code(), Some(2), "{}", stdout(&out));
    assert!(
        stderr(&out).contains("`research_parents` needs `board_export`"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn log_prints_the_records_in_date_order() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    archive(d, CONFIG);
    write(
        d,
        "docs/decisions/D000-early.md",
        "---\nstatus: accepted\ndate: 2026-09-30\ntitle: Early\n---\n# Early\n",
    );
    let out = lab(d, &["archive", "log"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        format!(
            "2026-09-30\t-\tD000\tdocs/decisions/D000-early.md\tEarly\n\
             2026-10-01\tdecision\tD001\t{D001}\tA summary that is long enough for the rule.\n\
             2026-10-05\tresult\tD002\t{D002}\tA summary that is long enough for the rule.\n"
        )
    );
    let out = lab(d, &["archive", "log", "--since", "2026-10-02"]);
    assert_eq!(stdout(&out).lines().count(), 1);
    assert!(stdout(&out).starts_with("2026-10-05\tresult\tD002"));
    assert_eq!(
        lab(d, &["archive", "log", "--since", "soon"]).status.code(),
        Some(2)
    );

    // One record: its commits, newest first.
    append(d, D001, "\nMore.\n");
    commit(d, "append to the first");
    let out = lab(d, &["archive", "log", "D001"]);
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(lines[0].ends_with("append to the first") && lines[1].ends_with("start"));
}

#[test]
fn done_is_the_completion_gate_and_writes_no_index_file() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let config = CONFIG.to_string()
        + "\n[[lab.archive.index]]\nname = \"results\"\ntitle = \"Results\"\ninclude = [\"docs/decisions/D*.md\"]\nid_from = '^(D\\d+)-'\nkinds = [\"result\"]\n";
    archive(d, &config);
    let done = |d: &Path| {
        let out = lab(d, &["archive", "done"]);
        (out.status.code(), stdout(&out) + &stderr(&out))
    };
    let (code, text) = done(d);
    assert_eq!(code, Some(0), "{text}");
    assert!(
        text.contains("search index: up to date, 5 unit(s) of 3 file(s)"),
        "{text}"
    );
    assert!(text.contains("done: links resolve"), "{text}");
    // The tree holds no generated file.
    assert_eq!(git(d, &["status", "--porcelain"]), "");

    // The index is printed instead; an index with `kinds` lists those kinds.
    let out = lab(d, &["archive", "index", "--print", "decisions"]);
    let text = stdout(&out);
    assert!(text.contains("# Decisions"), "{text}");
    assert!(
        text.contains(&format!("- [D001]({D001}) · 2026-10-01 · accepted")),
        "{text}"
    );
    assert!(text.contains(&format!("- [D002]({D002})")), "{text}");
    let text = stdout(&lab(d, &["archive", "index", "--print", "results"]));
    assert!(
        text.contains("[D002]") && !text.contains("[D001]"),
        "{text}"
    );
    assert_eq!(
        lab(d, &["archive", "index", "--print", "nothing"])
            .status
            .code(),
        Some(2)
    );
    assert!(stdout(&lab(d, &["archive", "index"])).contains("No index file is written"));
    assert_eq!(git(d, &["status", "--porcelain"]), "");

    // A broken link stops the gate and prints the fix.
    append(d, D002, "\nSee [the plan](../plan.md).\n");
    let (code, text) = done(d);
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains(&format!("error broken-link {D002}:")),
        "{text}"
    );
    assert!(text.contains("  fix: correct the link target"), "{text}");
    assert!(text.contains("Stopped at `links and metadata`"), "{text}");
    assert!(!text.contains("done: links resolve"), "{text}");
}

#[test]
fn done_brings_the_search_index_up_to_date_past_the_refresh_budget() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    archive(
        d,
        &(CONFIG.to_string() + "\n[lab.search]\nrefresh_budget_ms = 0\n"),
    );
    let out = lab(d, &["archive", "done"]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    // A changed file: a query's refresh would stop at the budget of 0 ms.
    append(d, D002, "\nA wombat appears.\n");
    let out = lab(d, &["archive", "done"]);
    let text = stdout(&out) + &stderr(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("search index: up to date"), "{text}");
    let out = lab(d, &["search", "wombat", "--json"]);
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["state"], "hits", "{report}");
}

#[test]
fn done_fails_only_on_findings_outside_the_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let config = format!(
        "{CONFIG}\n[[lab.checks]]\nname = \"archive\"\nrun = [\"{LAB}\", \"archive\", \"check\", \"--format\", \"jsonl\"]\n"
    );
    archive(d, &config);
    append(d, D002, "\nSee [the plan](../plan.md).\n");
    commit(d, "a link that was always broken");
    let done = |d: &Path| {
        let out = lab(d, &["archive", "done"]);
        (out.status.code(), stdout(&out) + &stderr(&out))
    };
    // Without a baseline every finding is new.
    assert_eq!(done(d).0, Some(1));
    assert!(
        lab(d, &["baseline", "set", "--note", "known"])
            .status
            .success()
    );
    let (code, text) = done(d);
    assert_eq!(code, Some(0), "{text}");
    assert!(!text.contains("broken-link"), "{text}");

    // A sealed line changes: the gate stops at `sealed`.
    write(d, D001, &read(d, D001).replace("Line one.", "Line 1."));
    let (code, text) = done(d);
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains(&format!("error sealed-edited {D001}")),
        "{text}"
    );
    assert!(text.contains("  fix: restore the sealed text"), "{text}");
    assert!(text.contains("Stopped at `sealed`"), "{text}");
}

#[test]
fn brief_names_the_gate_and_an_old_lab() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    archive(
        d,
        &CONFIG.replace(
            "[lab.archive]",
            "[lab]\nmin_version = \"99.1\"\n\n[lab.archive]",
        ),
    );
    let out = lab(d, &["brief"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        text.starts_with(&format!(
            "> This lab is version {version}, but research.toml needs 99.1 or newer"
        )),
        "{text}"
    );
    assert!(
        text.contains("- Before finishing: `lab archive done`."),
        "{text}"
    );
    assert!(text.contains("`lab archive index --print <name>`; the names are decisions."));

    write(
        d,
        "research.toml",
        &CONFIG.replace(
            "[lab.archive]",
            "[lab]\nmin_version = \"0.1.0\"\n\n[lab.archive]",
        ),
    );
    assert!(stdout(&lab(d, &["brief"])).starts_with("# "));
    write(
        d,
        "research.toml",
        &CONFIG.replace(
            "[lab.archive]",
            "[lab]\nmin_version = \"new\"\n\n[lab.archive]",
        ),
    );
    assert!(stderr(&lab(d, &["brief"])).contains("min_version must be numbers"));
}

#[test]
fn a_kept_link_covers_text_that_git_sealed() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let config = CONFIG.to_string()
        + "\n[[lab.archive.kept_link]]\nfile = \"docs/log/2026-10.md\"\ntarget = \"gone.md\"\nreason = \"The page was removed on purpose.\"\n";
    archive(d, &config);
    append(d, "docs/log/2026-10.md", "- see [gone](gone.md).\n");
    // Not committed yet: the line is not sealed, so the link is an error.
    let (code, text) = check(d);
    assert_eq!(code, Some(1), "{text}");
    commit(d, "a link to a removed page");
    let (code, text) = check(d);
    assert_eq!((code, rules(&text)), (Some(0), Vec::new()), "{text}");
    assert!(text.contains("info kept-link"), "{text}");
}

#[test]
fn a_kept_link_covers_text_sealed_by_an_earlier_status() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let config = format!(
        "{CONFIG}\n[[lab.archive.kept_link]]\nfile = \"{D001}\"\ntarget = \"../gone.md\"\nreason = \"The page was removed on purpose.\"\n"
    );
    archive(d, &config);
    append(d, D001, "\nSee [gone](../gone.md).\n");
    commit(d, "a link to a removed page");
    // `accepted` sealed D001 with the link; `proposed` at HEAD does not unseal it.
    write(d, D001, &read(d, D001).replace("accepted", "proposed"));
    commit(d, "back to proposed");
    let (code, text) = check(d);
    assert_eq!((code, rules(&text)), (Some(0), Vec::new()), "{text}");
    assert!(text.contains("info kept-link"), "{text}");
}

#[test]
fn a_record_sealed_by_status_stays_sealed_when_its_status_moves_back() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    archive(d, CONFIG);
    // `accepted` seals D001; moving it back to `proposed` does not unseal it.
    write(d, D001, &read(d, D001).replace("accepted", "proposed"));
    commit(d, "back to proposed");
    // The clean history is cached with D001 sealed.
    clean(d);
    write(d, D001, &read(d, D001).replace("Line two.", "Line 2."));
    commit(d, "edit");
    let expected = [format!("error sealed-edited {D001}")];
    let (code, text) = check(d);
    assert_eq!((code, rules(&text)), (Some(1), expected.to_vec()), "{text}");
    // The same without the cache: a full check of history.
    std::fs::remove_file(d.join(".research/lab/archive-git-seal.json")).unwrap();
    let (code, text) = check(d);
    assert_eq!((code, rules(&text)), (Some(1), expected.to_vec()), "{text}");
}

#[test]
fn an_accepted_edit_on_a_branch_stays_accepted_at_its_merge() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    archive(d, CONFIG);
    git(d, &["checkout", "-q", "-b", "side"]);
    write(d, D001, &read(d, D001).replace("Line two.", "Line 2."));
    let edit = commit(d, "edit on the side");
    git(d, &["checkout", "-q", "main"]);
    append(d, "docs/log/2026-10.md", "- main.\n");
    commit(d, "main");
    git(d, &["merge", "-q", "--no-ff", "-m", "merge side", "side"]);
    // An upper-case hash names the same commit.
    let config = format!(
        "{CONFIG}\n[[lab.archive.accepted_edit]]\npath = \"{D001}\"\ncommit = \"{}\"\nreason = \"removed a personal number\"\n",
        edit[..10].to_ascii_uppercase()
    );
    write(d, "research.toml", &config);
    commit(d, "accept the edit");
    clean(d);
}

#[test]
fn a_record_left_out_of_a_kinds_index_is_still_checked() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let config = CONFIG.to_string()
        + "\n[[lab.archive.index]]\nname = \"results\"\ntitle = \"Results\"\ninclude = [\"docs/results/*.md\"]\nkinds = [\"result\"]\n";
    archive(d, &config);
    write(
        d,
        "docs/results/R1.md",
        "---\nid: R1\nkind: [a]\nsummary: A summary that is long enough for the rule.\n---\n# R1\n",
    );
    let (code, text) = check(d);
    assert_eq!(code, Some(1), "{text}");
    assert_eq!(
        rules(&text),
        ["error front-matter-invalid docs/results/R1.md"],
        "{text}"
    );
    let printed = stdout(&lab(d, &["archive", "index", "--print", "results"]));
    assert!(!printed.contains("[R1]"), "{printed}");
}

#[test]
fn an_allow_entry_accepts_the_open_items_of_a_sealed_file() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let config = CONFIG.to_string()
        + "\n[lab.archive.open_items]\nsince = \"2026-11-01\"\nboards = [\"rf\"]\n";
    archive(d, &config);
    let log = "docs/log/2026-11.md";
    write(
        d,
        log,
        "---\ntask: rf-1\n---\n# November\n\n## Limits\n\n- The second source was not read.\n",
    );
    commit(d, "log");
    let (code, text) = check(d);
    assert_eq!(code, Some(1), "{text}");
    assert_eq!(rules(&text), [format!("error open-item {log}:8")]);

    let allowed = config
        + &format!(
            "\n[[lab.archive.allow]]\nrule = \"open-item\"\nsubject = \"{log}\"\nreason = \"Sealed text; rf-2 holds the item.\"\n"
        );
    write(d, "research.toml", &allowed);
    let (code, text) = check(d);
    assert_eq!(
        (code, rules(&text)),
        (Some(0), Vec::<String>::new()),
        "{text}"
    );
    assert!(
        text.contains(&format!("info open-item {log}:8"))
            && text.contains("(Sealed text; rf-2 holds the item.)"),
        "{text}"
    );
}
