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
