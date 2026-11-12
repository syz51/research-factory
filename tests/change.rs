//! `lab check --new-only` against the change: the findings a branch adds to
//! the files it touches, compared with the merge base of the default branch.

use std::path::Path;
use std::process::{Command, Output};

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Isolates git from the user's configuration and stops repository
/// discovery at `ceiling`.
fn isolated<'a>(cmd: &'a mut Command, ceiling: &Path) -> &'a mut Command {
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CEILING_DIRECTORIES", ceiling)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
}

fn git(d: &Path, ceiling: &Path, args: &[&str]) -> String {
    let out = isolated(&mut Command::new("git"), ceiling)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .current_dir(d)
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {}", stderr(&out));
    stdout(&out).trim().to_string()
}

/// Runs `lab` with git isolated and temporary files in `tmp`.
fn lab(d: &Path, ceiling: &Path, tmp: &Path, args: &[&str]) -> Output {
    isolated(&mut Command::new(env!("CARGO_BIN_EXE_lab")), ceiling)
        .args(args)
        .env("TMPDIR", tmp)
        .current_dir(d)
        .output()
        .expect("run lab")
}

fn finding(severity: &str, subject: &str, message: &str, evidence: &str) -> String {
    format!(
        r#"{{"schema":"rf.finding/1","tool":"t","rule":"r","severity":"{severity}","subject":"{subject}","message":"{message}","evidence":[{evidence}]}}"#
    )
}

fn write(d: &Path, rel: &str, text: &str) {
    std::fs::write(d.join(rel), text).unwrap();
}

/// The check prints the committed `f.jsonl`, the git-ignored
/// `sources/w.jsonl`, and a warning whose message names the directory it ran
/// in. It exits 1 when `f.jsonl` holds an error.
const CHECK: &str = r#"cat f.jsonl; echo
cat sources/w.jsonl 2>/dev/null; echo
printf '{"schema":"rf.finding/1","tool":"t","rule":"where","severity":"warn","subject":"a.md:2","message":"ran in %s"}\n' "$(pwd)"
grep -q '"error"' f.jsonl && exit 1
exit 0
"#;

#[test]
fn new_only_gates_what_the_change_adds_to_the_files_it_touches() {
    let dir = tempfile::tempdir().unwrap();
    let ceiling = dir.path();
    let tmp = &ceiling.join("tmp");
    let d = &ceiling.join("repo");
    std::fs::create_dir_all(tmp).unwrap();
    std::fs::create_dir_all(d).unwrap();
    git(d, ceiling, &["init", "-q", "-b", "main"]);
    assert!(
        lab(d, ceiling, tmp, &["init", "--name", "demo"])
            .status
            .success()
    );
    let mut cfg = std::fs::read_to_string(d.join("research.toml")).unwrap();
    cfg += "\n[[lab.checks]]\nname = \"claims\"\nrun = [\"sh\", \"check.sh\"]\nversion = []\n\n[lab.archive]\nsealed = [\"log.md\"]\n";
    write(d, "research.toml", &cfg);
    write(d, ".gitignore", ".research/\nsources/\n");
    write(d, "check.sh", CHECK);
    let lines = |n: usize| (1..=n).map(|i| format!("line {i}\n")).collect::<String>();
    write(d, "a.md", &lines(10));
    write(d, "b.md", &lines(3));
    write(d, "log.md", "# Log\n\n- first\n");
    write(d, "f.jsonl", &finding("warn", "a.md:3", "old", ""));
    std::fs::create_dir_all(d.join("sources")).unwrap();
    write(
        d,
        "sources/w.jsonl",
        &finding("warn", "a.md:4", "from an ignored input", ""),
    );
    git(d, ceiling, &["add", "-A"]);
    git(d, ceiling, &["commit", "-qm", "base"]);

    let clean = lab(d, ceiling, tmp, &["check", "--new-only"]);
    assert_eq!(clean.status.code(), Some(0), "{}", stdout(&clean));
    assert!(
        stdout(&clean).contains("No file differs from main"),
        "{}",
        stdout(&clean)
    );

    git(d, ceiling, &["switch", "-qc", "feat"]);
    write(d, "a.md", &format!("new top\nnew top\n{}", lines(10)));
    write(d, "log.md", "# Log\n\n- first\n- second\n- third\n");
    write(d, "c.md", "cites a text\n");
    let now = [
        // At the base too, on another line: not new.
        finding("warn", "a.md:5", "old", ""),
        finding("warn", "a.md:12", "new in a touched file", ""),
        finding("info", "a.md:7", "a note", ""),
        finding("warn", "b.md:2", "in an untouched file", ""),
        finding("warn", "log.md:2", "in sealed text", ""),
        finding("error", "log.md:5", "in appended text", ""),
        finding(
            "warn",
            "c.md:1",
            "no source text",
            r#"{"path":"sources/9.txt"}"#,
        ),
    ];
    write(d, "f.jsonl", &now.join("\n"));
    git(d, ceiling, &["commit", "-qam", "change"]);

    let out = lab(d, ceiling, tmp, &["check", "--new-only"]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "{text}{}", stderr(&out));
    assert!(text.contains("Against main at the merge base"), "{text}");
    assert!(
        text.contains(
            "2 error(s) or warning(s) the change adds to the files it touches:\n\
             a.md:12 warn t:r: new in a touched file\n\
             log.md:5 error t:r: in appended text\n"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "not gated: 1 info in the files it touches, 1 in text sealed at the merge base, \
             1 naming a missing file that git ignores, 1 in files the change does not touch"
        ),
        "{text}"
    );
    // Same ignored input and the project root's path at the base: not new.
    assert!(!text.contains("from an ignored input"), "{text}");
    assert!(!text.contains("ran in"), "{text}");

    let verbose = stdout(&lab(d, ceiling, tmp, &["check", "--new-only", "--verbose"]));
    assert!(
        verbose
            .contains("New, in text sealed at the merge base:\nlog.md:2 warn t:r: in sealed text"),
        "{verbose}"
    );

    // An accepted finding no longer fails the gate.
    let set = lab(d, ceiling, tmp, &["baseline", "set", "--note", "accepted"]);
    assert!(set.status.success(), "{}", stderr(&set));
    let accepted = lab(d, ceiling, tmp, &["check", "--new-only"]);
    let text = stdout(&accepted);
    assert_eq!(accepted.status.code(), Some(0), "{text}");
    assert!(text.contains("2 accepted in the baseline"), "{text}");

    // Without --new-only, warnings never fail.
    write(d, "f.jsonl", &finding("warn", "a.md:13", "another", ""));
    assert_eq!(lab(d, ceiling, tmp, &["check"]).status.code(), Some(0));
    let uncommitted = lab(d, ceiling, tmp, &["check", "--new-only"]);
    assert_eq!(
        uncommitted.status.code(),
        Some(1),
        "{}",
        stdout(&uncommitted)
    );
    assert!(stdout(&uncommitted).contains("a.md:13 warn t:r: another"));
    // No replay worktree is left behind.
    assert_eq!(
        git(d, ceiling, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
}

#[test]
fn new_only_outside_git_keeps_the_baseline_gate() {
    let dir = tempfile::tempdir().unwrap();
    let ceiling = dir.path();
    let d = &ceiling.join("p");
    std::fs::create_dir_all(d).unwrap();
    assert!(
        lab(d, ceiling, ceiling, &["init", "--name", "demo"])
            .status
            .success()
    );
    let mut cfg = std::fs::read_to_string(d.join("research.toml")).unwrap();
    cfg += "\n[[lab.checks]]\nname = \"claims\"\nrun = [\"cat\", \"f.jsonl\"]\nversion = []\n";
    write(d, "research.toml", &cfg);
    write(d, "f.jsonl", &finding("warn", "a.md:1", "w", ""));
    let out = lab(d, ceiling, ceiling, &["check", "--new-only"]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("No change check: the project is not in a git repository"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn new_only_in_a_linked_worktree_reads_the_main_checkouts_ignored_input_at_the_base() {
    let dir = tempfile::tempdir().unwrap();
    let ceiling = dir.path();
    let tmp = &ceiling.join("tmp");
    let main = &ceiling.join("main");
    std::fs::create_dir_all(tmp).unwrap();
    std::fs::create_dir_all(main).unwrap();
    git(main, ceiling, &["init", "-q", "-b", "main"]);
    assert!(
        lab(main, ceiling, tmp, &["init", "--name", "demo"])
            .status
            .success()
    );
    let mut cfg = std::fs::read_to_string(main.join("research.toml")).unwrap();
    cfg += "\n[[lab.checks]]\nname = \"claims\"\nrun = [\"cat\", \"data/f.jsonl\"]\nversion = []\n";
    write(main, "research.toml", &cfg);
    write(main, ".gitignore", ".research/\ndata/\n");
    write(main, "a.md", "one\ntwo\n");
    git(main, ceiling, &["add", "-A"]);
    git(main, ceiling, &["commit", "-qm", "base"]);
    // Only the main checkout has the ignored input the check names.
    std::fs::create_dir_all(main.join("data")).unwrap();
    write(main, "data/f.jsonl", &finding("warn", "a.md:1", "old", ""));
    let wt = &ceiling.join("wt");
    git(
        main,
        ceiling,
        &["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()],
    );
    write(wt, "a.md", "one\ntwo\nthree\n");

    let out = lab(wt, ceiling, tmp, &["check", "--new-only"]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{text}{}", stderr(&out));
    assert!(
        text.contains("The change adds no error or warning to the files it touches."),
        "{text}"
    );
    assert!(!text.contains("did not run to completion"), "{text}");
}

#[test]
fn new_only_treats_text_sealed_by_an_earlier_status_as_sealed() {
    let dir = tempfile::tempdir().unwrap();
    let ceiling = dir.path();
    let tmp = &ceiling.join("tmp");
    let d = &ceiling.join("repo");
    std::fs::create_dir_all(tmp).unwrap();
    std::fs::create_dir_all(d.join("docs")).unwrap();
    git(d, ceiling, &["init", "-q", "-b", "main"]);
    assert!(
        lab(d, ceiling, tmp, &["init", "--name", "demo"])
            .status
            .success()
    );
    let mut cfg = std::fs::read_to_string(d.join("research.toml")).unwrap();
    cfg += r#"
[[lab.checks]]
name = "claims"
run = ["cat", "f.jsonl"]
version = []

[lab.archive]
model = 2
seal = "git"
write_indexes = false
statuses = ["proposed", "accepted"]
sealed_statuses = ["accepted"]
inactive_statuses = []

[[lab.archive.index]]
name = "decisions"
title = "Decisions"
include = ["docs/D*.md"]
output = "docs/decisions.md"
id_from = '^(D\d+)-'
"#;
    write(d, "research.toml", &cfg);
    write(d, ".gitignore", ".research/\n");
    let rec = |status: &str| format!("---\nstatus: {status}\n---\n# D1\n\nLine one.\nLine two.\n");
    write(d, "docs/D1-first.md", &rec("accepted"));
    write(d, "f.jsonl", "");
    git(d, ceiling, &["add", "-A"]);
    git(d, ceiling, &["commit", "-qm", "accepted"]);
    // Moving the status back does not unseal the text `accepted` sealed.
    write(d, "docs/D1-first.md", &rec("proposed"));
    git(d, ceiling, &["commit", "-qam", "back to proposed"]);

    git(d, ceiling, &["switch", "-qc", "feat"]);
    write(d, "docs/D1-first.md", &(rec("proposed") + "Line three.\n"));
    let now = [
        finding("warn", "docs/D1-first.md:6", "in sealed text", ""),
        finding("warn", "docs/D1-first.md:8", "in appended text", ""),
    ];
    write(d, "f.jsonl", &now.join("\n"));
    git(d, ceiling, &["commit", "-qam", "change"]);

    let out = lab(d, ceiling, tmp, &["check", "--new-only"]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "{text}{}", stderr(&out));
    assert!(
        text.contains(
            "1 error(s) or warning(s) the change adds to the files it touches:\n\
             docs/D1-first.md:8 warn t:r: in appended text\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("1 in text sealed at the merge base"),
        "{text}"
    );
}

#[test]
fn new_only_reads_no_status_from_a_seal_baseline_after_the_merge_base() {
    let dir = tempfile::tempdir().unwrap();
    let ceiling = dir.path();
    let tmp = &ceiling.join("tmp");
    let d = &ceiling.join("repo");
    std::fs::create_dir_all(tmp).unwrap();
    std::fs::create_dir_all(d.join("docs")).unwrap();
    git(d, ceiling, &["init", "-q", "-b", "main"]);
    assert!(
        lab(d, ceiling, tmp, &["init", "--name", "demo"])
            .status
            .success()
    );
    let base_cfg = std::fs::read_to_string(d.join("research.toml")).unwrap()
        + r#"
[[lab.checks]]
name = "claims"
run = ["cat", "f.jsonl"]
version = []

[lab.archive]
model = 2
seal = "git"
write_indexes = false
statuses = ["proposed", "accepted"]
inactive_statuses = []
sealed_statuses = ["accepted"]

[[lab.archive.index]]
name = "decisions"
title = "Decisions"
include = ["docs/D*.md"]
output = "docs/decisions.md"
id_from = '^(D\d+)-'
"#;
    write(d, "research.toml", &base_cfg);
    write(d, ".gitignore", ".research/\n");
    let rec = |status: &str| format!("---\nstatus: {status}\n---\n# D1\n\nLine one.\nLine two.\n");
    write(d, "docs/D1-first.md", &rec("proposed"));
    write(d, "f.jsonl", "");
    git(d, ceiling, &["add", "-A"]);
    git(d, ceiling, &["commit", "-qm", "proposed"]);
    git(d, ceiling, &["branch", "feat"]);
    // Accepted on main after the branch left it, and named as the baseline.
    write(d, "docs/D1-first.md", &rec("accepted"));
    git(d, ceiling, &["commit", "-qam", "accepted"]);
    let accepted = git(d, ceiling, &["rev-parse", "HEAD"]);

    git(d, ceiling, &["switch", "-q", "feat"]);
    write(
        d,
        "research.toml",
        &base_cfg.replace(
            "inactive_statuses = []",
            &format!("inactive_statuses = []\nseal_baseline = \"{accepted}\""),
        ),
    );
    write(
        d,
        "f.jsonl",
        &finding("warn", "docs/D1-first.md:6", "new in unsealed text", ""),
    );
    write(d, "docs/D1-first.md", &(rec("proposed") + "Line three.\n"));
    git(d, ceiling, &["commit", "-qam", "change"]);

    let out = lab(d, ceiling, tmp, &["check", "--new-only"]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "{text}{}", stderr(&out));
    assert!(
        text.contains("docs/D1-first.md:6 warn t:r: new in unsealed text"),
        "{text}"
    );
}
