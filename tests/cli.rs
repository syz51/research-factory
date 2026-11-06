//! End-to-end: init, two check runs, a lesson, the verified gate, and the brief.

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

const ERR: &str = r#"{"schema":"rf.finding/1","tool":"claimcheck","rule":"quote-not-found","severity":"error","subject":"doc.md:N","message":"quote missing"}"#;

#[test]
fn full_cycle() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    assert!(lab(d, &["init", "--name", "demo"]).status.success());
    let mut cfg = std::fs::read_to_string(d.join("research.toml")).unwrap();
    assert!(cfg.contains("name = \"demo\""));
    cfg += "\n[[lab.checks]]\nname = \"claims\"\nrun = [\"sh\", \"-c\", \"cat f.jsonl; grep -q error f.jsonl && exit 1; exit 0\"]\n";
    std::fs::write(d.join("research.toml"), cfg).unwrap();

    let errs: Vec<String> = (1..=3).map(|i| ERR.replace('N', &i.to_string())).collect();
    std::fs::write(d.join("f.jsonl"), errs.join("\n")).unwrap();
    let first = lab(d, &["check"]);
    assert_eq!(first.status.code(), Some(1), "{}", stdout(&first));

    std::fs::write(d.join("f.jsonl"), &errs[0]).unwrap();
    let second = lab(d, &["check", "--new-only"]);
    assert_eq!(
        second.status.code(),
        Some(0),
        "an old error is not new: {}",
        stdout(&second)
    );
    assert!(stdout(&second).contains("claims"));

    let retro = stdout(&lab(d, &["retro"]));
    assert!(retro.contains("claimcheck:quote-not-found"), "{retro}");

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
            "--metric",
            "claimcheck:quote-not-found",
            "--status",
            "adopted",
        ],
    );
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
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
    assert!(String::from_utf8_lossy(&refused.stderr).contains("cannot be verified"));

    let brief = stdout(&lab(d, &["brief"]));
    assert!(
        brief.contains("[avoid] Copy quotations from extracted text."),
        "{brief}"
    );
    assert!(brief.contains("1 errors"), "{brief}");
    assert!(
        !stdout(&lab(d, &["retro"])).contains("- error"),
        "the lesson now covers the rule"
    );
}
