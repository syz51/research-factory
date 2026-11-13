I'll start by reading the shared brief, then the two tool repositories.
The brief defines four items and a report shape. Next I'll set up scratch and check the two repositories' HEADs and sizes.
Both repositories are clean at the stated HEADs. Next I'll find the document sections the brief names.
The §4.4, §4.5 and §8 text is clear. Now I'll read claimcheck's manifest and entry point.
Cargo.toml says claimcheck 0.10.0, while the document cites 0.11.0 and 0.12.0. I'll check that against tags later. Next, lib.rs and the source loader.
The source loader indexes each text at most once per run through `OnceLock`. Next I'll read the config and registry loaders.
The registry reader parses each JSONL line twice, once as `Value` and again as the typed struct. That goes on the hot-path list. Next, the config loader.
Config validation is strict and happens once at load. Now the core: check.rs, the largest source file, read in chunks.
`Project::new` keeps only the first `sha256` when two registry entries name the same text path with different hashes. That is a candidate gap. Continuing through check.rs.
`check_numbers` calls `nearby` twice for each unmatched number, once in `near_number` and again in `nearby_numbers`. I'll note it as a candidate and keep reading.
The `required` map at check.rs:1190-ish goes into JSON from a `HashMap`, so key order could vary between runs. I'll verify with a probe later. Continuing.
`check_quote` runs the approximate matcher over every cited text whenever an exact match fails. I'll read matcher.rs and the number and normalization modules next.
The matcher allocates four DP rows per `align` call, and the vote loop can touch up to 20,000 postings for each common trigram. Both are candidates for the hot-path measurements. Next, finding.rs and the normalizer.
Finding serialization is straightforward. The `HashMap` at check.rs:1245 becomes a `serde_json::Map`, which is a `BTreeMap` unless `preserve_order` is on, so I need to check Cargo.lock for that feature. Next, normalize.rs.
Next is doc.rs, the Markdown parser. I'll read it in two chunks.
doc.rs is clear. An unclosed curly quote is silently dropped, which I'll probe. Next I'll map numbers.rs by its function signatures, then read the parts that matter.
I've read the claimcheck source files and still need the git history and tags. I'll check them to test the document's version claims.
The claimcheck repo has no tags and tops out at 0.10.0, while the document cites 0.11.0 and 0.12.0. I'll find when the `source-text-changed` rule was added and check the installed binary's version.
The installed `claimcheck` reports 0.12.0, but the repo at HEAD 65d0aa9 is 0.10.0. The brief asks for a build at HEAD, so the measurements should use that build. I'll check whether the repo has other branches or refs that explain the gap.
Local `main` in claimcheck is behind `origin/main` (344c307), and the installed binary matches neither. Prov has the same kind of gap: Cargo.toml says 0.1.1 and the installed binary says 0.1.5. I'll compare these refs and the version history.
The local checkouts are behind their remotes in both repos. I'll check how far origin/main has moved and which files changed, so I can say which verdicts might already be fixed upstream.
The local checkouts at HEAD are behind origin/main in both repos. The document's claims (claimcheck 0.11.0 fix, prov PR #4) appear to refer to origin/main commits. So I'll verify each claim at both HEAD and origin/main. I'll set up clones in scratch so no build writes into the source repositories, then check the toolchain and masters config.
The clones of the origin refs landed on the local `main` commits, because a local clone only copies `refs/heads`. I'll fetch the remote-tracking refs from the source repos, read-only, and check out those commits.
Masters' `[claimcheck]` config scans `docs/research` against `docs/research/references.jsonl`, with several exclusions. Next I'll start release builds of all four trees in the background, each with its own target directory.
While the builds run, I'll read prov's source, starting with its entry point and library layout.
The core is `execute` (run.rs:855-1185) plus the record types and the signal guard. I'll read the record types first.
The `RunRecord` struct at HEAD has no `status` field. I'll read the placeholder writer and the run-start code to see what HEAD writes for the document's `incomplete` state.
HEAD writes a placeholder with `"status": "incomplete"`, but its filled record type has no `status` field. I'll read `execute` in two chunks to see when the placeholder is written and when it is replaced.
The placeholder is written before the child starts, and `execute` runs several process spawns before that: `uv --version`, `git --version`, and `git_state`. The stream threads are joined with no timeout after `child.wait()`, which I'll probe for a background grandchild. Reading the rest of `execute`.
`execute` writes the final record to a `.partial` file and renames it. Next I'll read `read_record`, the snapshot code, and the verifier, which decide how a placeholder is read back.
Records with a `status` of `incomplete` are rejected, and filled HEAD records have no `status` field, so they pass. I'll read the snapshot types and the directory hashing path, because `execute` hashes inputs twice.
Parent says both main checkouts moved, so I'll re-audit the new HEADs and re-check my old-HEAD results first.
**Result.** At prov 8743e9d, the old gap where the placeholder was written before the interrupt guard is refuted. Commit 9ea76bb fixed it. Three prov gaps remain: an unbounded wait for output pipes, incomplete records with no exit, and sequential preflight spawns. At claimcheck 0eae562, the §4.4 changed-text claim is true. It was false only at 65d0aa9 (0.10.0), which predates the fix bc7c914 (0.11.0). Three claimcheck gaps are confirmed: first sha wins, unclosed quotes are dropped, and one unreadable file aborts the run.

**Commit basis.** Every result is from claimcheck 0eae562 (0.13.1) or prov 8743e9d (0.1.5), unless marked. Old results are marked 65d0aa9 (0.10.0), 344c307 (0.13.0) or 45bd0b1 (0.1.1). `file:line` refs are in `/Users/sunyizhe/Developer/claimcheck` and `/Users/sunyizhe/Developer/prov` at those commits. Both main checkouts are clean.

## A. Files read

**prov, 8743e9d.** Only `run.rs`, `tests/cli.rs`, `README.md` and the Cargo files changed since 45bd0b1.
- `src/run.rs` (2419 lines): read about 40%, covering the record lifecycle, interrupt guard, pipe wait, `read_record`, and the `verify`/`rerun` entry points. NOT read: env selection, `program_info`, the `rerun` and `verify` bodies, output formatting.
- `src/manifest.rs` (1047): read 34–66, 204–222, 300–335, 389–440. NOT read: build walk, verify body, diff.
- `src/import.rs` (636): read 81–100, 415–460. NOT read: parse, resolve, import body 259–415 and 460–508, tests.
- `src/lineage.rs` (248): read 30–70. NOT read: 70–248.
- `src/main.rs`: read in full earlier; re-read 75–110, 240, 313–314.
- `src/config.rs`, `src/hash.rs`: read in full earlier; unchanged in the range.
- `src/walk.rs` (290): read 143–252 earlier. NOT read: 1–142, 253–290.
- `src/compare.rs`, `src/dupes.rs`, `src/finding.rs`: NOT read. Grep found no run-state role.
- `tests/cli.rs`: read 1383–1410 and 1834–1864; grepped test names.

**claimcheck, 0eae562.**
- `src/check.rs` (about 2000 lines): read 31–44, 105–116, 167–197, 758–812, 921–943, 1066–1090, 1140–1162. NOT read: citations, doc walk, `check_depth`, `reference_entry`, `check_document`.
- `src/source.rs`: read 15–45, 128–145, 248–310 in part, 363–403. NOT read: normalisation and index internals.
- `src/matcher.rs`: read signatures and `best_approx` start.
- `src/registry.rs`: read 123–140. `src/config.rs`: read 36–44. `src/doc.rs`: read 642–650. `src/finding.rs`: read 56–78. `src/lib.rs`: read 178.
- `tests/rules.rs`: read 681–716, plus parts of 2011–2150. `tests/cli.rs` and `src/main.rs`: NOT read this session.

## B. Machines

Explicit machines from the document are checked first. Implicit machines follow.

**B1. Run record lifecycle (doc §4.5 "Run record"), prov 8743e9d. Explicit.**
- States: none; placeholder (`status: incomplete`); complete record; removed; placeholder left after a kill.
- Transitions:
  - none to placeholder: `run.rs:1179` (`create_new`), `run.rs:1190` (write).
  - placeholder to removed: `abandon`, `run.rs:1039–1066`. Tests `a_signal_before_the_command_starts_stops_it_and_leaves_no_record` (`tests/cli.rs:1749`) and `a_failed_placeholder_write_leaves_no_record` (`:1761`).
  - placeholder to complete: `run.rs:1379` (status), `run.rs:1415–1421` (`.partial`, then rename).
  - placeholder left after kill: no exit. Test `killed_run_leaves_an_incomplete_marker` (`tests/cli.rs:1834`).

| Gap | Verdict | Evidence | Proposed change | Size | Priority |
|---|---|---|---|---|---|
| Placeholder written before the interrupt guard (doc has no row) | Refuted at 8743e9d. Confirmed at 45bd0b1. | Guard created at `run.rs:1176`, before the placeholder at `:1179`. Debug probe: SIGINT while paused leaves no files, exit 130 (`probes/prov/n8743/k-pause-int2`). | None. | — | — |
| Kill between create and write leaves a 0-byte `.json` | Confirmed (debug build with `PROV_TEST_PAUSE_UNTIL`, `run.rs:1185`). The window is microseconds in release. | `probes/prov/n8743/k-pause-kill`: file bytes=0. `lineage` skips it: "invalid JSON: EOF", exit 0. No test. | Write the placeholder to `.partial`, then rename. | S | p3 |
| Incomplete record has no exit | Confirmed. | `kill.sh` SIGKILL: prov exit 137, placeholder 203 bytes. `run verify` exit 2. `rerun` exit 2 (`probes/prov/n8743/rerun-incomplete.err`). `lineage` exit 0 with "prov: skipped". SIGTERM gives the same (exit 143). Only manual delete works. | Add a cleanup command for placeholders and their `.stdout`/`.stderr` files. | S–M | p3 |
| Records without `status` read as complete | By design. | `run.rs:351` (`#[serde(default)]`), `RecordStatus` default at `run.rs:336–356`. Test `run_record_names_its_status_and_old_records_still_load` (`tests/cli.rs:1927`). Masters 0.1.1 record reads as complete (`lineage` exit 0). | None. | — | — |

**B2. Interrupt guard (implicit; type `CatchInterrupts`), prov 8743e9d. New to the doc.**
- States: not installed; installed (catching SIGINT and SIGQUIT); restored; signal pending.
- Transitions: `new` (`run.rs:948`); `restore_in` `pre_exec` (`:994–1020`, returns EINTR if a signal is pending); `Drop` (`:1028`); `abandon` raises the noted signal (`:1039–1066`).
- Verdict: by design. A signal that was ignored at start stays ignored (doc comment `run.rs:918–942`). A non-interactive `&` child inherits SIGINT as ignored, so the probe needed a `perl` default-disposition wrapper.
- Tests: `interrupts_reach_the_command_and_the_record_is_still_written` (`tests/cli.rs:1596`), `the_command_starts_with_the_interrupt_dispositions_prov_started_with` (`:1620`).
- Gap: SIGTERM and SIGHUP are not caught. By design; the record stays incomplete.

**B3. Command process and output pipes, prov 8743e9d. Implicit.**
- States: spawned; waiting for exit; draining stdout and stderr; finished.
- Transitions: spawn at `run.rs:1220–1230`; `child.wait()` at `:1255`; `t_out.join()` at `:1259`; `t_err.join()` at `:1264`. `tee` loops until read returns 0 (`:814`).

| Gap | Verdict | Evidence | Proposed change | Size | Priority |
|---|---|---|---|---|---|
| Drain waits for EOF with no time limit. A background grandchild holds the pipe open. | Confirmed at 8743e9d (5.03 s at 45bd0b1). | `prov run -- sh -c '(sleep 5 &); echo hi'`: wall 5.10 s. Control with the grandchild's output redirected: 0.40 s. No test covers a grandchild (grep of `tests/cli.rs`). | Time-bound the drain after exit. On timeout, record a `streams-open` finding. | S–M | p2 |

**B4. Preflight tool probes (implicit), prov 8743e9d.**
- Git state: `git_state` at `run.rs:687–704`. Spawns: `git --version`, `rev-parse --show-toplevel`, `rev-parse --verify`, `symbolic-ref`, `status`.
- Tools: `uv --version` (`run.rs:1141`), `git --version` (`:1145`), optional `uv run --no-project python --version` (`:1152`).
- Gap: all are sequential, and `git --version` runs twice (`:689` and `:1145`).
- Shim log (`probes/prov/shim/log`): 7 spawns by default, 8 with `--python`. Same at 45bd0b1 and 8743e9d.
- Verdict: confirmed. Priority p3. Details in C1.

**B5. Import pairing (doc §4.5 "Import pairing"), prov 8743e9d. Explicit.**
- Gap: an object with a path but no string hash yields no finding (`src/import.rs:90–92` returns empty). A hash with no path gives `hash-without-path` or `record-without-path` (`import.rs:428`, `:446`).
- Probe `probes/prov/n8743/imp`: 5 objects. `b.txt` (no hash) and `d.txt` (hash 123) vanish. Output shows only "2 records" and one `hash-without-path`.
- Test `import_explicit_fields_report_entries_without_paths` (`tests/cli.rs:1383`) has no path-only entry.
- Verdict: confirmed (doc drift too). Fix: report path-only entries as `record-without-hash`. Size S. Priority p3.

**B6. Manifest completeness (doc §4.5 "Manifest"), prov 8743e9d. Explicit.**
- Flag: `header.complete` (`manifest.rs:39–40` serde default true; `:61`). Imported manifests set it false (`import.rs:488`).
- A complete manifest without a summary is `manifest-truncated` (`manifest.rs:317–327`).
- Verdict: matches the doc. Doc detail is missing.

**B7. claimcheck quote verdict (doc §4.4 "Quote verdict"), 0eae562.**
- States: exact (no finding); approximate with similarity ≥ `error_below` (`config.rs:41`, default 0.8): `quote-not-found`, warn (`check.rs:921–923`); below 0.8: `quote-not-found`, error (`check.rs:934–936`); changed text: not counted as supported (`check.rs:789`); kept: info (`rules.rs:2011`).

| Gap | Verdict | Evidence | Proposed change | Size | Priority |
|---|---|---|---|---|---|
| Unclosed quotation is dropped with no finding (`doc.rs:643`). | Confirmed at 0eae562. | Probe `cc-unclosed`: control gives 1 finding; unclosed gives 0. | Report an info or warn `unclosed-quotation` at the opener line. | S | p2 |

**B8. claimcheck source text (doc §4.4 "Source text load"), 0eae562.**
- States: loaded; hash mismatch (`source.rs:382–393`, `changed()` at `:401`); missing (`check.rs:116`, `source-text-missing`, warn).
- Transitions: `check.rs:144–154` (`source-text-changed`, warn); the quote gate at `check.rs:789`; `TextEntry::get` at `source.rs:374`, cached in `OnceLock`.
- Test: `hash_mismatch_makes_matches_unverified` (`tests/rules.rs:681–716`, renamed from `hash_mismatch_is_reported_once`). It expects two `source-text-changed` warns, `quotes_supported` 0 and `numbers_found` 0.

| Gap | Verdict | Evidence | Proposed change | Size | Priority |
|---|---|---|---|---|---|
| First sha wins for a text path shared by two references (`check.rs:40`). A later reference's wrong sha is never checked. | Confirmed at 0eae562. | Probe `cc-dup`: 0 findings where reference 2's recorded sha is wrong. No test pins a conflict. | Error on conflicting shas for one path (see E4). | S | p2 |
| Unreadable file (invalid UTF-8) aborts the whole run. | Confirmed at 0eae562. | Probe `cc-badutf8`: exit 2, stdout empty. `good.md` alone gives its finding. `lib.rs:178` (`run`). `unreadable_text_is_missing_not_supported` (`rules.rs:276`) covers texts, not checked files. | Report a per-file error finding and keep checking. | M | p2 |

**B9. claimcheck number verdict (doc §4.4 "Number verdict"), 0eae562.**
- States: found; near (rounded or context); not found (`number-not-in-source`, warn); changed text only (`source-text-changed`).
- Transitions: exact at `check.rs:1077–1088`; near at `:1104`; evidence pass at `:1156`.
- Gap: a number not found in a text is searched twice (C2).

**B10. claimcheck kept quotes (implicit), 0eae562.**
- States: kept (info); stale (`stale-kept-quote`, warn, `check.rs:170–197`); dropped (`off`).
- Tests: `kept_quote_is_info_with_its_reason` (`rules.rs:2011`), `unused_kept_quote_in_a_checked_file_is_stale` (`:2058`), `kept_quote_entries_are_validated` (`:2087`), `off_drops_a_kept_quote_without_making_it_stale` (`:2147`).
- Verdict: all states have tests. No gap.

**B11. claimcheck text index (implicit cache), 0eae562.**
- Built once per text on first use (`source.rs:42`, `OnceLock` in `TextEntry`). Builds `grams` (`:29`, `:62`), `words` (`:38`, `:101`), `norm`, numbers.
- Staleness: a text is re-keyed by its sha on each run (`hash_mismatch`). No cache is kept across runs.
- Verdict: the build is eager for every field. It is a hot path (C1).

**B12. Document drift in `docs/state-machines.md` (`/Users/sunyizhe/Developer/research-factory/docs/state-machines.md`).**
- Line 5: "claimcheck 0.12.0, prov 0.1.3" is now claimcheck 0.13.1 and prov 0.1.5.
- Line 315 (§4.4 Quote verdict): "verified (score ≥ 0.8), weak, not found" is wrong. Exact matches give no finding. Similarity ≥ 0.8 gives a warn (`check.rs:921`). Below 0.8 gives an error (`check.rs:934`). "weak" is not a state in code.
- Line 317 (§4.4 Source text load): accurate. Missing the first-sha gap (B8).
- Line 322 (§4.5 Manifest): accurate. Missing the summary rule and imported-manifest detail.
- Line 324 (§4.5 Run record): accurate at 8743e9d. The placeholder has said `incomplete` since 0.1.0 (commit 46dbf44).
- Line 326 (§4.5 Import pairing): wrong for path-only entries (B5).
- Line 329: "a8f3c596" is the 8-character form of commit a8f3c59. Correct.
- Line 493 (§8 row 16): "Fixed by 231 (prov 0.1.2)" overstates. 231 added the `complete` state in 0.1.2 (commit a8f3c59). The placeholder was already explicit.
- Line 334 (§7): depends on `lab check --new-only`, which this track did not check.

**Cross-track (lab).** `lab check --new-only` runs every check again at the merge base (`/Users/sunyizhe/Developer/research-factory/src/change.rs:117`, `replay::run_base`). Claimcheck work doubles. Found by reading at the current checkout. Not measured.

## C. Hot paths

**C1. Preflight spawns (`prov/src/run.rs:1133–1152`), prov.**
- Before (8743e9d): 7 spawns by default, 8 with `--python`. Sequential.
- Method: 40 interleaved runs per binary (`probes/prov/n8743/ab.sh`), wall-clock medians. Machine load 13–22. No `/usr/bin/time`.

| Binary | Default, batch 1 | Default, batch 2 | `--python` |
|---|---|---|---|
| 45bd0b1 (0.1.1) | 53.0 ms | 46.7 ms | 58.7 ms |
| 8743e9d (0.1.5) | 49.9 ms | 53.6 ms | 57.3 ms |
| 8743e9d + concurrent preflight (scratch) | 40.9 ms | 36.9 ms | 36.2 ms |

- Fix: run git state, `uv --version`, `git --version` and the python probe in one `std::thread::scope`. Also remove the duplicate `git --version` (`run.rs:689`).
- Test: the scratch prototype passes 25 unit and 47 CLI tests (`/private/tmp/claude-501/-Users-sunyizhe--t3-worktrees-research-factory-task-research-factory-4-1c1fdf969a284ada/1fb4fe71-1498-46b7-8a1e-3b6da0f588f3/scratchpad/claimcheck-prov/test-prov-proto2.log`). Not committed.
- Priority p3.

**C2. Duplicate nearby pass (`claimcheck/src/check.rs:1078` and `:1156`), 0eae562.**
- Before: masters full scan, instrumented copy (`src/cc-cnt`): `nearby` 140 calls = 73 from `near_number` + 67 from `nearby_numbers`. `find_exact` 32. `align` 8. 176 findings lines. Wall 0.35 s, one run.
- Same counts at 65d0aa9 (`nearby` 140, `find_exact` 32, `align` 8).
- Up to 67 calls repeat work for numbers not found. Time share not measured.
- Fix: compute `nearby` once per text and context, then reuse it. Size S. Test: `rules.rs` number tests. Priority p3.

**C3. Eager index build (`source.rs:42–120`), 0eae562.**
- Before (0eae562, masters full scan): instructions 11.27–11.31 G; cycles 3.35–3.40 G; RSS about 385 MB; wall 0.33–0.5 s (noisy). Same at 65d0aa9.
- Breakdown (instrumented 65d0aa9 only): 103 texts, 7.5 MB; index CPU 1.4–1.7 s summed over threads. `grams` about 45%, `words` about 18%, `norm` about 16%.
- Fix (scratch `cc-newproto`, on 0eae562): build `grams` and `words` lazily.
- After: 8.02 G instructions (−29%), 2.4 G cycles (−28%), RSS 291 MB (−24%), median wall 0.32 s against 0.58 s in the same batch.
- Output identical. Tests: 64 unit and 75 integration pass (`test-cc-newproto.log`).
- Priority p3.

**C4. Registry double parse (`claimcheck/src/registry.rs:133–136`), 0eae562.** Each line goes to `serde_json::Value`, then `from_value`. Not measured. Masters registry: 851 lines, 1.17 MB. Whole scan is 0.35 s. Fix: parse once into the typed struct. Size S. Priority p3.

**C5. `check_quote` repeated finds (`check.rs:782`, `:784`, `:806`, `:808`), 0eae562.** Up to four searches per segment. Masters checked 2 quotes in total. No measurable waste. Examined; no action.

**C6. `align` per-call row allocation (`matcher.rs:31`), 0eae562.** Called 8 times on masters. Allocation cost not separated from the 14.7 M cells measured at 65d0aa9. No action.

**C7. `canonicalize` per file (`lib.rs:53`, `:71`, `:137`, `:166`), 0eae562.** Not measured. Examined; no action.

**C8. Git status on repeat calls (`run.rs:687–704`), prov 8743e9d.** Five sequential git calls. On masters (read-only `git status` with `GIT_OPTIONAL_LOCKS=0`):
- `--untracked-files=all`: 0.365 s (median of 5).
- `--untracked-files=normal`: 0.346 s.
- The earlier claim that `all` is the cost driver is refuted. The cost is git's tree scan. Priority: none.

**C9. Record read (`run.rs:1436–1446`, `lineage.rs:45`), prov 8743e9d.** Two masters records (139 KB) through `lineage`: median 6.5 ms. Empty directory: 5.6 ms. Negligible. Double parse is not worth fixing. No action.

**C10. Snapshots (`run.rs:199`), prov.** Not re-measured at 8743e9d. Earlier numbers at 45bd0b1 on 100 MB and 2000 files: none 0.05–0.08 s, `--in` 0.13–0.17 s, `--out` 0.11–0.14 s. `hash.rs` and `walk.rs` did not change in the range. Double hashing before and after is by design.

## D. Other points (item 4)

- **Lost information, prov.** Path-only import entries vanish (B5). Confirmed. p3.
- **Lost information, claimcheck.** Unclosed quote (B7), first sha (B8), unreadable file (B8). All confirmed at 0eae562. p2 for each of the three.
- **Waits with no limit.** Output pipe drain (B3). Confirmed. p2. No test.
- **States with no test.**
  - Background grandchild (B3).
  - Path-only import entry (B5).
  - 0-byte record (B1).
  - Conflicting shas for one text path (B8).
  - Unclosed quote (B7).
  - Unreadable checked file in a run (B8).
- **By design, not a gap.** SIGINT inherited as ignored stays ignored (B2). `lineage` exit 0 with skipped records is pinned by `tests/cli.rs:1856–1862`. Owner decision E3.
- **No fsync, prov and manifest.** `write_atomic` (`manifest.rs:397–420`) and the record write (`run.rs:1418–1421`) rename without `sync_all`. Not verified; code reading only. A power cut could leave an empty file under the final name. No probe can simulate it here.

## E. Owner decisions

1. **Incomplete records.** Options: (a) add a cleanup command for placeholders and their stream files, with a dry-run list; (b) keep manual deletion. Recommendation: (a). Reason: design.
2. **Output pipe wait.** Options: (a) time-bound the drain after exit and record a `streams-open` finding; (b) keep waiting. Recommendation: (a). Reason: design.
3. **`lineage` with skipped records.** Options: (a) keep exit 0, which tests pin, and print a count on stdout; (b) exit 1 when any record is skipped. Recommendation: (a). Reason: design.
4. **Conflicting shas for one text path.** Options: (a) error; (b) warn. Recommendation: (a). Reason: design.
5. **Unreadable checked file.** Options: (a) keep aborting the run; (b) report the file and keep checking. Recommendation: (b). Reason: design.
6. **`lab check --new-only` double run.** Options: (a) run the base only for files the change touched; (b) keep. Recommendation: (a). This is a lab-track decision.

## F. Replacement markdown

Header line 5: "claimcheck 0.13.1, prov 0.1.5 (checked at 0eae562 and 8743e9d)".

**§4.4, changed rows**

| Machine | States | Defect |
|---|---|---|
| Quote verdict | exact match (no finding); near match, similarity 0.8 or more (`quote-not-found`, warn); not verbatim, similarity below 0.8 (`quote-not-found`, error); kept (info) | Threshold: `quotes.error_below`, default 0.8 (`config.rs:41`). Warn or error split: `check.rs:921–936`. Gap: an unclosed quotation gets no finding (`doc.rs:643`). Confirmed at 0eae562. Open. |
| Number verdict | found; near (context or rounded); not found (`number-not-in-source`, warn); changed text only (`source-text-changed`, warn) | Gap: a number not found is searched twice in each text (`check.rs:1078`, `:1156`). Speed only. Open. |
| Source text load | loaded; hash mismatch (`source-text-changed`, warn; matches do not count as supported); missing (`source-text-missing`, warn) | Fixed by research-factory-220 (claimcheck 0.11.0, commit bc7c914). True at 344c307 and 0eae562. Gap: a text path shared by two references uses the first sha only (`check.rs:40`). Confirmed at 0eae562. Open. |
| Kept quotation (new) | kept (info); stale (`stale-kept-quote`, warn); dropped (`off`) | No defect. Tests `rules.rs:2011`, `:2058`, `:2147`. |

**§4.5, changed rows**

| Machine | States | Note |
|---|---|---|
| Manifest | complete (summary required, `manifest.rs:317–327`); incomplete (imported, `import.rs:488`) | Explicit flag `header.complete` (`manifest.rs:39–40`, `:61`). Writes go through `.partial` (`manifest.rs:397–420`). No fsync. |
| Run record | `incomplete` (placeholder, `run.rs:906–916`); `complete` (`run.rs:1379`) | Explicit through `status`. Placeholder explicit since prov 0.1.0 (46dbf44). Filled `complete` since prov 0.1.2 (a8f3c59). Record without the field reads as complete (`run.rs:351`). Gaps: no command removes or finishes an incomplete record; `verify` and `rerun` refuse it (`run.rs:1441–1446`); a kill between create and write leaves a 0-byte file (`run.rs:1179–1190`). |
| Run interrupt guard (new) | not installed; installed; restored; signal pending | Explicit type `CatchInterrupts` (`run.rs:943`). Installed before the placeholder (`run.rs:1176`). Ctrl-C before start leaves no record (`tests/cli.rs:1749`). A kill during the run leaves the placeholder (`tests/cli.rs:1834`). |
| Import pairing | paired; unpaired: a hash without a path (`hash-without-path`, info), or an object with a hash and no path (`record-without-path`, info) | A path without a string hash gives no finding (`import.rs:90–92`). Open. |

Paragraph after the table, line 329: keep "No defect was found that affects the redesign." Add: "Two prov gaps are open (pipe wait, incomplete records). They do not change the state set."

**§8 row 16**

| 16 | Run record placeholder, import pairing | 4.5 | Placeholder explicit since prov 0.1.0. Filled `complete` state added in prov 0.1.2 (research-factory-231, a8f3c59). Import: path without hash not reported (open). |

**Setup feedback (for the parent to file, not filed by me)**

1. Main checkouts of claimcheck and prov were 9–10 commits behind origin at audit start. Cost: both audits redone at new HEADs. Setup: the audit brief should fetch origin and name the commit.
2. `git clone --local` from a checkout gave local branch tips, not `origin/main`. Cost: one wrong build. Workaround: fetch the origin ref explicitly. Setup: other.
3. The `guard-runtimes` hook refused `python3 -c 1`. It worked as designed. Cost: one call. Workaround: `uv run python`.
4. `timeout` is absent on macOS (exit 127). Cost: one rerun. Workaround: none needed.
5. Tool error "Claude V2 adapter has no active turn for this tool request" on three Bash calls. Retry worked. Setup: harness.

**Work to file (no board ids yet; I did not file)**

| Item | Repo | Size | Priority |
|---|---|---|---|
| prov: time-bound output pipe drain (B3) | prov | S–M | p2 |
| claimcheck: unreadable file aborts run (B8) | claimcheck | M | p2 |
| claimcheck: first sha wins for shared text path (B8) | claimcheck | S | p2 |
| claimcheck: unclosed quote dropped silently (B7) | claimcheck | S | p2 |
| prov: incomplete records have no exit (B1) | prov | S–M | p3 |
| prov: preflight spawns sequential, duplicate `git --version` (C1) | prov | S | p3 |
| prov: import path-only entries silent (B5) | prov | S | p3 |
| prov: 0-byte record window (B1) | prov | S | p3 |
| claimcheck: eager index build (C3, −29% instructions) | claimcheck | M | p3 |
| claimcheck: duplicate nearby pass (C2) | claimcheck | S | p3 |
| claimcheck: registry double parse (C4) | claimcheck | S | p3 |
| Doc drift in §4.4, §4.5, §8 row 16, header (B12) | research-factory | S | p3 |
| lab: `check --new-only` runs every check twice (cross-track) | research-factory | M | lab track |

**Cleanup.** I removed my scratch build directories and the 102 MB generated input (`probes/prov/indir`). I kept source copies, probe scripts, logs and `measure/`. No `prov` or `claimcheck` process of mine is running. A `sleep 30` child of another agent's `bun p-freshen.js` appeared in `ps`. It is not mine, and I left it alone. I wrote nothing to masters. Masters reads were `claimcheck check`, `git status` with `GIT_OPTIONAL_LOCKS=0`, and copying two JSON records to scratch.

**Open questions.** Not checked: whether masters has any text path shared by two references with different shas. Not checked: the lab `--new-only` claim in §7 of the document.
