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

- A table's sub-tables belong to the same tool: `lab` owns `[lab.archive]`, `[[lab.archive.index]]` and `[[lab.archive.budget]]` (section 6).
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
- Optional fields are omitted when empty, and readers keep fields they do not know. `papers` adds `authors_truncated` (the source lists only the first authors), `author_variants` (an index's misspellings of the first author, accepted by `verify`) and `citekey`; its README describes them.
- Unknown values are `null`, never invented.

## 4. Provenance records: `rf.run/1` and `rf.manifest/1`

`prov` owns these. Other tools may read them. Their schemas are in the `prov` README.

## 5. Agent-session records: `rf.session/1`

`agentprof` owns these. Their schema is in the `agentprof` README. `lab retro` reads `agentprof`'s `rf.finding/1` findings (the `findings.jsonl` that `agentprof scan --out <dir>` writes, named by `[lab] agent_findings` or `--agent-findings`), so agent-session problems join the same pool as checker findings. `lab evaluate` also reads `source`, `id` and `start` from the `sessions.jsonl` next to it (or `[lab] agent_sessions`), to measure a lesson about an agent-session rule per session. It matches a finding to its session by the finding's `subject`, `source:id`.

## 6. Archive records and seals: front matter and `rf.archive-seal/1`

`lab` owns these. A record is a Markdown file that a `[[lab.archive.index]]` includes. It may start with YAML front matter: a first line `---`, YAML, and a line `---`. Everything after it is the record's body. Every field is optional, unknown fields are ignored, and a single string is accepted where a list is expected.

```markdown
---
id: D012
title: Use BM25 as the first-stage retriever
date: 2026-11-07
status: accepted
summary: BM25 beat the dense baseline on the pilot queries.
aliases: [lexical baseline]
supersedes: [D009]
decisions: [D010]
evidence: [docs/research/followups/2026-11-06-pilot/README.md]
tags: [retrieval]
---
```

| Field | Meaning |
| --- | --- |
| `id` | Stable identifier, such as `H158` or `D012`. When absent, the index derives it from the file name (`id_from`, `id_prefix`, `trim_leading_zeros`), the folder name for `README.md`, or else the file stem. |
| `title` | When absent, the first `# ` heading after the front matter. |
| `date` | `YYYY-MM-DD`. When absent, the first date in the file's path. |
| `status` | One of `[lab.archive] statuses` (default: `current`, `draft`, `proposed`, `accepted`, `open`, `resolved`, `superseded`, `rejected`, `withdrawn`, `historical`). The `inactive_statuses` (default: `superseded`, `rejected`, `withdrawn`, `historical`) mark records that no longer apply. |
| `summary` | One line shown in the index. |
| `aliases` | Other names `lab archive find` matches. |
| `supersedes`, `superseded_by`, `decisions` | Ids of other records. Each must exist, and the two supersession lists must agree. |
| `evidence` | Paths, relative to the project root, that support the record. |
| `tags` | Free labels. |

Each index file is generated: its first line is ``<!-- Generated by `lab archive update` from front matter. Do not edit by hand. -->``, and it lists one record per line.

The seal manifest (`[lab.archive] seal_manifest`, default `<state_dir>/lab/archive-seal.json`) records the append-only files. Each entry holds the length and SHA-256 of the file's body, so front matter, such as `status` or `superseded_by`, can change without breaking the seal. A body that still starts with the sealed bytes was appended to; any other change is an edit, which `lab archive seal --accept <path> --note <why>` records in `history`. The file is pretty-printed JSON with keys in a fixed order and a trailing newline:

```json
{
  "schema": "rf.archive-seal/1",
  "files": {
    "docs/research/log/2026-11.md": { "bytes": 18234, "sha256": "…" }
  },
  "history": [
    { "at": "2026-11-07T03:00:00Z", "path": "docs/research/log/2026-10.md", "old_sha256": "…", "new_sha256": "…", "note": "removed a student number pasted by mistake" }
  ]
}
```

## 7. Conventions every repository follows

- Rust edition 2024, `rust-version = "1.99"`, stable toolchain. MIT OR Apache-2.0 licence files.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass, and a GitHub Actions workflow runs them.
- Network access happens only in explicit commands, goes through a client that spaces requests per host and retries 429 and 5xx with backoff, and identifies itself with a generic user agent (`<tool>/<version>`). No email address or personal data in requests.
- Tests use synthetic fixtures. Nothing from a private project, no personal data and no absolute home-directory paths enter a repository.
- Errors name the file and line or the record they concern. No silent fallbacks: a value a tool cannot establish is reported as unknown.
