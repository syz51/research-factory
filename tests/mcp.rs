//! End-to-end: `lab mcp` speaks MCP over stdio, and its tools give the same
//! answers as the commands they stand for.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn lab(dir: &Path, args: &[&str]) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_lab"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}

fn lab_json(dir: &Path, args: &[&str]) -> Value {
    serde_json::from_str(&lab(dir, &[args, &["--json"]].concat())).unwrap()
}

/// Sends `messages` to one `lab mcp` process and returns its replies by id.
fn session(dir: &Path, messages: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lab"))
        .arg("mcp")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for m in messages {
        writeln!(stdin, "{m}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("one JSON-RPC message per line"))
        .collect()
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(
        d,
        "research.toml",
        "[project]\nname = \"demo\"\n\n[[lab.archive.index]]\nname = \"decisions\"\noutput = \"docs/decisions/INDEX.md\"\ntitle = \"Decisions\"\ninclude = [\"docs/decisions/DR*.md\"]\nid_from = '^(DR\\d+)-'\nrequire = [\"id\", \"title\", \"status\"]\n",
    );
    write(
        d,
        "docs/decisions/DR001-bm25.md",
        "---\ntitle: Use BM25\ndate: 2026-01-10\nstatus: superseded\nsuperseded_by: DR002\n---\n# DR001\n\nThe okapi ranking uses BM25.\n",
    );
    write(
        d,
        "docs/decisions/DR002-hybrid.md",
        "---\ntitle: Use a hybrid\ndate: 2026-03-01\nstatus: accepted\nsupersedes: [DR001]\n---\n# DR002\n\nThe okapi ranking adds vectors to BM25.\n",
    );
    write(
        d,
        "docs/notes/okapi.md",
        "# Okapi\n\nWhy okapi ranking: see [DR002](../decisions/DR002-hybrid.md).\n",
    );
    dir
}

fn init() -> Value {
    json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {},
        "clientInfo": {"name": "test", "version": "0"}}})
}

fn call(id: u64, tool: &str, arguments: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {"name": tool, "arguments": arguments}})
}

#[test]
fn lists_three_tools_and_its_search_gives_the_hits_of_lab_search() {
    let dir = fixture();
    let replies = session(
        dir.path(),
        &[
            init(),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
            call(
                2,
                "archive_search",
                json!({"query": "okapi ranking", "limit": 5}),
            ),
            call(3, "archive_open", json!({"anchor": "DR001", "lines": 2})),
            call(4, "archive_related", json!({"name": "DR001"})),
            call(
                5,
                "archive_search",
                json!({"query": "okapi", "as_of": "March"}),
            ),
            json!({"jsonrpc": "2.0", "id": 6, "method": "no/such"}),
        ],
    );
    // No reply to the notification.
    let ids: Vec<u64> = replies.iter().map(|r| r["id"].as_u64().unwrap()).collect();
    assert_eq!(ids, [0, 1, 2, 3, 4, 5, 6]);
    // Both searches, also the failed one, and the open go to the query trace.
    let trace = std::fs::read_to_string(dir.path().join(".research/lab/search-queries.jsonl"))
        .expect("a query trace");
    let lines: Vec<Value> = trace
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let sources: Vec<&str> = lines
        .iter()
        .filter(|l| l["schema"] == "rf.search-query/1")
        .map(|l| l["source"].as_str().unwrap())
        .collect();
    assert_eq!(sources, ["mcp", "mcp"], "{trace}");
    let opens: Vec<&str> = lines
        .iter()
        .filter(|l| l["schema"] == "rf.search-open/1")
        .map(|l| l["anchor"].as_str().unwrap())
        .collect();
    assert_eq!(opens.len(), 1, "{trace}");
    assert!(
        opens[0].starts_with("docs/decisions/DR001-bm25.md#L"),
        "{trace}"
    );
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "lab");
    let names: Vec<&str> = replies[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["archive_search", "archive_open", "archive_related"]);
    for t in replies[1]["result"]["tools"].as_array().unwrap() {
        assert!(t["description"].as_str().unwrap().contains("Use it"), "{t}");
    }

    // The same hits, in the same order, as `lab search`.
    let search = &replies[2]["result"];
    assert_eq!(search["isError"], false);
    let cli = lab(dir.path(), &["search", "okapi ranking", "--limit", "5"]);
    let hits = lab_json(dir.path(), &["search", "okapi ranking", "--limit", "5"]);
    assert_eq!(hits["hits"].as_array().unwrap().len(), 2, "{hits}");
    // Text only: a client may show structured content in place of the text.
    assert!(search.get("structuredContent").is_none());
    // Two lines name the terminal state, the missing stages and the mode;
    // the rest is the text of `lab search`.
    let text = search["content"][0]["text"].as_str().unwrap();
    let (head, rest) = text
        .split_once("\nmode: lexical (model: unloaded)\n")
        .expect(text);
    assert_eq!(head, "state: hits; missing stages: dense, rerank");
    assert_eq!(rest, cli);

    let opened = replies[3]["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(
        opened.strip_prefix("state: hits\n"),
        Some(lab(dir.path(), &["open", "DR001", "--lines", "2"]).as_str())
    );
    assert!(opened.contains("superseded by"), "{opened}");
    let related = &replies[4]["result"]["content"][0]["text"];
    assert!(related.as_str().unwrap().contains("DR002"), "{related}");

    // A bad argument is a tool error; an unknown method is a protocol error.
    assert_eq!(replies[5]["result"]["isError"], true);
    assert_eq!(replies[6]["error"]["code"], -32601);
}
