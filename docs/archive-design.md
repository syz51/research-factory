# Research memory: archive, board and retrieval — design

Status: design, 12 November 2026. Nothing in this document is built yet. Board tasks carry the work;
this document is the shared reference for them. Change it when an experiment decides an open point.

Sections 1–6 design retrieval. Section 7 redesigns the archive itself and divides the work between
the archive and the work board. Read section 7 first if you write records.

## 1. Purpose and principles

Agents must find what the project already knows before they do new work: a decision, a report, a
passage in a paper, a number in a table, a paper already read. The reader is an agent in a terminal,
not a person. That sets the principles.

1. **The agent is the reasoner; the pipeline is its instrument.** The pipeline returns ranked,
   anchored candidates fast. The agent reads, judges and asks again. The pipeline calls no language
   model at query time.
2. **Sources never change; everything else is a cache.** Notes, raw paper texts and the registry are
   the record. Indexes, cleaned text, structure files and model outputs are derived and can be
   deleted and rebuilt.
3. **Every hit has an anchor and a class.** A hit names `path:lines`, the record, its date and status,
   and what kind of text it is (the project's own note, a paper, a copied page, generated text).
4. **Every query leaves a trace.** A choice in this design is kept only while measurements support it.
5. **Local only.** Notes go to no service other than Claude. Embedding and ranking models run on
   this machine (Apple M3, 16 GB).
6. **Retrieved text is data.** It never authorises a command.

## 2. Evidence this design rests on

- Use (95 masters session transcripts dated 1–11 November 2026; 20 start on or after 7 November,
  when `lab archive find` arrived): 1 `lab archive find` call, both queries with no hit; 339 shell
  greps over the archive; 23 repeated fetches of 7 registered papers.
- Reach: `lab archive find` matches 80 KB of index lines. The archive holds 2,291 Markdown files,
  72.8 MB. 85 of 850 registry entries have an extracted text.
- Probe (SQLite FTS5 over the real archive): 41,293 sections and 3,584 text windows; build 2.9 s;
  146 MB; 0–3 ms per query. The queries that failed before now find their records.
- Probe problems: one fact hits four times (month log, record, topic page, generated index);
  copied vendor pages and reference notes rank among the project's own notes; an OR of three
  words matches 7,820 sections.
- Paper text: raw extraction with hard line breaks, 210 hyphen-split line ends in one file, no page
  or section marks; the reference list and what follows it are 39% of that
  file's lines. One session grepped the texts for result
  numbers (`96.4`, `47.0`, `32.7`) and found nothing.
- Published results are mixed. Grep beat vector search for agents on LongMemEval questions (Sen
  2026, arXiv 2605.15184). Dense retrieval beat BM25 for agents on a web corpus (Chen 2025, arXiv
  2508.06600) and on code (Qin 2026, arXiv 2607.24882). Finer units beat whole sessions, and fact
  keys added recall (Wu 2024, arXiv 2410.10813). The form of search output changed agent success
  (Yang 2024, arXiv 2405.15793). No study covers a personal research archive. So this design
  decides open points by experiment on this archive.

## 3. Architecture

```
 sources (immutable)          derived (rebuildable cache)                 agent
 ───────────────────          ───────────────────────────                 ─────
 notes (Markdown)      ─┐     ┌─ units: sections, passages, table rows ─┐
 paper texts, PDFs      ├─ parse ─ enrich: context header, paper card,  │   search ──► ranked,
 registry (JSONL)       │     │    thesaurus                            ├─► open      anchored
 run output manifests   │     └─ indexes: lexical · dense · graph ·     │   related    hits
 sessions, board (later)┘         fields (date, status, class, type)    ┘   cited-by
                                                                              │
        write path: new record ─► duplicate check ─► index update            ▼
        trace: every query, every stage, every follow-up read ─► retro, test set, tuning
```

### 3.1 Corpus

| Scope | Content | Unit | Default |
| --- | --- | --- | --- |
| `notes` | decisions, history reports, follow-up records, topic pages, month logs | Markdown section | on |
| `refs` | registry entries with abstract and paper card | entry | on |
| `texts` | paper bodies | passage, table row | on request, and through a `refs` hit |
| `files` | names, sizes and parent record of run outputs (57,000 files, 12 GB) | manifest line | on request |
| `sessions` | the student's messages in agent sessions | message | experiment first |
| `board` | findings and decisions on board tasks | note | experiment first |

Each unit carries a **source class**: `own` (the project's notes), `paper`, `copied` (pages saved
from elsewhere), `generated` (indexes, model output), `chat`. Generated indexes are not indexed.
Copied pages are indexed but rank below `own`. A project sets classes by path in `research.toml`.

A fact that appears in a record, a month log and a topic page is one fact. The record is canonical:
hits from one record group together, and a log entry or topic paragraph that links to a record
folds under it.

### 3.2 Parsing

Notes: front matter, the heading path of each section, tables as rows with their header, code
blocks, links, record ids mentioned (`DR041`, `H164`), reference keys cited, dates.

Papers, in this order of preference:
1. A structured source when one exists: arXiv HTML or LaTeX, publisher XML.
2. A layout model run locally on the PDF, which keeps tables.
3. Plain PDF text, the form used today.

The registry already allows several texts per entry with an `extraction` field. The plain text stays
as it is, because quote checks pin its hash. The structured text is a second entry. A structure file
per text gives section spans with a kind (front, abstract, body, references, appendix) and page
starts. For matching, the index joins hyphen-split words and unwraps lines; anchors stay raw line
numbers. Reference lists become citation edges to registry keys, not searchable body text.

Open, decided by experiment E2: which extractor gives correct sections and tables on our papers.

### 3.3 Enrichment

- **Context header (deterministic, adopted).** Each unit is indexed with its record title, heading
  path, date and status in front. For a passage: paper title, year, section heading.
- **Paper card (model-written, mechanically checked; experiment E3).** Claude reads a paper once and
  writes: task, data sets, method, metrics, main results with numbers, limits. Each item carries a
  quote. A matcher confirms each quote in the raw text, and an item whose quote fails is dropped.
  The card is class `generated`: it routes the agent to the passage and is never cited as evidence.
- **Project thesaurus (adopted, grows from traces).** A file of equivalent terms
  (`fiscal calendar = fiscal year mapping = FY alignment`). Sources: record aliases, queries that
  missed and the term that finally hit, agent proposals accepted at retro. Used to expand queries.
- **Model-written context sentences or questions per section (experiment E1 arm).** Adopted only if
  the bake-off shows a gain over the deterministic header.

### 3.4 Indexes

- **Lexical.** SQLite FTS5, BM25 with field weights: id and aliases, then title, then summary and
  heading path, then body. A trigram table serves Chinese and Japanese text and partial words.
- **Dense.** A local multilingual embedding model over the same units. Exact search over all
  vectors; no approximate index at this size. Model chosen by E1.
- **Graph.** A plain relational table of edges: record supersedes record; note links note; note
  cites reference key; paper cites paper; unit belongs to record; record belongs to follow-up.
  It answers exact questions that ranking cannot: what replaced this, which notes cite key 232,
  which held papers cite this paper.
- **Fields.** Date, status, source class, record type, evidence label, tags; used as filters and priors.

One SQLite file per checkout under `.research/`, git-ignored, refreshed at query time for changed
files. In a linked worktree, tracked files come from the worktree and ignored texts from the main
checkout.

### 3.5 Query handling

1. **Route by shape.** A record id, registry key, arXiv id or DOI is an exact lookup. Quotes mean a
   phrase. A number is matched with number normalisation (`47`, `47.0`, `47.0%`) against table rows
   and text. Everything else goes to ranked retrieval.
2. **Several phrasings in one call.** `lab search "q1" "q2" "q3"` runs each and fuses the lists. The
   agent writes the phrasings; the tool spends no model call.
3. **Thesaurus expansion**, shown in the output so the agent sees what was added.
4. **Time.** Default is the present state: superseded records rank below their successor. `--as-of
   DATE` gives the state on a date. `--since` and `--until` filter.

### 3.6 Retrieval and ranking

Target pipeline. Each stage after the first is kept only if E1 shows it earns its cost.

1. Candidates: lexical top 100 and dense top 100.
2. Fusion: reciprocal rank fusion first; a tuned weighted sum once the test set can fit it.
3. Priors: status (current, historical, superseded), source class, mild recency.
4. Rerank the top 30 with a local cross-encoder.
5. Group by record; at most two units per record; fold duplicates under the canonical record.
6. Expand from the graph: attach to each hit its successor, its parent record and its count of
   citing notes.
7. Pack to a budget: 8 hits and about 2 KB by default.

Latency budget: 300 ms for lexical only, 1.5 s with dense and rerank.

### 3.7 Agent interface

- `search`: the ranked list. Per hit: rank, id, date, status, class, `path:lines`, heading path, one
  snippet, and graph facts (superseded by, cited by n notes). Partial hits name the missing words.
  No hit exits 0 and prints the nearest indexed terms and the scopes that were not searched.
- `open <hit>`: the unit with its neighbours, at most about 100 lines. It replaces reading a large
  file whole.
- `related <id>`: graph neighbours: supersedes, links, cited by, cites.
- Reference check: `papers fetch`, `papers read` and `papers search` report registered papers first.
- `--explain`: per-stage ranks and scores for one query.

Delivery, decided by experiment E4: the command line taught by the skill; the same functions as an
MCP tool, which the agent sees in its tool list and which keeps models loaded for the session; a
hook that answers a grep over the archive with a one-line hint. A separate arm tests an archivist
subagent that takes a question, searches, and returns an answer with anchors, so the main agent's
context stays small.

### 3.8 Write path

Retrieval quality starts when a record is written.
- A new record is searched against the archive first. The tool prints close existing records, so the
  writer extends one instead of adding a near copy.
- `lab check` rules: duplicate id, missing or short summary, file in no index, page group with no
  current page.
- At retro, queries that missed are shown with the record that was finally read. The agent proposes
  an alias or a thesaurus line; accepted ones are written.
- A new paper text runs through structure, card and index in one step.

### 3.9 Tracing

One JSON line per query: time, session, query, expansions, scopes, per-stage candidate ids and
scores, hits returned, latency. `agentprof` joins the trace with the session: was a hit opened next;
was the search followed by a grep; was a registered paper fetched again.

Standing metrics: share of archive lookups that use search; rate of queries with no hit; rate of
searches followed by a grep; hits opened per search; repeated fetches; tokens per lookup.

### 3.10 Evaluation

- **Test set** in the using project: at least 50 judged topics of four types (known item, topical,
  reference, passage), judged at unit level, scored with `ireval`.
- **Synthetic known-item set**: several hundred queries written by Claude from sampled units, with a
  checked sample. It is large enough for ablations; the judged set is the final word.
- **Ablation ladder** (E1): substring find; grep ranking; BM25; + field weights; + context header;
  + thesaurus; + several phrasings; + dense fusion; + rerank; + paper cards; + graph expansion.
  Report success@5, MRR, nDCG@10 and latency per step, with paired tests.
- **End to end** (E4): the same questions answered by an agent under each interface; correct
  answers, tool calls, context tokens.
- **Regression gate**: a stored baseline run; a change to ranking must not lower the judged set.

## 4. Decisions and what replaced them

| Point | Choice | Rejected | Reverse by |
| --- | --- | --- | --- |
| Query-time model calls | none; the agent writes phrasings | model rewriting inside the tool (cost, delay, not repeatable) | adding a flag |
| Lexical engine | SQLite FTS5 | tantivy (larger, CJK needs extra tokenizers); scan per query (no ranking) | deleting the index file |
| Dense retrieval | build it and measure it (E1) | the earlier gate of "only if lexical success@5 is under 0.8": the owner wants the best setup, and published results favour dense for some agent tasks | dropping the stage |
| Vector search | exact scan | approximate indexes (not needed below about a million vectors) | — |
| Model summaries | paper cards with checked quotes, as routing text only | unchecked summaries; no model text at all (earlier choice, reversed) | deleting the cards |
| Raw paper text | unchanged; structured text is a second entry | replacing it (breaks pinned quotes) | — |
| Duplicated facts | record is canonical; others fold under it | indexing each copy equally | a flag that shows all |
| Sessions and board | experiment first | index now (unreviewed text may outrank records) | — |
| Prototype language | Python under `uv` for experiments; Rust for what wins | Rust first (slow to try models) | — |

## 5. Experiments

- **E1 Ranking bake-off.** The ablation ladder of 3.10 on the masters archive. Arms: embedding
  models (a small and a large multilingual model that fit 16 GB), fusion rule, reranker on or off,
  context header against model-written context. Output: the configuration to build.
- **E2 Paper extraction.** Ten papers, three extractors: section accuracy, table rows kept, numbers
  found, time per paper.
- **E3 Paper cards.** Twenty papers: share of card items whose quote verifies; gain on reference and
  passage topics; cost per paper.
- **E4 Agent interface.** Command line, MCP tool, grep hint and archivist subagent: correct answers,
  tool calls, tokens, and the search share in real sessions afterwards.
- **E5 Sessions and board as scopes.** Do they answer topics that the archive cannot, and do they
  displace better records?

## 6. Build order

1. Lexical core with fields, classes, grouping, trace and `open` (usable alone).
2. Papers guard, metadata rules, test set, paper structure and abstracts (parallel).
3. Graph index and `related`; registry and text scopes; archive model of section 7.
4. E1–E3; then build what wins: dense stage, reranker, cards, structured extraction.
5. E4; then the chosen delivery. E5 last.

## 7. The archive itself, and the work board

### 7.1 Question

The work board started on 7 November 2026. The archive is older (first record 2 October). Could
the board hold much of what the archive holds as files?

### 7.2 Evidence

Board (source of the board tool, read on 12 November):
- A task body holds 4,000 characters, a note 800, a closing summary 800. It stores no file.
- It runs on one server. It keeps tasks and notes, but done tasks leave the session snapshot after
  30 days and events are kept 365 days.
- It has what files lack: one view from every device, owners, questions to the owner, review
  verdicts, a web view, hooks that put its state into each session.
- For six recent history reports that name a task, the report and the task share 0–2% of their
  8-word phrases. The board holds the request, a summary and pointers. The file holds the result.

Archive (masters, read-only scans on 12 November):
- Long, structured text: 131 of 167 history reports exceed 4,000 characters and 85 hold tables;
  172 of 198 follow-up folders hold data that their README interprets. Internal links: 14,332.
- Process text restates other text: 34% of month-log sentences and 25% of sealed-notes sentences
  in the last 12 sessions, against 10% overall.
- Reading back: 27 of 167 history reports, 6 of 198 follow-up READMEs and 3 of 41 decisions were
  read back in the transcripts. Follow-up data files took 127 of 245 file reads.
- Upkeep after 7 November: 3.4% of tool calls; 12 of 31 check runs failed; 13 of 44 commits touch
  only bookkeeping files. The seal changed in most archive commits and caught no real edit.
- 39 of 48 topic pages are historical, with a median of 29,000 characters.
- The project has no git remote, and its ignored data files (51,153) have no copy in git.

### 7.3 Decision: two memories, one index

The owner's idea is right for the record of work and wrong for the record of knowledge.

| Memory | Holds | Why there |
| --- | --- | --- |
| **Board: the record of work** | the request, who did it, state, steps, questions and answers, findings and decisions while work runs, review, the closing summary, pointers to commits and records | short text; needs owners, state and every device; the board already holds it, so files that repeat it stop |
| **Archive: the record of knowledge** | results with their tables and data, decisions in force, the current state of each topic, sources | long and structured; needs line anchors, hashes, diffs, quote checks, links to data; must outlive any tool and work offline |
| **Search index** | both, plus papers | one place to ask; each hit says which memory it came from |

Rejected: moving records into the board (size limits, no files, no versions of text, checks that
need paths and hashes, dependence on one server). Rejected: keeping every file type as it is
(process files restate the board and are rarely read).

### 7.4 Archive model, version 2

Old records stay where they are, unchanged and indexed. The new model applies to new work.

1. **One record per piece of work.** History reports and follow-up folders become one type: a folder
   with a README and its data. Front matter adds `task` (the board task) and `kind` (result,
   decision, survey, review). The request and the steps stay on the board; the record holds the
   result. Decision records keep their `DR` numbers.
2. **No month log.** New entries stop. `lab archive log` prints the timeline from record dates.
3. **A small state layer.** The start page and the current topic pages say what is true now, each
   within a size budget, each statement linked to a record. Historical topic pages get the class
   `historical` and rank last.
4. **No seal file.** A check on git history replaces the hash manifest: a finished record may only
   grow at its end. This removes a file that changed in most commits.
5. **Generated indexes are not committed.** Search and `lab archive index --print` replace them.
   The start page stays a file.
6. **One closing command.** `lab archive done` updates derived files, runs the archive checks and
   reports what to fix, so closing a session is one call.
7. **Both directions linked.** A task closes with the record path and commit as evidence. A record
   names its task. `lab check` reports a record with no task and a closed research task with no
   record.

### 7.5 What the board needs

Asked of the board's owner as proposals, not built here:
- one call that lists finding and decision notes across the tasks of a board;
- an export of tasks and notes that the search index can read;
- a link type for an archive record (path and commit) that the web view shows.

### 7.6 Checks on this decision

- After four weeks: count the queries that only an old-style history report or log entry could
  answer and that the board plus the new record could not.
- Bookkeeping share of tool calls, check failure rate, and bookkeeping-only commits, against the
  figures in 7.2.
- Reverse by restoring the month-log and history rules in the project's instructions; no data is
  lost, because old files are never removed.
