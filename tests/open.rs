//! End-to-end: `lab open` prints one unit of the search index within a line
//! budget, for a hit's anchor, a `path:line` and a content hash, and finds the
//! text of an old anchor again after its file changed.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn lab(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lab"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run lab")
}

fn ok(dir: &Path, args: &[&str]) -> String {
    let o = lab(dir, args);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn json(dir: &Path, args: &[&str]) -> Value {
    serde_json::from_str(&ok(dir, &[args, &["--json"]].concat())).expect("one JSON object")
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

const CONFIG: &str = r#"[project]
name = "demo"

[[lab.archive.index]]
name = "decisions"
output = "docs/decisions/INDEX.md"
title = "Decisions"
include = ["docs/decisions/DR*.md"]
id_from = '^(DR\d+)-'
require = ["id", "title", "status"]
"#;

const DR001: &str = "docs/decisions/DR001-bm25.md";
const DR002: &str = "docs/decisions/DR002-hybrid.md";

/// DR001 has a short section and a long one; DR002 supersedes it.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(d, "research.toml", CONFIG);
    let mut long = String::new();
    for i in 1..=30 {
        long.push_str(&format!("Row {i} of the wombat table.\n"));
    }
    // Lines: front matter 1-6, `# DR001` 7, text 9, `## Table` 11, rows 13-42.
    write(
        d,
        DR001,
        &format!(
            "---\ntitle: Use BM25\ndate: 2026-01-10\nstatus: superseded\nsuperseded_by: DR002\n---\n# DR001\n\nRetrieval uses BM25.\n\n## Table\n\n{long}"
        ),
    );
    write(
        d,
        DR002,
        "---\ntitle: Use a hybrid\ndate: 2026-03-01\nstatus: accepted\nsupersedes: [DR001]\n---\n# DR002\n\nRetrieval uses a hybrid.\n",
    );
    dir
}

#[test]
fn opens_a_hit_anchor_within_the_line_budget_with_its_neighbours() {
    let dir = fixture();
    let found = json(dir.path(), &["search", "wombat table"]);
    let anchor = found["hits"][0]["anchor"].as_str().unwrap().to_string();
    assert_eq!(anchor, format!("{DR001}#L11-L42"));
    let unit = json(dir.path(), &["open", &anchor, "--lines", "10"]);
    assert_eq!(unit["schema"], "rf.open/1");
    assert_eq!(unit["anchor"], anchor);
    assert_eq!(unit["id"], "DR001");
    assert_eq!(unit["lines"].as_array().unwrap().len(), 10);
    assert_eq!(unit["more_lines"], 22);
    let neighbours = unit["neighbours"].to_string();
    assert!(neighbours.contains("DR002"), "{unit}");
    let text = ok(dir.path(), &["open", &anchor, "--lines", "10"]);
    assert!(text.contains("11  ## Table"), "{text}");
    assert!(text.contains("20  Row 8 of the wombat table."), "{text}");
    assert!(!text.contains("Row 9 of"), "{text}");
    assert!(
        text.contains(&format!("22 more line(s): lab open {DR001}#L21-L42")),
        "{text}"
    );
    assert!(text.contains("superseded by"), "{text}");
    // The content hash of a hit opens the same unit.
    let hash = found["hits"][0]["hash"].as_str().unwrap();
    assert_eq!(json(dir.path(), &["open", hash])["anchor"], anchor);
}

#[test]
fn opens_the_unit_that_holds_a_path_line_and_a_record_id() {
    let dir = fixture();
    let unit = json(dir.path(), &["open", &format!("{DR001}:9")]);
    assert_eq!(unit["anchor"], format!("{DR001}#L7-L9"));
    assert_eq!(unit["lines"][2], "Retrieval uses BM25.");
    let unit = json(dir.path(), &["open", "DR002"]);
    assert_eq!(unit["path"], DR002);
    let o = lab(dir.path(), &["open", "DR999"]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn an_old_anchor_finds_its_text_after_the_lines_moved_and_says_so() {
    let dir = fixture();
    let anchor = format!("{DR001}#L11-L42");
    ok(dir.path(), &["open", &anchor]);
    // Three new lines above the table move it down.
    let path = dir.path().join(DR001);
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace(
            "Retrieval uses BM25.\n",
            "Retrieval uses BM25.\n\nA note.\nAnother.\n",
        ),
    )
    .unwrap();
    let unit = json(dir.path(), &["open", &anchor]);
    assert_eq!(unit["anchor"], format!("{DR001}#L14-L45"), "{unit}");
    assert!(
        unit["notice"]
            .as_str()
            .unwrap()
            .contains("moved to L14-L45"),
        "{unit}"
    );
    assert_eq!(unit["lines"][0], "## Table");
}
