//! The grep-hint hook of the skill (`skill/research-factory/scripts/grep-hint.sh`)
//! speaks only after a grep under a research archive, and never blocks.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const HOOK: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/skill/research-factory/scripts/grep-hint.sh"
);

/// The hook's stdout for one PostToolUse event; asserts exit 0.
fn hook(event: &Value) -> String {
    let mut child = Command::new("sh")
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
    String::from_utf8(out.stdout).unwrap()
}

fn grep(cwd: &Path, path: Option<&Path>) -> Value {
    let mut input = json!({"pattern": "BM25"});
    if let Some(p) = path {
        input["path"] = json!(p);
    }
    json!({"hook_event_name": "PostToolUse", "tool_name": "Grep", "cwd": cwd, "tool_input": input})
}

fn bash(cwd: &Path, command: &str) -> Value {
    json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "cwd": cwd,
        "tool_input": {"command": command}})
}

#[test]
fn speaks_only_for_a_grep_under_the_archive_root() {
    if Command::new("jq").arg("--version").output().is_err() {
        eprintln!("skipped: the hook needs jq");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("project");
    let docs = archive.join("docs/decisions");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        archive.join("research.toml"),
        "[project]\nname = \"demo\"\n\n[lab.archive]\nlinks = [\"docs/**/*.md\"]\n",
    )
    .unwrap();
    let plain = tmp.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let bare = tmp.path().join("bare");
    std::fs::create_dir_all(&bare).unwrap();
    std::fs::write(bare.join("research.toml"), "[project]\nname = \"x\"\n").unwrap();

    let said = hook(&grep(&archive, None));
    let reply: Value = serde_json::from_str(&said).expect("one JSON object");
    let line = reply["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert_eq!(reply["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    assert!(line.contains("`lab search <words>`"), "{line}");
    assert!(!line.contains('\n'));
    // A Grep with a path under the root, from elsewhere.
    assert!(!hook(&grep(&plain, Some(&docs))).is_empty());
    // grep and rg in Bash, under the root.
    assert!(!hook(&bash(&archive, "grep -rn BM25 docs")).is_empty());
    assert!(
        !hook(&bash(
            &plain,
            &format!("cd /tmp && rg -n BM25 {}", docs.display())
        ))
        .is_empty()
    );

    // Silent: another path, a project without an archive, another command or tool.
    assert_eq!(hook(&grep(&plain, None)), "");
    assert_eq!(hook(&grep(&archive, Some(&plain))), "");
    assert_eq!(hook(&grep(&bare, None)), "");
    assert_eq!(hook(&bash(&archive, "ls docs")), "");
    assert_eq!(hook(&bash(&archive, "echo grep")), "");
    assert_eq!(hook(&bash(&plain, "grep -rn BM25 .")), "");
    assert_eq!(
        hook(&json!({"tool_name": "Read", "cwd": archive, "tool_input": {"file_path": "x"}})),
        ""
    );
    // Not JSON: still exit 0 and silent.
    let mut child = Command::new("sh")
        .arg(HOOK)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"not json").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
}
