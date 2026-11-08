//! End-to-end: init, check runs against a baseline, lessons, the verified gate, and the brief.

use std::path::Path;
use std::process::{Command, Output};

fn lab(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lab"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run lab")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

const ERR: &str = r#"{"schema":"rf.finding/1","tool":"claimcheck","rule":"quote-not-found","severity":"error","subject":"doc.md:N","message":"quote M missing"}"#;

/// A project whose `claims` check prints `f.jsonl` and exits 1 when it holds an error.
fn project(d: &Path) {
    assert!(lab(d, &["init", "--name", "demo"]).status.success());
    let mut cfg = std::fs::read_to_string(d.join("research.toml")).unwrap();
    assert!(cfg.contains("name = \"demo\""));
    cfg += "\n[[lab.checks]]\nname = \"claims\"\nrun = [\"sh\", \"-c\", \"cat f.jsonl; grep -q error f.jsonl && exit 1; exit 0\"]\n";
    std::fs::write(d.join("research.toml"), cfg).unwrap();
}

fn errors(ids: &[usize]) -> String {
    ids.iter()
        .map(|i| {
            ERR.replace('N', &i.to_string())
                .replace('M', &i.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn full_cycle() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    project(d);

    std::fs::write(d.join("f.jsonl"), errors(&[1, 2, 3])).unwrap();
    let first = lab(d, &["check"]);
    assert_eq!(first.status.code(), Some(1), "{}", stdout(&first));
    assert!(
        stdout(&first).contains("3 error(s):"),
        "failing errors are listed without --verbose: {}",
        stdout(&first)
    );
    let set = lab(d, &["baseline", "set", "--note", "backlog before the gate"]);
    assert!(set.status.success(), "{}", stderr(&set));
    assert!(stdout(&set).contains("claims: 3 finding(s) accepted"));

    // Two fixed; the remaining one is in the baseline, even after lines move.
    std::fs::write(
        d.join("f.jsonl"),
        errors(&[1]).replace("doc.md:1", "doc.md:40"),
    )
    .unwrap();
    let second = lab(d, &["check", "--new-only"]);
    assert_eq!(
        second.status.code(),
        Some(0),
        "an accepted error is not new: {}",
        stdout(&second)
    );

    let retro = stdout(&lab(d, &["retro"]));
    assert!(retro.contains("claimcheck:quote-not-found"), "{retro}");
    assert!(retro.contains("3 distinct finding(s)"), "{retro}");

    // The exact command the skill gives.
    let add = lab(
        d,
        &[
            "lesson",
            "add",
            "--id",
            "quote-from-text",
            "--title",
            "Quote from text",
            "--kind",
            "bad",
            "--rule",
            "Copy quotations from extracted text.",
            "--evidence",
            "doc.md:2",
            "--metric",
            "claimcheck:quote-not-found",
            "--status",
            "adopted",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    let refused = lab(
        d,
        &[
            "lesson",
            "set",
            "quote-from-text",
            "verified",
            "--note",
            "looks better",
        ],
    );
    assert_eq!(refused.status.code(), Some(2));
    assert!(stderr(&refused).contains("cannot be verified"));

    let brief = stdout(&lab(d, &["brief"]));
    assert!(
        brief.contains("Learned from failures:\n\n- Copy quotations from extracted text."),
        "{brief}"
    );
    assert!(brief.contains("1 errors"), "{brief}");
    assert!(
        !stdout(&lab(d, &["retro"])).contains("- error"),
        "the lesson now covers the rule"
    );
}

#[test]
fn rerunning_does_not_clear_the_new_only_gate() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    project(d);
    std::fs::write(d.join("f.jsonl"), errors(&[1])).unwrap();
    assert_eq!(lab(d, &["check"]).status.code(), Some(1));
    assert!(
        lab(d, &["baseline", "set", "--note", "backlog"])
            .status
            .success()
    );

    std::fs::write(d.join("f.jsonl"), errors(&[1, 2])).unwrap();
    for _ in 0..3 {
        let out = lab(d, &["check", "--new-only"]);
        assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
        assert!(stdout(&out).contains("1 error(s) not in the baseline"));
    }
    let shown = stdout(&lab(d, &["baseline", "show"]));
    assert!(shown.contains("claims: 1 finding(s)") && shown.contains(": claims: backlog"));
}

#[test]
fn missing_agent_findings_do_not_break_the_brief() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    project(d);
    let cfg = std::fs::read_to_string(d.join("research.toml"))
        .unwrap()
        .replace("# agent_findings", "agent_findings");
    std::fs::write(d.join("research.toml"), cfg).unwrap();
    let brief = lab(d, &["brief"]);
    assert!(brief.status.success(), "{}", stderr(&brief));
    assert!(stdout(&brief).contains("Rules from lessons"));
    assert!(stderr(&brief).contains("does not exist yet"));
    let explicit = lab(d, &["retro", "--agent-findings", "nowhere.jsonl"]);
    assert_eq!(explicit.status.code(), Some(2), "a named file must exist");
}

#[test]
fn retro_counts_agent_findings_once_per_session_group() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    project(d);
    let finding = |id: &str| {
        format!(
            r#"{{"schema":"rf.finding/1","tool":"agentprof","rule":"reread-file","severity":"warn","subject":"claude:{id}","message":"reread"}}"#
        )
    };
    let session = |id: &str, parent: &str| {
        format!(r#"{{"schema":"rf.session/1","source":"claude","id":"{id}","parent_id":{parent}}}"#)
    };
    std::fs::create_dir(d.join("agent")).unwrap();
    std::fs::write(
        d.join("agent/findings.jsonl"),
        ["a", "b", "c"].map(finding).join("\n"),
    )
    .unwrap();
    let sessions = [
        session("root", "null"),
        session("a", "\"root\""),
        session("b", "\"root\""),
        session("c", "\"b\""),
    ];
    std::fs::write(d.join("agent/sessions.jsonl"), sessions.join("\n")).unwrap();
    let template = std::fs::read_to_string(d.join("research.toml")).unwrap();
    let config = |extra: &str| {
        let cfg = template.replace(
            "# agent_findings = \".research/agentprof/findings.jsonl\"",
            &format!("agent_findings = \"agent/findings.jsonl\"\n{extra}"),
        );
        std::fs::write(d.join("research.toml"), cfg).unwrap();
    };
    let run = |args: &[&str]| {
        let out = lab(d, args);
        assert!(out.status.success(), "{}", stderr(&out));
        (stdout(&out), stderr(&out))
    };
    let explicit = ["retro", "--agent-findings", "agent/findings.jsonl"];
    let none = "0 recurring rule(s) without a lesson";

    config("");
    let (grouped, _) = run(&["retro"]);
    assert!(
        grouped.contains(none),
        "three subagents of one root are one recurrence: {grouped}"
    );
    let (brief, _) = run(&["brief"]);
    assert!(!brief.contains("recurring finding rule(s)"), "{brief}");

    // A configured sessions file that is missing leaves the brief ungrouped,
    // but a findings file named on the command line uses its own sessions.
    config("agent_sessions = \"nowhere.jsonl\"");
    let (brief, note) = run(&["brief"]);
    assert!(brief.contains("1 recurring finding rule(s)"), "{brief}");
    assert!(note.contains("not once per session group"), "{note}");
    let (grouped, _) = run(&explicit);
    assert!(grouped.contains(none), "{grouped}");

    std::fs::remove_file(d.join("agent/sessions.jsonl")).unwrap();
    let (ungrouped, _) = run(&explicit);
    assert!(
        ungrouped.contains("not once per session group: sessions file unreadable"),
        "{ungrouped}"
    );
    assert!(
        ungrouped.contains("- warn agentprof:reread-file: 3 distinct finding(s)"),
        "{ungrouped}"
    );
}

/// Isolates git from the user's configuration and environment, and stops
/// repository discovery at `ceiling`.
fn isolated<'a>(cmd: &'a mut Command, ceiling: &Path) -> &'a mut Command {
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CEILING_DIRECTORIES", ceiling)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
}

/// Runs `lab check` in `d` and returns the header of the run it stored.
fn check_header(d: &Path, ceiling: &Path) -> serde_json::Value {
    let dir = d.join(".research/lab/checks");
    let runs = || -> std::collections::BTreeSet<std::path::PathBuf> {
        std::fs::read_dir(&dir)
            .map(|r| r.map(|e| e.unwrap().path()).collect())
            .unwrap_or_default()
    };
    let before = runs();
    let out = isolated(&mut Command::new(env!("CARGO_BIN_EXE_lab")), ceiling)
        .arg("check")
        .current_dir(d)
        .output()
        .expect("run lab");
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    let new: Vec<_> = runs().difference(&before).cloned().collect();
    assert_eq!(new.len(), 1, "{new:?}");
    let text = std::fs::read_to_string(&new[0]).unwrap();
    serde_json::from_str(text.lines().next().unwrap()).unwrap()
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

#[test]
fn check_runs_record_the_commit_and_a_dirty_work_tree() {
    let dir = tempfile::tempdir().unwrap();
    // Git does not look for a repository in the ceiling directory or above it.
    let ceiling = dir.path();
    let top = &ceiling.join("repo");
    let d = &top.join("proj");
    std::fs::create_dir_all(d).unwrap();
    project(d);
    std::fs::write(d.join("f.jsonl"), "").unwrap();
    let outside = check_header(d, ceiling);
    assert!(
        outside["commit"].is_null() && outside["dirty"].is_null(),
        "{outside}"
    );

    git(top, ceiling, &["init", "-q"]);
    git(top, ceiling, &["add", "-A"]);
    git(top, ceiling, &["commit", "-qm", "init"]);
    let head = git(top, ceiling, &["rev-parse", "HEAD"]);
    // Runs stored after the commit are untracked but in the state directory,
    // not checked text.
    check_header(d, ceiling);
    let clean = check_header(d, ceiling);
    assert_eq!(clean["commit"], head.as_str());
    assert_eq!(clean["dirty"], false, "{clean}");

    std::fs::write(d.join("new.md"), "untracked text").unwrap();
    assert_eq!(check_header(d, ceiling)["dirty"], true, "an untracked file");
    std::fs::remove_file(d.join("new.md")).unwrap();
    std::fs::write(d.join("f.jsonl"), "\n").unwrap();
    assert_eq!(check_header(d, ceiling)["dirty"], true, "a tracked change");

    // An ignored project root lists no changes, but no commit holds its files.
    git(top, ceiling, &["checkout", "-q", "--", "proj"]);
    std::fs::write(top.join(".gitignore"), "proj/\n").unwrap();
    git(top, ceiling, &["rm", "-rq", "--cached", "proj"]);
    git(top, ceiling, &["add", "-A"]);
    git(top, ceiling, &["commit", "-qm", "ignore"]);
    let ignored = check_header(d, ceiling);
    assert_eq!(ignored["dirty"], true, "{ignored}");
}

#[test]
fn a_bare_relative_config_works() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    project(d);
    std::fs::write(d.join("f.jsonl"), "").unwrap();
    let out = lab(d, &["--config", "research.toml", "check"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
}

#[test]
fn a_lesson_without_a_metric_needs_force_to_be_verified() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    project(d);
    let add = lab(
        d,
        &[
            "lesson",
            "add",
            "--id",
            "no-metric",
            "--title",
            "T",
            "--kind",
            "good",
            "--rule",
            "Do it.",
            "--status",
            "adopted",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    let set = |extra: &[&str]| {
        let mut args = vec![
            "lesson",
            "set",
            "no-metric",
            "verified",
            "--note",
            "seems fine",
        ];
        args.extend_from_slice(extra);
        lab(d, &args)
    };
    let refused = set(&[]);
    assert_eq!(refused.status.code(), Some(2));
    assert!(
        stderr(&refused).contains("no metric"),
        "{}",
        stderr(&refused)
    );
    assert!(set(&["--force"]).status.success());
}

#[test]
fn evaluate_counts_a_baselined_new_finding_but_not_a_moved_one() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    project(d);
    let add = lab(
        d,
        &[
            "lesson",
            "add",
            "--id",
            "quote-check",
            "--title",
            "T",
            "--kind",
            "bad",
            "--rule",
            "Check quotes.",
            "--metric",
            "claimcheck:quote-not-found",
            "--status",
            "adopted",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    // The first run since adoption holds the backlog.
    std::fs::write(d.join("f.jsonl"), errors(&[1, 2])).unwrap();
    lab(d, &["check"]);
    // Quote 1 moves to a new file; quote 3 is new and the baseline accepts it.
    std::fs::write(
        d.join("f.jsonl"),
        errors(&[1, 2, 3]).replace("doc.md:1", "moved/doc.md:1"),
    )
    .unwrap();
    lab(d, &["check"]);
    assert!(
        lab(d, &["baseline", "set", "--note", "accept 3"])
            .status
            .success()
    );
    assert_eq!(lab(d, &["check", "--new-only"]).status.code(), Some(0));

    let out = lab(d, &["evaluate", "--format", "jsonl"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let e: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(e["verdict"], "collecting");
    assert_eq!(e["after"]["n"], 3);
    assert_eq!(e["introduced"]["count"], 1, "{e}");
    assert_eq!(e["introduced"]["findings"][0]["message"], "quote 3 missing");
    let text = stdout(&lab(d, &["evaluate"]));
    assert!(text.contains("  +1 quote 3 missing [doc.md:3]"), "{text}");

    // The loop sends the introduced finding, and the brief shows it.
    let mut cfg = std::fs::read_to_string(d.join("research.toml")).unwrap();
    cfg += "\n[lab.loop]\nmeasure = [[\"true\"]]\nnotify = [\"sh\", \"-c\", \"cat >> sent.jsonl; echo; echo board:1\"]\n";
    std::fs::write(d.join("research.toml"), cfg).unwrap();
    let out = lab(d, &["loop"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert!(stdout(&out).contains("1 described"), "{}", stdout(&out));
    let sent = std::fs::read_to_string(d.join("sent.jsonl")).unwrap();
    let signal: serde_json::Value = serde_json::from_str(sent.lines().next().unwrap()).unwrap();
    assert_eq!(signal["kind"], "lesson-introduced");
    assert!(
        signal["body"]
            .as_str()
            .unwrap()
            .contains("+1 quote 3 missing [doc.md:3]"),
        "{signal}"
    );
    let brief = stdout(&lab(d, &["brief"]));
    assert!(
        brief.contains("- `quote-check`: 1 introduced since the first complete run after adoption"),
        "{brief}"
    );

    // A review moves the count to the reviewed run, so the signal clears.
    assert!(
        !lab(
            d,
            &[
                "lesson",
                "review",
                "quote-check",
                "--count",
                "1",
                "--note",
                " "
            ]
        )
        .status
        .success()
    );
    let out = lab(
        d,
        &[
            "lesson",
            "review",
            "quote-check",
            "--count",
            "2",
            "--note",
            "x",
        ],
    );
    assert!(
        stderr(&out)
            .contains("has 1 findings introduced since the first complete run after adoption"),
        "{}",
        stderr(&out)
    );
    let out = lab(
        d,
        &[
            "lesson",
            "review",
            "quote-check",
            "--count",
            "1",
            "--note",
            "quote 3 is accepted",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("reviewed 1 finding(s) introduced since"),
        "{}",
        stdout(&out)
    );
    let lesson = std::fs::read_to_string(d.join("lessons/quote-check.md")).unwrap();
    assert!(lesson.contains("\nreviewed: "), "{lesson}");
    assert!(
        lesson.contains(
            ": reviewed 1 finding(s) introduced since the first complete run after adoption"
        ),
        "{lesson}"
    );
    let out = lab(d, &["evaluate", "--format", "jsonl"]);
    let e: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(e["introduced"]["count"].as_u64(), Some(0), "{e}");
    assert_eq!(
        e["introduced"]["reviewed"], e["introduced"]["latest_run"],
        "{e}"
    );
    let brief = stdout(&lab(d, &["brief"]));
    assert!(
        brief.contains("- `quote-check`: reviewed after the last `lab loop`"),
        "{brief}"
    );
    let out = lab(d, &["loop"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert!(!stdout(&out).contains("[notified]"), "{}", stdout(&out));
    let brief = stdout(&lab(d, &["brief"]));
    assert!(
        brief.contains("- `quote-check`: none introduced since the reviewed run of "),
        "{brief}"
    );
    let out = lab(
        d,
        &[
            "lesson",
            "review",
            "quote-check",
            "--count",
            "1",
            "--note",
            "again",
        ],
    );
    assert!(
        stderr(&out).contains("has no findings introduced since the reviewed run of"),
        "{}",
        stderr(&out)
    );
}

/// Commits everything in `top` with a fixed committer time.
fn commit_at(top: &Path, ceiling: &Path, date: &str, message: &str) -> String {
    let out = isolated(&mut Command::new("git"), ceiling)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["commit", "-qm", message])
        .env("GIT_COMMITTER_DATE", date)
        .env("GIT_AUTHOR_DATE", date)
        .current_dir(top)
        .output()
        .expect("run git");
    assert!(out.status.success(), "{}", stderr(&out));
    git(top, ceiling, &["rev-parse", "HEAD"])
}

/// Runs `lab` with git isolated and temporary files in `tmp`.
fn lab_in(d: &Path, ceiling: &Path, tmp: &Path, args: &[&str]) -> Output {
    isolated(&mut Command::new(env!("CARGO_BIN_EXE_lab")), ceiling)
        .args(args)
        .env("TMPDIR", tmp)
        .current_dir(d)
        .output()
        .expect("run lab")
}

#[test]
fn check_at_replays_commits_in_a_temporary_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let ceiling = dir.path();
    let tmp = &ceiling.join("tmp");
    std::fs::create_dir_all(tmp).unwrap();
    let top = &ceiling.join("repo");
    let d = &top.join("proj");
    std::fs::create_dir_all(top).unwrap();
    git(top, ceiling, &["init", "-q"]);
    std::fs::write(top.join("README"), "before the project").unwrap();
    git(top, ceiling, &["add", "-A"]);
    let empty = commit_at(top, ceiling, "2026-09-30T00:00:00Z", "readme");

    // The check prints the committed f.jsonl and the git-ignored source
    // text's measure, which only the work tree holds: the commits hold only
    // the directory's .gitignore. Its version is the commit's v.txt.
    std::fs::create_dir_all(d).unwrap();
    assert!(lab(d, &["init", "--name", "demo"]).status.success());
    let mut cfg = std::fs::read_to_string(d.join("research.toml")).unwrap();
    cfg += "\n[[lab.checks]]\nname = \"claims\"\nrun = [\"sh\", \"-c\", \"cat f.jsonl; echo; cat sources/m.jsonl; exit 0\"]\nversion = [\"sh\", \"-c\", \"cat v.txt\"]\nuncommitted_inputs = [\"sources\", \"gone\"]\n";
    std::fs::write(d.join("research.toml"), cfg).unwrap();
    std::fs::write(d.join(".gitignore"), ".research/\n").unwrap();
    std::fs::create_dir_all(d.join("sources")).unwrap();
    std::fs::write(d.join("sources/.gitignore"), "*\n!.gitignore\n").unwrap();
    std::fs::write(d.join("v.txt"), "claims 1.0\n").unwrap();
    std::fs::write(
        d.join("sources/m.jsonl"),
        r#"{"schema":"rf.measure/1","tool":"claimcheck","unit":"citing-sentence","count":2,"keys":["00000000000000aa","00000000000000bb"]}"#,
    )
    .unwrap();
    std::fs::write(d.join("f.jsonl"), "").unwrap();
    git(top, ceiling, &["add", "-A"]);
    let first = commit_at(top, ceiling, "2026-10-01T00:00:00Z", "first");
    std::fs::write(d.join("f.jsonl"), errors(&[1])).unwrap();
    std::fs::write(d.join("v.txt"), "claims 2.0\n").unwrap();
    git(top, ceiling, &["add", "-A"]);
    let second = commit_at(top, ceiling, "2026-10-02T12:30:00Z", "second");
    // Uncommitted text the replay must not see.
    std::fs::write(d.join("f.jsonl"), errors(&[1, 2, 3])).unwrap();

    // A worktree that a killed replay left, and an unrelated one whose
    // directory is gone for now, as on an unmounted disk.
    let leftover = tmp.join("lab-replay-999999-1");
    let other = ceiling.join("other");
    for path in [&leftover, &other] {
        let path = path.to_str().unwrap();
        git(top, ceiling, &["worktree", "add", "--detach", "-q", path]);
    }
    std::fs::rename(&other, ceiling.join("moved")).unwrap();

    // No live run yet: a replay is not one, so no baseline can come from it.
    let out = lab_in(
        d,
        ceiling,
        tmp,
        &[
            "check",
            "--at",
            &first,
            "--at",
            &format!("{first}..{second}"),
        ],
    );
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    let text = stdout(&out);
    assert_eq!(text.matches("replayed ").count(), 2, "{text}");
    assert!(text.contains("read sources from the work tree"), "{text}");
    assert!(
        stderr(&out).contains("left by a stopped run"),
        "{}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("uncommitted input `gone` is missing"),
        "{}",
        stderr(&out)
    );
    let mut runs: Vec<serde_json::Value> = std::fs::read_dir(d.join(".research/lab/checks"))
        .unwrap()
        .map(|e| {
            let text = std::fs::read_to_string(e.unwrap().path()).unwrap();
            serde_json::from_str(text.lines().next().unwrap()).unwrap()
        })
        .collect();
    runs.sort_by_key(|h| h["at"].as_str().unwrap().to_string());
    let summary: Vec<_> = runs
        .iter()
        .map(|h| {
            let c = &h["checks"][0];
            (
                h["commit"].as_str().unwrap(),
                h["at"].as_str().unwrap(),
                h["dirty"].as_bool(),
                h["replayed"].as_bool(),
                c["findings"].as_u64(),
                c["version"].as_str(),
                c["uncommitted_inputs"].clone(),
            )
        })
        .collect();
    let inputs = serde_json::json!(["sources"]);
    assert_eq!(
        summary,
        [
            (
                first.as_str(),
                "2026-10-01T00:00:00Z",
                Some(false),
                Some(true),
                Some(0),
                Some("claims 1.0"),
                inputs.clone()
            ),
            (
                second.as_str(),
                "2026-10-02T12:30:00Z",
                Some(false),
                Some(true),
                Some(1),
                Some("claims 2.0"),
                inputs
            ),
        ]
    );
    let set = lab_in(d, ceiling, tmp, &["baseline", "set", "--note", "x"]);
    assert!(
        stderr(&set).contains("has no completed run"),
        "{}",
        stderr(&set)
    );

    // Every path removes the worktree: success, a commit without the project
    // root, and a revision that names nothing. The leftover is gone too, but
    // the unrelated worktree stays registered.
    let worktrees = || git(top, ceiling, &["worktree", "list", "--porcelain"]);
    let tmp_entries = || std::fs::read_dir(tmp).unwrap().count();
    let registered = || {
        let list = worktrees();
        assert!(list.contains("/other\n"), "{list}");
        list.matches("worktree ").count()
    };
    assert_eq!(registered(), 2);
    assert_eq!(tmp_entries(), 0);
    let out = lab_in(d, ceiling, tmp, &["check", "--at", &empty]);
    assert_eq!(out.status.code(), Some(2), "{}", stdout(&out));
    assert!(
        stderr(&out).contains("has no `proj`, the project root; skipped")
            && stderr(&out).contains("no commit named holds `proj`"),
        "{}",
        stderr(&out)
    );
    let out = lab_in(d, ceiling, tmp, &["check", "--at", "no-such-rev"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert_eq!(registered(), 2);
    assert_eq!(tmp_entries(), 0);
    // The work tree, with its uncommitted text, is untouched.
    assert_eq!(
        std::fs::read_to_string(d.join("f.jsonl")).unwrap(),
        errors(&[1, 2, 3])
    );
    let out = lab_in(d, ceiling, tmp, &["check", "--at", &second, "--new-only"]);
    assert_eq!(out.status.code(), Some(2), "--at conflicts with --new-only");
}
