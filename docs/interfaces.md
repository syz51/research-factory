# Shared interfaces

The research factory is a set of small, independent Rust CLIs. They share only the contracts below: one project config file, one finding format, and one reference-registry format. Every tool must work alone; none links another tool's crate.

Version every record with a `schema` field. A reader rejects a major version it does not know and ignores unknown fields.

## 1. Project config: `research.toml`

- Each tool locates the project by walking up from the current directory to the first `research.toml`. `--config <path>` overrides the search. Relative paths in the file resolve against the file's directory.
- Each tool owns one table named after it and ignores the others: `[papers]`, `[agentprof]`, `[claimcheck]`, `[prov]`, `[ireval]` and `[lab]`. The top-level `[project]` table is shared:

```toml
[project]
name = "my-thesis"
timezone = "Asia/Tokyo"        # date display; stored timestamps are UTC RFC 3339
state_dir = ".research"        # tool-owned state (logs, caches, ledgers); safe to gitignore except where a tool says otherwise
```

- A tool with no `research.toml` and no `--config` still runs with defaults and explicit flags.
- A tool never writes `research.toml`, except `lab init`, which creates it.

## 2. Findings: `rf.finding/1` (JSON Lines)

Every checker supports `--format jsonl` and then writes one finding per line on stdout. Human-readable text is the default format. The exit code is 0 when no finding has severity `error`, 1 when at least one does, and 2 for usage or I/O failure.

```json
{"schema":"rf.finding/1","tool":"claimcheck","rule":"quote-not-found","severity":"error","subject":"docs/survey.md:42","message":"quoted text not found in the source text of reference 177","evidence":[{"path":"sources/text/177.txt","locator":"best match line 310, similarity 0.71"}],"data":{"reference":"177"},"at":"2026-11-07T03:00:00Z"}
```

| Field | Meaning |
| --- | --- |
| `tool` | Binary name |
| `rule` | Stable kebab-case id; documented in the tool's README |
| `severity` | `error` (a claim, record or result is wrong or unsupported), `warn` (likely problem, needs a person), `info` (measurement) |
| `subject` | What the finding is about: `path:line`, a reference key, a session id, a run id |
| `message` | One sentence |
| `evidence` | Zero or more `{path, locator}` pointers; `path` is relative to the project root when inside it |
| `data` | Tool-specific object; optional |
| `at` | UTC RFC 3339 time the finding was produced |

## 3. Reference registry: `rf.reference/1` (JSON Lines)

`papers` owns the registry. Other tools only read it. The default location is `references.jsonl` next to `research.toml`; `[papers] registry = "<path>"` overrides it.

One line per catalogue entry, ordered by `key`. The key is the archive's stable identifier: the catalogue number as a string (`"177"`) for numbered catalogues, or the note id for one-note-per-paper archives.

```json
{"schema":"rf.reference/1","key":"177","type":"paper","title":"Utilizing Metadata for Better Retrieval-Augmented Generation","authors":["Raquib Bin Yousuf","Shengzhe Xu"],"year":2026,"venue":"arXiv preprint","ids":{"doi":null,"arxiv":"2601.11863","dblp":null,"openalex":null,"url":"https://arxiv.org/abs/2601.11863"},"version":"v1","accessed":"2026-10-31","depth":"method","texts":[{"path":"sources/text/2601.11863.txt","sha256":"…","origin":"https://arxiv.org/pdf/2601.11863v1"}],"duplicate_of":null,"source":{"path":"docs/research/references.md","line":3015},"verified":{"at":"2026-11-07","against":"arxiv","status":"match"}}
```

- `type`: `paper`, `dataset`, `software`, `web`, `standard` or `other`.
- `depth`: how far the archive inspected the source, from shallowest to deepest: `metadata`, `abstract`, `method`, `full`, `artifact`, `reproduced`. `method` covers reading the method or evaluation sections; `full` is the full text; `artifact` means released code or data was inspected; `reproduced` means a result was rerun independently.
- `texts`: local extracted-text copies a checker may quote against. Paths are relative to the project root. The `sha256` is of the text file.
- `duplicate_of`: the key of the surviving entry when two keys describe the same work. Keys are never renumbered or deleted.
- `verified`: the last metadata check against an index, or null.
- Unknown values are `null`, never invented.

## 4. Provenance records: `rf.run/1` and `rf.manifest/1`

`prov` owns these. Other tools may read them. Their schemas are in the `prov` README.

## 5. Agent-session records: `rf.session/1`

`agentprof` owns these. Their schema is in the `agentprof` README. `lab retro` reads `agentprof`'s `rf.finding/1` findings (the `findings.jsonl` that `agentprof scan --out <dir>` writes, named by `[lab] agent_findings` or `--agent-findings`), so agent-session problems join the same pool as checker findings. `lab evaluate` also reads `source`, `id` and `start` from the `sessions.jsonl` next to it (or `[lab] agent_sessions`), to measure a lesson about an agent-session rule per session. It matches a finding to its session by the finding's `subject`, `source:id`.

## 6. Conventions every repository follows

- Rust edition 2024, `rust-version = "1.99"`, stable toolchain. MIT OR Apache-2.0 licence files.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass, and a GitHub Actions workflow runs them.
- Network access happens only in explicit commands, goes through a client that spaces requests per host and retries 429 and 5xx with backoff, and identifies itself with a generic user agent (`<tool>/<version>`). No email address or personal data in requests.
- Tests use synthetic fixtures. Nothing from a private project, no personal data and no absolute home-directory paths enter a repository.
- Errors name the file and line or the record they concern. No silent fallbacks: a value a tool cannot establish is reported as unknown.
