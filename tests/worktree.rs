//! End-to-end: in a linked git worktree, `lab check` finds the gitignored
//! files that only the main checkout has: link targets and check arguments.
//! A replay's worktree does not.

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::{Command, Output};

const LAB: &str = env!("CARGO_BIN_EXE_lab");

fn run(program: &str, dir: &Path, ceiling: &Path, args: &[&str]) -> Output {
    Command::new(program)
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CEILING_DIRECTORIES", ceiling)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("run")
}

fn git(dir: &Path, ceiling: &Path, args: &[&str]) {
    let mut all = vec!["-c", "user.name=t", "-c", "user.email=t@example.com"];
    all.extend_from_slice(args);
    let out = run("git", dir, ceiling, &all);
    assert!(out.status.success(), "git {args:?}: {}", stderr(&out));
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

/// Reports the file named by its argument: info when it can read it, an error when not.
const PROBE: &str = r#"if [ -f "$1" ]; then
  printf '{"schema":"rf.finding/1","tool":"probe","rule":"read","severity":"info","subject":"%s","message":"read %s"}\n' "$1" "$1"
  exit 0
fi
printf '{"schema":"rf.finding/1","tool":"probe","rule":"unreadable","severity":"error","subject":"%s","message":"cannot read %s"}\n' "$1" "$1"
exit 1
"#;

/// A project in a subdirectory with a main checkout and a linked worktree.
/// Ignored, in the main checkout only: `data/`, `sub/data/`, `notes/` and
/// `private/`.
fn setup(ceiling: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let (main, linked) = (ceiling.join("main"), ceiling.join("linked"));
    let (m, w) = (main.join("proj"), linked.join("proj"));
    write(
        &m,
        "research.toml",
        &format!(
            r#"[project]
name = "demo"
timezone = "UTC"

[lab.archive]
links = ["docs/*.md"]

[[lab.checks]]
name = "archive"
run = ["{LAB}", "archive", "check", "--format", "jsonl"]

[[lab.checks]]
name = "manifest"
run = ["sh", "probe.sh", "data/manifest.json"]

[[lab.checks]]
name = "nested"
cwd = "sub"
run = ["sh", "../probe.sh", "data/list.json"]

[[lab.checks]]
name = "absent"
run = ["sh", "probe.sh", "data/absent.json"]
"#
        ),
    );
    write(&m, "probe.sh", PROBE);
    write(&m, "sub/README.md", "# Sub\n");
    write(
        &m,
        "docs/a.md",
        "# A\n\n[data](../data/big.csv)\n[private](../private/secret.pdf)\n\
         [none](../data/none.csv)\n[tracked](tracked.md)\n[notes](../notes/x.md#no-such-heading)\n",
    );
    write(&m, "docs/tracked.md", "# Tracked\n");
    write(&main, ".gitignore", ".research/\ndata/\nnotes/\nprivate/\n");
    git(&main, ceiling, &["init", "-q", "-b", "main"]);
    git(&main, ceiling, &["add", "-A"]);
    git(&main, ceiling, &["commit", "-qm", "project"]);
    git(&main, ceiling, &["worktree", "add", "-q", "../linked"]);
    write(&m, "data/big.csv", "a,b\n");
    write(&m, "data/manifest.json", "{}\n");
    write(&m, "sub/data/list.json", "[]\n");
    write(&m, "notes/x.md", "# X\n");
    write(&m, "private/secret.pdf", "%PDF\n");
    // Searchable but not listable: the link check may only look the file up.
    std::fs::set_permissions(m.join("private"), std::fs::Permissions::from_mode(0o111)).unwrap();
    std::fs::remove_file(w.join("docs/tracked.md")).unwrap();
    (m, w)
}

/// `lab check --verbose` lines of a check's findings: fingerprint, severity,
/// rule, subject and message.
fn findings(out: &Output, tool: &str) -> Vec<String> {
    let mut lines: Vec<String> = stdout(out)
        .lines()
        .filter(|l| l.split(' ').nth(2).is_some_and(|t| t.starts_with(tool)))
        .map(str::to_string)
        .collect();
    lines.sort();
    lines
}

#[test]
fn check_in_a_linked_worktree_finds_ignored_files_in_the_main_checkout() {
    let dir = tempfile::tempdir().unwrap();
    let ceiling = dir.path();
    let (m, w) = setup(ceiling);
    let lab = |d: &Path, args: &[&str]| run(LAB, d, ceiling, args);

    let here = lab(&w, &["check", "--verbose"]);
    let there = lab(&m, &["check", "--verbose"]);
    std::fs::set_permissions(m.join("private"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let (h, t) = (stdout(&here), stdout(&there));

    // Links: ignored targets the main checkout has are found, and only looked
    // up, so the missing heading in notes/x.md is not reported. An ignored
    // target that exists nowhere and a tracked file deleted in the worktree
    // stay broken.
    let links = findings(&here, "lab:broken");
    assert_eq!(links.len(), 2, "{h}");
    assert!(links.iter().any(|l| l.contains("`tracked.md`")), "{h}");
    let main_links = findings(&there, "lab:broken-link");
    assert_eq!(main_links.len(), 1, "{t}");
    assert!(main_links[0].contains("`../data/none.csv`"), "{t}");
    assert!(links.contains(&main_links[0]), "the same fingerprint: {h}");

    // Arguments: an ignored file the worktree lacks is the main checkout's,
    // also below a check's cwd, and findings name it as the check does, so
    // fingerprints match the main checkout's.
    let probe = findings(&here, "probe:");
    assert_eq!(probe, findings(&there, "probe:"), "{h}\n{t}");
    assert_eq!(probe.len(), 3, "{h}");
    assert!(
        probe
            .iter()
            .any(|l| l.ends_with("info probe:read data/manifest.json: read data/manifest.json")),
        "{h}"
    );
    assert!(
        probe
            .iter()
            .any(|l| l.ends_with("info probe:read data/list.json: read data/list.json")),
        "{h}"
    );
    assert!(
        probe.iter().any(|l| l
            .ends_with("error probe:unreadable data/absent.json: cannot read data/absent.json")),
        "{h}"
    );
    let main_root = m.canonicalize().unwrap();
    let e = stderr(&here);
    assert!(
        e.contains(&format!(
            "check `manifest`: this linked git worktree has no data/manifest.json, which git \
             ignores; using the main checkout's {}",
            main_root.join("data/manifest.json").display()
        )),
        "{e}"
    );
    assert!(e.contains("check `nested`: this linked git worktree has no sub/data/list.json"));
    assert!(!e.contains("absent"), "{e}");
    assert!(
        !stderr(&there).contains("main checkout"),
        "{}",
        stderr(&there)
    );

    // Nothing was copied or linked into the worktree.
    for rel in ["data", "notes", "private"] {
        assert!(!w.join(rel).exists(), "{rel}");
    }
}

/// Reports `data/text.txt`, a file no argument names (as a registry names a
/// source text): a warning with the file as evidence when it is missing.
const TEXTS: &str = r#"[ -f data/text.txt ] && exit 0
printf '{"schema":"rf.finding/1","tool":"texts","rule":"source-text-missing","severity":"warn","subject":"docs/a.md:1","message":"no source text","evidence":[{"path":"data/text.txt"}]}\n'
"#;

#[test]
fn a_worktree_that_lacks_a_named_ignored_file_stores_no_run() {
    let dir = tempfile::tempdir().unwrap();
    let ceiling = dir.path();
    let (m, w) = setup(ceiling);
    std::fs::set_permissions(m.join("private"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let lab = |d: &Path| run(LAB, d, ceiling, &["check", "--only", "texts"]);
    let runs = |d: &Path| {
        std::fs::read_dir(d.join(".research/lab/checks"))
            .map(|r| r.count())
            .unwrap_or(0)
    };
    for d in [&m, &w] {
        let cfg = std::fs::read_to_string(d.join("research.toml")).unwrap();
        write(
            d,
            "research.toml",
            &format!("{cfg}\n[[lab.checks]]\nname = \"texts\"\nrun = [\"sh\", \"texts.sh\"]\n"),
        );
        write(d, "texts.sh", TEXTS);
    }
    write(&m, "data/text.txt", "text\n");

    // The main checkout has the text; the worktree lacks it, so its finding
    // is not the main checkout's and no run is stored.
    let out = lab(&w);
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    let e = stderr(&out);
    assert!(
        e.contains("this linked git worktree lacks 1 git-ignored file(s) that the findings name")
            && e.contains("such as data/text.txt")
            && e.contains("no run is stored"),
        "{e}"
    );
    assert_eq!(runs(&w), 0);

    // A file missing in both checkouts is a finding of both.
    std::fs::remove_file(m.join("data/text.txt")).unwrap();
    let out = lab(&w);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert_eq!(runs(&w), 1, "{}", stdout(&out));

    // Once the worktree has the main checkout's file, the run is stored.
    write(&m, "data/text.txt", "text\n");
    write(&w, "data/text.txt", "text\n");
    let out = lab(&w);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(stdout(&out).contains("stored "), "{}", stdout(&out));
    assert_eq!(runs(&w), 2);
    let out = lab(&m);
    assert!(!stderr(&out).contains("lacks"), "{}", stderr(&out));
}

#[test]
fn a_replay_does_not_look_in_the_main_checkout() {
    let dir = tempfile::tempdir().unwrap();
    let ceiling = dir.path();
    let (m, _) = setup(ceiling);
    std::fs::set_permissions(m.join("private"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = run(
        LAB,
        &m,
        ceiling,
        &[
            "check", "--at", "HEAD", "--only", "archive", "--only", "manifest",
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(!stderr(&out).contains("main checkout"), "{}", stderr(&out));
    let runs = std::fs::read_dir(m.join(".research/lab/checks"))
        .unwrap()
        .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
        .collect::<Vec<_>>()
        .concat();
    // The replayed commit has none of the ignored files, so its links to
    // them are broken and the manifest is unreadable.
    for dest in ["../data/big.csv", "../private/secret.pdf", "../notes/x.md"] {
        assert!(
            runs.contains(&format!("links to `{dest}")),
            "{dest}: {runs}"
        );
    }
    assert!(runs.contains("cannot read data/manifest.json"), "{runs}");
}

#[test]
fn a_link_to_a_file_git_does_not_track_is_a_warning_until_it_is_committed() {
    let dir = tempfile::tempdir().unwrap();
    let ceiling = dir.path();
    let (m, _) = setup(ceiling);
    let lab = |d: &Path, args: &[&str]| run(LAB, d, ceiling, args);
    let untracked = |out: &Output| -> Vec<String> {
        stdout(out)
            .lines()
            .filter(|l| l.contains("untracked-link"))
            .map(str::to_string)
            .collect()
    };
    // Without the broken link and the absent file of `setup`, so that only
    // the new warnings remain.
    write(&m, "docs/a.md", "# A\n");
    write(&m, "data/absent.json", "{}\n");
    write(
        &m,
        "docs/b.md",
        "# B\n\n[run](../runs/r1.jsonl)\n[data](../data/big.csv)\n[tracked](tracked.md)\n\
         [runs](../more/)\n[docs](../docs)\n",
    );
    write(&m, "runs/r1.jsonl", "{}\n");
    write(&m, "more/r2.jsonl", "{}\n");

    // The untracked run and the directory that holds only untracked files are
    // flagged; the ignored data file, the tracked note and the tracked
    // directory with an untracked note in it are not. The untracked note
    // that links them is checked as well.
    let out = lab(&m, &["archive", "check"]);
    let lines = untracked(&out);
    assert_eq!(lines.len(), 2, "{}", stdout(&out));
    assert!(
        lines[0].starts_with("warn untracked-link docs/b.md:3"),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains("`../runs/r1.jsonl`"), "{}", lines[0]);
    assert!(
        lines[1].starts_with("warn untracked-link docs/b.md:6")
            && lines[1].contains("`../more/`, a directory that git tracks nothing in"),
        "{}",
        lines[1]
    );
    // A warning: the gate still passes and names the fix.
    let done = lab(&m, &["archive", "done"]);
    assert_eq!(
        done.status.code(),
        Some(0),
        "{}{}",
        stdout(&done),
        stderr(&done)
    );
    assert!(
        stdout(&done).contains("commit the target in the same commit"),
        "{}",
        stdout(&done)
    );

    let top = m.parent().unwrap();
    git(top, ceiling, &["add", "-A"]);
    git(top, ceiling, &["commit", "-qm", "run"]);
    assert!(untracked(&lab(&m, &["archive", "check"])).is_empty());
}
