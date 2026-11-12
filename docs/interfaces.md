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

- A table's sub-tables belong to the same tool: `lab` owns `[lab.archive]`, `[[lab.archive.index]]`, `[[lab.archive.budget]]`, `[[lab.archive.offloaded]]`, `[[lab.archive.kept_link]]`, `[lab.archive.open_items]`, the tables of archive model 2 and `[lab.search]` (section 6), and `[lab.evaluate]` and `[lab.loop]` (section 7).
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
| `evidence` | Zero or more `{path, locator}` pointers; `path` is relative to the project root when inside it. A checker may leave it out or write `null`, read as none; `lab check` stores it in every record, `[]` when empty (since lab 0.6.1) |
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
- `texts`: local extracted-text copies a checker may quote against. Paths are relative to the project root. The `sha256` is of the text file. `origin` is the URL the text came from. The optional `retrieved` is the UTC time `papers read` downloaded it (RFC 3339). Since claimcheck 0.7.0, a number in a citing sentence that occurs in none of the cited texts can still match a source number near the sentence's context words (within `[claimcheck.numbers] context_window`): one with more fractional digits that rounds half-up or half-even to it, for a document number of 2 or more significant digits (truncation does not match; `[claimcheck.numbers] match_rounded`, default `true`, turns rounding off when `false`), or, for a document percentage (`%` or `pp`), a fraction between 0 and 1 whose value × 100 equals it, or with rounding on rounds to it. Each such match is an info `number-not-in-source` finding whose `data.match` gives the `kind` (`rounded`, `percent` or `rounded-percent`), `reference`, `path`, `line` and source `value`; a negative number still needs a negative source number. Since claimcheck 0.7.0, texts also match thousands groups split by a space or by a comma and a space (`1, 590, 436`) as the joined number, and each group still matches on its own. In texts and documents, a dash right after `%`, `‰` or `pp` reads as a range, not a minus sign (`52%–90%`); in a text such a number also counts as negative, since extracted table cells can run together. Since claimcheck 0.8.0, notes and source texts read a scale after one space (`26.6 M`, `4.57 bn`) and the words thousand, million, billion, trillion and mn, and a scaled note number matches a nearby source number whose full value rounds to it at that scale (75.1M for 75,088,497); each such match is an info `number-not-in-source` finding with `data.match` kind `scaled`.
- `duplicate_of`: the key of the surviving entry when two keys describe the same work. Keys are never renumbered or deleted.
- `source`: where the archive records the entry, as a `path` relative to the project root and a `line`. Since claimcheck 0.6.0, a prose sentence without a citation, in a file that exactly one entry names as `source.path`, counts as citing that entry; a duplicate entry resolves to the survivor at the end of its `duplicate_of` chain, and a broken or cyclic chain gives no implied citation. A file that two or more entries name, such as a shared catalogue, gets no implied citation, so `uncited-claim` still fires there. Paths are compared after resolving `.`, `..` and symlinks. An implied citation stops `uncited-claim`, but `labels.require` still asks for an evidence label. Its numbers are checked against the entry's texts only when one of them can be read; otherwise they are not checked, and no `source-text-missing` is reported. Its quotations and claim depth are not checked. A written citation in a sentence replaces the implied one. Since claimcheck 0.9.0, `[claimcheck.paper_notes] commentary_sections` (a list of heading texts, default empty) turns the implied citation off under a listed heading: an uncited sentence there gets the normal rules (`uncited-claim`, `links_are_evidence` and evidence labels), and a cited sentence is checked as before. Headings match as reference-list headings do: by visible text, case-insensitive, at any level, with a trailing colon ignored. The section runs to the next heading of the same or higher level. trading sets `["Trading implications", "Related"]`.
- `verified`: the last metadata check against an index, or null.
- Optional fields are omitted when empty, and readers keep fields they do not know. `papers` adds `authors_truncated` (the source lists only the first authors), `author_variants` (an index's misspellings of the first author, accepted by `verify`) and `citekey`; its README describes them.
- Unknown values are `null`, never invented.

**Paper cards: `rf.paper-card/1`.** `papers card --key <key>` (papers 0.6.0) writes `<card_dir>/<key>.json`, only when `[papers] cards = true`. It is one JSON object with these fields:

- `schema`, `key`, `class` (`generated`), `text {path, sha256}`, `model`, `created`, `prompt_sha256` and `checker`.
- `usage {input_tokens, output_tokens, cost_usd}`. `cost_usd` is absent when the model reply gives none.
- `written`, `dropped`, and `dropped_by {field, short, not_found, number}`.
- `items [{field, value, quote, lines}]`. `field` is `task`, `dataset`, `method`, `metric`, `result` or `limit`.

Each kept item meets these conditions:

- Its `quote` occurs in the text within `lines`, after claimcheck's normalisation. `lines` are 1-based, inclusive and raw.
- Its `quote` has at least 8 words. A CJK character counts as a word.
- Every number in its `value` occurs in its `quote`.

A card that keeps under 80% of its items is not stored. A card is stale when `text.path` no longer has `text.sha256`. `papers registry check` reports a stale card as `derivative-stale`, and `lab archive check` reports it as `stale-derivative`. A card is routing text, never a citable source. papers' default `card_dir` is `sources/cards` under the project root. The `refs` scope of `lab search` reads `[lab.search] cards`, by default `sources/cards/*.json` beside the registry. The two defaults agree when the registry is at the project root. The full definition is in [papers `docs/interfaces.md`](https://github.com/syz51/papers/blob/main/docs/interfaces.md#paper-cards-rfpaper-card1), section "Paper cards: `rf.paper-card/1`".

## 4. Provenance records: `rf.run/1` and `rf.manifest/1`

`prov` owns these. Other tools may read them. Their schemas are in the `prov` README.

## 5. Agent-session records: `rf.session/1`

`agentprof` owns these. Their schema is in the `agentprof` README. `lab retro` reads `agentprof`'s `rf.finding/1` findings (the `findings.jsonl` that `agentprof scan --out <dir>` writes, named by `[lab] agent_findings` or `--agent-findings`), so agent-session problems join the same pool as checker findings. `lab retro`, `lab brief` and `lab loop` count a rule's agent findings once per session tree: a session without a `parent_id` together with every session below it, within one `source`. A finding whose `subject` is not a session in the file counts on its own. They read the sessions file only when there are agent findings, and `lab retro --agent-findings FILE` reads the `sessions.jsonl` next to FILE. Without a readable sessions file each distinct finding counts, and they say why. `lab evaluate` also reads `source`, `id`, `role`, `start` and `parent_id` from the `sessions.jsonl` next to it (or `[lab] agent_sessions`), to measure a lesson about an agent-session rule per session within strata of the same `source` and `role` (a missing `role` is `unknown`). Within a stratum, the sessions below one root session (following `parent_id` up, within one `source`) form one session group, on the root's side of the adoption date; a session without a `parent_id` is a group of its own. It matches a finding to its session by the finding's `subject`, `source:id`. A lesson metric whose rule starts with `session.` names a numeric field of the record by its dotted path, such as `session.max_prompt_tokens` or `session.total.tokens.output`; a missing, `null` or non-numeric value leaves that session out. Since lab 0.9.10, in a linked git worktree that lacks the configured findings or sessions file, which git ignores there, these commands read the main checkout's file instead (`git worktree list` names the main checkout first), and say so on stderr.

**Archive lookups.** Since `agentprof` 0.3.0, each session record has an `archive` object that counts how the session looked up the archive. `agentprof` takes the archive root from the first of these that is set:

1. `--archive-root DIR` on the command line.
2. `[agentprof] archive_roots = [..]` in `research.toml`.
3. The directory of `[lab.archive] start`. This is the only `[lab]` key `agentprof` reads.

The object has eight fields. A lesson names each as a metric path:

| Metric path | Counts |
| --- | --- |
| `agentprof:session.archive.searches` | archive searches |
| `agentprof:session.archive.opens` | archive records opened by id |
| `agentprof:session.archive.related` | graph lookups |
| `agentprof:session.archive.no_hit_searches` | searches that found nothing |
| `agentprof:session.archive.greps` | grep runs under the archive root |
| `agentprof:session.archive.reads` | file reads under the archive root |
| `agentprof:session.archive.search_share` | share of lookups that are searches |
| `agentprof:session.archive.grep_share` | share of lookups that are greps |

`greps`, `reads`, `search_share` and `grep_share` are `null` when no archive root is set. `agentprof` counts a search that exits 1 as a no-hit search, not as a tool error. It counts by name: `lab search`, `lab open`, `lab related` and `lab archive find` on the command line, and the MCP tools `archive_search`, `archive_open` and `archive_related`. A new lookup command or tool must keep these names, or `agentprof` must learn the new name first.

Lab reads a metric through a lesson: `lab lesson add --metric agentprof:session.archive.<field>` names it, and `lab evaluate` measures it. `lab evaluate` treats a lower value as better, so a lesson about archive search measures `grep_share` or `greps`, not `search_share`.

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
| `title` | When absent, the first `# ` heading after the front matter, without a leading copy of the record's id and its separator (`# H158 — Moved` gives `Moved`). |
| `date` | `YYYY-MM-DD`. When absent, the first date in the file's path. |
| `status` | One of `[lab.archive] statuses` (default: `current`, `draft`, `proposed`, `accepted`, `open`, `resolved`, `superseded`, `rejected`, `withdrawn`, `historical`). The `inactive_statuses` (default: `superseded`, `rejected`, `withdrawn`, `historical`) mark records that no longer apply. |
| `summary` | One line shown in the index. |
| `aliases` | Other names `lab search` matches, with the weight of the id. |
| `supersedes`, `superseded_by`, `decisions` | Ids of other records. Each must exist, and the two supersession lists must agree. |
| `evidence` | Paths, relative to the project root, that support the record. |
| `tags` | Free labels. |
| `task`, `kind`, `amends`, `amended_by`, `withdrawn_reason` | Archive model 2 only; see "Archive model 2" below. |

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

An offloaded list (`[[lab.archive.offloaded]]`, since lab 0.9.1, whose older versions reject the key) records files moved out of the project, such as to cloud storage, that the archive still links. `list` is a committed plain-text file, a relative path below the project root without `.` or `..`, configured once. It holds one path per line, written like a link from the project root: a leading `/` or `./`, `.`, `..`, a trailing `/` and a byte-order mark are resolved or ignored, blank lines are skipped, and a line that leaves the root is an error. The list is not compressed. `location` says where the files went, such as `cloud:archive/2026-11-08/masters/`. `lab archive check` does not report a link as `broken-link` when its missing target is a listed path, lies in a listed directory, or is a directory that holds a listed path. It reports one `archived-link` info finding per list instead, with the list as its subject and `data` `{"links": N, "location": "…"}`. Its message names the location and changes only when the count reaches or leaves 0, so the finding keeps its fingerprint while links come and go; the text output adds the count. A missing target that no list holds, or one outside the project root, stays a `broken-link` error. A configured list that is missing or unreadable, or holds a line that leaves the root, stops the check with an error (exit 2). Only links honour the list: a sealed file moved out still fails `sealed-missing`. Since lab 0.9.10, in a linked git worktree, a missing link target below the project root that git ignores there is found when it exists in the main checkout. lab only checks that it exists there and does not check its anchors; a replay's worktree does not look there.

Since lab 0.20.0, a link in a checked file to a file that exists but that git neither tracks nor ignores is an `untracked-link` warning. Such a file exists only in this checkout: the link breaks in every other checkout, and a linked worktree's copy is lost with the worktree. A link to a directory counts when the directory holds such files and git tracks nothing in it. Commit the target in the same commit as the link. A target that git ignores, such as data or downloads, is not flagged, and outside a git repository nothing is. A warning does not stop `lab archive done`; it prints the line and its fix.

A kept link (`[[lab.archive.kept_link]]`, since lab 0.9.11, whose older versions reject the key) records a broken link that stays in the sealed text of an append-only file, such as a quoted past answer that links a file on another machine, where no permitted edit can fix it. Each entry has three fields, and any other field is refused:

```toml
[[lab.archive.kept_link]]
file = "docs/research/history/01-2026-10-02-start.md"  # relative to the project root
target = "/Users/me/Downloads/plan.pdf"               # the link destination exactly as written
reason = "quotes a past chat answer that linked a file on another machine"
```

`lab archive check` does not report a missing link target in `file` whose destination equals `target` as `broken-link` when the link's line lies wholly in the file's sealed text: the file matches `sealed`, has a seal-manifest entry whose seal still holds (unchanged or appended to), and the line ends within the entry's `bytes`, counted from the start of the body, so front matter may change. It reports one `kept-link` info finding per such link instead, with the link as its subject and `data` `{"target": "…", "reason": "…"}`; the text output adds the reason. The same link outside the sealed text, in a file that is not append-only or not sealed yet, or in text appended after the seal, stays a `broken-link` error whose message says that `kept_link` covers only sealed text. An entry that matches no missing link in a checked file, such as one whose target exists now, is a `stale-kept-link` warning with `file` as its subject. An entry with an empty `target` or `reason`, a `file` that is not a relative path below the root without `.`, `..` or empty segments, or a `file` and `target` configured twice stops the check with an error (exit 2). An offloaded list takes precedence: a target it holds is counted there.

An open-item rule (`[lab.archive.open_items]`, since lab 0.13.0, whose older versions reject the key) makes a record of a board task name the task that holds each of its open items. It is off without the table:

```toml
[lab.archive.open_items]
since = "2026-11-12"               # the first day checked
boards = ["research-factory"]      # a task id is <board>-<number>
headings = ["Open questions", "Limits", "Follow-ups", "Next steps", "Not done"]  # the default
```

It checks every Markdown file that `lab archive check` scans (an index's records and the files of `sealed`, `links`, `current_pages` and each `budget`), except index outputs, whose front matter is a mapping with a non-empty `task` field. A file is skipped when its date is before `since`: the front matter's valid `date`, else the first date in its path. A file with neither date is checked. In such a file, a section whose ATX heading (`## Limits`) matches one of `headings` (by visible text, case-insensitive, at any level, with a trailing colon ignored) runs to the next heading of the same or higher level. Each list item there (`-`, `*`, `+` or `1.`, with its continuation lines) must hold a task id, `<board>-<number>` for one of `boards` and not inside a longer name (`old-masters-3` names no `masters` task), or the words `no action:` followed by a reason; `**` and backticks are ignored. An item passes when an item it is nested in passes, and an item with nested items is checked through them. Paragraphs, tables, thematic breaks (`* * *`), fenced code and HTML comments are not checked; a numbered line that interrupts a paragraph starts a list only with `1.`, as in CommonMark. Each miss is an `open-item` error with the item as its subject, `data` `{"section": "…"}` and the item's first 60 characters in the text output. The rule checks only that an id is written: not that the task exists, is open, or covers the item. In archive model 2, an `[[lab.archive.allow]]` entry with `rule = "open-item"` and a file's path as `subject` (since lab 0.13.1) turns every miss in that file into a note with the entry's reason; it is meant for sealed text that no permitted edit can change, and it also covers items appended to the file later. An empty `boards` or `headings`, a board name other than letters, digits, `-` and `_`, or a `since` that is not `YYYY-MM-DD` stops the check with an error (exit 2).

### Search: `[lab.search]` and `rf.search/1`

`lab search` (since lab 0.11.0, whose older versions reject the `[lab.search]` table) needs `[lab.archive]` and no other configuration. Every key of `[lab.search]` is optional:

```toml
[lab.search]
notes = ["docs/**/*.md"]             # the files of the `notes` scope; since 0.19.2 without the texts the registry lists
notes_exclude = ["docs/drafts/**"]
stages = ["lexical"]                 # the stages a query should run: lexical, dense, rerank, graph
cites = ['\[(?P<key>\d+)\]']         # since 0.15.0: more citation forms, each with a group `key`
refresh_budget_ms = 5000             # a query's refresh of changed files stops after this time; `lab archive done` indexes them all
lock_timeout_ms = 30000              # how long a query waits for another process that builds the index
default_scopes = ["notes", "refs", "texts"]  # the scopes a query without --in searches (since lab 0.16.0; texts by default since 0.19.0)
cards = ["docs/research/sources/cards/*.json"]  # paper cards of the `refs` scope
files = ["runs/**"]                  # the files of the `files` scope
files_exclude = ["runs/**/*.tmp"]

[lab.search.classes]                 # source classes by path; a file in none is `own`
paper = ["docs/papers/**"]
copied = ["docs/inputs/**"]
generated = ["docs/**/GENERATED-*.md"]
chat = ["docs/chats/**"]
historical = ["docs/old/**"]
```

- Without `notes`, the scope is every `.md` file below the directories that the globs of `[lab.archive]` name (`index.include`, `sealed`, `links`, `current_pages`, `start`): `docs/history/[0-9]*.md` gives `docs/history/**/*.md`. A glob for files in the project root, and a path without a pattern, stay as they are.
- A `generated` file is not indexed, and each `[[lab.archive.index]]` output is always generated. The other classes are indexed and labelled. `paper` keeps its whole score (0.8 before lab 0.19.0), and so does a paper card of class `generated` (0.5 before lab 0.19.0); `chat` keeps 0.6, `copied` 0.5 and `historical` 0.25, so they rank below `own`. A record with one of the `inactive_statuses` keeps 0.25, and a superseded record ranks below each successor that the same query found.
- Of the stages, this version builds `lexical` and, since 0.15.0, `graph`. A query that `stages` asks another stage for ends as `partial` and names the stage.
- `cites` adds citation patterns (Rust regular expressions) to the two that are always read: `ref:<key>`, and a link to the `#reference-<key>` anchor of a file. Each pattern needs a group named `key`; a pattern without one, or one that does not compile, stops the query with an error.

#### Scopes

`--in` takes scopes, repeated or comma-separated, or `all`; without it a query searches `default_scopes`. A query builds and refreshes only the scopes it searches; the first query of a scope adds the whole scope, without the refresh budget.

| Scope | Sources | Unit | Anchor | Class |
|---|---|---|---|---|
| `notes` | the Markdown files above; since 0.19.2 without each text that a registry entry lists, preferred or not, because `texts` holds one text of each paper (a registry that cannot be read leaves them in and gives a notice) | Markdown section | `path#Lstart-Lend` | by `classes` |
| `refs` | the entries of the registry (`[papers] registry`, default `references.jsonl`), and the paper cards | an entry: its `abstract` and its catalogue line (`source.path`, `source.line`); each card item | `ref:<key>`; a card item `text#Lfirst-Llast` of its quote | `paper`; a card `generated` |
| `texts` | the extracted text of each entry | passage of about 200 words | `text#Lstart-Lend` | `paper` |
| `files` | `files`, or else the files git tracks that are not Markdown and not notes | the file's path words, size and the record of the nearest directory that holds a note with an id | the path | by `classes` |
| `sessions` | the main sessions of the `rf.session/1` list (`[lab] agent_sessions`, else `sessions.jsonl` beside `agent_findings`) whose transcript is `present` | a message the person typed, at most 8000 characters; tool results, injected context and, by rule, compaction summaries (`compaction`), delegation briefs (`brief`), wake-ups (`wake-up`) and messages of fewer than 3 words (`short`) are left out, and `lab search --rebuild` prints the count left out by each rule | `transcript#Ln` (JSONL line) | `chat` |
| `board` | the tasks that `[lab.archive] board_export` prints (an array, or `{"tasks": [...]}`; each task's `notes` when the export has them) | the task's title, body, acceptance and summary; each note | `board:<id>`, `board:<id>#note-<n>` | `chat` |

- **Registry.** lab reads each line as JSON without a fixed schema: `key` (string or number), `title`, `authors` (may be null), `year`, `venue`, `depth`, `abstract`, `ids.arxiv`, `ids.doi`, `texts` (`[{path, extraction}]`), `duplicate_of` and `source`. An entry whose `duplicate_of` chain ends at another entry is a unit of that entry, with the heading `duplicate ref:<key>`; a hit names the target's key. A chain that loops or names a missing key ends at the entry itself. `ref:<key>`, the arXiv id without its version and the DOI without `https://doi.org/` are exact lookups, as is a query of one registry key alone (`lab search 232`). The name of a text file is a short name of its entry and an exact lookup too (`hirec.txt` makes `HiREC` find its entry), unless the name holds no letter, is the key, is a generic word such as `paper` or `main`, lies in a directory named by the key, or is shared with another entry's text. Text output of a `refs` hit: `ref:<key> · year · first author · title · depth · text path`.
- **Texts.** Of an entry and its duplicates, the text with an `extraction` other than null or `plain` (the last one listed) is preferred, else the first plain one; a text path that this checkout lacks and git ignores is read from the main checkout of a linked worktree. NUL bytes read as spaces. With a structure file `<text>.structure.json` (`rf.paper-structure/1`) whose `text.sha256` equals the text's, a passage stays within one section, the section's heading is its heading, and `references` sections are left out; without one, passages run through the whole text. Lines are joined for matching (a word split by a hyphen at a line end is one word) while anchors keep the text's line numbers. Each passage carries the paper's title and its registry key (`hits[].key`).
- **Paper cards** (`rf.paper-card/1`, default `sources/cards/*.json` beside the registry): each item is a unit of `field`, `value` and `quote` at the quote's `lines`. A card is routing text: class `generated`, which keeps its whole score (0.5 before lab 0.19.0).
- **Stale derivative.** A structure file or card whose recorded `text.sha256` differs from its text's sha256 is ignored: the passages are cut without the structure file, the card gives no unit. A query of the scope ends `partial` and names it, and `lab archive check` and `lab archive done` report it as a `stale-derivative` warning.
- **Unreachable sources.** A board export that fails, prints no task array or runs longer than 15 seconds, no configured board command, and a missing session list make the query `partial` with a notice naming the scope, never an error; the index keeps what it held of that scope.
- **Numbers.** A decimal number in a query is one term, without a fraction of only zeros: `47.0%` finds `47` and `47.0`, and `0.50` finds `0.50`.

The unit of the `notes` scope is one Markdown section: from a heading to the line before the next heading, cut at a blank line when it is longer than 120 lines. A file without text below a heading is one unit. A unit's anchor is `path#Lstart-Lend`: the path relative to the project root and 1-based line numbers of the file, both inclusive, the heading line included. Two anchors name the same text when their paths are equal and their line ranges overlap.

The index is `<state_dir>/lab/search/index.db` (SQLite FTS5), one for each checkout: a linked git worktree builds its own and never reads the main checkout's. Its directory ignores itself in git. Each query first compares the stamp of every source of the scopes it searches (a file's size and modification time; an entry's JSON line and catalogue line; a task's JSON) with the index, and reads the changed sources (a file whose content hash is the same is not indexed again) and removes the deleted ones. An index of another schema version or another configuration, and a file that SQLite cannot read, are replaced: the new index is written to a second file and renamed over the old one. One process writes at a time (`lock`, an advisory file lock); a query that finds the lock held answers from the last committed state and says the index is stale.

`lab search --json` prints one object:

```json
{"schema":"rf.search/1","state":"hits","query":["wombat"],"scopes":["notes"],
 "stages":{"dense":"missing","graph":"ready","lexical":"ready","rerank":"missing"},
 "partial":[],"unknown":[{"word":"retreiver","nearest":["retriever"]}],
 "hits":[{"rank":1,"anchor":"docs/history/01-2026-10-02-start.md#L5-L7",
   "path":"docs/history/01-2026-10-02-start.md","start":5,"end":7,
   "heading":"Start of the project > Plan","snippet":"Measure the retriever on the wombat questions.",
   "hash":"e98921524562063a3bd527c2b9ffcbc4",
   "record":{"id":"H1","title":"Start of the project","date":"2026-10-02","status":null,
     "class":"own","kind":null,"task":null,"inactive":false},
   "missing":["retreiver"]}],
 "more":0}
```

| Field | Meaning |
|---|---|
| `state` | `hits`; `partial` (hits, but `partial` lists why they may be incomplete: a stale index, a stage that is not built); `zero` (no hit); `error` (with `error`, the message; exit 2). The first three exit 0. |
| `query` | The phrasings: the arguments as one phrasing when each is one word, otherwise one phrasing for each argument. |
| `stages` | Each stage as `ready`, `missing` or `failed`. |
| `unknown` | The query words that no unit holds, each with up to three near indexed words. |
| `scopes` | The scopes searched. A query with no hit prints the scopes it did not search. |
| `hits[].scope`, `hits[].key`, `hits[].info` | The hit's scope; the registry key of a `refs` or `texts` hit; facts of the source: for an entry `key`, `year`, `first_author`, `depth`, `venue`, `text` (the local text) and `duplicates`; for a passage `extraction` and `structure` (`used`, `stale` or `none`). |
| `hits[].hash` | The content hash of the unit's indexed text (32 hexadecimal characters of its SHA-256). |
| `hits[].record` | The file's record: `id`, `title`, `date`, `status`, `kind` and `task` are null when the file has none; `superseded_by` lists the successors' ids when there are any. |
| `hits[].missing` | The query words the unit lacks; absent when it has all. |
| `hits[].also_in` | Since 0.15.0, and since 0.19.0 only with `--fold`: the units folded under this hit because they link its record, each with `anchor` and, when its file is a record, `id` and `title`; absent when there are none. |
| `hits[].explain` | With `--explain`: `exact_id`, `bm25`, `trigram` (matches of a part of a word), `prior` (class and status), `score` (`bm25 + trigram`; before 0.19.1 times `prior`), `fused` (since 0.19.0 the sum over the phrasings of the min-max normalised score; since 0.19.1 each phrasing adds `prior * (score - low) / (high - low)`, where `low` and `high` are its lowest and highest `score`; before 0.19.0, reciprocal rank fusion) and `matched`. |
| `more` | How many more units matched than `hits` shows. |

BM25 weights the columns as E1 fitted them (DR045 of masters; since 0.19.0): the title and the heading path 4, the body 1, the id and aliases and the metadata (date, status and file path) 0.5, the summary 0.25. Text that a search returns is data: no tool runs or follows it.

### Search graph, `lab related`, `lab open` and `lab mcp`

Since lab 0.15.0 (index schema 2; an index of schema 1 is built again on the first query), the index holds a table of edges. It is filled when a file is indexed and removed with the file. The edge types:

| Edge | From | To | Read from |
|---|---|---|---|
| `supersedes`, `superseded_by`, `amends`, `amended_by` | a note | a record id | front matter: a string or a list of strings |
| `task` | a note | a board task id | front matter: a string or a list of strings |
| `links` | a note's line | a path below the project root | a Markdown link to a file of the project, without its fragment; links to other sites, to the same file and outside the project make no edge |
| `cites` | a note's line | a reference key | `ref:<key>`, a link to `#reference-<key>`, and the `[lab.search] cites` patterns |
| `cites` | a paper | a reference key | the reference list of the `rf.paper-structure/1` sidecar (`<text>.structure.json`) of each text in the registry that `[papers] registry` names; a sidecar whose text's sha256 changed gives no edge |

Belonging is not stored: a unit belongs to its file, and a file belongs to the record whose `README.md` is in a directory above it.

`lab search` uses the graph:

- `--cites KEY` (`232` or `ref:232`) keeps only units that cite the key. `--links-to ID_OR_PATH` keeps only units that link the record or file. Without words, each lists every such unit, newest file first.
- `--as-of DATE` leaves out records dated after the date and ranks a record by its state on that date: a record superseded later ranks as in force. `--since DATE` and `--until DATE` keep only records dated in the range, ends included.
- With `--fold` (`fold` in the MCP tool), a unit that links the record of a better hit is folded under that hit as an `also in` line (`also_in` in JSON). Two records of one id series (`DR040` and `DR041`) and two records in a supersession or amendment are not folded. From lab 0.15.0 to 0.18.0 folding was the default and `--all` turned it off; since 0.19.0 `--all` is accepted and does nothing, because folding cost 0.01 nDCG@10 on the masters test set (research-factory-295).

`lab related NAME [--limit N] [--json]` lists the neighbours of a record id (`DR041`), a path or anchor, `ref:<key>` or `task:<id>`, at most `--limit` (default 20) for each relation. The relations: `superseded by`, `supersedes`, `amended by`, `amends`, `task`, `part of`, `parts`, `links to`, `linked from` and `cites`; for a reference key, `cited by` and the papers it `cites`; for a task, its `records`. Each neighbour has its anchor when the edge starts at a line. `--json` prints one `rf.related/1` object:

```json
{"schema":"rf.related/1","node":{"node":"DR002","id":"DR002","date":"2026-10-05","status":"accepted",
  "title":"Use BM25","path":"docs/decisions/DR002-bm25.md","indexed":true},
 "relations":[{"name":"supersedes","direction":"out","total":1,
   "neighbours":[{"node":"DR001","id":"DR001","date":"2026-10-01","status":"superseded","title":"Use grep",
     "path":"docs/decisions/DR001-grep.md","indexed":true}]},
  {"name":"linked from","direction":"in","total":1,
   "neighbours":[{"node":"docs/notes/n.md","title":"Note","path":"docs/notes/n.md",
     "anchor":"docs/notes/n.md#L1-L3","indexed":true}]}]}
```

`direction` is `out` for an edge that starts at the node and `in` for one that ends at it; `anchor` is the unit that holds a link or citation (front-matter edges have none); `total` counts the neighbours before `--limit`; `indexed` is false for an id or path that names no indexed file. A name that names nothing exits 2.

`lab open ANCHOR [--lines N] [--json]` prints one unit with at most `--lines` (default 80) lines of text, then the anchors of up to three neighbours of each relation of its file. ANCHOR is a hit's anchor (`path#Lstart-Lend`), `path:line` (the unit that holds the line), `path:a-b` (the unit with exactly that range, else the text found as below), a path or a record id (its first section below the front matter, since 0.19.0), or a hit's content hash. When the file changed after the anchor was made and no unit has that line range, the anchor finds its old text by content hash: the output's notice says `the file changed; the text of La-Lb moved to Lx-Ly (same content hash)`, or that the text is gone and the unit now at those lines is shown. `--json` prints one `rf.open/1` object with `anchor`, `path`, `start`, `end`, `more_lines` (the lines past the budget), `heading`, the record's `id`, `title`, `date` and `status`, `notice`, `lines` and `neighbours`. A stale index prints `lab: partial: the index is stale` on stderr.

`lab mcp` is a Model Context Protocol server on stdio: newline-delimited JSON-RPC 2.0, protocol versions `2025-06-18`, `2025-03-26` and `2024-11-05`, methods `initialize`, `ping`, `tools/list` and `tools/call`; notifications get no reply and other methods get error -32601. It reads the project from its working directory and writes logs to stderr. Its tools:

| Tool | Arguments | Same as |
|---|---|---|
| `archive_search` | `query` (a string or an array of phrasings), `limit`, `records`, `status`, `cites`, `links_to`, `as_of`, `since`, `until`, `fold` (`all` before 0.19.0) | `lab search` |
| `archive_open` | `anchor` (required), `lines` | `lab open` |
| `archive_related` | `name` (required), `limit` | `lab related` |

Each description says when to use the tool. A result is text only: its first line is the state (`state: hits; missing stages: dense, rerank`, with `partial` notices after `;`), then for `archive_search` a line `mode: lexical`, then the text output of the command. Lexical search is the chosen configuration (masters DR045), so no stage loads a model; before 0.19.0 the line was `mode: lexical (model: unloaded)`. A failed call returns `isError: true` with `state: error` and the message. An agent connects it with an MCP entry such as `{"mcpServers":{"lab":{"type":"stdio","command":"lab","args":["mcp"]}}}` in the project's `.mcp.json`.

### Search learning loop: `rf.search-query/1`, `rf.search-open/1`, the thesaurus and `rf.retro/1`

Since lab 0.14.0. Two more keys of `[lab.search]`:

```toml
[lab.search]
thesaurus = "docs/search-thesaurus.txt"  # a tracked file of equivalent terms; no thesaurus without it
review_days = 90                         # `lab retro` lists entries unused for this long
```

**Query trace.** Each query of `lab search`, `lab archive find`, `lab search --run` and the duplicate check appends one line to `<state_dir>/lab/search-queries.jsonl`. In a linked git worktree where git ignores that path, the line goes to the main checkout's file, so every checkout of a project shares one trace. A trace that cannot be written leaves the query as it was: lab prints a notice on stderr and keeps the exit code.

```json
{"schema":"rf.search-query/1","at":"2026-11-12T10:00:00Z","session":"79065b65","source":"cli",
 "query":["marsupial"],"scopes":["notes"],"state":"hits","stages":["lexical"],
 "expansions":[{"term":"marsupial","added":["wombat"],"entry":"wombat = marsupial"}],
 "candidates":{"lexical":[{"id":"docs/history/01-2026-10-02-start.md#L5-L7","score":0.0328}]},
 "hit_count":1,"complete":1,"hits":["docs/history/01-2026-10-02-start.md#L5-L7"],"latency_ms":12}
```

| Field | Meaning |
|---|---|
| `at` | The time of the query, RFC 3339 UTC. |
| `session` | `LAB_SESSION`, else `CLAUDE_CODE_SESSION_ID`; null when neither is set. |
| `source` | `cli` (`lab search`, `lab archive find`), `mcp` (the `archive_search` tool of `lab mcp`, since 0.15.0), `run` (`--run`) or `similar` (the duplicate check). Only `cli` and `mcp` lines count as missed queries. |
| `state` | `hits`, `partial`, `zero` or `error` (then `error` holds the message). |
| `stages`, `missing_stages` | The stages that ran, and the configured stages that are not built. |
| `expansions` | The thesaurus entries the query applied: the query `term`, the terms `added` for it, and the `entry` as `a = b`. Absent when none. |
| `candidates` | For each stage, its 10 best units before `--records` keeps one for each record, as `id` (the anchor) and `score`. |
| `hit_count`, `complete`, `hits` | The number of hits, the hits that hold every term, and the hits' anchors, best first. |
| `unknown`, `notices` | The query words no unit holds, and why the answer may be incomplete. Absent when empty. |
| `latency_ms` | The time the query took. |

`lab open` and the `archive_open` tool (since 0.15.0) append, and another tool that reads a hit may append, `{"schema":"rf.search-open/1","at":"…","session":"…","anchor":"path#Lstart-Lend"}` to the same file. `lab retro` uses it to find the record a missed query was looking for. A reader skips lines of other schemas and lines it cannot parse.

**Thesaurus.** One entry per line, its terms separated by `=`: `fiscal calendar = fiscal year = FY`. Terms are compared as lowercased words. A line that starts with `retired:` stays in the file but is not applied. `#` starts a comment line. An entry with fewer than two different terms is an error that names the file and line (exit 2). At query time, each active entry with a term that occurs as consecutive words of a phrasing adds its other terms as alternatives to those words: `marsupial` also matches `wombat`. The human output names each one as `Also searched: wombat (for marsupial)`, and `--explain` adds the entry. The thesaurus is read from the checkout, with no network and no model call.

**Missed queries.** `lab retro` reads the `cli` and `mcp` lines of the trace. A query is missed when it found nothing (`zero`), when no hit held every term (`partial-hit`), or when the next query of the same session within 600 seconds differs and shares a word of 3 or more characters (`reformulated`). Its target is the record the session read next in that window: the first `rf.search-open/1` line, else the best hit of a later query whose hits hold every term. A line without a session is never linked to a later line. Queries are grouped by their lowercased words and listed most frequent first, each with the target most of its misses led to and an id of 8 hexadecimal digits taken from the query alone, so the id stays the same when the target changes. `--since DATE` lists only the queries last missed on or after DATE; their counts and targets still come from the whole trace. When the trace reaches back `review_days`, `lab retro` also lists the active thesaurus entries that no `cli` or `mcp` query applied in that time.

**Decisions.** `<state_dir>/lab/retro.json` (`rf.retro/1`, in the main checkout's state directory like the trace) records what was decided, so that a decided item does not return:

```json
{"schema":"rf.retro/1",
 "dismissed":[{"tool":"agentprof","rule":"reread-file","at":"…","note":"harmless"}],
 "accepted":[{"id":"3f1c2a9b","query":"fy shift","target":"D002","wrote":"front matter","at":"…"}],
 "rejected":[{"id":"9a0b1c2d","query":"calendar","target":"docs/a.md","at":"…","note":"not a synonym"}]}
```

- `lab retro accept ID [--record RID]` adds the query as an alias of the target record (`--record` names one when the trace has none). An open record gets the alias in its front matter `aliases`. A finished record, which matches `sealed` or has a status in `sealed_statuses`, `terminal_statuses` or `inactive_statuses`, is never edited: the thesaurus gets the line `query = RID` instead (`wrote` is `thesaurus`). An accepted query is not proposed again.
- `lab retro reject ID... [--note TEXT]` records the proposals; a rejected query is never proposed again, whatever its target.
- `lab retro dismiss TOOL:RULE [--note TEXT]` hides a recurring-rule candidate from later `lab retro` runs.
- `lab retro retire LINE` marks the thesaurus entry on that line `retired:`.
- `lab retro seed` adds an `alias = RID` entry for each front-matter alias of an indexed record that the thesaurus lacks. The alias is written as its lowercased words.

An ID may be given by a unique prefix.

**TREC runs.** `lab search --run TOPICS [--out FILE] [--phrasings] [--limit N] [--records]` answers a topics file and writes a TREC run, `topic Q0 docid rank score lab-search`, with the score `limit + 1 - rank`, so no two scores of a topic tie. A topics line is a JSON object with `id` (string or number) and `query`, other fields ignored, or `id<TAB>query`; `#` starts a comment. `--phrasings` adds a JSON topic's `phrasings` as more phrasings. The docid is the hit's anchor, `path#Lstart-Lend` or `ref:<key>`, or with `--records` the record's path; `%` and spaces are escaped as `%25` and `%20`. Since 0.19.0 the run lists the docids in the order the text output shows them: each hit, then, with `--fold`, the units folded under it as `also in` lines, each docid once, up to the limit. The default limit is 10. `ireval eval QRELS RUN` scores the run.

**Duplicate check.** `lab archive similar FILE|WORDS...` searches the records whose sections hold at least 60% of the weight (`ln(1 + units / units holding it)`) of the subject's words, and at least two of them when the subject has two or more. The subject is a record's title and aliases, or the words given. It prints `similar: none`, `similar: N record(s) on the same topic` with up to 5 lines, or `skipped: REASON` when the search fails, and always exits 0. It builds the index when it is absent. `lab archive next PREFIX --title TITLE` prints the same list on stderr. `lab archive check` runs it for each new record file (untracked and not ignored, or added in the git index, and matched by an index's `include`) against the records that are not new: a match is a `similar-record` warning, and a failed search is a `similar-skipped` info finding.
### Archive model 2

Since lab 0.12.0, whose older versions reject these keys. `[lab.archive] model = 2` turns on the rules and commands below. With `model = 1`, the default, every key of this section is refused, and the archive behaves as lab 0.9.11 does: old sealed files, the seal manifest and its check stay as they are. In model 2 git holds the sequence of work and the board holds the open work, so the archive keeps only records: no month log is appended, and no index file needs to be written.

```toml
[lab]
min_version = "0.12.0"        # `lab brief` says when the installed lab is older

[lab.archive]
model = 2
seal = "git"                  # default "manifest"
seal_baseline = "745f806"     # optional: the history check starts after this commit
sealed_statuses = ["accepted", "superseded", "rejected", "withdrawn"]
terminal_statuses = ["superseded", "withdrawn"]   # the default, where `statuses` has them
reason_statuses = ["superseded", "rejected", "withdrawn"]   # default: `inactive_statuses`
write_indexes = false         # default true
kinds = ["result", "decision", "survey", "review"]   # the default
task_from = "2026-11-12"
board_export = ["board", "ls", "--json", "--limit", "1000", "--fields", "id,status,parent,updated_at"]
research_parents = ["research-factory-185"]   # or research_tag = "research", for an export with tags
summary_min_chars = 40        # the default
unindexed_exclude = ["docs/research/decisions/README.md"]

[[lab.archive.accepted_edit]]
path = "docs/research/decisions/DR007-scope.md"
commit = "c0c1783016"
reason = "removed a student number pasted by mistake"

[[lab.archive.page_group]]
name = "topics"
include = ["docs/research/topics/*.md"]
current = ["current"]         # the default

[lab.archive.historical_link]
from = ["current"]            # the defaults
to = ["historical"]
marks = ["historical"]

[[lab.archive.allow]]
rule = "missing-summary"
subject = "docs/research/history/001-2026-10-02-start.md"
reason = "the first report has no single result"
```

| Key | Meaning |
| --- | --- |
| `[lab] min_version` | The oldest lab version the project works with, as numbers separated by dots. `lab brief` starts with a line that says so when the installed lab is older. |
| `model` | `1` or `2`. |
| `seal` | `"manifest"` (default) or `"git"`. With `"git"`, `seal_manifest` is refused, no manifest is written or read, and the four keys below apply. |
| `seal_baseline` | A commit. The history check reads only the commits after it. Without it, the check reads all history. A value that is not a commit stops the check (exit 2). |
| `sealed_statuses` | A record is sealed from the first commit in which its status is one of these. Each must be in `statuses`. |
| `terminal_statuses` | Statuses that no later commit may change. |
| `[[lab.archive.accepted_edit]]` | `path`, `commit` (the hash, or at least its first 7 characters) and `reason`. The edit of sealed `path` in that commit stays, and so does the same edit at a merge commit that brings the commit in. It is the only way to keep an edit. |
| `reason_statuses` | The statuses that need `superseded_by` or `withdrawn_reason`. Each must be in `inactive_statuses`. |
| `write_indexes` | `false`: `lab archive index`, `update` and `done` write no index file, an index needs no `output`, and an index file that is still there is an `index-leftover` warning. The start page stays a file with its budget. |
| `kinds` | The allowed values of a record's `kind`. An index may set `kinds = [...]` to list only records of those kinds; a record that its include matches and that it does not list is still a record that every rule checks. |
| `task_from` | A day as `YYYY-MM-DD`. A record dated on or after it must name a `task` and a `kind`. |
| `board_export` | A program and its arguments that print the board's tasks as JSON: an array of objects, or an object with a `tasks` array. Each object has `id`, and may have `status`, `tags` (or `labels`), `parent` and `updated_at`. lab runs it in the project root with a limit of 30 seconds. |
| `research_tag` | A task of the board export with status `done` and this tag needs a record that names it. Use it for an export that has tags; agent-board tasks have none. Needs `board_export`. |
| `research_parents` | Task ids. A task of the board export with status `done` needs a record that names it when it is one of these tasks or below one, following `parent`. Group research work under these umbrella tasks. Needs `board_export` with the fields `parent` and `updated_at`. Since lab 0.13.3. |
| `summary_min_chars` | The shortest accepted `summary`, in characters. |
| `unindexed_exclude` | Globs of Markdown files that `unindexed-file` leaves alone. |
| `[[lab.archive.page_group]]` | `name`, `include`, optional `exclude` and `current`. At least one page of the group must have a status in `current`. |
| `[lab.archive.historical_link]` | A link from a record with a status in `from` to a record with a status in `to` must have one of `marks` on its line, in any letter case. |
| `[[lab.archive.allow]]` | `rule`, `subject` and `reason`, and for `historical-link` an optional `target` (the link destination as written). Accepts the findings that match its rule and subject (and target), for `duplicate-id`, `missing-summary`, `unindexed-file`, `no-current-page`, `historical-link`, `task-without-record` or (since lab 0.13.1) `open-item`. An `open-item` entry accepts every miss in the file, including items appended later. `subject` is the finding's subject without a line number: a path, the duplicated id, the page group's name or the task id. |

Model 2 adds these front matter fields. A value of the wrong type is a `front-matter-invalid` error in model 2, and is ignored in model 1.

| Field | Meaning |
| --- | --- |
| `task` | The board task, or a list of tasks, that the record is the result of. |
| `kind` | One of `kinds`. |
| `amends`, `amended_by` | Ids of other records, for a partial supersession. Each must exist, and the two lists must agree, like `supersedes` and `superseded_by`. An amended record stays in force and keeps its status. |
| `withdrawn_reason` | Why a record with an inactive status no longer applies, when no record replaced it. |

The status of a record moves only forward. A record whose status is in `reason_statuses` needs `superseded_by` or `withdrawn_reason`. A record whose index requires `status` and that has none is a `missing-field` error, as in model 1. With the git seal, a status in `terminal_statuses` never changes again.

The git seal. A file is sealed from the first commit in which it matches `sealed` or its status is in `sealed_statuses`, and stays sealed when a later commit changes its status. In each later commit, in the staged change and in the working tree, its body must start with the sealed body; front matter may change. A merge commit is compared with its first parent. An edit that is restored later is still an edit: both commits change sealed text. An edit in the working tree or the index cannot be accepted, because it has no commit yet. lab keeps the last commit whose history was clean in `<state_dir>/lab/archive-git-seal.json` (`{"schema": "rf.archive-git-seal/1", "commit": "…", "key": "…", "status_sealed": ["…"]}`, where `key` is a hash of the settings the check depends on and `status_sealed` lists the records sealed by status so far), so the next check reads only newer commits. The file is a cache: when it is missing, does not parse, has another key, or names a commit that is not an ancestor of `HEAD`, lab checks all history again. The check needs no remote and works in a linked worktree. `lab archive seal` checks the history and writes nothing; `--accept` is refused.

Model 2 adds these rules to `lab archive check`. Each finding has the tool `lab`.

| Rule | Severity | Reported when |
| --- | --- | --- |
| `sealed-edited` | error | Git seal: a commit, the staged change or the working tree changes sealed text. The message names the commit. |
| `sealed-missing` | error | Git seal: a commit, the staged change or the working tree removes a sealed file. |
| `status-reversed` | error | Git seal: a commit changes a status in `terminal_statuses`. |
| `seal-unverified` | error | Git seal: the project is not in a git repository, or git is not installed. |
| `inactive-without-reason` | error | A status in `reason_statuses` without `superseded_by` or `withdrawn_reason`. |
| `unknown-id` | error | `amends` or `amended_by` names an id that no record has. |
| `amendment-mismatch` | warn | `amends` and `amended_by` disagree. |
| `missing-task`, `missing-kind` | error | A record dated on or after `task_from` has no `task`, or no `kind`. |
| `unknown-task` | error | `task` names an id that the board export does not list. |
| `task-link-skipped` | info | The state `skipped`: no `board_export` is configured while records name tasks, or the command failed, timed out or printed no task list. No `task` value was compared with the board. `detail` gives the reason. |
| `task-without-record` | warn | A done task with `research_tag`, or at or below a task of `research_parents`, that no record names. With `task_from` set, a task whose `updated_at` is before that day is left out. The subject is the task id. |
| `missing-summary` | warn | An indexed record has no `summary`, or one shorter than `summary_min_chars`. |
| `unindexed-file` | warn | A Markdown file next to an index's records (`<directory of an include glob>/*.md`) that no index includes or excludes, and that is not an index output, the start page or in `unindexed_exclude`. |
| `no-current-page` | warn | A page group has no page with a status in `current`. The subject is the group's name. |
| `historical-link` | warn | A link that `[lab.archive.historical_link]` covers has no mark on its line. |
| `index-leftover` | warn | `write_indexes = false` and an index's `output` file exists. |
| `stale-allow` | warn | An `[[lab.archive.allow]]` entry matches no finding. The subject is `rule:subject`. |

`duplicate-id` (error) exists in both models and names every path that has the id. An allowed finding stays in the output as an info finding: its message ends with `; accepted by [[lab.archive.allow]]`, and `data` holds the reason. An entry silences only the finding with its rule and subject (and target).

Commands of model 2:

- `lab archive done` is the one gate for "this record is complete". Its steps, in order: check front matter and write the indexes (or name where they print); seal with the manifest, where `seal = "manifest"`; bring the search index up to date with the files (it reads only the files that changed); then run the configured `[[lab.checks]]` (the archive rules alone when none is configured) and report the findings that are not in the baseline in four stages: `links and metadata`, `task named and known`, `sources cited`, `sealed`. It stops at the first stage that has an error, prints `  fix: …` under each finding, and exits 1. Findings of other tools belong to `sources cited`. It exits 0 with a line that starts `done:`.
- `lab archive log [--since YYYY-MM-DD]` prints the indexed records in date order, oldest first, as `date`, `kind`, `id`, `path` and `summary` separated by tabs. A record without a date is left out and counted on stderr. `lab archive log --record <id or path>` prints the commits that changed one record.
- `lab archive index --print <name>` prints one index on stdout, with links relative to the project root, and writes nothing.
- `lab archive next <PREFIX> --reserve` (the default in model 2) gives a number that no other worktree gets. It takes the largest number among the ids in this checkout, the ids in the main checkout and the numbers given out before, adds one, and records it in the main checkout's `<state_dir>/lab/archive-reserved.json` (`{"schema": "rf.archive-reserved/1", "series": {"D": {"max": 12, "width": 3}}}`) under an exclusive lock on `archive-reserved.lock`. `--peek` prints the number and reserves nothing. Both files are local state, not committed.
- `lab archive update` stops before the seal step when the index step fails, prints `Nothing sealed: the index step failed.` and exits 1. This holds in both models since 0.12.0.

### Rule table

Every rule that `lab archive check` and `lab check` report a finding under. Severity is the default. An `[[lab.archive.allow]]` entry or a `[[lab.archive.kept_link]]` entry lowers a finding to info. `src/rules.rs` holds the same table: a test keeps the two equal, and debug builds stop on a finding whose rule is not in it or whose severity differs. A new rule goes into both in the same change. "Model 2" means `[lab.archive] model = 2`.

| Rule | Severity | Enabled by | Since |
| --- | --- | --- | --- |
| `amendment-mismatch` | warn | model 2, records with `amends` or `amended_by` | 0.12.0 |
| `archived-link` | info | `[[lab.archive.offloaded]]` | 0.9.1 |
| `broken-anchor` | error | `links` | 0.2.0 |
| `broken-link` | error | `links` | 0.2.0 |
| `check-exit-mismatch` | warn | `[[lab.checks]]` | 0.1.0 |
| `check-failed` | error | `[[lab.checks]]` | 0.1.0 |
| `check-output-invalid` | error | `[[lab.checks]]` | 0.1.0 |
| `duplicate-id` | error | `[[lab.archive.index]]` | 0.2.0 |
| `front-matter-invalid` | error | `[[lab.archive.index]]` | 0.2.0 |
| `historical-link` | warn | model 2, `[lab.archive.historical_link]` | 0.12.0 |
| `inactive-without-reason` | error | model 2, `reason_statuses` (default `inactive_statuses`) | 0.12.0 |
| `index-leftover` | warn | model 2, `write_indexes = false` | 0.12.0 |
| `index-missing` | error | `[[lab.archive.index]]` with `write_indexes = true` | 0.2.0 |
| `index-stale` | error | `[[lab.archive.index]]` with `write_indexes = true` | 0.2.0 |
| `kept-link` | info | `[[lab.archive.kept_link]]` | 0.9.11 |
| `missing-field` | error | `[[lab.archive.index]]` | 0.2.0 |
| `missing-kind` | error | model 2, `task_from` | 0.12.0 |
| `missing-summary` | warn | model 2, `[[lab.archive.index]]` | 0.12.0 |
| `missing-task` | error | model 2, `task_from` | 0.12.0 |
| `no-current-page` | warn | model 2, `[[lab.archive.page_group]]` | 0.12.0 |
| `open-item` | error | `[lab.archive.open_items]` | 0.13.0 |
| `seal-unverified` | error | model 2, `seal = "git"` | 0.12.0 |
| `sealed-edited` | error | `sealed`, or model 2 with `seal = "git"` | 0.2.0 |
| `sealed-missing` | error | `sealed`, or model 2 with `seal = "git"` | 0.2.0 |
| `similar-record` | warn | `[[lab.archive.index]]`, a new record file in git | 0.14.0 |
| `similar-skipped` | info | `[[lab.archive.index]]`, a new record file in git | 0.14.0 |
| `size-budget` | warn (configurable) | `[[lab.archive.budget]]` | 0.2.0 |
| `stale-allow` | warn | model 2, `[[lab.archive.allow]]` | 0.12.0 |
| `stale-derivative` | warn | a registry (`[papers] registry`) with structure files or paper cards | 0.16.0 |
| `stale-kept-link` | warn | `[[lab.archive.kept_link]]` | 0.9.11 |
| `status-reversed` | error | model 2, `seal = "git"` | 0.12.0 |
| `superseded-but-active` | warn | `[[lab.archive.index]]` | 0.2.0 |
| `superseded-link` | warn | `current_pages` | 0.2.0 |
| `supersession-mismatch` | warn | `[[lab.archive.index]]` | 0.2.0 |
| `task-link-skipped` | info | model 2, records with `task` | 0.12.0 |
| `task-without-record` | warn | model 2, `board_export`, and `research_tag` or `research_parents` | 0.12.0 |
| `unindexed-file` | warn | model 2, `[[lab.archive.index]]` | 0.12.0 |
| `unknown-id` | error | `[[lab.archive.index]]` | 0.2.0 |
| `unknown-task` | error | model 2, `board_export` | 0.12.0 |
| `unsealed` | warn | `sealed` (seal manifest) | 0.2.0 |
| `untracked-link` | warn | `links`, in a git repository | 0.20.0 |

## 7. Check runs, lesson evaluations and loop signals: `rf.labcheck/1`, `rf.evaluation/2`, `rf.signal/1`, `rf.loop/1`

`lab` owns these. [factory.md](factory.md#how-a-lesson-is-evaluated) explains the method.

```toml
[lab.evaluate]                 # all optional; these are the defaults
min_sessions = 10              # session groups, or check-run intervals, needed on each side
alpha = 0.05                   # two-sided; each direction is tested at alpha/2
baseline_days = 90             # baseline: observations this many days before adoption; 0 = all; at most 36500

[lab.loop]
measure = [["agentprof", "scan", "--out", ".research/agentprof", "--timezone", "Asia/Tokyo"]]
notify = ["sh", "scripts/notify.sh"]   # optional
renotify_hours = 24            # at most 87600
```

**`rf.evaluation/2`**: one per active lesson, printed by `lab evaluate --format jsonl` and stored in `last.json`. Version 2 (lab 0.4.0) counts agent sessions in session groups: `unit` `session-group`, and `sessions` on each side. lab 0.4 still reads version 1 records in an older `last.json`; lab 0.3 cannot read version 2. lab 0.5.0 adds `introduced` for check-run metrics, and lab 0.6.0 its `reviewed` and `latest_run`; older readers ignore them. lab 0.8.0 adds the unit `interval`, the method `conditional-binomial`, `measure`, and `findings` and `new_units` on each side; lab 0.7 and older cannot read a `last.json` that has the new unit or method, so update lab on every machine that shares the state directory.

```json
{"schema":"rf.evaluation/2","lesson":"read-by-section","lesson_status":"adopted","metric":"agentprof:file-reread","unit":"session-group","method":"cluster-permutation","verdict":"collecting","reason":"3 of 10 session groups after adoption, 540 of 10 before, in comparable strata","adopted":"2026-11-07","before":{"n":540,"sessions":612,"positives":5,"rate":0.008169934640522876,"mean":null,"median":null},"after":{"n":3,"sessions":4,"positives":0,"rate":0.0,"mean":null,"median":null},"p_lower":null,"p_upper":null,"strata":[{"source":"claude","role":"main","before":540,"after":3}],"excluded":12,"sessions_needed":295,"min_sessions":10,"alpha":0.05,"baseline_days":90}
```

| Field | Meaning |
| --- | --- |
| `metric` | `tool:rule`, or `null` for a lesson without one |
| `unit` | `session-group` (a root session, or the subagents below one root session), `run` (check runs without intervals) or `interval` (between two clean, measured check runs); `null` when nothing measures the metric. Version 1 records say `session` |
| `method` | `stratified-hypergeometric` (a finding rule, every group a single session), `van-elteren` (a `session.` field, every group a single session), `cluster-permutation` (either, when a group has several sessions), `descriptive` (check runs without measured intervals, described by `introduced`, with the verdict `collecting`), `conditional-binomial` (check-run intervals, see "Check-run denominators" below) or `null` |
| `verdict` | `collecting`, `improved`, `worse`, `inconclusive` or `unmeasurable` |
| `reason` | Why the verdict is `collecting` or `unmeasurable`. Otherwise `null`, except that a check-run lesson with a verdict notes a reviewed run that is no longer stored, intervals left out for another unit, (since lab 0.9.0) intervals left out across a checker version change, or (since lab 0.9.6) intervals left out for another checker version than the latest |
| `before`, `after` | `n` observations (session groups or check runs), `sessions` (the agent sessions in those groups; `null` for check runs), and `positives` (sessions with the finding) and `rate` (per session) for a rule, or `mean` and `median` (per session, or per check run); over the contributing strata only, so 0 when no stratum contributes yet. For intervals: `n` intervals, `findings` (introduced in them), `new_units` and `rate` (`findings` per new unit) |
| `p_lower`, `p_upper` | One-sided p-values for a decrease and an increase after adoption; `null` without a test |
| `strata` | Each contributing `(source, role)` with its `before` and `after` session-group counts |
| `excluded` | Session groups in strata without groups on both sides |
| `measure` | Intervals only: the `rf.measure/1` unit they count, such as `citing-sentence`. Several units are joined by `+`, as `citing-sentence+uncited-sentence`: a rule's summed units (since lab 0.9.4), or the different units of several required checks; either way their new units add up. Text output names each in the plural. Left out otherwise |
| `sessions_needed` | For a rule that is collecting or inconclusive: about how many more session groups without the finding would show a decrease |
| `introduced` | Check runs only, when a complete run exists since adoption: `since` (the first such run's `at`, UTC, or, after `lab lesson review`, the reviewed run's), `since_date` (its date in the project's time zone), `reviewed` (the reviewed run's file name when `since` is that run; left out otherwise), `latest` (the latest complete run's `at`), `latest_run` (its file name), `count`, and `findings`, one per message and data with more findings in the latest run than in the `since` run: `message`, `added` (the excess), `subjects` (where the new ones are: the latest run's subjects for that key whose fingerprint the `since` run lacks, or all of them when none does; at most ten) and `more` (subjects left out). Since lab 0.9.3, `version_changed` names each required check whose version changed between the `since` run and the latest run, as `{"<check>": ["<old>", "<new>"]}`, since the checker update alone can add or remove findings. As for intervals, each complete live run takes the last known version up to it; a check with none known at the `since` run takes the first one known after it, as in runs stored before lab 0.9.0. A check known on one side only is no change, and the field is left out when nothing changed. It does not change `count` and does not say which findings the update caused: `lab check --at` on the `since` run's commit shows what today's checker finds there. The `lab evaluate` line, `lab brief`, and the body and detail of the `lesson-introduced` and `lesson-verifiable` signals then name the change. `lab lesson review` records all listed findings at once, so it fits only when every remaining one comes from the update or cannot be fixed. Left out otherwise |

**`rf.signal/1`**: something to act on, sent to the notify command.

```json
{"schema":"rf.signal/1","project":"my-thesis","id":"lesson-verifiable:read-by-section","kind":"lesson-verifiable","lesson":"read-by-section","title":"Verify lesson read-by-section: its metric improved","body":"**Lesson `read-by-section`: …**\n\n…\n```sh\nlab lesson set read-by-section verified --note \"…\"\n```\n","detail":"improved","repeat":false,"reopened":false,"ref":null,"first_seen":"2026-11-20T03:00:00Z","at":"2026-11-20T03:00:00Z"}
```

| `kind` | `id` | When |
| --- | --- | --- |
| `lesson-verifiable` | `lesson-verifiable:<lesson>` | An adopted lesson's verdict is `improved`: verify it |
| `lesson-worse` | `lesson-worse:<lesson>` | An adopted or verified lesson's verdict is `worse`: revise the rule or retire it |
| `lesson-introduced` | `lesson-introduced:<lesson>` | An adopted or verified check-run lesson has findings introduced (unless another signal above is sent for it; `lesson-verifiable` then lists them) since adoption, or since its last review; the body lists them. Its detail is the run they are counted from, the messages and their counts, so a change, including a review, sends it again (once `renotify_hours` after the last send) |
| `lesson-unmeasurable` | `lesson-unmeasurable:<lesson>` | An adopted lesson's verdict is `unmeasurable` (a verified one stays verified on its other evidence): choose a metric that can show an effect, or decide on other evidence with `--force` and a note |
| `uncovered-rules` | `uncovered-rules` | `lab retro` finds recurring rules without a lesson; one signal lists them all |
| `measure-failed` | `measure-failed:<index>` | A `measure` command failed; the body has its command, status and last output lines |

`title` is imperative and at most 72 characters; `body` is standalone Markdown with the numbers, the method and the `lab` commands to act. `id` is stable, and `detail` is a short summary whose change makes the signal worth sending again. `repeat` marks a changed signal sent again and `reopened` one that had been resolved; both carry the `ref` the notify command returned for the first send. A notify command should add to that ref while it is still open and file anew when it is closed or gone. `first_seen` is when `lab loop` first produced the id.

**Notify protocol.** For each signal to send, `lab loop` runs the `notify` program once, from the project root, with the signal as one JSON object on stdin, and waits up to 60 seconds. Exit 0 means sent: the first non-empty line of stdout (trimmed, at most 200 characters), if any, becomes the signal's `ref`, such as a task id, and is passed back in later sends. Any other exit, or a timeout, leaves the signal unsent; the next loop retries it. A program with a directory part resolves against the project root, as for checks.

**Dedupe.** `<state_dir>/lab/loop/signals.json` (`rf.loop-signals/1`) keeps, per id, `first_seen`, `last_notified`, `detail_hash`, `ref`, `resolved`, `error` and `pending`. A signal is sent when its id is new, when it was resolved and appears again at least `renotify_hours` after its last send (`reopened`; sooner it is flapping and stays quiet), when its detail changed and it was last sent at least `renotify_hours` ago (`repeat`), and on every loop until a send succeeds. An id missing from a loop is resolved. A notice still unsent when its signal resolves is dropped, since the condition is gone; the loop record lists it as `unsent`. Since lab 0.17.0, a signal that was never sent and appears again is sent as new, not `reopened`. Without a `notify` command the state is still kept, and the signals stay `pending`.

**`rf.loop/1`**: `<state_dir>/lab/loop/last.json`, written atomically at the end of each loop, also after a `lab` error (in `errors`).

```json
{"schema":"rf.loop/1","started":"2026-11-20T03:00:00Z","finished":"2026-11-20T03:00:41Z","measure":[{"argv":["agentprof","scan","--out",".research/agentprof"],"ok":true,"status":"exit 0"}],"evaluations":[],"signals":[{"id":"uncovered-rules","kind":"uncovered-rules","status":"unchanged","ref":"research-factory-243"}],"resolved":[{"id":"lesson-worse:brief-subagents","ref":"research-factory-39"}],"errors":[]}
```

A measure's `status` is `exit N`, `killed by a signal`, `timed out after 600 s` or `could not start: …`; a signal's is `notified`, `pending` (no notify command), `failed` (retried next loop) or `unchanged`. Since lab 0.17.0, a signal carries its `ref` when it has one, and `resolved` lists the ids this loop resolved, each with its `ref` and with `unsent: true` when its notice was dropped. `lab brief` names the task that tracks each signal and the tasks of resolved signals, which may be closed. Both fields are left out when empty; older readers ignore them. When `last.json` cannot be read, `lab loop` runs even under `--if-older`, since the record cannot show a recent loop. The new record's `errors` starts with `the previous loop record was unreadable (…)`, and the brief reports that note apart from loop failures. The same directory holds `lock` (an exclusive OS file lock held while a loop runs and released when its process exits, so a loop that died never blocks the next; the file names the holder's pid) and `loop.log` (the measure commands' output and the background loop's own; moved to `loop.log.1` past 1 MiB). A measure or notify command that times out is killed with its whole process group. An error after the measure commands, such as an unreadable signal state, is recorded in `last.json`, so `--if-older` backs off and the brief shows it. Since lab 0.9.8, `lab loop` started in a linked git worktree (its `git rev-parse --git-dir` differs from `--git-common-dir`) runs for the same project in the main checkout instead: that checkout's `research.toml`, root, measure and notify commands, lock, `--if-older` record and signal state. It says so in its output and in `loop.log`. When the main checkout has no such `research.toml`, or the repository is bare, it does nothing, says why and exits 0. Since lab 0.9.9, `lab brief` in a linked git worktree reads the main checkout's `last.json` the same way and says whose results they are, so a worktree's own `last.json`, left by an older lab, is ignored; when the main checkout has no such project, or the repository is bare, its lesson-measurements section says why there is nothing to show. `lab loop` exits 0, 1 when a measure or notify command failed, and 2 on a `lab` error.

**`rf.labcheck/1`**: one file per `lab check` run in `<state_dir>/lab/checks/`, named after the time it was stored (`<YYYYMMDDTHHMMSS.mmmZ>.jsonl`, with `-1`, `-2` on a clash). The first line is the run header; each further line is an `rf.labcheck.item/1` record with the `check`, the finding's `fingerprint` and the `finding` itself, or, since lab 0.8.0, an `rf.labcheck.measure/1` record (see "Check-run denominators" below). Since lab 0.7.0, readers skip a further line whose `schema` is another one, so a later version can add record kinds; older versions refuse such a run.

```json
{"schema":"rf.labcheck/1","at":"2026-11-20T03:00:00Z","commit":"04d97b1c…","dirty":false,"checks":[{"name":"claims","command":["claimcheck","check","--format","jsonl"],"exit":1,"status":"ok","findings":12,"unparsed":0,"duration_ms":5321,"version":"claimcheck 0.2.0"}]}
```

| Field | Meaning |
| --- | --- |
| `at` | UTC start time of the run |
| `commit` | `git rev-parse HEAD` of the repository holding the project root when the run started (since lab 0.7.0); `null` outside git or before the first commit. Runs stored by older versions have neither field and load as `null` |
| `dirty` | Whether files under the project root differed from `commit` when the run started (since lab 0.7.0): tracked changes, or untracked files that git does not ignore, whatever `status.showUntrackedFiles` says, leaving out the state directory. A project root that the repository ignores is always dirty, since no commit holds its files. Checks read the work tree, so a dirty run's findings belong to `commit` plus uncommitted text; `null` when `commit` is. lab reads both with `git --no-optional-locks status`, so a background run never holds the index lock; a git failure other than "not a git repository" is printed and leaves both `null` |
| `replayed` | `true` for a run of a past commit by `lab check --at` (since lab 0.9.0; see "Check-run denominators" item 6); left out otherwise. Its `commit` is that commit, `dirty` is `false` and `at` is the commit's committer time, while the file name stays the time it was stored |
| `checks` | Each check that ran: `name`, `command`, `exit` (`null` when it could not start or was killed), `status` (`ok` for exit 0 or 1, else `failed`; an exit that disagrees with the findings, 1 without an error finding or, since lab 0.17.0, 0 with one, adds a `check-exit-mismatch` warning), `findings`, `unparsed` (stdout lines that were neither findings nor valid `rf.measure/1` records), `duration_ms`, `version` and, in a replayed run, `uncommitted_inputs` |
| `version` | Since lab 0.9.0: the checker's version, the first non-empty stdout line (at most 200 characters, not a JSON object) of the check's `version` command, run where the check runs (the project root, or a replayed commit's copy of it), with its program resolved as the check's, and a 10-second limit. Without `version` in `[[lab.checks]]`, the command is `<program> --version` when the program is a bare name and not an interpreter or wrapper (any name starting with `python`, and `sh`, `bash`, `zsh`, `dash`, `ksh`, `fish`, `env`, `uv`, `uvx`, `pipx`, `poetry`, `pdm`, `hatch`, `conda`, `node`, `npm`, `npx`, `pnpm`, `yarn`, `bun`, `bunx`, `deno`, `perl`, `ruby`, `bundle`, `cargo`, `go`, `java`, `make`, `just`, `timeout`, `nice`, `nohup`, `time`, `xargs`, `docker`, `podman`); `version = []` means none. `null` when unknown: no command, or one that failed, timed out, kept its output open after it exited or printed no version line (lab then prints a warning). Runs stored by older versions have no field and load as `null`. lab 0.8.0 and older refuse a `[[lab.checks]]` entry with `version` or `uncommitted_inputs` |
| `uncommitted_inputs` | A replayed run's paths, relative to the project root, that the check read from the current work tree because the commit lacked them; left out when empty |

**Check-run denominators: `rf.measure/1`.** Since lab 0.8.0, a check-run lesson can get a verdict. A count of introduced findings says nothing without the amount of new text it came from: ten introduced findings in one new sentence and in a thousand are different results. So a checker can report what it checked, and `lab evaluate` compares findings introduced per new unit of text.

1. *The denominator record.* A checker that supports a `--measure` flag prints one `rf.measure/1` line per unit it checks beside its findings. Add the flag to the check's `run` in `research.toml`; checkers without it keep their current output.

   ```json
   {"schema":"rf.measure/1","tool":"claimcheck","unit":"citing-sentence","count":412,"keys":["5f1c09a2e4b7d013", "…"]}
   ```

   `unit` names what is counted (for claimcheck, `citing-sentence`, sentences that cite a reference, and since claimcheck 0.2.0 `uncited-sentence`, sentences that do not; since claimcheck 0.4.0, `uncited-sentence` counts only uncited prose sentences, in paragraphs and list items, outside blockquotes, so uncited table rows and blockquote sentences are in no unit, and no sentence is in both; since claimcheck 0.6.0, such sentences in a file that one registry entry names as `source.path` cite it (section 3) and count as `citing-sentence`, not `uncited-sentence`, which moves units between the two; lab 0.9.6 and later handle that as a checker version change), and `count` how many the run saw. `keys`, optional, are the sorted, distinct first 16 lowercase hex digits of the SHA-256 of each unit's normalized text; there are at most `count` of them. They let lab count new units exactly, as it keys introduced findings by message and data rather than by path. `lab check` prints what each check measured and stores each record as an `rf.labcheck.measure/1` line, `{"schema":"rf.labcheck.measure/1","check":"claims","measure":{…}}`, after the findings. It does not count these lines as `unparsed`. A check that did not run to completion stores no measure, and a run that is not clean (`dirty` not `false`) stores no keys, since it can never end an interval. A line whose schema starts with `rf.measure/` but is not a valid version 1 record (no tool or unit, a malformed, unsorted or repeated key, or more keys than `count`) counts as `unparsed` and fails the check. lab 0.7.0 skips stored measure lines; older versions refuse such a run.
2. *New units.* Between two runs, a check's new units are the keys in the later run that the earlier one lacks. Without keys on both sides, they are the count's increase, at least 0. That misses text that replaced deleted text, while the findings in it still count, so revising text raises the rate; checkers should print keys. A run's new units are the sum over the required checks (those that report the metric's tool).
3. *Intervals.* A run is an interval end when it is complete, has `dirty` `false` and every required check measured the metric's tool in it. Only the last such run on each `commit`, in the order runs were stored, counts, so a commit is measured once and uncommitted text never is; a replay of a commit replaces its earlier runs. Ordered by `at`, consecutive ends form an interval with `introduced` (counted as for `introduced` above, between the interval's two runs) and `new_units`. An interval belongs to the side of adoption of its later run's date, within `baseline_days` before adoption. Intervals with no new units are left out, with the findings introduced in them. So are intervals across a change of a check's unit, and intervals in another unit than the latest interval's, so a renamed unit gets a new baseline instead of mixing rates. Since lab 0.9.0, so are intervals across a change of a required check's version, since a checker change moves the counts by itself; the reason counts them (`N interval(s) across a checker version change left out`). Since lab 0.9.6, only intervals in the current checker versions count, since an update can change what a unit counts under the same name. The current versions are those known at the interval end stored last, not the latest in time, so a replay with an updated checker, dated in the past, sets them. An interval counts when, for each required check, its version is the current one or either is unknown; an interval's version is the one known at its later end, which its earlier end shares or, knowing none, takes. Since lab 0.9.7, both ends must match, and a run without a recorded version, as one stored before lab 0.9.0, takes for this comparison the first version recorded by any run stored after it, dirty or not, since its checker was no newer; stored order, not time, decides, since a replay is stored after the live runs it predates. A version stays unknown only when no run stored later records one, as when the run stored last records none. lab 0.9.6 let an unknown version match any, so such runs pooled with the current checker. The `lab evaluate` line still shows the introduced findings and any version change when no interval counts. The reason counts the others per side (`N interval(s) (B before adoption, A after) measured with another checker version than the current one (<check> <version>, …) left out, which a replay of their commits measures again`). Replaying every measured commit with the current checker (`lab check --at <first>^..HEAD`) gives both sides back. Each interval end takes, per check, the last known version up to it, and an interval crosses a change when a check has a known version at both ends and they differ. So a `null` version, as in runs stored before lab 0.9.0, still forms intervals, and an unknown version between two known ones does not hide their change. A check's records in one unit add their counts and keys. Since lab 0.9.2, each check-run lesson counts one unit per check, chosen by its metric, never by record order. A rule with its own units counts only those, summed (several since lab 0.9.4): their counts add, their keys join, and they are named joined by `+`. Since keys join, a sentence that moves from one unit to the other is not new, and a text in both units of one run counts once: claimcheck keys a text without its citation markers, so one text cited in one place and uncited in another has one key, as a repeated sentence has within one unit. `claimcheck:uncited-claim` counts `uncited-sentence`, since it fires only on prose sentences that do not cite. `claimcheck:evidence-label` counts `citing-sentence+uncited-sentence`, since it fires on prose paragraphs that cite or claim; lab 0.9.2 and 0.9.3 counted `uncited-sentence` alone for it. The sum leaves out uncited table rows and blockquote sentences, which claimcheck 0.4.0 and later count in no unit, and includes cited table rows, which `citing-sentence` counts though `evidence-label` never fires on a table. `evidence-label` does fire on uncited blockquote paragraphs, so those findings have no unit: an interval that adds only them has no new units and is left out with its findings. A run whose check did not print each of the rule's units, such as one from claimcheck before 0.2.0, ends no interval for that rule. Any other rule counts the one unit its check printed, or, when the check printed several, its tool's unit (`citing-sentence` for claimcheck); a check that printed several units of a tool without one ends no interval. An interval across a change of a check's unit is left out, and the reason counts it (`N interval(s) across a change of unit left out`). So when claimcheck starts printing `uncited-sentence`, an `uncited-claim` lesson gets a new baseline, which `lab check --at` can replay. lab 0.9.1 and older end no interval at a run whose check printed several units, so with claimcheck 0.2.0 they give no check-run verdict. Each interval is one observation counted against `min_sessions`.
4. *The comparison.* The rate on each side is Σ introduced / Σ new units over its intervals. Given the total introduced `N = a + b`, adoption changing nothing makes the after count `b` binomial with `N` trials and probability `U_after / (U_before + U_after)`, from the new units on each side. `p_lower` is `P(X ≤ b)` and `p_upper` is `P(X ≥ b)`. The verdict is `collecting` until both sides have `min_sessions` intervals, `improved` when `p_lower < alpha/2`, `worse` when `p_upper < alpha/2`, and otherwise `inconclusive`. The evaluation then has `unit` `interval`, `method` `conditional-binomial`, `measure` (the unit's name), and on each side `n` intervals, `findings` (Σ introduced), `new_units` and `rate`. It keeps `introduced` and its `lesson-introduced` signal with any verdict.
5. *Without intervals.* With no interval yet, the lesson stays `collecting` with method `descriptive`, and the reason says what a verdict needs. With fewer than `min_sessions` intervals before adoption, it stays `collecting`, and the reason starts `no baseline:`. It is not `unmeasurable` as for sessions: replaying past commits (below) can still supply the baseline. The reason then advises `lab check --at`, unless (since lab 0.9.5) a replay cannot end a before-interval. A replayed run is dated at its commit's committer time, and a before-interval ends at a run in the before-window (from adoption minus `baseline_days`, or any time when it is 0, up to the day before adoption, in the project's time zone). So when no commit that changed the project, reachable from any ref or reflog entry, has its date in the window, and no stored interval end is in it, the reason says that no commit predates adoption in that window, so no replay can supply one, and adds when earlier commits exist that a larger `baseline_days` would include. lab reads the commit dates only for such a lesson. When git history cannot be read (git missing, no repository, git failing, or a shallow clone, which lab notes on stderr), the advice stays and `lab evaluate` does not fail.
6. *Replaying past commits.* `lab check --at <commit>`, repeatable and accepting a range `A..B` (the commits `git rev-list --reverse A..B -- .` lists from the project root: those that changed the project, without `A` itself, so `A^..B` includes it), rebuilds the baseline of lessons adopted before their checker printed a denominator, such as masters' fetch-cited-texts and link-own-measurements. For each commit, lab checks it out into a temporary git worktree (`git worktree add --detach` in the system temporary directory, with hooks off), runs the selected checks and their `version` commands in the commit's copy of the project root, with only the program (`run[0]`) resolved against the current project root, and stores the run with that `commit`, `dirty` `false`, `at` the commit's committer time and `"replayed": true`; the file name stays the store time. It removes the worktree and its registration when the commit is done, also when a step fails or panics, and before replaying removes the worktrees that a `lab` process stopped by a signal left (named `lab-replay-<pid>-…`, with that process gone); other worktrees are never pruned. A commit without the project root is skipped with a note, and a replay in which no commit has it is an error. Each run is printed as it is stored. A replay with `--only` warns that a run counts only when every configured check reporting the metric's tool ran in it. Since `lab evaluate` orders interval ends by `at`, replayed runs slot in before adoption. They serve only as interval ends: `introduced`, the per-run means, `lab brief`, `lab retro` and `lab baseline set` leave them out, since they check past text again. A check whose inputs no commit holds, such as git-ignored source texts, cannot be replayed faithfully. Listed as `uncommitted_inputs` in `[[lab.checks]]`, those that the commit lacks are linked from the current work tree, as are the entries of a listed directory that the commit holds only part of, such as its `.gitignore`, and the stored check names the inputs it linked in `uncommitted_inputs`; unlisted, they are missing and the check sees no such files. The links point into the work tree, so a check that writes to such an input changes the work tree. lab 0.8.0 and older ignore `replayed` and read such a run as a live one, so update lab on every machine that shares the run directory before replaying.
7. *Limits.* Intervals are treated as independent, and findings as independent within them. Interval ends are ordered by time, not by commit ancestry: when runs from several branches share the run directory, an interval from a branch tip to another branch's commit counts the other branch's text and findings again. When one interval holds most findings, a permutation test over whole intervals, like the session-group test, should replace the binomial one. Text written between adoption and the first run after it falls in an interval that straddles adoption; its later run assigns it to the after side. A checker or checker-version change between two runs moves counts by itself; intervals across a recorded version change are left out, but a checker without a known version, or a change in its configuration or in `research.toml`, goes unseen. Since only the current checker versions count, every checker update starts both sides again, also one that changed no unit, until `lab check --at` replays the measured commits; machines that share a run directory with different checker versions make the counted intervals follow whichever stored a run last; lab 0.9.5 and older pooled the intervals on either side of an update, so one that changed what a unit counts under the same name, as claimcheck 0.4.0 did for `uncited-sentence`, mixed two measures. Introduced findings count per message and data, so for a rule whose findings share both, such as claimcheck's `evidence-label` finding on a paragraph without a label, an interval counts the net change: a paragraph fixed in it hides a new one. Replayed runs use today's checkers on past text, so an interval between the last replayed run and the first live run usually crosses a version change and is left out. A replay that read uncommitted inputs from today's work tree may count findings that the commit's text did not have, or miss some.

## 8. Conventions every repository follows

- Rust edition 2024, `rust-version = "1.99"`, stable toolchain. MIT OR Apache-2.0 licence files.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass, and a GitHub Actions workflow runs them.
- Network access happens only in explicit commands, goes through a client that spaces requests per host and retries 429 and 5xx with backoff, and identifies itself with a generic user agent (`<tool>/<version>`). No email address or personal data in requests.
- Tests use synthetic fixtures. Nothing from a private project, no personal data and no absolute home-directory paths enter a repository.
- Errors name the file and line or the record they concern. No silent fallbacks: a value a tool cannot establish is reported as unknown.
