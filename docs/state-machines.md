# State machines of the research pipeline

This document models the research pipeline as state machines.
It names each machine, its states, its transitions and the place that enforces each transition.
It covers the tools as they are (lab 0.22.0, papers 0.12.1, claimcheck 0.12.0, prov 0.1.3, agentprof 0.4.0, the work board; a section names the version that changed a machine) and the machines that [archive-design.md](archive-design.md) adds.

An **explicit** machine has its states in code or in a config list.
An **implicit** machine has states that exist only as a combination of files, fields or habits.
Nothing names those states, so nothing can refuse a wrong move.
Implicit machines are marked **(implicit)**.

Each defect has one of three outcomes:

- **Fixed by** a task of the redesign (parent research-factory-211).
- **Proposed** as a task outside the redesign.
- **Accepted**: the document gives the reason.

Counts for the masters project are from commit `c0c1783` (12 November 2026).

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

States are strings from `[lab.archive] statuses`. `inactive_statuses` is the subset that means "no longer in force" (`src/config.rs:161`).
The masters project uses `proposed`, `accepted`, `superseded` for decisions (2, 36, 4 records).

Current enforcement (`src/archive/rules.rs`):

| Transition or invariant | Enforced | Defect |
|---|---|---|
| Status is one of `statuses` | `lab archive check` | A missing status passes when the index does not require the field |
| A record with `superseded_by` has an inactive status | checked | The reverse is not checked: an inactive record may name no successor |
| `supersedes` and `superseded_by` agree | checked (0 errors in masters) | None |
| Any status may follow any status | not enforced | A record can go from `superseded` back to `accepted` |

Model v2 (task research-factory-207) makes the machine explicit:

```text
draft ──► final ──► appended ──► superseded (names successor)
  │         │           │    └─► withdrawn  (names reason)
  └─────────┴───────────┴──► amended (stays in force; names the amending record)
```

- `draft`: the file exists and is not committed in a sealed state. Free to edit.
- `final`: committed. The body is sealed (section 2.3).
- `appended`: later text was added after the sealed body.
- `superseded` and `withdrawn` are terminal. `lab archive check` refuses a move out of them.
- `amended` is not terminal. It is the partial supersession that DR042 applies to DR007 and DR039: both stay accepted and gain a line that names DR042. The front matter key is `amended_by`, with `amends` on the other record. Fixed by research-factory-207 (rule) and research-factory-200 (graph edge).

Guards added by research-factory-207: an inactive status requires `superseded_by` or `withdrawn_reason`; a record that requires a status and has none is an error; a status outside the allowed moves is an error.

### 2.2 Record identity

| Machine | States | Defect | Outcome |
|---|---|---|---|
| Numbered id (DR, H) **(implicit)** | free, taken | `src/archive/index.rs:232-259` takes the highest number plus one with no reservation. Two worktrees can take the same number. Masters has H147 and H89 twice. | Fixed by 207: decision numbers are reserved in the main checkout's `.research` directory. Results use a folder slug with a date and need no number. |
| Id on another branch **(implicit)** | free, reserved, taken here, taken on the default branch, taken in another worktree | A reservation counted only this checkout's ids and the numbers given out. A branch from an older main, or a record in another worktree, could still take a number (masters DR042 twice, research-factory-203). | Fixed by 227 (lab 0.21.3): `lab archive next` counts the ids on the default branch and in every other worktree (`src/archive/reserve.rs:1-8`). Fixed by 337 (lab 0.21.4): `lab archive check` warns `id-taken-on-default-branch` when the default branch added a record with the same id at another path since the merge base (`src/archive/gitseal.rs:152-157`). |
| Id change | id, renumbered | Repair was by hand: rename the file, the `id:`, the heading and each link. | Fixed by 336 (lab 0.21.6): `lab archive renumber OLD NEW`. It refuses a `NEW` taken here, on the default branch or in another worktree (`src/archive/renumber.rs:264-275`), and a sealed record, since a new name would break its seal (`src/archive/renumber.rs:287-296`). It renames first, then rewrites links, and lists links in sealed text and plain mentions for a person. |
| Follow-up folder **(implicit)** | folder without front matter, with front matter, with date | 198 folders in masters; 32 have front matter | Fixed by 193 (repair) and 189 (rule) |

### 2.3 Seal

Current states (`src/archive/seal.rs:40-48`): `New`, `Unchanged`, `Appended`, `Edited`, compared with a committed manifest file.

| Transition | Enforced | Defect |
|---|---|---|
| New → Unchanged | `lab archive seal` | A missing manifest makes every file `New`. The check passes. |
| Unchanged → Appended | allowed | None |
| Unchanged → Edited | `lab archive check` error | None |
| Entry removed | no command | An entry never leaves the manifest. A deleted file stays listed. |
| File not sealed | finding `unsealed`, severity Warn | A Warn never fails a gate |
| `lab archive update` | `src/archive/mod.rs:499-507` | It seals even when the index write was refused. The seal then covers a state the index does not show. |

Model v2 replaces the manifest with git (research-factory-207):

```text
untracked ─► committed draft ─► sealed ─► appended ─► appended ...
                                  │
                                  └─► edited ─► error, unless [[lab.archive.accepted_edit]] names path, commit and reason
```

- A file is `sealed` from the first commit in which its status is a sealed status.
- From then on, each later version must start with the sealed body. Front matter may change.
- `lab archive log` shows the history of one record from git. `lab archive check` caches the last verified commit, so the check reads only newer commits.
- An accepted edit is a config entry with a reason. It is the only way out of `edited`.
- `lab archive update` stops before the seal step when the index step fails.
- Since lab 0.21.2 (research-factory-252), a kept link and `lab check --new-only` treat text as sealed when an earlier commit gave the record a sealed status, even if a later commit changed the status (`src/archive/gitseal.rs:662-716`, `status_sealed_by`). A baseline counts only when it is an ancestor of the commit checked.

### 2.4 Generated index

States **(implicit)**: `missing`, `stale`, `fresh`.
Today the index files are committed, so a stale index is a check error and each merge can conflict.
Model v2 does not commit them. `lab archive index --print` writes the index to standard output, and the search index (section 3.1) answers lookups.
The state `stale` then cannot occur in git. Fixed by 207; masters removes the committed indexes in research-factory-208.

### 2.5 Record completion

`lab archive done` (research-factory-207) is the one gate for "this record is complete":

```text
written ─► links resolve ─► task named and known ─► sources cited ─► sealed ─► done
```

It runs the index print, the checks for changed records, and the seal check, and it exits non-zero at the first state it cannot leave.
It replaces the instruction "run `lab archive update`, then `lab check --new-only`".

Link target **(implicit)**: a link resolves in this checkout, but its target can be `tracked`, `ignored` or `untracked` by git.
An untracked target exists only in this checkout, so the link breaks in every other one.
Since lab 0.20.0 (research-factory-228), `lab archive check` warns `untracked-link` for a link to an untracked file, or to a directory that holds untracked files when git tracks nothing in it (`src/archive/rules.rs:740-766`).
It is a Warn: `lab archive done` prints the fix and still exits 0.
Since lab 0.26.0 (research-factory-388), it warns `ignored-link` for a link to an ignored file, or to an ignored directory that git tracks nothing in: no commit holds the target, so a fresh clone lacks it (`ignored_targets` in `src/archive/rules.rs`).
A target that a linked worktree finds in the main checkout counts too; the worktree data seed (section 5.6) still carries ignored targets to linked worktrees.

### 2.6 Topic pages and the entry page **(implicit)**

| Machine | States | Evidence in masters | Outcome |
|---|---|---|---|
| Topic page | current, historical | 9 current and 39 historical pages. 125 links go from a current page to a historical page that `current_pages` does not mark. | Fixed by 189 (rule: a link from a current page to a historical page must be marked) and 193 (repair). Search gives historical pages the source class `historical`. |
| Entry page `START.md` | fresh, stale, over budget | 4,690 of 8,192 bytes. The line "Last rewritten 8 November" is stale. | Fixed by 193: the date line is removed, because git holds the date. The size budget stays a check. |
| Open question | open, closed | OQ01–OQ12 have ids. 8 closed bullets have none. | Fixed by 193. Under model v2 an open question is a board task; the archive keeps only the answer. |
| Month log | entry added | The November note says 155 entries; the file has 168. | Removed by model v2: git and the board hold the sequence of work. |

## 3. Search machines (lab, new)

### 3.1 Search index

One index per checkout, in the ignored `.research` directory. Owner: research-factory-187.

```text
absent ─► building ─► fresh ─► stale ─► refreshing ─► fresh
             │                             │
             └──────────► failed ◄─────────┘
schema mismatch ─► rebuilding ─► fresh        corrupt ─► rebuilding ─► fresh
locked: another process holds the write lock; readers continue on the last committed state
```

| Transition | Trigger | Guard |
|---|---|---|
| absent → building | first `lab search` or `lab search index` | write lock |
| fresh → stale | a source file's size, mtime or hash differs from the stored value | detected at each query |
| stale → refreshing | the query itself (incremental) | only changed files are read; a refresh over the time budget returns results marked `stale` |
| any → rebuilding | stored schema version differs, or the integrity check fails | the old file is replaced by rename, never edited in place |
| building → failed | error | the partial file is removed; the next query starts again |

Sub-machine **stages**: `lexical`, `dense`, `rerank`, `graph`. Each is `ready`, `missing` or `failed`.
A query that runs without a stage is in **degraded mode**. The output and the trace name the missing stages (rule 3).
Vectors are keyed by the content hash of the unit, so an unchanged unit is never embedded twice.

### 3.2 Unit

A unit is one searchable passage with an anchor (`path#Lstart-Lend` or `ref:<key>`).

```text
discovered ─► parsed ─► indexed (lexical) ─► embedded ─► live
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

### 3.4 Missed query and thesaurus entry

```text
zero-hit or reformulated query ─► missed ─► alias proposed ─► accepted ─► thesaurus entry (active)
                                                    └────────► rejected (kept, never proposed again)
thesaurus entry: active ─► retired (no query used it in the review window)
```

`lab retro` proposes; a person or an agent accepts with `lab retro accept` (research-factory-205).
Rejections are stored. Without that state, the same alias returns at each retro. This is the main implicit machine of the learning loop.

### 3.5 Model loading in `lab mcp` (retired)

`unloaded ─► loading ─► ready`, or `failed`, while a dense or rerank stage was planned.
Experiment E1 (masters DR045) chose lexical search, so no stage loads a model.
Since lab 0.19.0 the machine is removed and each `archive_search` result says `mode: lexical` (research-factory-295).

### 3.6 Board export cache

The cached output of the board export that feeds the `board` scope, one for each checkout, in `<state_dir>/lab/search/` (lab 0.22.0, research-factory-345).
Before, each query ran the export with a limit of 15 seconds, so an export with notes (about 14 s) made every query with the board slow (research-factory-273).
Code: `src/search/board.rs`; the query side is `list_board` in `src/search/chat.rs`.

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
| refreshing → fresh | the export prints a task array within 120 s | the cache is written to a temporary file and renamed; `board.error` is removed | `board::refresh` |
| refreshing → old state | the export fails, prints no task array or runs past 120 s | the old cache stays; `board.error` holds the reason, which the next stale query names; its mtime holds the next detached refresh back for 60 s | `board::refresh`, `board::start` |
| any → refreshing (foreground) | `lab search --refresh-board` | waits for the lock, then runs the export; a failure exits 2 | `board::refresh` |

One refresh at a time: the query takes the lock and hands it to the detached refresh as its standard input, so the lock stays held until the refresh exits.
A second query that finds the lock held starts none and says that a refresh is running.
The detached refresh checks the cache again before it runs the export, so a query that looked just before the last refresh ended starts no export.
The refresh runs in its own process group with its output in `board.log`; the export runs in a group of its own, which a timeout kills whole.

## 4. Paper machines (papers, claimcheck)

### 4.1 Paper lifecycle

Most of this machine is implicit today: the state is the set of fields and files that exist for a key.

```text
candidate ─► registered ─► abstract ─► fetched ─► text ─► structure ─► card ─► indexed ─► cited ─► quote-checked
                 │                        │         │         │          │
                 └─► duplicate_of ─► ...  │         └─────────┴──────────┴─► stale when the text's sha256 differs
                                          └─► fetch failed (attempt count, last error)
```

| State | Stored as | Set by | Task |
|---|---|---|---|
| candidate | a search result, not stored | `papers search` | research-factory-188 reports registered papers first |
| registered | registry entry `rf.reference/1` | `papers add` | exists |
| abstract | `abstract` field | `papers` metadata fetch | research-factory-198 |
| fetched | PDF in the ignored store | `papers fetch` | exists |
| text | `.txt` and sidecar with sha256 | `papers text` | exists; extractor choice by research-factory-202 |
| structure | `rf.paper-structure/1` sidecar | `papers structure` | research-factory-198 |
| card | `rf.paper-card/1` | `papers card` | research-factory-213, after DR043 |
| indexed | units in the search index | `lab search` | research-factory-191 |
| cited | a note cites `ref:<key>` | author | checked by claimcheck |
| quote-checked | verdict per quote | `claimcheck` | exists |

Guards the redesign adds:

- **Stale derivative.** A structure sidecar or a card records the sha256 of the text it was made from. When the text's sha256 differs, the derivative is `stale`. The index ignores a stale card. It still cuts a text's passages within a stale sidecar's sections, found again in the changed text by their heading lines (since lab 0.21.5, research-factory-334). `papers registry check` reports both.
- **Write order.** A derivative is written to a temporary file and renamed after its record.

Defects outside the redesign, fixed by research-factory-219 (papers 0.7.0):

- `verified` is written by `papers verify --record`. `papers registry check` now reads it:
  - a status other than `match`, `mismatch` or `not-found` is `invalid-value`;
  - a bad `at` date is `malformed-date`;
  - a recorded `mismatch` or `not-found` is `verified-mismatch` (warn).
- `duplicate_of` links are checked. A cycle is `duplicate-cycle` (error, once per cycle). A chain longer than one link is `duplicate-chain` (warn), which names the key it ends at.
- `papers read` stages a plain or layout text as `<name>.txt.tmp`, records it, writes its manifest, then renames it. A failed step undoes the earlier ones. A Docling text already worked this way.
- A catalogue entry with no depth is `depth-missing` (warn), as section 4.2 says. The 17 masters entries that had a null depth now state `unknown` with a reason.

Withdrawn or retracted paper: accepted as a notice, not a registry state. Since papers 0.8.0 (research-factory-282), `papers verify` reports `verify-withdrawn` (warn) from arXiv's OAI-PMH version history, `verify-retracted` and `verify-notice` (warn) from Crossref `updated-by` and `update-to`, and `verify-updated` (info) for corrections, on every run. `verified.status` stays `match`, since these are notices, not metadata errors. No `withdrawn{at, source}` registry field was added, because nothing reads it. On masters key 435, `papers verify` reports `verify-withdrawn 435: arXiv v2 withdrawn 2025-11-12; ours pins v1` (research-factory-291, papers 0.10.0). Since papers 0.10.0, an export API 429 falls back to OAI-PMH.

### 4.2 Reading depth

States are the values of `depth` in the registry, from "listed" to "read in full", plus `unknown`. The tool accepts any listed value in any order.
Accepted: depth describes how far a person or an agent read, and a re-read at a lower depth does not happen in practice.

- `unknown` is valid only as `Inspection depth: unknown. Reason: …`, with text after the word `Reason` (papers 0.10.1). The label may carry a note in parentheses, such as `Reason (12 November 2026): …`.
- Other text after `unknown`, such as `unknown; abstract only`, gives `depth-unparsed` (warn) and no level (papers 0.11.1, research-factory-299).
- An entry with no depth gives `depth-missing` (warn) (papers 0.7.0, research-factory-219). So do a bare `Inspection depth: unknown` and a `Reason` with no word after it, such as `Reason:` or a copied `Reason: …` (papers 0.11.1).
- An entry that is a duplicate of another entry gives no `depth-missing` (papers 0.10.1).

### 4.3 Network machines of papers

| Machine | States | Note |
|---|---|---|
| HTTP attempt | try 1–5, then failed | explicit, with backoff |
| HTTP cache entry | fresh for 7 days, then expired | explicit |
| Host throttle | free, waiting | explicit |
| Browser page | closed, opening, loaded, blocked | explicit |
| Registry lock | free, held | explicit file lock |
| Seen index | built from notes | **(implicit)**: it skips invalid notes without a finding. Fixed by 188, which makes the registered set the first answer. |
| Graph source | primary, fallback | explicit fallback order; research-factory-178 proposes a fast failure on rate limits |

### 4.4 Claim and quote verdicts (claimcheck)

| Machine | States | Defect |
|---|---|---|
| Finding | reported; severity Error or Warn | No baseline or allow state. `lab check --new-only` supplies the baseline from outside, and since lab 0.21.0 the merge base with the default branch, so a finding the change adds to the files it touches is gated. Accepted. |
| Quote verdict | verified (score ≥ 0.8), weak, not found | None |
| Number verdict | found, not found | None |
| Source text load | loaded, hash mismatch, missing | Fixed by research-factory-220 (claimcheck 0.11.0). A quote or number matched only in a text whose sha256 differs from the registry gets `source-text-changed` (warn) on its line and does not count as supported. |

### 4.5 Provenance (prov)

| Machine | States | Note |
|---|---|---|
| Manifest | incomplete, `complete` | explicit flag |
| Run record | `incomplete` (placeholder), `complete` | explicit through `status`; a filled record says `complete` since prov 0.1.2, and a record without the field reads as complete |
| Run outcome | ok, failed | explicit |
| Rerun output | snapshot taken, compared | explicit |
| Import pairing | paired, unpaired | explicit: an unpaired entry yields `hash-without-path` or `record-without-path` |

No defect was found that affects the redesign. research-factory-231 found that the placeholder already said `"status": "incomplete"` and readers refused it, and named the filled state (prov PR #4, a8f3c596). prov pairs a path field with a hash field inside one JSON object; it has no pairing between a run and an imported output.

### 4.6 Citation

A citation in a note is `uncited claim ─► cited ─► resolved (key exists) ─► quote-checked`.
`claimcheck` enforces each step for changed notes through `lab check --new-only`.
DR042 added 8 uncited-claim warnings; they are Warn findings and stay visible in research-factory-193.
Since lab 0.21.0, `lab check --new-only` fails on such a warning when a change adds it to a note it touches.

## 5. Loop machines (lab)

### 5.1 Lesson

States (`src/lessons.rs:27`): `observed`, `proposed`, `adopted`, `verified`, `retired`. Masters has 11 observed, 5 adopted, 3 verified.

Status moves forward (`src/lessons.rs`, `transition`). Since lab 0.17.0 two moves back are allowed and every other one is refused: `verified → adopted`, when later evidence no longer shows the lesson helps, and a retired lesson reopening at any status. Masters' lesson histories hold only forward moves. A retired lesson keeps its `adopted` and `reviewed` dates as history; no reader uses them outside the active states, and re-adoption resets both. Fixed by research-factory-217.

### 5.2 Loop run

```text
hook fires ─► skip (disabled, background session) | too recent | lock held ─► measure ─► notify ─► record last.json
```

Explicit, with a file lock. An unreadable `last.json` cannot show a recent loop, so the loop runs. Since lab 0.17.0 it says so in `loop.log`, its output, the new record's `errors` and the brief. Fixed by 217.

### 5.3 Signal

| Machine | States | Defect |
|---|---|---|
| Signal kind | fixed list | None |
| Send state | pending (`new`, `repeat`, `reopened`), sent, resolved | A notice still pending when its signal resolves is dropped, since the condition is gone. Since lab 0.17.0 the drop is reported (`unsent` in the loop record, the brief), and a signal never sent that comes back is `new`, not `reopened`. Fixed by 217. |
| Board task for a signal | none, filed, repeated | explicit: `signals.json` keeps the task id that `board-notify.sh` returns as `ref`, and repeats go to that task. Since lab 0.17.0 the loop record carries the `ref` of each signal and of each resolved one, and `lab brief` names them. The task's own status stays on the board (rule 1). Fixed by 217. |

### 5.4 Evaluation

`Verdict` (`src/evaluate.rs:38`): `Collecting`, `Improved`, `Worse`, `Inconclusive`, `Unmeasurable`. Explicit. A lesson moves to `verified` only from an active status (`src/lessons.rs:207`); the verdict informs that move and does not force it.

### 5.5 Check run

| Machine | States | Defect |
|---|---|---|
| Check run status | ok, findings, failed | `ok` means the checker ran to completion. An exit that disagrees with the findings adds a `check-exit-mismatch` warning: exit 1 without an error finding, and since lab 0.17.0 exit 0 with one. clap refuses `--new-only` with `--at`. Fixed by 217. |
| Baseline | none, recorded | explicit |
| Finding severity | Error, Warn, Info | Each rule's default is in docs/interfaces.md "Rule table". A Warn fails no gate, except, since lab 0.21.0, a Warn that `lab check --new-only` finds new against the merge base in a file the change touches (research-factory-226). Accepted as the meaning of Warn; each rule this document depends on is an Error. |
| Stored check run **(implicit)** | stored, committed, superseded and committed; not stored when it repeats the newest stored run | A stored run in `.research/lab/checks/` is evidence, but an agent could delete or edit it, and a superseded run was often left uncommitted. Fixed by 228 (lab 0.20.0): the skill says to commit every run of a session, superseded runs included, with the text it checked. The opt-in PreToolUse hook `skill/research-factory/scripts/guard-check-runs.sh` refuses a tool call that deletes or edits a stored run; masters wires it in `.claude/settings.json`. It guards against mistakes, not intent, and fails open without jq. A check of a head whose run was committed, as after a merge, stored one more run that nobody committed (5 such files in the masters main checkout on 14 November 2026). Fixed by 391 (lab 0.23.0): `lab check` and `lab archive done` store no run that repeats the newest stored run for the same text (both clean, on the same day, commits equal outside the state directory, equal checks, exits, statuses, versions, measures, errors and warnings; info findings may differ) and print `not stored`; see docs/interfaces.md section 7. Since 392 (lab 0.24.0), a linked worktree stores no run whose findings name git-ignored files that the main checkout has and the worktree lacks (row Worktree data). |

### 5.6 Replay and worktree

| Machine | States | Note |
|---|---|---|
| Replay worktree | created, checked, removed | explicit; removed on each exit path |
| Worktree `Location` (`src/worktree.rs:17`) | NotLinked, Bare, Linked | explicit |
| Ignored-file fallback | own checkout, main checkout | explicit since lab 0.9.10; the search index does **not** fall back: each checkout has its own index (section 3.1), but it reads the paper texts that a linked worktree lacks from the main checkout and, since lab 0.25.1 (research-factory-311), says so on stderr |
| Worktree data (masters) **(implicit)** | empty, seeded, pushed back | A new worktree had none of the main checkout's ~54,000 git-ignored files (research-factory-312). masters' `.claude/hooks/worktree-data.py` clones them (APFS copy-on-write) on SessionStart and after EnterWorktree, and pushes files made in the worktree back to the main checkout on Stop. It never copies `private/` or secrets; `.env` templates and files a provenance manifest lists are copied (research-factory-316). The hook's context line `worktree-data: cloned N ...` reaches the agent (verified live by research-factory-325). A worktree that no masters session started in stayed empty: 9 of 21 were on branches older than the hook (masters 69393fa8), whose own `.claude/settings.json` lacks it, and one was made by `git worktree add` from a research-factory session. Its `lab check` stored a run with 162 `source-text-missing` warnings and no `quote-not-found` errors, while main's run of the same text had 2 errors. Fixed in lab by 392 (lab 0.24.0): in a linked worktree, `lab check` stores no run whose findings name git-ignored files that the main checkout has and the worktree lacks, prints why, and exits 2; see docs/interfaces.md section 7. |

### 5.7 Retro candidate

`found ─► shown ─► filed or dismissed`. Today only `found` and `shown` exist; a dismissed candidate returns at the next retro.
Fixed by research-factory-205 together with section 3.4 (stored rejections).

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
| The `task` exists on the board | `lab archive done`, through a configured board export command | 207 |
| Board unreachable | state `skipped`, reported; not an error and not a pass | 207 |
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

### 7.5 Tool version and project

`merged ─► installed ─► required by the project`. A project that needs a lab version states it; install is by git revision of the merge commit.
**(implicit)**: nothing compares the installed version with the one a project needs, except the masters lesson loop (lab ≥ 0.3.0). `lab brief` reports a too-old version under research-factory-207.

## 8. Implicit machines found

| # | Machine | Section | Outcome |
|---|---|---|---|
| 1 | Record status moves | 2.1 | Fixed by 207 |
| 2 | Partial supersession (`amends`) | 2.1 | Fixed by 207, 200 |
| 3 | Numbered id reservation | 2.2 | Fixed by 207 |
| 4 | Follow-up folder metadata | 2.2 | Fixed by 189, 193 |
| 5 | Generated index freshness | 2.4 | Removed by 207, 208 |
| 6 | Record completion | 2.5 | Fixed by 207 |
| 7 | Topic page current or historical | 2.6 | Fixed by 189, 193 |
| 8 | Entry page freshness | 2.6 | Fixed by 193 |
| 9 | Open question | 2.6 | Moved to the board by 208 |
| 10 | Search index and its stages | 3.1 | Built by 187 |
| 11 | Degraded query | 3.3 | Built by 187, 192 |
| 12 | Missed query and rejected alias | 3.4 | Built by 205 |
| 13 | Paper lifecycle | 4.1 | Built by 188, 198, 213, 191 |
| 14 | Stale derivative | 4.1 | Built by 198, 213, 191 |
| 15 | Seen index | 4.3 | Fixed by 188 |
| 16 | Run record placeholder, import pairing | 4.5 | Fixed by 231 (prov 0.1.2); pairing was already explicit |
| 17 | Board task for a signal | 5.3 | Fixed by 217 (lab 0.17.0) |
| 18 | Retro candidate dismissal | 5.7 | Fixed by 205 |
| 19 | Rule lifecycle | 5.8 | Fixed by 232 |
| 20 | Session order | 5.9 | Fixed by 207, 208 |
| 21 | Feedback state | 6 | Proposed in agent-board-454 (note) |
| 22 | Record and task link | 7.1 | Fixed by 189, 207, 208 |
| 23 | Board note to record | 7.2 | Built by 187 (board scope) |
| 24 | Experiment to build | 7.3 | Held by the umbrella task |
| 25 | Tool version needed by a project | 7.5 | Fixed by 207 |
| 26 | Id on the default branch or in another worktree | 2.2 | Fixed by 227, 337; renumber by 336 |
| 27 | Link target tracked, ignored or untracked | 2.5 | Fixed by 228 (warning) |
| 28 | Stored check run | 5.5 | Fixed by 228 (skill rule, opt-in hook) |
| 29 | Worktree data | 5.6 | Fixed by 312, 316 (masters hook); an unseeded worktree's run is refused by 392 (lab 0.24.0) |
| 30 | Experiment call batch | 7.3 | Fixed by 224 and 322 with its sub-tasks 323, 324, 327, 332, 333 |
| 31 | Test-set judgment pin | 7.3 | Fixed by 245, 340, 341 |
| 32 | Board export cache | 3.6 | Built by 345 (lab 0.22.0) |
