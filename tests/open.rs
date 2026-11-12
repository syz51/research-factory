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
    // A record id opens its first section, not its front matter.
    let unit = json(dir.path(), &["open", "DR002"]);
    assert_eq!(unit["path"], DR002);
    assert_eq!(unit["anchor"], format!("{DR002}#L7-L9"), "{unit}");
    assert_eq!(unit["lines"][0], "# DR002");
    assert_eq!(json(dir.path(), &["open", DR002])["anchor"], unit["anchor"]);
    // The exact-id hit is the same section.
    let found = json(dir.path(), &["search", "DR002"]);
    assert_eq!(found["hits"][0]["anchor"], unit["anchor"], "{found}");
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

/// Moves DR001's table down by three lines.
fn move_dr001_table(dir: &Path) {
    let path = dir.join(DR001);
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace(
            "Retrieval uses BM25.\n",
            "Retrieval uses BM25.\n\nA note.\nAnother.\n",
        ),
    )
    .unwrap();
}

#[test]
fn an_old_anchor_finds_its_text_after_a_rebuild() {
    let anchor = format!("{DR001}#L11-L42");
    let moved = |unit: &Value| {
        assert_eq!(unit["anchor"], format!("{DR001}#L14-L45"), "{unit}");
        assert!(
            unit["notice"]
                .as_str()
                .unwrap()
                .contains("moved to L14-L45"),
            "{unit}"
        );
        assert_eq!(unit["lines"][0], "## Table");
    };
    // The text moved before the last refresh: the rebuild copies the anchor.
    let dir = fixture();
    ok(dir.path(), &["open", &anchor]);
    move_dr001_table(dir.path());
    ok(dir.path(), &["search", "wombat"]);
    ok(dir.path(), &["search", "--rebuild", "wombat"]);
    moved(&json(dir.path(), &["open", &anchor]));
    // The text moved after the last refresh: the rebuild keeps the units the
    // replaced index had as the file's last version.
    let dir = fixture();
    ok(dir.path(), &["open", &anchor]);
    move_dr001_table(dir.path());
    ok(dir.path(), &["search", "--rebuild", "wombat"]);
    moved(&json(dir.path(), &["open", &anchor]));
    // A second rebuild still holds the anchor.
    ok(dir.path(), &["search", "--rebuild", "wombat"]);
    moved(&json(dir.path(), &["open", &anchor]));
}

#[test]
fn a_rebuild_over_a_corrupt_index_starts_without_old_anchors_and_says_so() {
    let dir = fixture();
    let anchor = format!("{DR001}#L11-L42");
    ok(dir.path(), &["open", &anchor]);
    let db = dir.path().join(".research/lab/search/index.db");
    std::fs::write(&db, "not a database").unwrap();
    let out = lab(dir.path(), &["search", "--rebuild", "wombat"]);
    assert!(out.status.success(), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("the anchors of the replaced index were not carried over"),
        "{err}"
    );
    let unit = json(dir.path(), &["open", &anchor]);
    assert_eq!(unit["anchor"], anchor, "{unit}");
}

#[test]
fn a_rebuild_copies_what_it_can_read_of_the_replaced_index_and_no_part() {
    let anchor = format!("{DR001}#L11-L42");
    let prepare = |sql: &str| {
        let dir = fixture();
        ok(dir.path(), &["open", &anchor]);
        move_dr001_table(dir.path());
        ok(dir.path(), &["search", "wombat"]);
        let db =
            rusqlite::Connection::open(dir.path().join(".research/lab/search/index.db")).unwrap();
        db.execute_batch(sql).unwrap();
        drop(db);
        let out = lab(dir.path(), &["search", "--rebuild", "wombat"]);
        assert!(out.status.success(), "{out:?}");
        (dir, String::from_utf8_lossy(&out.stderr).into_owned())
    };
    // Old units of another shape: the kept anchors still carry over.
    let (dir, err) = prepare(
        "DROP TABLE units; CREATE TABLE units (file_id INTEGER, start_line INTEGER, end_line INTEGER);",
    );
    assert!(!err.contains("not carried over"), "{err}");
    let unit = json(dir.path(), &["open", &anchor]);
    assert_eq!(unit["anchor"], format!("{DR001}#L14-L45"), "{unit}");
    // Old units that fail to read after the copy began: no anchor stays.
    let (dir, err) = prepare(
        "CREATE TABLE gone (x); DROP TABLE units; CREATE VIEW units AS SELECT x FROM gone; DROP TABLE gone;",
    );
    assert!(
        err.contains("the anchors of the replaced index were not carried over"),
        "{err}"
    );
    assert_eq!(old_anchors(dir.path(), DR001).0, 0);
}

#[test]
fn an_old_anchor_finds_its_text_after_a_front_matter_edit_moved_it() {
    let dir = fixture();
    let anchor = format!("{DR001}#L11-L42");
    ok(dir.path(), &["open", &anchor]);
    // A new date and one more front-matter line keep each section's hash.
    let path = dir.path().join(DR001);
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace("date: 2026-01-10\n", "date: 2026-01-11\nsupersedes: []\n"),
    )
    .unwrap();
    let unit = json(dir.path(), &["open", &anchor]);
    assert_eq!(unit["anchor"], format!("{DR001}#L12-L43"), "{unit}");
    assert_eq!(unit["lines"][0], "## Table");
}

const DR003: &str = "docs/decisions/DR003-anchors.md";

fn dr003(body: &str) -> String {
    format!("---\ntitle: Anchors\nstatus: accepted\n---\n# DR003\n\n{body}")
}

const ALPHA: &str = "## Alpha\n\nThe alpha text.\n\n";
const GAMMA: &str = "## Gamma\n\nThe gamma text.\n\n";
const BETA: &str = "## Beta\n\nThe beta text.\n";

/// The rows `old_anchors` holds for a path, their distinct versions, and the
/// units the path has now.
fn old_anchors(dir: &Path, path: &str) -> (i64, i64, i64) {
    let db = rusqlite::Connection::open(dir.join(".research/lab/search/index.db")).unwrap();
    db.query_row(
        "SELECT count(*), count(DISTINCT version), \
         (SELECT count(*) FROM units u JOIN files f ON f.id = u.file_id WHERE f.path = ?1) \
         FROM old_anchors WHERE path = ?1",
        [path],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .unwrap()
}

#[test]
fn an_anchor_finds_its_text_across_two_edits_and_old_versions_are_bounded() {
    let dir = fixture();
    let d = dir.path();
    write(d, DR003, &dr003(&format!("{ALPHA}{BETA}")));
    let alpha = json(d, &["open", &format!("{DR003}:9")]);
    assert_eq!(alpha["lines"][0], "## Alpha", "{alpha}");
    let anchor = alpha["anchor"].as_str().unwrap().to_string();
    // Edit 1: Gamma takes the lines Alpha had; the anchor opens Gamma.
    write(d, DR003, &dr003(&format!("{GAMMA}{ALPHA}{BETA}")));
    let unit = json(d, &["open", &anchor]);
    assert_eq!(unit["lines"][0], "## Gamma", "{unit}");
    // Edit 2: Gamma is gone and Alpha moved down. The range's latest text is
    // gone, so the anchor finds the text it held one version earlier.
    write(
        d,
        DR003,
        &dr003(&format!("An intro.\n\nMore intro.\n\n{ALPHA}{BETA}")),
    );
    let unit = json(d, &["open", &anchor]);
    assert_eq!(unit["lines"][0], "## Alpha", "{unit}");
    assert!(
        unit["notice"].as_str().unwrap().contains(
            "the latest text of L7-L9 is gone, and the text it held 1 version(s) earlier moved to"
        ),
        "{unit}"
    );
    assert_ne!(unit["anchor"], anchor);
    // Twelve more edits keep the anchors of the last ten versions only.
    for i in 0..12 {
        let intro = "Intro.\n\n".repeat(i + 3);
        write(d, DR003, &dr003(&format!("{intro}{ALPHA}{BETA}")));
        ok(d, &["open", &anchor]);
    }
    // Each version had as many units as the file has now.
    let (rows, versions, units) = old_anchors(d, DR003);
    assert_eq!(versions, 10);
    assert!(
        rows > units && rows <= 10 * units,
        "{rows} rows, {units} units"
    );
    // A file whose source is gone keeps no anchor.
    std::fs::remove_file(d.join(DR003)).unwrap();
    ok(d, &["search", "wombat"]);
    assert_eq!(old_anchors(d, DR003), (0, 0, 0));
}
