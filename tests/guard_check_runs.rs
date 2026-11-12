//! The check-run guard of the skill
//! (`skill/research-factory/scripts/guard-check-runs.sh`) refuses tool calls
//! that delete or edit a stored check run, and allows everything else.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const HOOK: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/skill/research-factory/scripts/guard-check-runs.sh"
);

fn isolated<'c>(c: &'c mut Command, ceiling: &Path) -> &'c mut Command {
    c.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CEILING_DIRECTORIES", ceiling)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
}

/// The refusal reason for one PreToolUse event, or `None` when the hook
/// allows it; asserts exit 0.
fn refusal(ceiling: &Path, event: &Value) -> Option<String> {
    let mut child = isolated(&mut Command::new("sh"), ceiling)
        .arg(HOOK)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(event.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    if stdout.trim().is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(&stdout).unwrap();
    let h = &v["hookSpecificOutput"];
    assert_eq!(h["hookEventName"], "PreToolUse");
    assert_eq!(h["permissionDecision"], "deny");
    Some(h["permissionDecisionReason"].as_str().unwrap().to_string())
}

fn bash(cwd: &Path, command: &str) -> Value {
    json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "cwd": cwd,
           "tool_input": {"command": command}})
}

fn git(dir: &Path, ceiling: &Path, args: &[&str]) {
    let out = isolated(&mut Command::new("git"), ceiling)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A git project with one tracked run.
fn project(ceiling: &Path) -> std::path::PathBuf {
    let p = ceiling.join("proj");
    write(&p, "research.toml", "[project]\nname = \"demo\"\n");
    write(
        &p,
        ".research/lab/checks/20261112T000000.000Z.jsonl",
        "{}\n",
    );
    write(&p, "notes/a.md", "# A\n");
    git(&p, ceiling, &["init", "-q"]);
    git(&p, ceiling, &["add", "-A"]);
    git(&p, ceiling, &["commit", "-qm", "init"]);
    p
}

#[test]
fn deleting_or_editing_a_stored_run_is_refused_with_the_rule() {
    let dir = tempfile::tempdir().unwrap();
    let c = dir.path();
    let p = project(c);
    let run = ".research/lab/checks/20261112T000000.000Z.jsonl";

    let reason = refusal(c, &bash(&p, &format!("rm {run}"))).expect("rm is refused");
    assert!(reason.contains("never deleted or edited"), "{reason}");
    assert!(
        reason.contains("commit every run of the session"),
        "{reason}"
    );

    for command in [
        format!("rm -f {run}"),
        format!("sed -i '' 's/a/b/' {run} && git add -A"),
        format!("git rm -q {run}"),
        format!("git checkout -- {run}"),
        format!("mv {run} /tmp/old.jsonl"),
        format!("echo '{{}}' > {run}"),
        format!("echo '{{}}' >> {run}"),
        "ls .research/lab/checks/*.jsonl | xargs rm".to_string(),
        "find .research/lab/checks -name '*.jsonl' -delete".to_string(),
        "find .research/lab/checks -name x -exec rm {} \\;".to_string(),
        "sudo rm -rf .research/lab/checks".to_string(),
        "cd .research/lab && rm checks/20261112T000000.000Z.jsonl".to_string(),
        "rm -rf .research".to_string(),
        format!("rm -rf {}", p.display()),
        "git -C . rm -r .research/lab".to_string(),
    ] {
        assert!(
            refusal(c, &bash(&p, &command)).is_some(),
            "not refused: {command}"
        );
    }
    for tool in ["Edit", "Write", "MultiEdit"] {
        let event = json!({"tool_name": tool, "cwd": p,
                           "tool_input": {"file_path": p.join(run)}});
        assert!(refusal(c, &event).is_some(), "{tool} not refused");
    }
}

#[test]
fn reading_committing_and_other_files_are_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let c = dir.path();
    let p = project(c);
    let run = ".research/lab/checks/20261112T000000.000Z.jsonl";
    for command in [
        format!("git add {run} && rm notes/a.md"),
        format!("cat {run} | jq .rule 2>/dev/null"),
        format!("git restore --staged {run}"),
        "find .research/lab/checks -name '*.jsonl' -exec cat {} \\;".to_string(),
        format!("sed -n 1p {run}"),
        "lab check --new-only".to_string(),
        "rm -rf notes".to_string(),
        "git clean -fd".to_string(),
        "echo 'rm .research/lab/checks/x.jsonl'".to_string(),
    ] {
        assert_eq!(refusal(c, &bash(&p, &command)), None, "refused: {command}");
    }
    let event = json!({"tool_name": "Write", "cwd": p,
                       "tool_input": {"file_path": p.join("notes/a.md")}});
    assert_eq!(refusal(c, &event), None);
    let event = json!({"tool_name": "Read", "cwd": p, "tool_input": {"file_path": p.join(run)}});
    assert_eq!(refusal(c, &event), None);
}

#[test]
fn git_clean_is_refused_while_a_run_is_untracked() {
    let dir = tempfile::tempdir().unwrap();
    let c = dir.path();
    let p = project(c);
    write(
        &p,
        ".research/lab/checks/20261112T010000.000Z.jsonl",
        "{}\n",
    );
    assert!(refusal(c, &bash(&p, "git clean -fd")).is_some());
    assert!(refusal(c, &bash(&p.join("notes"), "git clean -f")).is_some());
    assert_eq!(refusal(c, &bash(&p, "git clean -nd")), None);
    assert_eq!(refusal(c, &bash(&p, "git clean --dry-run -fd")), None);

    // An ignored run goes only with -x or -X.
    git(&p, c, &["add", "-A"]);
    git(&p, c, &["commit", "-qm", "run"]);
    write(&p, ".gitignore", "/.research/lab/checks/*.ignored.jsonl\n");
    write(&p, ".research/lab/checks/a.ignored.jsonl", "{}\n");
    git(&p, c, &["add", ".gitignore"]);
    git(&p, c, &["commit", "-qm", "ignore"]);
    assert_eq!(refusal(c, &bash(&p, "git clean -fd")), None);
    assert!(refusal(c, &bash(&p, "git clean -fdx")).is_some());
}
