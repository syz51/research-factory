//! End-to-end: the graph stage of the search index. `lab related` lists the
//! neighbours of a record; `lab search` filters by citation, link and date,
//! folds copies of a fact under their record and ranks by an earlier state.

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

fn paths(report: &Value) -> Vec<&str> {
    report["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["path"].as_str().unwrap())
        .collect()
}

const CONFIG: &str = r#"[project]
name = "demo"

[lab.archive]
links = ["docs/**/*.md"]

[[lab.archive.index]]
name = "decisions"
output = "docs/decisions/INDEX.md"
title = "Decisions"
include = ["docs/decisions/DR*.md"]
id_from = '^(DR\d+)-'
require = ["id", "title", "status"]

[[lab.archive.index]]
name = "history"
output = "docs/history/INDEX.md"
title = "History reports"
include = ["docs/history/[0-9]*.md"]
id_from = '^(\d+[a-z]?)-'
id_prefix = "H"
trim_leading_zeros = true
require = ["id", "title"]
"#;

const DR001: &str = "docs/decisions/DR001-tfidf.md";
const DR002: &str = "docs/decisions/DR002-bm25.md";
const DR003: &str = "docs/decisions/DR003-hybrid.md";
const LOG: &str = "docs/history/01-2026-03-02-log.md";
const TOPIC: &str = "docs/topics/retrieval.md";
const OTHER: &str = "docs/topics/other.md";

/// Three decisions in a chain of supersession, a log entry and a topic page
/// that restate a fact of the middle one, and the citations of key 232.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(d, "research.toml", CONFIG);
    write(
        d,
        DR001,
        "---\ntitle: Use TF-IDF\ndate: 2026-01-10\nstatus: superseded\nsuperseded_by: DR002\n---\n# DR001\n\nRetrieval uses TF-IDF for the pilot.\n",
    );
    write(
        d,
        DR002,
        "---\ntitle: Use BM25\ndate: 2026-03-01\nstatus: superseded\nsupersedes: [DR001]\ntask: rf-7\n---\n# DR002\n\nRetrieval uses BM25 for the pilot.\n\n## Corpus\n\nThe capybara corpus is ranked with BM25.\n",
    );
    // DR003 names its predecessor only in its own front matter.
    write(
        d,
        DR003,
        "---\ntitle: Use a hybrid\ndate: 2026-06-01\nstatus: accepted\nsupersedes: DR002\n---\n# DR003\n\nRetrieval uses a hybrid of BM25 and vectors for the pilot.\n",
    );
    write(
        d,
        LOG,
        "---\ntitle: March log\ndate: 2026-03-02\n---\n# March log\n\n## Corpus\n\nThe capybara corpus is ranked with BM25, see [DR002](../decisions/DR002-bm25.md).\nAs in ref:232.\n\n## Other\n\nNothing about [232](../refs.md#reference-2321).\n",
    );
    write(
        d,
        TOPIC,
        "# Retrieval\n\n## Corpus\n\nThe capybara corpus is ranked with BM25 ([DR002](../decisions/DR002-bm25.md)).\nSource: [1](../refs.md#reference-232).\n",
    );
    write(
        d,
        OTHER,
        "# Other\n\nCites ref:233 and ref:2320, never the key itself.\n\n```\nref:232 inside code is no citation\n```\n",
    );
    dir
}

#[test]
fn related_lists_both_ends_of_a_supersession_and_the_notes_that_link_a_record() {
    let dir = fixture();
    let report = json(dir.path(), &["related", "DR002"]);
    assert_eq!(report["schema"], "rf.related/1");
    assert_eq!(report["node"]["id"], "DR002");
    let relation = |name: &str| -> Vec<String> {
        report["relations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == name)
            .unwrap_or_else(|| panic!("no relation `{name}` in {report}"))["neighbours"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["node"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(relation("superseded by"), ["DR003"]);
    assert_eq!(relation("supersedes"), ["DR001"]);
    assert_eq!(relation("task"), ["rf-7"]);
    assert_eq!(relation("linked from"), ["H1", TOPIC]);
    // The neighbours carry date, status and path.
    let text = ok(dir.path(), &["related", "DR002"]);
    assert!(
        text.contains(
            "superseded by (1):\n  DR003 · 2026-06-01 · accepted · docs/decisions/DR003-hybrid.md"
        ),
        "{text}"
    );
    assert!(
        text.contains("docs/history/01-2026-03-02-log.md#L7-L10"),
        "{text}"
    );
    // A board task and a reference key are nodes too.
    let task = ok(dir.path(), &["related", "task:rf-7"]);
    assert!(task.contains("records (1):\n  DR002"), "{task}");
    let cited = ok(dir.path(), &["related", "ref:232"]);
    assert!(cited.contains("cited by (2):"), "{cited}");
    // An unknown name is an error that says what to try.
    let o = lab(dir.path(), &["related", "DR999"]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn cites_selects_the_notes_that_cite_a_key_and_no_other() {
    let dir = fixture();
    let report = json(dir.path(), &["search", "--cites", "232"]);
    let mut found = paths(&report);
    found.sort_unstable();
    assert_eq!(found, [LOG, TOPIC]);
    // The unit that holds the citation, not the whole file.
    assert_eq!(report["hits"][0]["anchor"], format!("{LOG}#L7-L10"));
    // With words, the filter applies to the ranked hits.
    let report = json(dir.path(), &["search", "BM25", "--cites", "ref:232"]);
    let mut found = paths(&report);
    found.sort_unstable();
    assert_eq!(found, [LOG, TOPIC]);
    let report = json(dir.path(), &["search", "--links-to", "DR002"]);
    let mut found = paths(&report);
    found.sort_unstable();
    assert_eq!(found, [LOG, TOPIC]);
}

#[test]
fn with_fold_a_fact_in_a_record_a_log_and_a_topic_page_is_one_hit_with_also_in_lines() {
    let dir = fixture();
    let report = json(dir.path(), &["search", "capybara corpus", "--fold"]);
    assert_eq!(paths(&report), [DR002], "{report}");
    let also: Vec<&str> = report["hits"][0]["also_in"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["anchor"].as_str().unwrap())
        .collect();
    assert_eq!(also.len(), 2, "{report}");
    assert!(also.iter().any(|a| a.starts_with(LOG)));
    assert!(also.iter().any(|a| a.starts_with(TOPIC)));
    // The hit shows its successor and how many notes link it.
    assert_eq!(report["hits"][0]["record"]["superseded_by"][0], "DR003");
    assert_eq!(report["hits"][0]["linked_by"], 2);
    let text = ok(dir.path(), &["search", "capybara corpus", "--fold"]);
    assert!(text.contains("superseded by DR003 · linked by 2"), "{text}");
    assert!(text.contains("   also in: "), "{text}");
    assert!(
        text.contains("H1 docs/history/01-2026-03-02-log.md#L7-L10"),
        "{text}"
    );
    // By default, and with the old `--all`, each copy is a hit of its own.
    let all = json(dir.path(), &["search", "capybara corpus"]);
    assert_eq!(
        all,
        json(dir.path(), &["search", "capybara corpus", "--all"])
    );
    let mut found = paths(&all);
    found.sort_unstable();
    let mut expected = [DR002, TOPIC, LOG];
    expected.sort_unstable();
    assert_eq!(found, expected);
}

#[test]
fn as_of_ranks_by_the_records_in_force_on_that_date() {
    let dir = fixture();
    let now = json(dir.path(), &["search", "retrieval pilot", "--records"]);
    assert_eq!(paths(&now)[0], DR003);
    // Between the second and the third decision, the second was in force.
    let then = json(
        dir.path(),
        &[
            "search",
            "retrieval pilot",
            "--records",
            "--as-of",
            "2026-03-15",
        ],
    );
    // The topic page that links the second decision is a hit of its own,
    // above the first decision, which was no longer in force.
    assert_eq!(paths(&then), [DR002, TOPIC, DR001], "{then}");
    assert_eq!(then["hits"][0]["record"]["inactive"], false);
    assert_eq!(then["hits"][2]["record"]["inactive"], true);
    // A successor dated after the date is not listed; an earlier one is.
    assert!(
        then["hits"][0]["record"]["superseded_by"].is_null(),
        "{then}"
    );
    assert_eq!(
        then["hits"][2]["record"]["superseded_by"],
        serde_json::json!(["DR002"])
    );
    // Before the second decision, the first ranks first.
    let early = json(
        dir.path(),
        &["search", "retrieval pilot", "--as-of", "2026-02-01"],
    );
    assert_eq!(paths(&early)[0], DR001);
    // --since and --until filter by record date.
    let window = json(
        dir.path(),
        &[
            "search",
            "retrieval pilot",
            "--records",
            "--since",
            "2026-02-01",
            "--until",
            "2026-05-01",
        ],
    );
    assert_eq!(paths(&window), [DR002]);
    let o = lab(dir.path(), &["search", "pilot", "--as-of", "March"]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn the_graph_stage_is_ready_and_edges_follow_an_edit() {
    let dir = fixture();
    let report = json(dir.path(), &["search", "capybara"]);
    assert_eq!(report["stages"]["graph"], "ready");
    // The topic page stops citing 232.
    write(
        dir.path(),
        TOPIC,
        "# Retrieval\n\n## Corpus\n\nThe capybara corpus is ranked with BM25.\n",
    );
    let report = json(dir.path(), &["search", "--cites", "232"]);
    assert_eq!(paths(&report), [LOG]);
}

#[test]
fn paper_citations_come_from_current_structure_sidecars() {
    use sha2::{Digest, Sha256};
    let dir = fixture();
    let d = dir.path();
    let config = std::fs::read_to_string(d.join("research.toml")).unwrap();
    write(
        d,
        "research.toml",
        &format!("[papers]\nregistry = \"refs/registry.jsonl\"\n\n{config}"),
    );
    write(
        d,
        "refs/registry.jsonl",
        "{\"key\":\"232\",\"texts\":[{\"path\":\"refs/texts/232.txt\"}]}\n",
    );
    let text = "BM25 is a ranking function [100].\n";
    write(d, "refs/texts/232.txt", text);
    let sha: String = Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    write(
        d,
        "refs/texts/232.txt.structure.json",
        &format!(
            "{{\"schema\":\"rf.paper-structure/1\",\"text\":{{\"sha256\":\"{sha}\"}},\
             \"references\":{{\"entries\":[{{\"key\":\"100\",\"first_line\":1}},{{\"key\":\"232\"}}]}}}}"
        ),
    );
    let cites = |d: &Path| -> Vec<String> {
        let report = json(d, &["related", "ref:232"]);
        report["relations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == "cites papers")
            .map(|r| {
                r["neighbours"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n["node"].as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    // A paper's reference to itself gives no edge.
    assert_eq!(cites(d), ["ref:100"]);
    // A text edited after its sidecar was made gives no edge.
    write(d, "refs/texts/232.txt", "BM25 is a ranking function.\n");
    assert!(cites(d).is_empty());
}
