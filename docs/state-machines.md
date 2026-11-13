# State machines of the research pipeline

This document models the research pipeline as state machines.
It names each machine, its states, its transitions and the place that enforces each transition.
It covers the tools as they are: lab 0.25.2, papers 0.15.0 (6851b18), claimcheck 0.13.1 (0eae562), prov 0.1.5 (8743e9d), agentprof 0.5.0 (6a72921), ireval 0.3.0 (fb47fb2), trading (911e485) and the work board.
A section names the version that changed a machine. The document also covers the machines that [archive-design.md](archive-design.md) adds.

An **explicit** machine has its states in code or in a config list.
An **implicit** machine has states that exist only as a combination of files, fields or habits.
Nothing names those states, so nothing can refuse a wrong move.
Implicit machines are marked **(implicit)**.

Each defect has one of three outcomes:

- **Fixed by** a task of the redesign (parent research-factory-211).
- **Proposed** as a task outside the redesign.
- **Accepted**: the document gives the reason.

The audit research-factory-414 (13 to 16 November 2026) read the code of each tool at the versions above.
It gives each gap one verdict:

- **Confirmed**: a probe or the code shows the gap. The board task that holds the fix is named (research-factory-NNN).
- **By design**: the behaviour is intended. A test, a code comment or a document says so.
- **Refuted**: a probe or a test shows that the gap does not exist.
- **Fixed in research-factory-414**: the audit changed the code.

File references are to the versions above.
A reference to another tool starts with its repository name, such as `papers src/http.rs:531`.
A reference without a repository name is to lab, in this repository.

Counts for the masters project are from commit `c0c1783` (12 November 2026), unless a section names another date.

## 1. The pipeline as one machine

A piece of research moves through these stages. Each stage is a machine in a later section.

```text
question ─► board task ─► session ─► sources ─► record ─► sealed record ─► indexed unit ─► hit ─► citation
            (§6)          (§5.9)     (§4)       (§2.1)    (§2.3)           (§3.2)         (§3.3)  (§4.6)
                │                                  ▲                                         │
                └──────── task ◄──► record ────────┘                                         │
                          (§7.1)                                                             ▼
                                              lesson ◄── signal ◄── check finding ◄── missed query
                                              (§5.1)     (§5.3)     (§5.5)            (§3.4)
```

Three rules hold across all machines:

1. **The board holds work state. The archive holds knowledge state.** A status that says "who does what next" belongs to a task. A status that says "is this still true" belongs to a record (archive-design section 7).
2. **Every terminal state names its reason or its successor.** `superseded` names the successor. `withdrawn`, `dropped` and `rejected` name a reason.
3. **A degraded state says so in its output.** A command that runs with a part missing states which part is missing. It does not return a smaller answer silently.

## 2. Archive machines (lab)

### 2.1 Record status

States are strings from `[lab.archive] statuses` (`src/config.rs:224`).
`inactive_statuses` means "no longer in force" (`src/config.rs:227`; default superseded, rejected, withdrawn, historical).
`terminal_statuses` defaults to superseded and withdrawn (`src/config.rs:257-260`).

| Transition or invariant | Enforced | Gap and verdict |
|---|---|---|
| Status is one of `statuses` | `front-matter-invalid` | None |
| A required status is present | `missing-field` (`src/archive/rules.rs:207`) | None |
| An inactive status names `superseded_by` or `withdrawn_reason` | `inactive-without-reason` (`src/archive/meta_rules.rs:275`) | By design: a superseded record that names no successor is caught only through `reason_statuses`. |
| `supersedes` and `superseded_by` agree | `supersession-mismatch`, `superseded-but-active` (`src/archive/rules.rs:268-298`) | None |
| A terminal status never changes | `status-reversed`, git seal only (`src/archive/gitseal.rs:537`) | None |
| Any other move | not enforced | Confirmed (probe): `accepted` to `proposed` and `rejected` to `accepted` pass. Masters seals `rejected` but does not make it terminal. The lab guard is research-factory-434. Whether `rejected` is terminal in masters is the owner's decision, research-factory-501 (ask open). |

The draft, final and appended states of model 2 belong to the seal (2.3), not to `status`.
`amends` and `amended_by` are a relation (`amendment-mismatch`, `src/archive/meta_rules.rs:322`), not a state. research-factory-207 added the rule and research-factory-200 the graph edge.

### 2.2 Record identity

| Machine | States | Enforcement | Gap and verdict |
|---|---|---|---|
| Numbered id | free, reserved, taken | `lab archive next --reserve` counts this checkout, the default branch and other worktrees (`src/archive/reserve.rs:51-86`). It writes the number under a lock in the main checkout's `.research` (`src/archive/reserve.rs:89-157`). | Confirmed (probe): the lock waits with no time limit (`reserve.rs:99`), research-factory-430. Confirmed (probe): a reserved-ids file that does not parse is reset silently (`reserve.rs:132-140`), so a number is given out twice, research-factory-431. |
| Id on the default branch | free, taken here, taken there | `id-taken-on-default-branch` (`src/archive/gitseal.rs:219-286`) | None |
| Id change | id, renumbered | `lab archive renumber` refuses a taken id (`src/archive/renumber.rs:266-276`) and a sealed record (`renumber.rs:283-297`). | Confirmed (probe): it renames first (`renumber.rs:652`), then writes (`renumber.rs:654-661`). A failed write leaves the rename and broken links, with no rollback. It prints "changed" before the write. research-factory-429. |
| Follow-up folder | without front matter, with front matter, with date | Fixed by 189 and 193 | None |

### 2.3 Seal

Manifest seal (`src/archive/seal.rs:40-48`): `New`, `Unchanged`, `Appended`, `Edited`, and missing.

| Transition | Enforced | Gap and verdict |
|---|---|---|
| New → Unchanged | `lab archive seal`, `update` (`src/archive/mod.rs:407-463`) | Confirmed (probe): with `model = 2` and `write_indexes = false`, `update` seals a record with invalid front matter (`mod.rs:594-611`). `done` refuses it (`src/archive/done.rs:235-251`). research-factory-426. |
| Unchanged → Edited | `sealed-edited` (`src/archive/rules.rs:886-893`) | None |
| Edited → Unchanged | `lab archive seal --accept` (`mod.rs:526-570`) | None |
| any → missing | `sealed-missing` (`rules.rs:898-909`) | Confirmed (probe): there is no exit. `--accept` fails on a deleted file, and the entry stays. research-factory-425. |

Git seal (`seal = "git"`, `src/archive/gitseal.rs`):

```text
untracked ─► committed ─► sealed ─► appended ─► appended ...
                             └─► edited ─► error, unless [[lab.archive.accepted_edit]] names path, commit and reason
```

- A file is sealed from the first commit where it matches `sealed` or has a status in `sealed_statuses`. Each later version must start with the sealed body (`gitseal.rs:506-547`).
- The check stores the last clean commit in `<state_dir>/lab/archive-git-seal.json` (`gitseal.rs:666-675`). So `lab archive check` writes state, although its doc comment says read-only (`src/archive/mod.rs:633`). Confirmed (probe). The decision is to keep the write and fix the comment: research-factory-427.
- Kept links and `lab check --new-only` read sealed text through one reader per commit (`AtRev`, `gitseal.rs:695-825`). Fixed in research-factory-414: the kept-link check went from 17 git calls to 3, and the `--new-only` seal lookup from 20 to 2.
- The reader treats a record as sealed when an earlier version had a sealed status (`status_sealed_by`, `gitseal.rs:770`). A baseline counts only when the commit contains it (`gitseal.rs:803`).
- Confirmed (probe): front matter that stops being a YAML mapping makes the whole file the sealed text (`src/archive/front.rs:100-108`). The check then reports `sealed-edited`, though the body did not change. research-factory-428.

### 2.4 Generated index

States **(implicit)**: `missing`, `stale`, `fresh`.
Today the index files are committed, so a stale index is a check error and each merge can conflict.
Model v2 does not commit them. `lab archive index --print` writes the index to standard output, and the search index (section 3.1) answers lookups.
The state `stale` then cannot occur in git. Fixed by 207; masters removes the committed indexes in research-factory-208.

### 2.5 Record completion

`lab archive done` (`src/archive/done.rs:254-302`) is the one gate:

```text
written ─► front matter valid ─► (manifest) sealed ─► search refreshed ─► links ─► task ─► sources ─► sealed (git)
```

It exits non-zero at the first stage with an error.

Link target: `untracked-link` (`src/archive/rules.rs:814-835`) is a Warn for a link to an untracked file, or to a directory that holds untracked files and nothing tracked.
An untracked target exists only in this checkout, so the link breaks in every other checkout.
`lab archive done` prints the fix for this Warn and still exits 0 (`src/archive/done.rs:73-76`).
Since lab 0.26.0 (research-factory-388), `ignored-link` (`src/archive/rules.rs:836-853`) is a Warn for a link to a file or directory that git ignores, on a line that the change adds.
A line counts as added when it differs from the merge base of HEAD with the default branch (`src/archive/gitseal.rs:190`); without a default branch or a merge base, no line counts.
The other ignored-link findings are dropped after `[[lab.archive.allow]]` has matched them, so an entry stays in use after its branch merges (`src/archive/rules.rs:187`).
The worktree data seed (section 5.6) still carries ignored targets to a linked worktree.
Git lists untracked files only at those link targets (`src/archive/gitseal.rs:141-166`). Fixed in research-factory-414: the scan went from 1.06-1.54 s to 0.08-0.11 s on masters.

Gaps in `lab archive check`:

- The duplicate-record check treats a git error as "no new files" (`src/search/similar.rs:160-176`). Confirmed: research-factory-493.
- Errors of the stale-derivative check go only to stderr (`src/archive/mod.rs:617-620`). Confirmed: research-factory-494.

### 2.6 Topic pages and the entry page **(implicit)**

| Machine | States | Evidence in masters | Outcome |
|---|---|---|---|
| Topic page | current, historical | 9 current and 39 historical pages. 125 links go from a current page to a historical page that `current_pages` does not mark. | Fixed by 189 (rule: a link from a current page to a historical page must be marked) and 193 (repair). Search gives historical pages the source class `historical`. |
| Entry page `START.md` | fresh, stale, over budget | 4,690 of 8,192 bytes. The line "Last rewritten 8 November" is stale. | Fixed by 193: the date line is removed, because git holds the date. The size budget stays a check. |
| Open question | open, closed | OQ01–OQ12 have ids. 8 closed bullets have none. | Fixed by 193. Under model v2 an open question is a board task; the archive keeps only the answer. |
| Month log | entry added | The November note says 155 entries; the file has 168. | Removed by model v2: git and the board hold the sequence of work. |

## 3. Search machines (lab)

### 3.1 Search index

One index per checkout, at `<state_dir>/lab/search/index.db`, with the write lock `lock` beside it. Owner: research-factory-187. Code: `src/search/index.rs`.

```text
absent ─► building ─► fresh ─► stale ─► refreshing ─► fresh
schema or fingerprint mismatch, unreadable meta ─► building
SQLite reports corrupt or not a database ─► building (forced)
locked: another process holds the write lock; the query answers from the last committed state, marked stale
```

| Transition | Trigger | Guard | Enforced by |
|---|---|---|---|
| absent → building | first `lab search`, `lab search index` or a scope set never built | the write lock; the wait stops at `lock_timeout_ms` (30 s) with exit 2 | `build` `src/search/index.rs:314-330` |
| building → fresh | the build writes `index.db.tmp-<pid>` and renames it over the index | the old file is never edited in place | `index.rs:333` |
| fresh → stale | a source's stamp differs from the stored one | checked at each query | `refresh` `index.rs:442` |
| stale → refreshing | the query itself (incremental) | only changed sources are read; the write lock must be free, or the query answers stale (`index.rs:491`) | `refresh` |
| refreshing → fresh, or stale with notice | the refresh ends, or reaches `refresh_budget_ms` (5 s) | `lab search index` and `refresh` have no budget (`src/search/mod.rs:787`) | `index.rs:513`, `:544` |
| any → building | stored schema version (`6`, `index.rs:24`) or fingerprint differs; the meta table does not read | any other open error than busy also rebuilds (`index.rs:291`) | `open_current` `index.rs:263-293` |
| any → building (forced) | SQLite returns `DatabaseCorrupt` or `NotADatabase` | there is no `PRAGMA integrity_check` | `open` `index.rs:107`, `:236` |

A batch of queries (`lab search --run`) refreshes once and reuses that refresh's stale flag and notices for each query (`refresh_batch` `src/search/mod.rs:801`, `open_listed` `index.rs:165`). Fixed in research-factory-414: 20 topics took 1.2-4.1 s instead of 10-26 s, with byte-identical output.

Stages: `lexical`, `dense`, `rerank`, `graph`. `lexical` and `graph` are `ready`; `dense` and `rerank` are `missing` (`src/search/mod.rs:266-269`).
No stage is ever `failed`. A query with a configured missing stage is `partial`, and the output and the trace name the stage (`mod.rs:969`).

Gaps:

- A killed build leaves `index.db.tmp-<pid>` (up to about 118 MB on masters). The next query does not remove it. An error in the build does remove it. Confirmed: research-factory-416.
- The graph stage is `ready` even when it finds no paper edges (3.7). Confirmed: research-factory-423.
- No test covers the stop at the refresh budget (`index.rs:544`): research-factory-417.
- An open error other than busy rebuilds the index silently. Refuted (probe): a bad `index.db` is rebuilt, the query exits 0, and stderr names the cause.

### 3.2 Unit

A unit is one searchable passage with an anchor (`path#Lstart-Lend` or `ref:<key>`).

```text
discovered ─► parsed ─► indexed (lexical) ─► live
                                              │
                   source changed ─► replaced │ source deleted ─► removed
```

A unit of a superseded or withdrawn record stays `live` and carries that status. Ranking lowers it and the output labels it.
A unit never outlives its source: a deleted file removes its units in the same refresh.
A replaced unit leaves its anchor (path, line range and content hash) in the index, so `lab open` finds the text an old anchor held. The index keeps these anchors for a file's last 10 versions. A deleted source drops them in the same refresh (since lab 0.20.1, research-factory-266). A rebuild carries them over from the index it replaces, and keeps that index's units of each file that changed since its last refresh as the file's next version. It starts without them when the replaced index has no `old_anchors` table with the same columns, and keeps no changed units when that index's `files` or `units` table lacks a column it reads. It also starts without them when that index does not read, and then it prints a note on stderr; the query is not partial (since lab 0.21.1, research-factory-326).

### 3.3 Query

```text
received ─► parsed ─► routed ─► retrieved ─► fused ─► reranked ─► grouped ─► answered
```

Terminal states and exit codes:

| State | Meaning | Exit |
|---|---|---|
| `hits` | at least one hit | 0 |
| `partial` | hits, with a stage missing or a stale index | 0, with a notice on standard error and in the trace |
| `zero` | no hit | 0 |
| `error` | bad query, unreadable index, lock timeout | 2 |

Each query writes one `rf.search-query/1` trace line with its terminal state (research-factory-192).
Since lab 0.19.0 (research-factory-295, masters DR045): `parsed` keeps the first 32 terms of a phrasing, and more terms give a `partial` notice, not an `error`. `fused` sums each phrasing's min-max normalised BM25 score; it was reciprocal rank fusion. Since lab 0.19.1 the scale of a phrasing is its lowest and highest score without the class and status prior, and each unit adds its prior times its scaled score, a share within 0..=1. Before, the scale held the priors, so one low-prior unit moved the shares of the others (research-factory-309; +0.007 nDCG@10, p 0.17). `reranked` passes the list on unchanged, because experiment E1 chose no reranker. `grouped` makes each unit a hit of its own and, with `--records`, keeps a record's best unit; it no longer puts at most two units of a record under its best one, and it folds a unit under the record it links only with `--fold`. On the masters test set, fully judged, grouping cost 0.02 nDCG@10 and folding 0.01.
Scopes share one scale and one set of statistics (research-factory-355, decided 13 November 2026). `retrieve` (`src/search/query.rs`) takes each phrasing's top 100 candidates over all searched scopes at once, and `normalised` scales them by the lowest and highest `score` of that list. A unit of an added scope can set either end, or push other units out of the top 100. Their shares then change. Two units with different priors can swap within one phrasing, because each share is `prior * (score - low) / (high - low)`, and the sum over the phrasings can reorder any units. This happens even when the added unit itself is not in the top 10. FTS5 takes BM25's statistics (term frequencies across units, mean length) from the whole index, which holds every scope ever searched in this checkout, not only the scopes of the query. On masters, adding the `board` scope reordered the non-board units of 10 of the 82 base topics; nDCG@10 moved +0.0049, not significant (research-factory-354). The decision is to keep this: masters DR045 fuses one candidate list per phrasing over the searched scope, and a scale per scope would give each scope's best unit share 1, which E1 did not test. To compare scope sets, run them on one index, or rebuild with `--rebuild --in` for each. Reverse by normalising per scope in `search`, measured on the masters test set and approved by the owner, because it changes DR045's configuration.
`zero` and `partial` are not errors, so a script can tell "nothing found" from "search broken".

Since lab 0.25.1 (research-factory-311), `lab search` prints one info line when paper texts come from the main checkout. The line is not a partial notice.
A query of `notes` and `files` walks the notes once; `files` reuses the notes listing (`sources::list`, `src/search/sources.rs:217`). Fixed in research-factory-414.

Gaps:

- The MCP tools `archive_open` and `archive_related` report only a stale index. They drop the other notices (`src/search/mcp.rs:204-217`). Confirmed: research-factory-418.
- The notes walk goes through about 69,000 entries in ignored data folders. It costs 0.3-1.5 s of each query and each `lab mcp` call (`default_notes`, `mod.rs:316`). Confirmed: research-factory-424 lists notes from git instead.
- The `files` scope drops the notices of the notes listing (`sources.rs:289`). Refuted (probe): the notice reaches a query with `--in files`.

### 3.4 Missed query and thesaurus entry

```text
zero-hit or reformulated query ─► missed ─► alias proposed ─► accepted ─► thesaurus entry (active)
                                                    └────────► rejected (kept, never proposed again)
thesaurus entry: active ─► retired (no query used it in the review window)
```

`lab retro` proposes; a person or an agent accepts with `lab retro accept` (research-factory-205).
Rejections are stored. Without that state, the same alias returns at each retro. This is the main implicit machine of the learning loop.
Decisions persist in `<state_dir>/lab/retro.json` (`src/search/learn.rs:27`).

Gap, confirmed: `load` (`learn.rs:81`) and `save` (`learn.rs:106`) take no lock. 20 parallel `lab retro dismiss` calls stored 11 decisions; the others were lost.
`thesaurus::append` and `retire` (`src/search/thesaurus.rs:100`, `:123`) use the same pattern, and they race too. research-factory-419 holds both.

### 3.5 Model loading in `lab mcp` (retired)

`unloaded ─► loading ─► ready`, or `failed`, while a dense or rerank stage was planned.
Experiment E1 (masters DR045) chose lexical search, so no stage loads a model.
Since lab 0.19.0 the machine is removed and each `archive_search` result says `mode: lexical` (research-factory-295).

### 3.6 Board export cache

The cached output of the board export that feeds the `board` scope, one for each checkout, in `<state_dir>/lab/search/` (research-factory-345).
Before, each query ran the export with a limit of 15 seconds, so an export with notes (about 14 s) made every query with the board slow (research-factory-273).
Code: `src/search/board.rs` (`board::read` `:82`, `board::start` `:137`, `board::refresh` `:197`); the query side is `list_board` (`src/search/chat.rs:224`).

```text
absent ──query──► absent + refreshing ──ok──► fresh
fresh ──board_max_age passes──► stale ──query──► stale + refreshing ──ok──► fresh
refreshing ──export fails──► the old state, with board.error
another command configured: the cache counts as absent
```

| Transition | Trigger | Guard | Enforced by |
|---|---|---|---|
| absent → refreshing | a query of the `board` scope | the lock on `board.lock` is free and no refresh failed in the last 60 s; the query answers without the scope, `partial` | `list_board`, `board::start` |
| fresh → stale | the cache file's mtime is `board_max_age` minutes old or older | checked at each query; a fresh cache answers in full | `board::read`, `list_board` |
| stale → refreshing | a query of the `board` scope | the lock is free and no refresh failed in the last 60 s; the query answers from the stale copy, `partial`, with its age | `list_board`, `board::start` |
| refreshing → fresh | the export prints a task array within 120 s (`board.rs:29`) | the cache is written to a temporary file and renamed; `board.error` is removed | `board::refresh` |
| refreshing → old state | the export fails, prints no task array or runs past 120 s | the old cache stays; `board.error` holds the reason, which the next stale query names; its mtime holds the next detached refresh back for 60 s | `board::refresh`, `board::start` |
| any → refreshing (foreground) | `lab search --refresh-board` | waits for the lock, then runs the export; a failure exits 2 | `board::refresh` |

One refresh at a time: the query takes the lock and hands it to the detached refresh as its standard input, so the lock stays held until the refresh exits.
A second query that finds the lock held starts none and says that a refresh is running.
The detached refresh checks the cache again before it runs the export, so a query that looked just before the last refresh ended starts no export.
The refresh runs in its own process group with its output in `board.log`; the export runs in a group of its own, which a timeout kills whole.

Gaps:

- `lab search --refresh-board` waits for `board.lock` with no time limit (`board.rs:218`). A held lock kept it waiting until it was killed. Confirmed: research-factory-420.
- A `board.json` that does not parse counts as absent, and the query says "no board export is cached yet" (`board.rs:82-93`). The next refresh replaces it. Confirmed: research-factory-421.
- No test covers the 120 s export timeout: research-factory-417.

### 3.7 Paper texts in a linked worktree

Search reads ignored paper texts from the main checkout (research-factory-311). Two readers do not:

- `graph::paper_texts` and `read_papers` (`src/search/graph.rs:359-420`) read sidecars under the worktree only. `lab related` loses "cited by papers" without a notice. Confirmed: research-factory-422.
- `lab open` (`src/search/open.rs:218`) exits 2 for a text that search returned. Confirmed: research-factory-422.

Other gaps of the paper graph:

- `graph::registry_path` (`graph.rs:329`) reads only `[papers] registry`. Without that table, refs are indexed from the default registry, but the graph has no paper edges. Confirmed: research-factory-423, which also makes the graph stage `partial` in that case.
- `read_papers` skips a sidecar that does not read, with no notice. Confirmed: research-factory-491.
- `entry["key"].as_str()` skips a numeric registry key, so its paper gets no edges (`graph.rs:374`). Confirmed: research-factory-492.

## 4. Paper machines (papers, claimcheck)

### 4.1 Paper lifecycle

Most of this machine is implicit. The state is the set of fields and files that exist for a key.

```text
candidate ─► registered ─► abstract ─► fetched ─► text ─► structure ─► card ─► indexed ─► cited ─► quote-checked
                 │                        │         │         │          │
                 └─► duplicate_of ─► ...  │         └─────────┴──────────┴─► stale when the text's sha256 differs
                                          └─► fetch failed (attempt count, last error)
```

| State | Stored as | Set by | Task |
|---|---|---|---|
| candidate | a search result, not stored | `papers search` | research-factory-188 |
| registered | registry entry `rf.reference/1` | `papers registry build` | exists |
| abstract | `abstract` field | `papers registry abstracts` | research-factory-198 |
| fetched | PDF in the ignored store | `papers fetch`, `papers read` | exists |
| text | `.txt` or `.docling.md`, with manifest and sha256 | `papers read`, `papers registry record-pdfs` | exists |
| structure | `rf.paper-structure/1` sidecar | `papers structure` | exists |
| card | `rf.paper-card/1` | `papers card` (calls a model) | research-factory-213 |
| indexed | units in the search index | `lab search` | research-factory-191 |
| cited | a note cites `ref:<key>` | author | checked by claimcheck |
| quote-checked | verdict per quote | `claimcheck` | exists |

Guards:

- **Stale derivative.** A structure sidecar or a card records the sha256 of the text it was made from. When the text's sha256 differs, the derivative is `stale`. The index ignores a stale card. It still cuts a text's passages within a stale sidecar's sections, found again in the changed text by their heading lines (since lab 0.21.5, research-factory-334). `papers registry check` reports both.
- **Write order.** A derivative is written to a temporary file, recorded, then renamed.

Defects outside the redesign, fixed by research-factory-219 (papers 0.7.0):

- `verified` is written by `papers verify --record`. `papers registry check` now reads it:
  - a status other than `match`, `mismatch` or `not-found` is `invalid-value`;
  - a bad `at` date is `malformed-date`;
  - a recorded `mismatch` or `not-found` is `verified-mismatch` (warn).
- `duplicate_of` links are checked. A cycle is `duplicate-cycle` (error, once per cycle). A chain longer than one link is `duplicate-chain` (warn), which names the key it ends at.
- `papers read` stages a plain or layout text as `<name>.txt.tmp`, records it, writes its manifest, then renames it. A failed step undoes the text; the saved PDF stays (see the gaps below). A Docling text already worked this way.
- A catalogue entry with no depth is `depth-missing` (warn), as section 4.2 says. The 17 masters entries that had a null depth now state `unknown` with a reason. research-factory-414 checked this: the 17 null-depth registry entries match 17 `Inspection depth: unknown` lines, and `papers registry check` gives 0 depth findings.

Withdrawn or retracted paper: accepted as a notice, not a registry state. Since papers 0.8.0 (research-factory-282), `papers verify` reports `verify-withdrawn` (warn) from arXiv's OAI-PMH version history, `verify-retracted` and `verify-notice` (warn) from Crossref `updated-by` and `update-to`, and `verify-updated` (info) for corrections, on every run. `verified.status` stays `match`, since these are notices, not metadata errors. No `withdrawn{at, source}` registry field was added, because nothing reads it. On masters key 435, `papers verify` reports `verify-withdrawn 435: arXiv v2 withdrawn 2025-11-12; ours pins v1` (research-factory-291, papers 0.10.0). Since papers 0.10.0, an export API 429 falls back to OAI-PMH.

Gaps at papers 0.15.0, each confirmed by a probe unless it says otherwise:

- `papers read` keeps the PDF after a failed step; it undoes the text (`papers src/main.rs:3223-3284`). `save_pdf` writes in place. research-factory-468.
- `sha256_file` drops read errors (`papers src/main.rs:2687-2689`). A recorded text that cannot be read counts as missing, and `papers read` downloads and replaces it with no notice. research-factory-469.
- `papers registry record-pdfs` writes the manifest before the registry (`papers src/main.rs:758-993`). A failed registry save leaves a `pdf_sha256` only in the manifest, after a false success line. A rerun does not repair it. research-factory-470.
- `papers verify --record` writes once at the end (`papers src/main.rs:3920-3960`). An interrupted run records nothing. research-factory-473.
- `papers card` calls the model again when a stored card cannot be read (`papers src/card.rs:695-708`). It does not reuse the kept reply. research-factory-474.
- `papers registry check` does not flag a recorded text that is missing on disk. research-factory-475.
- `papers fetch` exits 1 on input that is not a DOI or URL, and `papers seen` exits 2. research-factory-467.
- `derivative::check` skips unreadable inputs without a finding (`papers src/derivative.rs:52-62`). Code reading only; held by research-factory-495.

### 4.2 Reading depth

States are the values of `depth` in the registry. The code accepts these values: metadata, abstract, method, full, artifact, reproduced, plus `unknown` (`papers src/registry.rs:16-23`).
This document follows the code's list (research-factory-414, decision E1).
The tool accepts any listed value in any order.
Accepted: depth describes how far a person or an agent read, and a re-read at a lower depth does not happen in practice.

- `unknown` is valid only as `Inspection depth: unknown. Reason: …`, with text after the word `Reason` (papers 0.10.1).
- Other text after `unknown` gives `depth-unparsed` (Warn) and no level (papers 0.11.1).
- An entry with no depth gives `depth-missing` (Warn), `papers src/catalogue.rs:1008`.
- A duplicate entry gives no `depth-missing` (papers 0.10.1).

### 4.3 Network machines of papers

| Machine | States | Note |
|---|---|---|
| HTTP attempt | API calls: up to 5 tries; waits 5, 10, 20, 40 s, or `Retry-After` (max 300 s). PDF and `read` downloads: 2 tries, one 5 s wait. | explicit, `papers src/http.rs:196-262`. The first wait was measured. The later API waits are from the code, because no test host can stand in for an API host: research-factory-479. |
| HTTP cooldown | per host, after a 429 on an API call; kept in the lock folder | explicit. An unreadable cooldown file counts as none, with no notice (`papers src/http.rs:531-536`). Confirmed: research-factory-472. |
| HTTP cache entry | fresh for 7 days, then expired | explicit |
| Host throttle | free, waiting, request, done; one request per host each 1 s (arXiv 3 s), counted from the end of the last request | explicit, `papers src/http.rs:22-25`, `:698-712`. A waiter has no time limit and prints nothing. Confirmed (probe: a 12 s silent wait): research-factory-471. |
| Browser page | closed, opening, loading, ready, challenge, blocked, loaded; 45 s limit | explicit, `papers src/browser.rs:24-28`, `:99-132` |
| Browser link queue | first list capped at 8; links from loaded pages not capped | `papers src/main.rs:3554`. Open: not probed, because no Chromium is installed. research-factory-480. |
| PDF reader | running, exited, output joined | The join after the reader exits has no time limit (`papers src/pdf.rs:330-335`). A grandchild that holds stdout blocks `papers read`. Confirmed (probe: 45.95 s): research-factory-476. |
| Registry lock | free, held | explicit file lock, `papers src/registry.rs:319-337`. A waiter has no time limit and prints nothing. Confirmed (probe: a 12 s silent wait): research-factory-471. |
| Seen index | built from the registry, then notes | An invalid note is skipped with a stderr line and no finding (`papers src/seen.rs:69-71`). A DOI held only by that note gives "not seen", exit 1. Confirmed: research-factory-466. |
| Graph source | primary, then fallback | S2 paging is bounded (`papers src/graph.rs:215-232`). The fallback order was not re-checked at 0.15.0; research-factory-495 holds that check. research-factory-178 proposes a fast failure on rate limits. |

Speed, measured by research-factory-414:

- A Docling read starts Python twice: once for `--version`, once to convert (`papers src/main.rs:3172`, `:3739`). research-factory-477.
- `papers cited-by` asks OpenAlex for 100 results per page. 200 saves one request per run (`papers src/openalex.rs:390`). research-factory-478.
- A parallel Crossref and OpenAlex fetch gave no gain (3.93 s against 3.95 s). The 1 s spacing per host sets the floor. No task.

### 4.4 Claim and quote verdicts (claimcheck)

| Machine | States | Defect |
|---|---|---|
| Finding | reported; severity Error or Warn | No baseline or allow state. `lab check --new-only` supplies the baseline from outside, and since lab 0.21.0 the merge base with the default branch, so a finding the change adds to the files it touches is gated. Accepted. |
| Quote verdict | exact match (no finding); near match, similarity 0.8 or more (`quote-not-found`, Warn); not verbatim, similarity below 0.8 (`quote-not-found`, Error); kept (Info) | Threshold: `quotes.error_below`, default 0.8 (`claimcheck src/config.rs:41`). The Warn or Error split is at `claimcheck src/check.rs:921-936`. Gap: an unclosed quotation gets no finding (`claimcheck src/doc.rs:643`). Confirmed (probe): research-factory-450. |
| Number verdict | found; near (context or rounded); not found (`number-not-in-source`, Warn); changed text only (`source-text-changed`, Warn) | Gap: a number not found is searched twice in each text (`claimcheck src/check.rs:1078`, `:1156`). Speed only: research-factory-453. |
| Source text load | loaded; hash mismatch (`source-text-changed`, Warn; matches do not count as supported); missing (`source-text-missing`, Warn) | Fixed by research-factory-220 (claimcheck 0.11.0, commit bc7c914). Gap: a text path shared by two references uses the first sha256 only (`claimcheck src/check.rs:40`). Confirmed (probe): research-factory-451, which also checks whether masters has such a path. |
| Checked file read | read; unreadable | Gap: a checked file that is not valid UTF-8 aborts the whole run, with exit 2 and no output (`claimcheck src/lib.rs:178`). Confirmed (probe): research-factory-452. |
| Kept quotation | kept (Info); stale (`stale-kept-quote`, Warn); dropped (`off`) | By design. Each state has a test (`claimcheck tests/rules.rs:2011`, `:2058`, `:2087`, `:2147`). |
| Text index **(implicit)** | not built; built once per text and run (`OnceLock`, `claimcheck src/source.rs:42`) | The first use builds every field. A lazy build cut instructions by 29% in a prototype: research-factory-454. Registry lines are parsed twice, as a JSON value and then as the typed struct: research-factory-455. |

### 4.5 Provenance (prov)

| Machine | States | Note |
|---|---|---|
| Manifest | complete (summary required, `prov src/manifest.rs:317-327`); incomplete (imported, `prov src/import.rs:488`) | Explicit flag `header.complete` (`prov src/manifest.rs:39-40`, `:61`). Writes go through `.partial` (`prov src/manifest.rs:397-420`). There is no fsync before the rename: research-factory-449 decides whether one is needed. |
| Run record | `incomplete` (placeholder, `prov src/run.rs:906-916`); `complete` (`prov src/run.rs:1379`) | Explicit through `status`. The placeholder is explicit since prov 0.1.0 (46dbf44). The filled `complete` state is there since prov 0.1.2 (a8f3c59). A record without the field reads as complete (`prov src/run.rs:351`), by design (test `run_record_names_its_status_and_old_records_still_load`). Gaps: no command removes or finishes an incomplete record, and `verify` and `rerun` refuse it (`prov src/run.rs:1441-1446`): research-factory-445. A kill between create and write leaves a 0-byte file (`prov src/run.rs:1179-1190`): research-factory-444. |
| Run interrupt guard | not installed; installed; restored; signal pending | Explicit type `CatchInterrupts` (`prov src/run.rs:943`). It is installed before the placeholder (`prov src/run.rs:1176`). Ctrl-C before the start leaves no record (`prov tests/cli.rs:1749`). A kill during the run leaves the placeholder (`prov tests/cli.rs:1834`). By design, SIGTERM and SIGHUP are not caught. |
| Command output drain **(implicit)** | spawned; waiting for exit; draining stdout and stderr; finished | `child.wait()` at `prov src/run.rs:1255`, then the stream joins at `:1259` and `:1264`. Gap: the drain waits for end of file with no time limit, so a background grandchild holds `prov run` (probe: 5.10 s against 0.40 s). Confirmed: research-factory-446. |
| Preflight tool probes **(implicit)** | not run; run | `git_state` (`prov src/run.rs:687-704`) and the tool versions (`prov src/run.rs:1141-1152`) spawn 7 processes one after another, 8 with `--python`. `git --version` runs twice. Confirmed: research-factory-447. |
| Run outcome | ok, failed | explicit |
| Rerun output | snapshot taken, compared | explicit |
| Import pairing | paired; unpaired: a hash without a path (`hash-without-path`, Info), or an object with a hash and no path (`record-without-path`, Info) | Gap: a path without a string hash gives no finding (`prov src/import.rs:90-92`). Confirmed (probe): research-factory-448. |

No defect was found that affects the redesign. research-factory-231 found that the placeholder already said `"status": "incomplete"` and readers refused it, and named the filled state (prov PR #4, a8f3c596). prov pairs a path field with a hash field inside one JSON object; it has no pairing between a run and an imported output.
The open prov gaps do not change the state set.
`prov lineage` exits 0 when it skips records; by design (`prov tests/cli.rs:1856-1862`).

### 4.6 Citation

A citation in a note is `uncited claim ─► cited ─► resolved (key exists) ─► quote-checked`.
`claimcheck` enforces each step for changed notes through `lab check --new-only`.
DR042 added 8 uncited-claim warnings; they are Warn findings and stay visible in research-factory-193.
Since lab 0.21.0, `lab check --new-only` fails on such a warning when a change adds it to a note it touches.
The research-factory-414 audit did not re-check these claims; research-factory-497 holds that check.

## 5. Loop machines (lab)

### 5.1 Lesson

States (`src/lessons.rs:27`): `observed`, `proposed`, `adopted`, `verified`, `retired`. Masters has 17 observed, 5 adopted, 3 verified (`lab lesson list`, 14 November 2026).

`transition` (`src/lessons.rs:199`) moves status forward. Two moves back are allowed (`:214`): `verified → adopted`, and a retired lesson reopening at any status. `verified` needs an active status (`:208`). Re-adoption resets `adopted` and `reviewed`. Fixed by research-factory-217.

Gaps:

- Two `lab lesson set` runs on one lesson race: `save` uses the fixed temporary name `<id>.md.tmp` and no lock (`:365`). In 40 concurrent rounds, 5 lost a History line and 25 had one command fail. Confirmed: research-factory-437.
- A hand edit can move a lesson from observed to verified without a History line; `load` checks field shape only. Confirmed by probe. By design: git history records the edit. No task.

### 5.2 Loop run

```text
hook fires ─► checkout: main checkout redirect, or skip (bare, no config in the main checkout, not `[lab.loop] home`)
           ─► lock held: stop ─► too recent (`--if-older`, under the lock): stop
           ─► measure (600 s limit each) ─► evaluate ─► plan ─► notify each due signal (60 s limit each)
           ─► save signals.json ─► save last.json
```

Explicit, with a file lock (`src/lesson_loop.rs:456`); `checkout` is at `:392`, `home` at `:426`. An unreadable `last.json` cannot show a recent loop, so the loop runs and replaces it (lab 0.17.0, 217).

Gaps:

- `signals.json` is saved once, after all sends (`:653`). A loop killed after a successful notify, or a notify that files a task and then times out, sends the signal again. `board-notify.sh` then files a second task. Confirmed (probe: 2 notify calls, first ref lost): research-factory-435.
- An unreadable `signals.json` fails every loop (exit 2, no sends); the error is in `last.json`. Confirmed by probe. research-factory-440 names the fix in the error.
- `--background` rotates `loop.log` outside the lock (`:136`). Two starts at once lose the old generation (94 of 100 concurrent rounds, 0 of 100 single). Confirmed: research-factory-438.
- The masters Stop hook runs `lab loop --background` with its output discarded, so a failed start is never seen. Confirmed: research-factory-443.

### 5.3 Signal

| Machine | States | Defect |
|---|---|---|
| Signal kind | fixed list | None |
| Send state (`src/signals.rs:475`, `plan` `:565`) | pending (`new`, `repeat`, `reopened`), sent, resolved | A notice still pending when its signal resolves is dropped and reported (`unsent`). A signal never sent that comes back is `new`. Fixed by 217. A signal whose detail does not change is never sent again, even when its task was closed while the condition stays. Confirmed (probe; test `dedupe_sends_new_changed_and_reopened_signals_once`): research-factory-439. |
| Board task for a signal | none, filed, repeated | `signals.json` keeps the task id that `board-notify.sh` returns as `ref`; repeats go to that task while it is open. Fixed by 217. `board-notify.sh` does not look up an open task by signal id before `board add`, so a resend after a crash or timeout files a duplicate (5.2, research-factory-435). |

### 5.4 Evaluation

`Verdict` (`src/evaluate.rs:38`): `Collecting`, `Improved`, `Worse`, `Inconclusive`, `Unmeasurable`. Explicit. A lesson moves to `verified` only from an active status (`src/lessons.rs:208`); the verdict informs that move and does not force it.

`lab brief` and `lab evaluate` load runs in parallel, and `lab brief` reads run history without the measure lines. Fixed in research-factory-414: `lab brief` on masters went from 0.54-0.78 s to 0.18-0.20 s.
`check_runs` rebuilds each run's measured key sets for every lesson, about half of the `lab evaluate` samples. Confirmed: research-factory-441.

### 5.5 Check run

| Machine | States | Defect |
|---|---|---|
| Check status | failed (default, `src/check.rs:400`), ok (exit 0 or 1 with every line parsed, `:462`) | A disagreeing exit adds `check-exit-mismatch`: exit 1 without an error finding, and exit 0 with one. clap refuses `--new-only` with `--at`. Fixed by 217. Since research-factory-414 the checks of one run run at the same time, each after its version command (`run_checks`, `:639`). The run keeps the configured order, and `duration_ms` per check overlaps. Gap: a check has no time limit and waits for its stdout to close (`:391`). A checker that leaves a child running holds `lab check` (probe: 20 s for a checker that exits at once). Confirmed: research-factory-436. |
| Baseline | none, recorded | explicit |
| Finding severity | Error, Warn, Info | Each rule's default is in docs/interfaces.md "Rule table". A Warn fails no gate, except, since lab 0.21.0, a Warn that `lab check --new-only` finds new against the merge base in a file the change touches (research-factory-226). Accepted as the meaning of Warn; each rule this document depends on is an Error. |
| Stored check run **(implicit)** | stored, committed, superseded and committed; not stored when it repeats the newest stored run (`repeats`, `:976`); stored with a suffix on a name clash (`store`, `:1180`) | A stored run in `.research/lab/checks/` is evidence, but an agent could delete or edit it, and a superseded run was often left uncommitted. Fixed by 228 (lab 0.20.0): the skill says to commit every run of a session, superseded runs included, with the text it checked. The opt-in PreToolUse hook `skill/research-factory/scripts/guard-check-runs.sh` refuses a tool call that deletes or edits a stored run; masters wires it in `.claude/settings.json`. It guards against mistakes, not intent, and fails open without jq. Since research-factory-414 it runs one jq call and exits early when a command has no deleting or writing word. A check of a head whose run was committed, as after a merge, stored one more run that nobody committed (5 such files in the masters main checkout on 14 November 2026). Fixed by 391 (lab 0.23.0): `lab check` and `lab archive done` store no run that repeats the newest stored run for the same text (both clean, on the same day, commits equal outside the state directory, equal checks, exits, statuses, versions, measures, errors and warnings; info findings may differ) and print `not stored`; see docs/interfaces.md section 7. Since 392 (lab 0.24.0), a linked worktree stores no run whose findings name git-ignored files that the main checkout has and the worktree lacks (row Worktree data). |

### 5.6 Replay and worktree

| Machine | States | Note |
|---|---|---|
| Replay worktree | created, checked, removed | `Drop` guard (`src/replay.rs:250`) on each exit path. A run killed by a signal leaves the worktree, and the next replay removes worktrees of dead pids (`remove_leftovers`, `:198`). By design; a reused pid keeps a leftover. Since research-factory-414 the `--new-only` merge-base run runs while the work tree's run does (`src/main.rs:849`). The base run still spends about 6.7 s in `git worktree remove` and 2.4 s linking 25,614 ignored files: research-factory-442. |
| Worktree `Location` (`src/worktree.rs:17`) | NotLinked, Bare, Linked | explicit |
| Ignored-file fallback | own checkout, main checkout | explicit since lab 0.9.10; the search index does **not** fall back: each checkout has its own index (section 3.1), but it reads the paper texts that a linked worktree lacks from the main checkout and, since lab 0.25.1 (research-factory-311), says so on stderr. `lab related` and `lab open` do not fall back (3.7). |
| Worktree data (masters) **(implicit)** | empty, seeded, pushed back | A new worktree had none of the main checkout's ~54,000 git-ignored files (research-factory-312). masters' `.claude/hooks/worktree-data.py` clones them (APFS copy-on-write) on SessionStart and after EnterWorktree, and pushes files made in the worktree back to the main checkout on Stop. It never copies `private/` or secrets; `.env` templates and files a provenance manifest lists are copied (research-factory-316). The hook's context line `worktree-data: cloned N ...` reaches the agent (verified live by research-factory-325). A worktree that no masters session started in stayed empty: 9 of 21 were on branches older than the hook (masters 69393fa8), whose own `.claude/settings.json` lacks it, and one was made by `git worktree add` from a research-factory session. Its `lab check` stored a run with 162 `source-text-missing` warnings and no `quote-not-found` errors, while main's run of the same text had 2 errors. Fixed in lab by 392 (lab 0.24.0): in a linked worktree, `lab check` stores no run whose findings name git-ignored files that the main checkout has and the worktree lacks, prints why, and exits 2; see docs/interfaces.md section 7. |

### 5.7 Retro candidate

`found ─► shown ─► filed (a lesson's metric covers the rule) or dismissed`.
Explicit: `lab retro dismiss` stores the decision, and a dismissed candidate does not return (`src/main.rs:1315`, `src/search/learn.rs:114`; test `a_dismissed_candidate_does_not_return`). Fixed by 205.

### 5.8 Rule lifecycle **(implicit)**

A check rule is `proposed ─► Warn ─► Error ─► retired`.
Fixed by research-factory-232: `src/rules.rs` holds every rule with its default severity, and docs/interfaces.md "Rule table" adds the config that enables it and the lab version that added it.
A test keeps the two tables equal. Debug builds stop on a finding whose rule is not in the table or whose severity differs from it.
A rule that moves from Warn to Error, or is retired, changes both tables in one change, so git history records each move.

### 5.9 Session

```text
started ─► brief shown ─► working ─► record written ─► lab archive done ─► task done ─► ended
```

**(implicit)** today: the Stop gate `.claude/hooks/archive-gate.sh` in masters covers only `docs/research` and `AGENTS.md`.
Under model v2 the order is enforced in two places: `lab archive done` (section 2.5) and the board's `task_done` evidence, which names that command.
Sessions as a search scope (transcripts) are indexed from agentprof's session list; agentprof 0.3.0 counts archive searches, greps and reads per session (research-factory-212).
agentprof's own machines are in section 9.1.

## 6. Board machines (agent-board)

| Machine | States | Enforced | Defect |
|---|---|---|---|
| Task | open, doing, blocked, review, done, dropped | move table, `ops/tasks.rs:26-35` | `done` accepts a check with a non-zero exit |
| Proposal | pending, accepted (`auto`), rejected | instruction only | The server does not check who sets `auto` |
| Ask | open, answered, withdrawn | server | An answer does not unblock the task |
| Lease | held, expired, released | advisory | Accepted: leases are hints |
| Agent session | joined, working, idle, ended | none | No transition guard |
| Handoff | version n → n+1 | server | None |
| Feedback | reported; routed; resolved | two fields | **(implicit)** state from two fields. Proposed in a note on agent-board-454: one status with a move table. |
| Board | active, closed; temporary | server | None |
| Retention | kept, pruned | server | None |

The four defects are proposed in agent-board-454.

## 7. Joints between machines

A joint is a state that two tools share. Each joint was implicit.

### 7.1 Record and task

Evidence: only 24 of 134 done tasks are cited in the archive. 16 of 19 sampled decision notes are not cited. Only DR042 names a task in its front matter. `lab` does not check that a `task` value exists.

```text
task open ─► doing ─► record drafted (front matter: task, kind) ─► record done ─► task done (evidence: record path)
                                                                        └─► decision note on the task names the record
```

| Guard | Place | Task |
|---|---|---|
| A new record names a `task` and a `kind` | `lab archive check` | 189 |
| The `task` exists on the board | `lab archive done` and `lab archive check`, through a configured board export command | 207 |
| Board unreachable | `task-link-skipped`, an Info note (`src/archive/meta_rules.rs:530-540`). `lab archive check` exits 0, so it is a pass. Confirmed (probe): the export's error output is dropped (`meta_rules.rs:401`). A background process the export starts holds lab for 30 s (`meta_rules.rs:31`), and a timeout kills only the direct child (`meta_rules.rs:441-448`). research-factory-432. A list cut at the export's `--limit` makes `unknown-task` wrong with no warning: research-factory-433. Since research-factory-414 the export starts before the archive walk and runs while the files are read. | 207, 414 |
| A done task with a record names it | board evidence; rule in masters `AGENTS.md` | 208 |

### 7.2 Board note and record

A finding lives first as a board note (work state). When it is knowledge that outlasts the task, it moves to a record.
`note ─► cited in a record ─► record sealed`. The board scope of `lab search` reads notes through a board export, `[lab.search] board_export` or else the same one, cached (3.6), so a note that never became a record is still found, with the source class `chat`.

### 7.3 Experiment, decision and build

```text
experiment task ─► measured ─► decision record (proposed) ─► accepted ─► build task ─► installed ─► final evaluation
                                        └─► rejected (kept with reason)                                │
                                                                              regression gate: figures not below the chosen configuration's
```

The umbrella task holds this machine: its acceptance says that each configuration an experiment chose is built and installed.
A decision that waits on the owner is a blocked task with an ask (for example research-factory-213 and DR043).

Two machines inside an experiment were implicit until 12 November 2026:

| Machine | States | Enforced |
|---|---|---|
| Experiment call batch **(implicit)** | unchecked, preflight passed, preflight failed, calling | Plain `claude -p` and `claude-api` calls also sent the model the account email and an `# Environment` block. Fixed by 224, 322 and its sub-tasks: headless calls go through `~/.claude/bin/claude-experiment` (dotfiles). `claude-experiment preflight --cap N` exits 4 when a forwarded canary request still carries either block, whatever the prompt size (327), and fails over the prompt-token cap. It turns off Claude Code's auto-memory by default (332). Callers stop the batch on a nonzero exit before any call: papers `card --preflight` (323, papers 0.12.0), and in masters E1's `llm.py` (one preflight per batch under a lock), `run-cards.sh` and `research.toml` `[papers] claude` (324). Agentic harnesses that need tools, such as E4's `run_session.sh`, still call `claude -p`. |
| Test-set judgment pin (masters) **(implicit)** | pinned at `as_of`, drifted, re-pinned, judge again, judged | The test set names units by line range at the commit in `topics.jsonl`'s `as_of`; an edit moves them. `check_qrels.py` exits 1 for a docid that no longer matches a unit, and for each row of `work/judge-again.tsv`. `repin.py` sorts each judged pair as same, moved, changed or vanished, rewrites the first three and rewrites `as_of`; with `--write` it lists changed pairs in `work/judge-again.tsv`, so they are judged again, not rewritten silently (245). Runs and extra grade files scored against the set are moved into its coordinates too (340, 341). |

### 7.4 Text and its derivatives

Covered in section 4.1 (stale derivative). The joint is between `papers` (writer) and `lab search` (reader); the sha256 in each sidecar is the shared state.
A linked worktree breaks this joint for `lab related` and `lab open`; see 3.7.
`lab archive check` prints the errors of its stale-derivative check only on stderr (2.5, research-factory-494).

### 7.5 Tool version and project

`merged ─► installed ─► required by the project`. A project that needs a lab version states it; install is by git revision of the merge commit.
Explicit since 207: `[lab] min_version` (`src/config.rs:86`). `lab brief` says first when the installed lab is older (`version_note`, `src/brief.rs:545`).

## 8. Implicit machines found

Each row names its verdict and the task that holds each open gap.
Rows 21, 24, 30 and 31 describe machines outside the research-factory-414 audit (the board and experiments); they keep their earlier outcome.

| # | Machine | Section | Status |
|---|---|---|---|
| 1 | Record status moves | 2.1 | Fixed by research-factory-207 for terminal statuses. Other moves are free: research-factory-434 (lab guard), research-factory-501 (`rejected` in masters; owner's decision, ask open). |
| 2 | Partial supersession (`amends`) | 2.1 | Fixed by research-factory-207, research-factory-200. |
| 3 | Numbered id reservation | 2.2 | Fixed by research-factory-207. Lock with no time limit: research-factory-430. Silent reset of a reserved-ids file: research-factory-431. |
| 4 | Follow-up folder metadata | 2.2 | Fixed by research-factory-189, research-factory-193. |
| 5 | Generated index freshness | 2.4 | Fixed (removed) by research-factory-207, research-factory-208. |
| 6 | Record completion | 2.5 | Fixed by research-factory-207. `update` skips the front-matter check with `write_indexes = false`: research-factory-426. Duplicate-record check hides a git failure: research-factory-493. |
| 7 | Topic page current or historical | 2.6 | Fixed by research-factory-189, research-factory-193. |
| 8 | Entry page freshness | 2.6 | Fixed by research-factory-193. |
| 9 | Open question | 2.6 | Moved to the board by research-factory-208. |
| 10 | Search index and its stages | 3.1 | Built by research-factory-187. Temporary file after a kill: research-factory-416. No test of the budget stop: research-factory-417. Silent rebuild on an open error: refuted (probe). Document drift (`failed` stage, integrity check, vectors) fixed in research-factory-414. |
| 11 | Degraded query | 3.3 | Built by research-factory-187, research-factory-192. MCP tools drop notices: research-factory-418. Notes walk through ignored folders: research-factory-424. `files` scope loses the notes notices: refuted (probe). One notes walk for `notes` and `files`: fixed in research-factory-414. |
| 12 | Missed query and rejected alias | 3.4 | Built by research-factory-205. Parallel writers lose retro decisions and thesaurus lines: research-factory-419. |
| 13 | Paper lifecycle | 4.1 | Built by research-factory-188, research-factory-198, research-factory-213, research-factory-191. Gaps: research-factory-467, research-factory-468, research-factory-469, research-factory-470, research-factory-473, research-factory-474, research-factory-475. 17 null-depth entries: claim refuted as a gap (registry check, 0 depth findings). |
| 14 | Stale derivative | 4.1, 7.4 | Built by research-factory-198, research-factory-213, research-factory-191. papers skips unreadable inputs: held by research-factory-495. lab prints its check errors only on stderr: research-factory-494. |
| 15 | Seen index | 4.3 | Fixed by research-factory-188 for registered papers. A DOI held only by a skipped invalid note gives "not seen": research-factory-466. |
| 16 | Run record placeholder, import pairing | 4.5 | Placeholder explicit since prov 0.1.0. Filled `complete` state added by research-factory-231 (prov 0.1.2, a8f3c59). 0-byte record after a kill: research-factory-444. No exit from `incomplete`: research-factory-445. Path without hash not reported: research-factory-448. No fsync before rename: research-factory-449. |
| 17 | Board task for a signal | 5.3 | Fixed by research-factory-217 (lab 0.17.0). Duplicate task on a resend after a crash or timeout: research-factory-435. No resend after the task closed while the condition stays: research-factory-439. |
| 18 | Retro candidate dismissal | 5.7 | Fixed by research-factory-205; test `a_dismissed_candidate_does_not_return`. |
| 19 | Rule lifecycle | 5.8 | Fixed by research-factory-232. |
| 20 | Session order | 5.9 | Fixed by research-factory-207, research-factory-208. |
| 21 | Feedback state | 6 | Proposed in agent-board-454 (note). |
| 22 | Record and task link | 7.1 | Fixed by research-factory-189, research-factory-207, research-factory-208. Board failure is an Info note and a pass. Export error output, 30 s hold and kill: research-factory-432. List cut at `--limit`: research-factory-433. Export overlaps the archive walk: fixed in research-factory-414. |
| 23 | Board note to record | 7.2 | Built by research-factory-187 (board scope); its cache is row 32. |
| 24 | Experiment to build | 7.3 | Held by the umbrella task research-factory-211. |
| 25 | Tool version needed by a project | 7.5 | Fixed by research-factory-207 (`min_version`, `version_note`). |
| 26 | Id on the default branch or in another worktree | 2.2 | Fixed by research-factory-227, research-factory-337. Renumber by research-factory-336; no rollback on a failed write: research-factory-429. |
| 27 | Link target tracked, ignored or untracked | 2.5 | Fixed by research-factory-228 (warning). Scan limited to link targets: fixed in research-factory-414. |
| 28 | Stored check run | 5.5 | Fixed by research-factory-228 (skill rule, opt-in hook), research-factory-391, research-factory-392. Hook fast path: fixed in research-factory-414. |
| 29 | Worktree data | 5.6 | Fixed by research-factory-312, research-factory-316 (masters hook); an unseeded worktree's run is refused by research-factory-392 (lab 0.24.0). |
| 30 | Experiment call batch | 7.3 | Fixed by research-factory-224 and research-factory-322 with its sub-tasks 323, 324, 327, 332, 333. |
| 31 | Test-set judgment pin | 7.3 | Fixed by research-factory-245, research-factory-340, research-factory-341. |
| 32 | Board export cache | 3.6 | Built by research-factory-345. Foreground lock wait with no limit: research-factory-420. Corrupt `board.json` reported as absent: research-factory-421. No timeout test: research-factory-417. |
| 33 | Paper texts in a linked worktree | 3.7 | `lab related` and `lab open` lack the main-checkout fallback: research-factory-422. `project.registry` ignored: research-factory-423. Unreadable sidecar skipped silently: research-factory-491. Numeric registry keys skipped: research-factory-492. |
| 34 | Signal state and loop log in a loop run | 5.2 | Signal state saved after all sends: research-factory-435. Log rotation outside the lock: research-factory-438. Unreadable `signals.json` fails every loop: research-factory-440. Failed background start not seen: research-factory-443. |
| 35 | Lesson file writers | 5.1 | Unlocked writes with a fixed temporary name: research-factory-437. Hand edit without a History line: by design (git history records it). |
| 36 | Check with no time limit | 5.5 | research-factory-436. |
| 37 | Seal exits and cache | 2.3 | Deleted manifest-sealed file has no exit: research-factory-425. Check writes the cache, comment says read-only: research-factory-427. Front matter that is not a mapping: research-factory-428. One reader per commit: fixed in research-factory-414. |
| 38 | Waits of the papers network | 4.3 | Registry lock and throttle waits with no limit: research-factory-471. Unreadable cooldown file: research-factory-472. PDF reader join: research-factory-476. Browser link cap, not probed: research-factory-480. Graph fallback order: held by research-factory-495. |
| 39 | claimcheck verdict inputs | 4.4 | Unclosed quotation: research-factory-450. One text path with two sha256: research-factory-451. Unreadable checked file aborts the run: research-factory-452. Number searched twice: research-factory-453. |
| 40 | claimcheck kept quotation | 4.4 | By design; each state has a test (`claimcheck tests/rules.rs:2011`, `:2058`, `:2087`, `:2147`). |
| 41 | claimcheck text index | 4.4 | Eager build: research-factory-454. Registry lines parsed twice: research-factory-455. |
| 42 | prov run interrupt guard | 4.5 | By design (tests `prov tests/cli.rs:1596`, `:1620`, `:1749`). Placeholder before the guard: refuted at 8743e9d (fixed in prov 9ea76bb). |
| 43 | prov command output drain | 4.5 | research-factory-446. |
| 44 | prov preflight tool probes | 4.5 | research-factory-447. |
| 45 | agentprof scan output set | 9.1 | Mixed set after a stop in the rename phase; `report` compares counts only: research-factory-456. |
| 46 | agentprof Claude candidate read | 9.1 | Whole-file read of each candidate: research-factory-457. Path-boundary needle: by design (test `cwd_mentions_stop_at_path_boundaries`). |
| 47 | agentprof Claude open turns | 9.1 | Counts tool calls, documents say turns: research-factory-458. |
| 48 | agentprof Claude content blocks | 9.1 | A failed parse drops every block silently: research-factory-459. |
| 49 | agentprof T3 run payload | 9.1 | Fallback to model `unknown` with no note: research-factory-459. |
| 50 | agentprof Codex rollout state | 9.1 | Rule by design. 468 repeated `transcript-missing` warnings per scan: research-factory-460. |
| 51 | agentprof Codex turn and `last_ts` | 9.1 | By design at segment ends; a test pins it. End-of-file branch test: research-factory-498. |
| 52 | ireval duplicate policy | 9.2 | By design (probe; test `check_reports_duplicates_in_unjudged_queries`). |
| 53 | ireval missing-query policy | 9.2 | By design (probe; test `config_supplies_defaults_and_flags_override`). |
| 54 | ireval config source | 9.2 | Not recorded in JSON: research-factory-461. Check message text not asserted: research-factory-462. |
| 55 | ireval tie reorder | 9.2 | By design (README, tests); no ties in the 5 real runs. |
| 56 | ireval score input and output pipe | 9.2 | NaN panic: refuted (probe; the parser rejects NaN). Broken pipe: refuted (probe). |
| 57 | trading archive INDEX and status fields | 9.3 | INDEX after a crash: by design (test `test_index_rewrites_stale_index_after_rename`). Idea status moves: research-factory-463. Paper `status: read`: research-factory-464. Tests: research-factory-465. |

## 9. Session, evaluation and trading machines

These tools are outside lab.
agentprof reads agent sessions (section 5.9). ireval scores retrieval runs for experiments (section 7.3). The trading project checks its notes archive with `scripts/archive.py`.

### 9.1 agentprof (0.5.0, 6a72921)

| Machine | States | Code | Gap and verdict |
|---|---|---|---|
| Scan output set **(implicit)** | old set; staged (hidden `.<name>.<pid>.tmp`, synced); renamed in order; mixed | `write_outputs` (`agentprof src/scan.rs:1265-1287`) stages three files, then renames sessions, findings and `scan.json` (`scan.rs:1284`). A staging error removes the temporary files (`scan.rs:1275-1281`). | A stop during the rename phase leaves a mix, with no rollback. `report` compares counts only (`agentprof src/report.rs:181`), so a mix with equal counts passes. The test at `scan.rs:1309` covers a staging failure only. Confirmed (probe): research-factory-456. |
| Claude candidate read | candidate; read whole; prefilter hit or miss; parsed | `agentprof src/scan.rs:463-482`; `cwd_needle` (`agentprof src/claude.rs:108-111`); `mentions_cwd` (`scan.rs:324-326`) | Every candidate is read whole. A parallel prefilter cut the median full scan from 8.39 s to 6.27 s in a prototype: research-factory-457. The needle stops at path boundaries, by design (test `cwd_mentions_stop_at_path_boundaries`, `scan.rs:1344`). |
| Claude open turns | tool call pending; answered; open at end | `agentprof src/claude.rs:889-892`; `agentprof src/session.rs:314-319` | `open_turns` counts unanswered tool calls, not turns; README line 163 says turns. Confirmed (probe: `turns` 1, `open_turns` 2): research-factory-458. |
| Claude content blocks | parsed; dropped | `agentprof src/claude.rs:684`, `:784` (`unwrap_or_default`) | One bad block drops every block of the message, with no note. Confirmed (probe): research-factory-459. |
| T3 run payload | parsed; defaulted | `agentprof src/t3.rs:128-135`, `:312-314` | A malformed payload gives model `unknown`, with no note. Confirmed (probe): research-factory-459. |
| Codex rollout presence | plain; zstd; missing | `agentprof src/codex.rs:589-600`; rule at `agentprof src/detect.rs:648-660` | The `transcript-missing` rule matches the state, by design. On this machine 468 such warnings repeat at each full scan. research-factory-460 summarises them. |
| Codex spawn fixpoint | pass n; stable | `agentprof src/scan.rs:628-646` | Each pass parses `source` again. No waste: about 6 ms for 3 passes over 1,680 threads. |
| Codex turn and `last_ts` | turn open; turn closed at a segment end | `agentprof src/codex.rs:1040-1062`, note at `:1092-1094` | By design at segment ends; a test pins it. The end-of-file branch has no test: research-factory-498. |
| Billing mode lookup | matched; not matched | `agentprof src/billing.rs:98-105`; window at `:42` | The reverse scan has no early break. No waste. Test `lookup_needs_same_cwd_and_window`. |

The archive shell lexer (`agentprof src/archive.rs:330-827`) was not read; research-factory-498 holds it (section 10).

### 9.2 ireval (0.3.0, fb47fb2)

| Machine | States | Code | Gap and verdict |
|---|---|---|---|
| Duplicate policy | `Error` (default); `First` | `ireval src/eval.rs:209-212`; flag at `ireval src/main.rs:289-291`; `check` uses `First` (`main.rs:599`) | By design. Probe: the default run exits 2, `--duplicates first` exits 0, and `check` exits 1 with `run-duplicate-doc`. Test `check_reports_duplicates_in_unjudged_queries`. |
| Missing-query policy | flag; config; `skip` | `ireval src/main.rs:266-268` | The flag beats the config, which beats `skip`. By design; verified by probe and test `config_supplies_defaults_and_flags_override`. |
| Config source **(implicit)** | discovered; loaded; not recorded | `discover` (`ireval src/config.rs:62`), `load` (`:74`); `settings_json` (`ireval src/main.rs:316-325`) | The JSON settings do not record the config path. Confirmed: research-factory-461. The `check` severity override (`main.rs:604-609`) has a test, but no test asserts its message text: research-factory-462. |
| Ties | ranked by score descending, then docid descending | `trec_order` (`ireval src/eval.rs:109-113`); finding `tie-reorders-run` (`eval.rs:490`) | By design (README, named tests). The 5 real runs hold no ties. |
| Score input | parsed; rejected | `parse_run` (`ireval src/input.rs:161`); NaN check (`input.rs:210`) | A NaN score could panic at `eval.rs:113`. Refuted (probe): the parser rejects it with `score is NaN` and exit 2. |
| Output path | written; broken pipe | `out` (`ireval src/main.rs:369-379`); broken pipe at `:375` | Refuted (probe): a closed pipe causes no fault. |

The `score` command is an alias of `eval`; verified by probe and test `score_is_an_alias_of_eval`.

### 9.3 trading (911e485)

`scripts/archive.py` loads notes (`load_notes`, `trading scripts/archive.py:93`), checks markets (`check_markets`, `:134`), validates (`validate`, `:140`), checks links (`check_links`, `:196`), renders the INDEX (`render`, `:225`) and runs (`main`, `:297`).
`scripts/archive.py.lock` is a uv lockfile, not runtime state.

| Machine | States | Gap and verdict |
|---|---|---|
| INDEX | fresh; stale after a crash between rename and rewrite | The next run rewrites a stale INDEX. By design: test `test_index_rewrites_stale_index_after_rename`. |
| Idea status **(implicit)** | the values of `status` in an idea note | `validate` (`trading scripts/archive.py:140-193`) does not check status moves. Confirmed: research-factory-463. |
| Paper status **(implicit)** | the values of `status` in a paper note | `status: read` is accepted on a paper with no recorded text. Confirmed: research-factory-464. |
| Link target | present; missing | A link to a missing directory is reported. Refuted as a silent gap (probe: 93 broken-link errors). |

Only 3 tests cover the archive, all of them for the INDEX rebuild: research-factory-465.
The YAML loader takes 180-470 ms without the C loader. Measured; not a defect.

## 10. Code the audit did not read

The research-factory-414 audit read each tool in part. These tasks hold the code it did not read:

| Tool | Code not read | Task |
|---|---|---|
| papers | `verify.rs` lookups (`:534-680`), `arxiv.rs`, `crossref.rs`, `atom.rs`, `notes.rs`, `text.rs`, `refs.rs`, `id.rs`, `finding.rs`, most of `structure.rs`, `sources.rs`, `cite.rs`, `bib.rs`, `check.rs`, `catalogue.rs`, `http.rs`, `openalex.rs`, `searchlog.rs` and `graph.rs`; `card.rs:190-494`; parts of `main.rs`; the tests were not run. Also the graph fallback order (4.3) and the unreadable inputs of `derivative::check` (4.1). | research-factory-495 |
| prov | `run.rs` environment selection, `program_info`, the `rerun` and `verify` bodies and output formatting; the `manifest.rs` build walk, verify body and diff; the `import.rs` parse, resolve and import body; `lineage.rs:70-248`; `walk.rs:1-142` and `:253-290`; `compare.rs`, `dupes.rs` and `finding.rs`. | research-factory-496 |
| claimcheck | `check.rs` citations, document walk, `check_depth`, `reference_entry` and `check_document`; `source.rs` normalisation and index internals; `matcher.rs` past its signatures; `main.rs`; `tests/cli.rs`. Also the `lab` and claimcheck claims of section 4.6. | research-factory-497 |
| agentprof | Most of `scan.rs`, `claude.rs`, `codex.rs`, `detect.rs`, `session.rs` and `t3.rs`; `billing.rs:1-41` and `:113-144`; the archive shell lexer `archive.rs:330-827`; `util.rs`, `pricing.rs`, `main.rs`; `tests/scan.rs`. Also a test of the Codex end-of-file branch (9.1). | research-factory-498 |
| ireval | The rest of `eval.rs`, `main.rs` and `config.rs`. | research-factory-499 |
| lab | `src/stats.rs`, `src/finding.rs` and the rest of `src/evaluate.rs` (past `Verdict`, `measured` and `new_units`). | research-factory-500 |
