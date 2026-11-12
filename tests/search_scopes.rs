//! End-to-end: the scopes of `lab search` beyond `notes`: the reference
//! registry and its paper cards, extracted texts, files, sessions and the
//! board, and the stale derivatives that search ignores.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use sha2::{Digest, Sha256};

fn lab(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lab"))
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("run lab")
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

fn sha(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn search(dir: &Path, args: &[&str]) -> String {
    let o = lab(dir, &[&["search"], args].concat());
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    stdout(&o)
}

fn json(dir: &Path, args: &[&str]) -> Value {
    serde_json::from_str(&search(dir, &[&["--json"], args].concat())).expect("one JSON object")
}

fn hits(report: &Value) -> &Vec<Value> {
    report["hits"].as_array().unwrap()
}

/// The registry keys of the hits, best first.
fn keys(report: &Value) -> Vec<&str> {
    hits(report)
        .iter()
        .map(|h| h["key"].as_str().unwrap_or("-"))
        .collect()
}

const CONFIG: &str = r#"[project]
name = "demo"

[lab.archive]
links = ["docs/**/*.md"]
"#;

/// 260 words of filler, one word per line, with `marker` in its middle.
fn body(marker: &str) -> String {
    let mut lines: Vec<String> = (0..130).map(|i| format!("filler{i}")).collect();
    lines.push(marker.to_string());
    lines.extend((130..260).map(|i| format!("filler{i}")));
    lines.join("\n") + "\n"
}

/// A registry of five entries, two extracted texts and one structure file.
fn project(d: &Path) {
    write(d, "research.toml", CONFIG);
    write(d, "docs/notes.md", "# Notes\n\nNothing about papers.\n");
    write(
        d,
        "catalogue.md",
        "# Catalogue\n\n- 12: hierarchical retrieval over long filings\n",
    );
    let text16 = format!(
        "Quantum Annealing Paper\n# Method\nThe quantum annealing sche-\nduling works well.\n{}# References\nzebrafish et al. 2020\n",
        body("middle")
    );
    let n = text16.lines().count();
    write(d, "sources/texts/16.txt", &text16);
    write(
        d,
        "sources/texts/16.txt.structure.json",
        &serde_json::json!({
            "schema": "rf.paper-structure/1",
            "text": {"sha256": sha(&text16)},
            "sections": [
                {"kind": "front", "heading": null, "first_line": 1, "last_line": 1},
                {"kind": "body", "heading": "Method", "first_line": 2, "last_line": n - 2},
                {"kind": "references", "heading": "References", "first_line": n - 1, "last_line": n},
            ],
        })
        .to_string(),
    );
    write(d, "sources/texts/hirec.txt", "Hierarchical filings text\n");
    write(
        d,
        "sources/texts/15.txt",
        "plain extraction words: kangaroo\n",
    );
    write(
        d,
        "sources/texts/15.md",
        "# Docling\n\nstructured extraction words: wallaby\n",
    );
    let entries = [
        serde_json::json!({"key": "12", "title": "Hierarchical Retrieval for Financial Filings",
            "authors": ["Alice Okonkwo", "Bo Chen"], "year": 2024, "venue": "ACL", "depth": "read",
            "abstract": "We retrieve over annual reports.",
            "texts": [{"path": "sources/texts/hirec.txt"}],
            "source": {"path": "catalogue.md", "line": 3}}),
        serde_json::json!({"key": 13, "title": "中文金融问答", "authors": ["王伟"], "year": 2023}),
        serde_json::json!({"key": "14", "title": "Hierarchical Retrieval (preprint)",
            "authors": ["Alice Okonkwo"], "duplicate_of": "12"}),
        serde_json::json!({"key": "15", "title": "Marsupial Text Extraction", "authors": null,
            "ids": {"arxiv": "2401.01234v2", "doi": "https://doi.org/10.1000/XYZ.9"},
            "texts": [{"path": "sources/texts/15.txt", "extraction": "plain"},
                      {"path": "sources/texts/15.md", "extraction": "docling"}]}),
        serde_json::json!({"key": "16", "title": "Quantum Annealing Paper", "authors": ["Q. Lee"],
            "texts": [{"path": "sources/texts/16.txt", "extraction": "plain"}]}),
        // Not marked a duplicate, but it shares the text of 16.
        serde_json::json!({"key": "17", "title": "Quantum Annealing, journal version",
            "texts": [{"path": "sources/texts/16.txt", "extraction": "plain"}]}),
    ];
    let lines: Vec<String> = entries.iter().map(Value::to_string).collect();
    write(d, "references.jsonl", &(lines.join("\n") + "\n"));
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    dir
}

#[test]
fn refs_finds_an_entry_by_title_words_surname_cjk_author_and_ids() {
    let dir = fixture();
    let d = dir.path();
    let report = json(d, &["--in", "refs", "hierarchical", "filings"]);
    assert_eq!(keys(&report)[0], "12");
    let hit = &hits(&report)[0];
    assert_eq!(hit["scope"], "refs");
    assert_eq!(hit["anchor"], "ref:12");
    assert_eq!(hit["info"]["first_author"], "Alice Okonkwo");
    assert_eq!(hit["info"]["year"], "2024");
    // The surname alone, and a two-character CJK name.
    assert_eq!(keys(&json(d, &["--in", "refs", "Okonkwo"]))[0], "12");
    assert_eq!(keys(&json(d, &["--in", "refs", "王伟"]))[0], "13");
    // The name of a local text file is the project's short name of the paper.
    assert_eq!(keys(&json(d, &["--in", "refs", "HiREC"]))[0], "12");
    // The abstract and the catalogue line are indexed.
    assert_eq!(keys(&json(d, &["--in", "refs", "annual reports"]))[0], "12");
    assert_eq!(keys(&json(d, &["--in", "refs", "long filings"]))[0], "12");
    // Exact lookups: the key alone, arXiv id without version, DOI without its URL.
    for q in ["ref:15", "15", "2401.01234", "10.1000/xyz.9"] {
        assert_eq!(keys(&json(d, &["--in", "refs", q]))[0], "15", "{q}");
    }
    // Text output names key, year, first author, title, depth and local text.
    let text = search(d, &["--in", "refs", "Okonkwo"]);
    assert!(
        text.contains(
            "ref:12 · 2024 · Alice Okonkwo · Hierarchical Retrieval for Financial Filings · read"
        ),
        "{text}"
    );
}

#[test]
fn a_duplicate_entry_resolves_to_its_target() {
    let dir = fixture();
    let report = json(dir.path(), &["--in", "refs", "preprint"]);
    assert_eq!(keys(&report), ["12"]);
    assert_eq!(hits(&report)[0]["heading"], "duplicate ref:14");
}

#[test]
fn texts_hit_with_line_range_and_key_and_leave_out_the_references() {
    let dir = fixture();
    let d = dir.path();
    // The hyphen at a line end joins `sche-` and `duling` for matching.
    let report = json(d, &["--in", "texts", "\"quantum annealing scheduling\""]);
    let hit = &hits(&report)[0];
    assert_eq!(hit["key"], "16");
    assert_eq!(hit["scope"], "texts");
    assert_eq!(hit["path"], "sources/texts/16.txt");
    assert_eq!(hit["heading"], "Method");
    assert_eq!(hit["start"], 2);
    assert_eq!(hit["info"]["structure"], "used");
    // A text that two entries share is indexed once, for the first entry.
    assert!(hits(&report).iter().all(|h| h["key"] == "16"), "{report}");
    let text = search(d, &["--in", "texts", "annealing", "scheduling"]);
    assert!(text.contains("sources/texts/16.txt#L2-"), "{text}");
    assert!(text.contains("ref:16"), "{text}");
    // The references section is not indexed.
    assert_eq!(json(d, &["--in", "texts", "zebrafish"])["state"], "zero");
    // The structured extraction is preferred over the plain one.
    assert_eq!(keys(&json(d, &["--in", "texts", "wallaby"])), ["15"]);
    assert_eq!(json(d, &["--in", "texts", "kangaroo"])["state"], "zero");
    // Since 0.19.0 texts are searched by default.
    let plain = json(d, &["quantum", "annealing"]);
    assert!(
        hits(&plain).iter().any(|h| h["scope"] == "texts"),
        "{plain}"
    );
}

#[test]
fn a_stale_structure_file_and_a_stale_card_are_ignored_and_reported() {
    let dir = fixture();
    let d = dir.path();
    let text15 = std::fs::read_to_string(d.join("sources/texts/15.md")).unwrap();
    let card = |sha: &str| {
        serde_json::json!({
            "schema": "rf.paper-card/1", "key": "15", "model": "m",
            "text": {"path": "sources/texts/15.md", "sha256": sha},
            "items": [{"field": "dataset", "value": "platypus corpus",
                       "quote": "structured extraction words", "lines": [3, 3]}],
        })
        .to_string()
    };
    write(d, "sources/cards/15.json", &card(&sha(&text15)));
    let report = json(d, &["--in", "refs", "platypus"]);
    assert_eq!(report["state"], "hits");
    assert_eq!(hits(&report)[0]["key"], "15");
    assert_eq!(hits(&report)[0]["start"], 3);

    // The texts change under their derivatives.
    write(
        d,
        "sources/texts/15.md",
        &format!("{text15}more platypus\n"),
    );
    let text16 = std::fs::read_to_string(d.join("sources/texts/16.txt")).unwrap();
    write(d, "sources/texts/16.txt", &format!("{text16}appendix\n"));

    let o = lab(
        d,
        &["search", "--json", "--in", "refs", "platypus", "corpus"],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(stderr(&o).contains("stale derivative"), "{}", stderr(&o));
    let report: Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert!(
        hits(&report)
            .iter()
            .all(|h| h["path"] != "sources/cards/15.json"),
        "{report}"
    );
    let notice = report["partial"].to_string();
    assert!(notice.contains("stale derivative"), "{notice}");

    // Without its structure file the references section is indexed again.
    let report = json(d, &["--in", "texts", "zebrafish"]);
    assert_eq!(report["state"], "partial");
    assert_eq!(hits(&report)[0]["info"]["structure"], "stale");

    let check = lab(d, &["archive", "check"]);
    let out = stdout(&check) + &stderr(&check);
    assert!(out.contains("stale-derivative"), "{out}");
    assert!(out.contains("sources/cards/15.json"), "{out}");
    assert!(out.contains("sources/texts/16.txt.structure.json"), "{out}");

    // The gate, without `[[lab.checks]]`, reports them too.
    let done = lab(d, &["archive", "done"]);
    let out = stdout(&done) + &stderr(&done);
    assert!(out.contains("No [[lab.checks]]"), "{out}");
    assert!(out.contains("sources/cards/15.json"), "{out}");
}

#[test]
fn several_scopes_and_all_are_searched_and_the_rest_are_named() {
    let dir = fixture();
    let d = dir.path();
    let report = json(d, &["--in", "refs,texts", "quantum"]);
    let scopes: Vec<&str> = hits(&report)
        .iter()
        .map(|h| h["scope"].as_str().unwrap())
        .collect();
    assert!(
        scopes.contains(&"refs") && scopes.contains(&"texts"),
        "{scopes:?}"
    );
    assert_eq!(report["scopes"], serde_json::json!(["refs", "texts"]));
    let text = search(d, &["--in", "notes", "--in", "refs", "zzqq"]);
    assert!(text.contains("Searched: notes, refs"), "{text}");
    assert!(
        text.contains("Not searched: texts, files, sessions, board"),
        "{text}"
    );
}

#[test]
fn files_are_found_by_path_words_and_name_their_parent_record() {
    let dir = fixture();
    let d = dir.path();
    write(
        d,
        "research.toml",
        &format!("{CONFIG}\n[lab.search]\nfiles = [\"runs/**\"]\n")
            .replace("links = [", "links = [\"runs/**/*.md\", "),
    );
    write(
        d,
        "runs/r7/README.md",
        "---\nid: R7\ntitle: Run seven\n---\n# Run seven\n",
    );
    write(d, "runs/r7/wombat_scores.csv", "a,b\n1,2\n");
    // A text of the registry is a source of `files` and of `texts`.
    write(d, "runs/r7/16.txt", "copy");
    write(
        d,
        "references.jsonl",
        &format!(
            "{}{}\n",
            std::fs::read_to_string(d.join("references.jsonl")).unwrap(),
            serde_json::json!({"key": "18", "title": "Run text",
                "texts": [{"path": "runs/r7/16.txt", "extraction": "plain"}]})
        ),
    );
    let all = json(d, &["--in", "all", "r7", "copy"]);
    let scopes: Vec<&str> = hits(&all)
        .iter()
        .map(|h| h["scope"].as_str().unwrap())
        .collect();
    assert!(
        scopes.contains(&"files") && scopes.contains(&"texts"),
        "{all}"
    );
    let report = json(d, &["--in", "files", "wombat", "scores"]);
    let hit = &hits(&report)[0];
    assert_eq!(hit["scope"], "files");
    assert_eq!(hit["path"], "runs/r7/wombat_scores.csv");
    assert!(hit.to_string().contains("R7"), "{hit}");

    // A record note added later to a file's directory is its parent at once.
    write(d, "runs/r8/numbat.csv", "a\n1\n");
    let hit = hits(&json(d, &["--in", "files", "numbat"]))[0].clone();
    assert_eq!(hit["info"]["parent"], Value::Null, "{hit}");
    write(d, "runs/r8/README.md", "---\nid: R8\n---\n# Run eight\n");
    let hit = hits(&json(d, &["--in", "files", "numbat"]))[0].clone();
    assert_eq!(hit["info"]["parent"], "R8", "{hit}");
}

#[test]
fn a_registry_entry_or_text_is_reindexed_when_what_it_shows_changes() {
    let dir = fixture();
    let d = dir.path();
    // An entry whose text is not extracted yet, then is.
    write(
        d,
        "references.jsonl",
        &format!(
            "{}{}\n",
            std::fs::read_to_string(d.join("references.jsonl")).unwrap(),
            serde_json::json!({"key": "19", "title": "Echidna Survey",
                "texts": [{"path": "sources/texts/19.txt"}]})
        ),
    );
    let hit = hits(&json(d, &["--in", "refs", "echidna"]))[0].clone();
    assert_eq!(hit["info"]["text"], Value::Null, "{hit}");
    write(d, "sources/texts/19.txt", "Echidna Survey\n");
    let hit = hits(&json(d, &["--in", "refs", "echidna"]))[0].clone();
    assert_eq!(hit["info"]["text"], "sources/texts/19.txt", "{hit}");

    // A structure file rewritten for the same text: its new sections are used.
    let hit = hits(&json(d, &["--in", "texts", "annealing", "scheduling"]))[0].clone();
    assert_eq!(hit["heading"], "Method");
    let path = d.join("sources/texts/16.txt.structure.json");
    let renamed = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\"Method\"", "\"Approach\"");
    write(d, "sources/texts/16.txt.structure.json", &renamed);
    let hit = hits(&json(d, &["--in", "texts", "annealing", "scheduling"]))[0].clone();
    assert_eq!(hit["heading"], "Approach");
}

#[test]
fn sessions_index_what_the_person_typed() {
    let dir = fixture();
    let d = dir.path();
    let transcript = d.join("t/one.jsonl");
    write(
        d,
        "t/one.jsonl",
        &[
            r#"{"type":"user","message":{"role":"user","content":"please compare the echidna baselines"}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"numbat"}]}}"#,
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"meta quoll"}}"#,
            r#"{"type":"user","message":{"role":"user","content":"This session is being continued from a previous conversation. Summary: platypus"}}"#,
            r#"{"type":"user","message":{"role":"user","content":"Act as the reader for platypus cards."}}"#,
            r#"{"type":"user","message":{"role":"user","content":"Act as the writer for platypus notes."}}"#,
            r#"{"type":"user","message":{"role":"user","content":"yes platypus"}}"#,
        ]
        .join("\n"),
    );
    let main = serde_json::json!({"source": "claude", "id": "s1", "role": "main", "title": "Baselines",
        "start": "2026-10-01T10:00:00Z",
        "transcript": {"path": transcript.to_string_lossy(), "status": "present"}});
    let sub = serde_json::json!({"source": "claude", "id": "s2", "role": "subagent",
        "transcript": {"path": transcript.to_string_lossy(), "status": "present"}});
    // The main session twice, as after two scans that both appended it.
    write(
        d,
        ".research/sessions.jsonl",
        &format!("{main}\n{sub}\n{main}\n"),
    );
    write(
        d,
        "research.toml",
        &CONFIG.replace(
            "[lab.archive]",
            "[lab]\nagent_sessions = \".research/sessions.jsonl\"\n\n[lab.archive]",
        ),
    );
    let report = json(d, &["--in", "sessions", "echidna"]);
    let hit = &hits(&report)[0];
    assert_eq!(hit["scope"], "sessions");
    assert_eq!(hit["record"]["class"], "chat");
    assert_eq!(hits(&report).len(), 1);
    assert!(
        hit["anchor"].as_str().unwrap().ends_with("one.jsonl#L1"),
        "{hit}"
    );
    for word in ["numbat", "quoll", "platypus"] {
        assert_eq!(
            json(d, &["--in", "sessions", word])["state"],
            "zero",
            "{word}"
        );
    }
    let out = stdout(&lab(d, &["search", "--in", "sessions", "--rebuild"]));
    assert!(
        out.contains("Session messages left out by rule: brief 2, compaction 1, short 1."),
        "{out}"
    );
    let out = stdout(&lab(d, &["search", "--in", "refs", "--rebuild"]));
    assert!(!out.contains("left out"), "{out}");
}

#[test]
fn the_board_is_searched_through_its_export_and_an_unreachable_one_is_partial() {
    let dir = fixture();
    let d = dir.path();
    let with = |export: &str| {
        CONFIG.replace(
            "[lab.archive]\n",
            &format!("[lab.archive]\nmodel = 2\nboard_export = {export}\n"),
        )
    };
    write(
        d,
        "board.json",
        &serde_json::json!({"tasks": [{"id": "rf-9", "title": "Measure the bandicoot set",
            "body": "Run it twice.", "notes": [{"kind": "decision", "text": "Use the bilby split",
            "at": "2026-10-02"}]}, {"id": "rf-9", "title": "Measure the bandicoot set"}]})
        .to_string(),
    );
    write(d, "research.toml", &with(r#"["cat", "board.json"]"#));
    let report = json(d, &["--in", "board", "bandicoot"]);
    assert_eq!(hits(&report)[0]["anchor"], "board:rf-9");
    assert_eq!(hits(&report)[0]["record"]["class"], "chat");
    let note = json(d, &["--in", "board", "bilby"]);
    assert_eq!(hits(&note)[0]["anchor"], "board:rf-9#note-1");

    // An export that fails: the query is partial and names the scope.
    write(d, "research.toml", &with(r#"["sh", "-c", "exit 3"]"#));
    let o = lab(d, &["search", "--json", "--in", "notes,board", "papers"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let report: Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(report["state"], "partial");
    assert!(
        report["partial"].to_string().contains("scope `board`"),
        "{report}"
    );
    // No board command at all is partial as well, not an error.
    write(d, "research.toml", CONFIG);
    let report = json(d, &["--in", "board", "bandicoot"]);
    assert!(
        report["partial"].to_string().contains("scope `board`"),
        "{report}"
    );
}

#[test]
fn cards_without_a_registry_are_searched_and_their_faults_reported() {
    let dir = fixture();
    let d = dir.path();
    std::fs::remove_file(d.join("references.jsonl")).unwrap();
    let text15 = std::fs::read_to_string(d.join("sources/texts/15.md")).unwrap();
    write(
        d,
        "sources/cards/15.json",
        &serde_json::json!({
            "schema": "rf.paper-card/1", "key": "15", "model": "m",
            "text": {"path": "sources/texts/15.md", "sha256": sha(&text15)},
            "items": [{"field": "dataset", "value": "platypus corpus",
                       "quote": "structured extraction words", "lines": [3, 3]}],
        })
        .to_string(),
    );
    write(d, "sources/cards/bad.json", "not json");
    let report = json(d, &["--in", "refs", "platypus"]);
    assert_eq!(hits(&report)[0]["key"], "15", "{report}");
    let notice = report["partial"].to_string();
    assert!(notice.contains("sources/cards/bad.json"), "{report}");

    // A card whose text changed is ignored and reported.
    write(d, "sources/texts/15.md", &format!("{text15}more\n"));
    let report = json(d, &["--in", "refs", "platypus"]);
    assert!(
        hits(&report)
            .iter()
            .all(|h| h["path"] != "sources/cards/15.json"),
        "{report}"
    );
    assert!(
        report["partial"].to_string().contains("stale derivative"),
        "{report}"
    );
}

#[test]
fn a_first_query_runs_the_board_export_once() {
    let dir = fixture();
    let d = dir.path();
    write(
        d,
        "board.json",
        &serde_json::json!({"tasks": [{"id": "rf-9", "title": "Measure the bandicoot set"}]})
            .to_string(),
    );
    write(
        d,
        "research.toml",
        &CONFIG.replace(
            "[lab.archive]\n",
            "[lab.archive]\nmodel = 2\nboard_export = [\"sh\", \"-c\", \"echo run >> runs.log; cat board.json\"]\n",
        ),
    );
    let runs = || {
        std::fs::read_to_string(d.join("runs.log"))
            .unwrap()
            .lines()
            .count()
    };
    let report = json(d, &["--in", "board", "bandicoot"]);
    assert_eq!(hits(&report)[0]["anchor"], "board:rf-9");
    assert_eq!(runs(), 1, "the build lists the board once");
    json(d, &["--in", "board", "--rebuild", "bandicoot"]);
    assert_eq!(runs(), 2, "a rebuild lists it once more");
    json(d, &["--in", "board", "bandicoot"]);
    assert_eq!(runs(), 3, "a later query refreshes it once");
}

#[test]
fn a_decimal_number_matches_without_trailing_zeros() {
    let dir = fixture();
    let d = dir.path();
    write(
        d,
        "docs/result.md",
        "# Result\n\nAccuracy was 47.0% on the pilot, with an F1 of 0.50.\n",
    );
    let report = json(d, &["--in", "notes", "47%"]);
    assert_eq!(hits(&report)[0]["path"], "docs/result.md");
    let report = json(d, &["--in", "notes", "0.50"]);
    assert_eq!(hits(&report)[0]["path"], "docs/result.md");
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(
            [
                &["-c", "user.name=t", "-c", "user.email=t@example.com"],
                args,
            ]
            .concat(),
        )
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {}", stderr(&out));
}

#[test]
fn a_linked_worktree_reads_ignored_texts_from_the_main_checkout() {
    let dir = tempfile::tempdir().unwrap();
    let (main, linked) = (dir.path().join("main"), dir.path().join("linked"));
    project(&main);
    write(&main, ".gitignore", "sources/\n.research/\n");
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["add", "."]);
    git(&main, &["commit", "-q", "-m", "fixture"]);
    git(&main, &["worktree", "add", "-q", linked.to_str().unwrap()]);
    assert!(!linked.join("sources").exists());
    let report = json(
        &linked,
        &["--in", "texts", "\"quantum annealing scheduling\""],
    );
    let hit = &hits(&report)[0];
    assert_eq!(hit["key"], "16");
    assert_eq!(hit["path"], "sources/texts/16.txt");
    assert_eq!(hit["info"]["structure"], "used");
}
