Now the code. Starting with mod.rs.
Now chat.rs and board.rs.
Now I'll apply a temporary probe patch (not to be committed) with a Python script.
Now the edits: Request field, execute, refresh helper.
Now index.rs: `Listed`, `From<Opened>`, `open_listed`.
Now alternate before/after runs to control for machine load.
Now the full required checks.
Now the checks.
All three checks pass. Next, the optional after-probe: it should show one notes walk for `--in notes,files`.
Confirmed: one walk instead of two. Now I check the status and commit.
Now I collect the current file:line references for the report.
Now I read the document text that the replacement markdown must follow.
Checks and commit are done. Next, cleanup: I check for leftover processes, then remove the large scratch directories.
I fixed two slow paths in `lab search` and committed the fix as 053e764 (not pushed). Batch topic runs are now about 2.5x to 22x faster, with byte-identical output. A query of both `notes` and `files` now walks the notes once instead of twice. The audit confirmed seven gaps. The most important are that a linked worktree silently loses paper citations, and that parallel `lab retro` decisions overwrite each other. I reported these gaps without fixing them, as the brief asked.
## A. Files read
- **Document:** `docs/state-machines.md` §1, §3 (lines 141-244), §4.1, §7.4 (465-467) and §8 (474-510).
- **Search code:** all of `src/search/*.rs` (board, chat, graph, index, learn, mcp, missed, mod, open, passages, query, refs, similar, sources, thesaurus, topics, trace, units).
- **Other code:** `src/main.rs` lines 557-668 (search dispatch), `src/worktree.rs`, `src/archive/files.rs` (`walk` at 95-142) and `src/config.rs` (registry, 729).
- **Tests:** all of `tests/search_scopes.rs`, plus the test names in `tests/search.rs`, `search_loop.rs`, `mcp.rs`, `open.rs` and `graph.rs`.
## B. Machines
File:line refs are to commit 053e764.
**B1. Search index (explicit, §3.1)**
- **States:** absent, building, fresh, stale, refreshing, rebuilding.
- **Transitions:**
  - `open` is at index.rs:218. `Replaced` makes it retry (235); `Corrupt` makes it force a rebuild (236).
  - `open_current` (263-293) treats a schema or fingerprint mismatch as "build anew".
  - `build` (314-330) waits for the lock and gives up at `lock_timeout_ms`. It writes `index.db.tmp-<pid>` (333).
  - `refresh` (442-550): if `try_lock` fails (491), the query answers stale. The budget is checked at 513 and the stop message is at 544.
- **Gaps:**
  1. **Confirmed (probe), S, p2:** killing a build leaves `index.db.tmp-<pid>` on disk (15 MB at the kill, up to about 118 MB). The next query does not remove it. The document's "the partial file is removed" holds only for errors. Fix: remove `index.db.tmp-*` while holding the lock in `build`.
  2. **Confirmed (code and MCP output), S, p2, document drift:**
     - The stage state `failed` never occurs. `Stage::state` is constant (mod.rs:266-269): lexical and graph are "ready", dense and rerank are "missing". MCP prints "state: hits; missing stages: dense, rerank".
     - The "integrity check" does not exist. A rebuild is triggered only by the SQLite `DatabaseCorrupt` or `NotADatabase` error codes (index.rs:107).
     - The "vectors keyed by content hash" sentence describes nothing in the code.
  3. **By design, no test, S, p3:** the refresh-budget stop at index.rs:544 is covered by no test. `lab search index` and `refresh` run without a budget (`budgeted: false`, mod.rs:787 and 807).
  4. **Confirmed (code), p3:** `open_current` maps every error that is not "busy" to a silent rebuild (index.rs:291). This includes I/O errors, so their cause is lost.
- **Existing tests that cover this machine** (tests/search.rs):
  - `another_schema_version_and_a_corrupt_file_rebuild_the_index`
  - `a_stale_index_answers_from_its_last_state_and_says_so`
  - `a_changed_and_a_deleted_file_show_in_the_next_query`
- **Lock timeout:** `--rebuild` with the index lock held exited 2 after 30.2 s with "lock timeout". So the bounded wait works.
**B2. Unit (explicit, §3.2)**
- The `embedded` state is gone (drift: no dense stage exists).
- The old-anchor text is accurate. It is covered by these tests in tests/open.rs:
  - `an_anchor_finds_its_text_across_two_edits_and_old_versions_are_bounded`
  - `a_rebuild_over_a_corrupt_index_starts_without_old_anchors_and_says_so`
**B3. Query (explicit, §3.3)**
- The terminal states are at mod.rs:500-503.
- `execute` (883) collects notices for missing stages (969). Exit 2 comes from `Err` (647).
- Covered by tests/search.rs `a_configured_stage_that_is_missing_makes_the_answer_partial` and tests/search_loop.rs `two_queries_append_two_valid_trace_lines`.
- **Gap, confirmed (probe), S, p3:** the MCP tools `archive_open` and `archive_related` (mcp.rs:204-217) report only `stale`. They drop `opened.notices`, so a partial answer does not say why.
- **Drift:** 0.25.1 added a "main checkout" info line, printed only by `lab search`.
**B4. Missed query and thesaurus (explicit, §3.4)**
- The states hold.
- Covered by tests/search_loop.rs: `a_rejected_proposal_never_returns`, `an_unused_entry_is_listed_and_retire_stops_it`, `accept_adds_the_alias_to_an_open_record`.
- **Gap, confirmed (`probe/race`), S, p2:** 20 parallel `lab retro dismiss tool:ruleN` stored only 11 entries. `Decisions::load` (learn.rs:81) and `save` (106) read, change and write the file with no lock. Fix: take a lock file around load-change-save.
- **Unverified (code reading only):** `thesaurus::append` (100) and `retire` (123) use the same pattern. I did not probe them.
**B5. Board export cache (explicit, §3.6)**
- `start` is at board.rs:137 and its 60 s retry guard at 138. `refresh` is at 197 and the 120 s export limit at 227. `list_board` is at chat.rs:224.
- Covered by tests/search_scopes.rs:
  - `the_board_is_searched_through_its_cached_export_and_without_one_is_partial`
  - `a_stale_cache_answers_at_once_and_starts_one_refresh`
  - `a_failed_refresh_keeps_the_old_cache_and_says_why`
- **Gaps:**
  1. **Confirmed (`probe/lock_wait.py`), S, p3:** `--refresh-board` waits for `board.lock` with no time limit (board.rs:218). With the lock held, it was still waiting after 15 s and I killed it.
  2. **Confirmed (probe), S, p3:** a corrupt `board.json` is reported as "no board export is cached yet". `read` returns `None` for any error (board.rs:82-93). It heals at the next refresh.
  3. **No test:** the 120 s export timeout.
**B6. Paper text across worktrees (implicit, joint with §7.4)**
- **Confirmed (`probe/zz_probe_search.rs`, log `zz_probe.log`), M, p2:** in a linked worktree, search reads ignored paper texts from the main checkout. Two other paths do not:
  - `lab related ref:2` silently loses "cited by papers (1): ref:1". `graph::paper_texts` and `read_papers` (graph.rs:359-420) read sidecars under `project.root` with no fallback to the main checkout.
  - `lab open sources/texts/1.txt` exits 2 with "No such file or directory", although search in the same worktree returns that hit. The cause is open.rs:218, plus 165 for body lines.
- **Confirmed (probe), S, p2:** without a `[papers]` table, refs from the default `references.jsonl` are indexed but get no paper edges. `graph::registry_path` (329-334) reads only `[papers] registry` and ignores `project.registry`. The graph stage still says "ready".
## C. Hot paths
1. **`lab search --run`, 20 topics (`probe/topics.tsv`), fixed:**
   - Measured wall time:
     | | Run 1 | Run 2 | Run 3 |
     |---|---|---|---|
     | Before | 10.25 s | 19.16 s | 25.91 s |
     | After | 1.17 s | 4.13 s | 1.37 s |
     Two earlier runs before the fix took 31.7 s and 39.6 s.
   - `cmp` shows the run files are byte-identical. Log: `probe/topics-timing.log`.
   - **Fix:**
     - `refresh_batch` (mod.rs:801) refreshes once per batch.
     - Each query uses `index::open_listed` (index.rs:145-175), which reuses that refresh's stale flag and notices (mod.rs:921).
     - topics.rs:123 calls it.
   - **Test:** `topics::tests::a_batch_lists_the_sources_once_and_carries_what_its_refresh_said`. It fails with the fix disabled.
2. **`--in notes,files` walked the notes twice, fixed:**
   - `sources::list` (sources.rs:217) now lists all scopes at once. `files(ctx, notes)` (289) reuses the notes listing.
   - **Proof:** with `LAB_PROBE=1`, the old binary logs 2 notes walks and the new one logs 1. Each walk takes 285-300 ms, and 1.4-1.9 s when the machine is loaded.
   - Wall time is noisy, because other tracks loaded the machine:
     | | Run 1 | Run 2 | Run 3 | Run 4 |
     |---|---|---|---|---|
     | Before | 3.36 s | 1.07 s | 1.66 s | 2.52 s |
     | After | 0.75 s | 0.75 s | 3.64 s | 1.22 s |
   - The hits are the same. Log: `probe/files-timing.log`.
   - **Test:** `sources::tests::notes_listed_once_serve_both_scopes_in_any_order`. It fails when the notes slot is left empty.
3. **Notes walk, not fixed (needs an owner decision), M:**
   - With `default_notes` (mod.rs:316), the walk goes through ignored data folders under `docs/`: about 69k entries.
   - It costs 0.3-1.5 s of every query. Each `lab mcp` call took 944-1640 ms (`probe/mcp-timing.log`).
   - All 2,042 indexed notes are tracked by git. `check-ignore` found 0 ignored notes, and there are 0 untracked `.md` files.
   - **Proposal:** list notes from git (tracked plus untracked files that are not ignored), or skip ignored folders in `archive::files::walk`. Either way, ignored notes would drop out of search.
4. **Not worth fixing:** the registry is read 3 times per query, but this costs only about 5 ms. In a warm default query, `refs` takes 15-40 ms, `texts` about 5 ms, `board` about 7 ms and search 17-70 ms.
## D. Other points
- **Information silently lost:**
  - `read_papers` skips unreadable sidecars without a notice.
  - `entry["key"].as_str()` skips numeric registry keys (graph.rs:374).
  - `similar::new_files` treats a git error as "no files" (similar.rs:160-176).
  - The `files` scope does not carry the notes listing's notices (sources.rs:289). A query that also searches `notes` still shows them.
- **Waits with no time limit:** only the board foreground lock (B5 gap 1).
- **States with no test:**
  - the refresh-budget stop (index.rs:544);
  - the 120 s board export timeout;
  - an interrupted build leaving its temporary file.
## E. Owner decisions
1. Whether to stop the notes walk at ignored folders. This drops ignored notes and saves 0.3-1.5 s per query and per MCP call (C3). I recommend listing notes from git.
2. Whether the graph stage should report `partial` when it cannot find paper edges (B6). I recommend yes.
3. Whether to drop `failed` and vectors from the stage model, or keep them as planned work. I recommend dropping them from §3.1 now.
## F. Replacement document text
````markdown
## 3. Search machines (lab)
### 3.1 Search index
One index per checkout, at `<state_dir>/lab/search/index.db`, with the write lock `lock` beside it. Owner: research-factory-187. Code: `src/search/index.rs` (lab 0.25.1).
```text
absent ─► building ─► fresh ─► stale ─► refreshing ─► fresh
schema or fingerprint mismatch, unreadable meta ─► building
SQLite reports corrupt or not a database ─► building (forced)
locked: another process holds the write lock; the query answers from the last committed state, marked stale
```
| Transition | Trigger | Guard | Enforced by |
|---|---|---|---|
| absent → building | first `lab search`, `lab search index` or a scope set never built | the write lock; the wait stops at `lock_timeout_ms` (30 s) with exit 2 | `build` index.rs:314-330 |
| building → fresh | the build writes `index.db.tmp-<pid>` and renames it over the index | the old file is never edited in place | index.rs:333 |
| fresh → stale | a source's stamp differs from the stored one | checked at each query | `refresh` index.rs:442 |
| stale → refreshing | the query itself (incremental) | only changed sources are read; the write lock must be free, or the query answers stale (index.rs:491) | `refresh` |
| refreshing → fresh, or stale with notice | the refresh ends, or reaches `refresh_budget_ms` (5 s) | `lab search index` and `refresh` have no budget (mod.rs:787) | index.rs:513, 544 |
| any → building | stored schema version (`6`, index.rs:24) or fingerprint differs; the meta table does not read | any other open error than busy also rebuilds, and its cause is not reported | `open_current` index.rs:263-293 |
| any → building (forced) | SQLite returns `DatabaseCorrupt` or `NotADatabase` | there is no `PRAGMA integrity_check` | `open` index.rs:107, 236 |
A batch of queries (`lab search --run`) refreshes once and reuses that refresh's stale flag and notices for each query (`refresh_batch` mod.rs:801, `open_listed` index.rs:165; research-factory-414).
Stages: `lexical`, `dense`, `rerank`, `graph`. `lexical` and `graph` are `ready`; `dense` and `rerank` are `missing` (mod.rs:266-269). No stage is ever `failed`. A query with a configured missing stage is `partial`, and the output and the trace name the stage (mod.rs:969).
Gaps:
- Confirmed: a killed build leaves `index.db.tmp-<pid>` (up to about 118 MB on masters). The next query does not remove it. An error in the build does remove it.
- Confirmed: the graph stage is `ready` even when it finds no paper edges (3.7).
- No test: the stop at the refresh budget (index.rs:544).
### 3.2 Unit
A unit is one searchable passage with an anchor (`path#Lstart-Lend` or `ref:<key>`).
```text
discovered ─► parsed ─► indexed (lexical) ─► live
                                              │
                   source changed ─► replaced │ source deleted ─► removed
```
[Keep the three paragraphs that start "A unit of a superseded", "A unit never outlives" and "A replaced unit leaves its anchor" unchanged.]
### 3.3 Query
```text
received ─► parsed ─► routed ─► retrieved ─► fused ─► reranked ─► grouped ─► answered
```
[Keep the terminal-state table, and the paragraphs "Each query writes", "Since lab 0.19.0", "Scopes share" and "`zero` and `partial`", unchanged.]
Since lab 0.25.1 (research-factory-311), `lab search` prints one info line when paper texts come from the main checkout. The line is not a partial notice.
A query of `notes` and `files` walks the notes once; `files` reuses the notes listing (`sources::list` sources.rs:217; research-factory-414).
Gap, confirmed: the MCP tools `archive_open` and `archive_related` report only a stale index. They drop the other notices (mcp.rs:204-217).
### 3.4 Missed query and thesaurus entry
[Keep the diagram and the first two paragraphs unchanged.]
Decisions persist in `<state_dir>/lab/retro.json` (learn.rs:27).
Gap, confirmed: `load` (learn.rs:81) and `save` (learn.rs:106) take no lock. 20 parallel `lab retro dismiss` calls stored 11 decisions; the others were lost. `thesaurus::append` and `retire` (thesaurus.rs:100, 123) use the same pattern; not probed.
### 3.5 Model loading in `lab mcp` (retired)
[Unchanged.]
### 3.6 Board export cache
[Keep the text and table, but change the code refs: `board::read` board.rs:82, `board::start` board.rs:137, `board::refresh` board.rs:197, `list_board` chat.rs:224.]
Gaps:
- Confirmed: `lab search --refresh-board` waits for `board.lock` with no time limit (board.rs:218). A held lock kept it waiting until it was killed.
- Confirmed: a `board.json` that does not parse counts as absent, and the query says "no board export is cached yet" (board.rs:82-93). The next refresh replaces it.
- No test: the 120 s export timeout (board.rs:29).
### 3.7 Paper texts in a linked worktree
Search reads ignored paper texts from the main checkout (research-factory-311). Two readers do not:
- `graph::paper_texts` and `read_papers` (graph.rs:359-420) read sidecars under the worktree only. `lab related` loses "cited by papers" without a notice. Confirmed.
- `lab open` (open.rs:218) exits 2 for a text that search returned. Confirmed.
`graph::registry_path` (graph.rs:329) reads only `[papers] registry`. Without that table, refs are indexed from the default registry, but the graph has no paper edges. Confirmed.
````
**§7.4:** append this sentence: "A linked worktree breaks this joint for `lab related` and `lab open`; see 3.7."
**§8 rows:**
```markdown
| 10 | Search index and its stages | 3.1 | Built by 187; gaps (temporary file after a kill, `failed` stage never occurs) found by 414 |
| 11 | Degraded query | 3.3 | Built by 187, 192; MCP notices gap found by 414 |
| 12 | Missed query and rejected alias | 3.4 | Built by 205; lost concurrent decisions found by 414 |
| 32 | Board export cache | 3.6 | Built by 345 (lab 0.22.0); unbounded foreground lock wait found by 414 |
| 33 | Paper texts in a linked worktree | 3.7 | Gaps found by 414 |
```
Row 23 does not change. The document's "lab 0.22.0" version line should become 0.25.1.
## G. Worktree, commit and checks
- **Worktree:** `/Users/sunyizhe/Developer/research-factory/.claude/worktrees/agent-a9a53306c236c4f37`
- **Branch:** `worktree-agent-a9a53306c236c4f37`, commit **053e764** (base 133b118). Not pushed, no PR.
- **Files changed (6):** `src/search/{index,mcp,mod,similar,sources,topics}.rs`, +208/−26 lines.
- **Checks**, run with `CARGO_TARGET_DIR` in scratch:
  - `cargo fmt --check`: exit 0.
  - `cargo clippy --all-targets -- -D warnings`: exit 0.
  - `cargo test`: exit 0, all passed (189 unit tests plus the integration suites).
- **Logs and probe scripts:** `/private/tmp/claude-501/-Users-sunyizhe--t3-worktrees-research-factory-task-research-factory-4-1c1fdf969a284ada/1fb4fe71-1498-46b7-8a1e-3b6da0f588f3/scratchpad/lab-search/probe/`
- **Cleanup:** I removed the scratch target folder, the masters clone (10 GB) and the probe binaries. No lab process is still running. Masters was only read; the commands that write ran on a clone in scratch.
## Work for the parent to file
Each gap above is a proposal:
- the worktree graph and `open` fallback (M, p2);
- `registry_path` ignoring `project.registry` (S, p2);
- removing `index.db.tmp-*` under the lock (S, p2);
- a lock around the retro decisions file, and checking the thesaurus files for the same problem (S, p2);
- the document rewrite from section F (S, p2);
- a time limit on the board foreground lock wait (S, p3);
- a corrupt `board.json` reported as absent (S, p3);
- MCP dropping notices (S, p3);
- tests for the refresh budget and the board timeout (S, p3);
- the notes walk (M, needs owner decision E1).
Release note: the `Cargo.toml` and `Cargo.lock` versions were not bumped, because the brief said not to. The release needs a version bump.
## Setup friction (feedback)
- The worktree isolation hook refused `git clone … | tail; echo` and a heredoc combined with `cd` and `uv run`. Workaround: plain single commands, and scripts written with the Write tool.
- Hook warnings "t3:2a2e edited src/search/… re-read before editing" named another checkout's edits, not this worktree. I ignored them.
## Open questions
- Owner decisions E1-E3.
