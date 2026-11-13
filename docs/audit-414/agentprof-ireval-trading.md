I'll start by reading the shared audit brief.
The brief defines four items and a report shape. Next I'll set up my scratch directory and read the state-machines document.
The document mentions agentprof only at lines 5 and 404, and never mentions ireval. Next I'll look at the agentprof repository layout and HEAD.
Cargo.toml says agentprof 0.2.0, but the document says 0.4.0. I'll record that as drift and read the README and main.rs next.
The README gives the design rules and the output files. Next I'll read main.rs and check the git log to see recent changes.
main.rs is short. Now the orchestrator, scan.rs, in ranges.
The Claude loop reads each transcript once with `std::fs::read`. I need to confirm the discovery order, since a subagent is kept only when its parent was already included. Continuing with the Codex section.
scan.rs writes sessions.jsonl and findings.jsonl before scan.json, with no temp-file or rename step. I'll check that later with a probe. Next is claude.rs, which handles discovery and parsing.
Discovery puts each directory's main transcripts before its subagents, so the parent check holds. Continuing with the parser.
`result_text` and `text_of_user_content` turn a JSON parse error into an empty string with `unwrap_or_default`. I'll note that for item 4. Continuing with the parser body.
The assistant content is parsed twice: once inside `Rec` as a RawValue and again into `Vec<Block>`. I'll measure that later. Continuing with the user-record and tool-result branches.
The tool-result content is walked about four times per record: Rec, text_of_user_content, the Block parse and result_text. I'll confirm the rest of claude.rs is test code.
Next is session.rs, which holds the Session struct, key(), and the cost and span logic.
Session.open_turns is documented as "turns still open," but claude.rs:~876 sets it to `pending.len()`, which counts tool calls without results. I'll verify that against the other users of the field. Next, util.rs and billing.rs.
billing.rs `lookup` scans every decision for each session, so I'll measure it on the real decisions log. Next is pricing.rs.
Pricing is a small linear table, so it is not a hot spot. Next is codex.rs, the largest file. I'll map its functions first.
`Thread::spawn()` parses the thread's `source` JSON, and scan.rs calls it inside a fixpoint loop over all threads. I'll check how many times that runs. Next, the rollout resolution and the segment index.
The Codex loop uses `continue` in several branches, and those branches skip the `last_ts = ts` update at the bottom. That affects active time for an open turn, so I'll probe it. Reading the end of `parse` next.
The Codex parser is read. Next is t3.rs, which reads the T3 SQLite state.
T3 loading runs three SQL queries per thread. It also silently replaces an unparseable payload with a default. Next is detect.rs, which holds the detector rules.
The detector rules are clear. Next I'll check how report.rs reads the scan directory, since it is the second command.
report.rs parses each JSONL line twice and does not check that its three files come from the same scan. Next I'll list the end-to-end tests.
I have read the main paths. I'll skim the file-reread and web-failure rules in detect.rs, since the README makes exact claims about them.
I have covered the agentprof source. Next I'll check whether either tool spawns processes, and confirm the toolchain and input sizes before I build and measure.
agentprof spawns no processes. Next is the ireval layout, excluding fixtures, plus its README and manifest.
ireval's README makes several concrete claims: exit codes, closed-pipe handling, and unknown-key warnings. Next I'll read main.rs for the command flow.
The parent says both main checkouts moved, so I'll check the new HEADs and what changed before I reuse any old results.
Both repos moved a lot, including a scan write-path change that touches my mid-write finding. Next I'll read the agentprof diffs in the areas I already cited.
Continuing the agentprof re-audit at 6a72921. I'll read the rest of the scan.rs diff, the new archive.rs, and the diff stats for the other changed files.
The scratch paths are in the summary. I'll clone 6a72921 into scratch and start the build and test run in the background so it runs while I read.
While the tests run, I'll check whether the Claude prefilter, `mentions_cwd`, and `open_turns` moved at the new HEAD.
The Claude candidate loop still reads each file in sequence (scan.rs:465). Next I'll read that loop and the new write call site at the new HEAD.
The rename loop at scan.rs:1282-1285 is not covered by the rollback. A failure partway through the renames would leave a mix. I'll verify that with a probe once the build finishes. Checking the test log now.
The build is still compiling. Meanwhile I'll read the codex.rs, billing.rs and session.rs diffs.
The README defines `searches` to include opens and related calls, so that counting is by design. Checking the test log again.
Still compiling (bundled SQLite is slow). I'll read the ireval diff in the meantime.
The tie-reorder logic is covered by named unit tests in eval.rs. Next I'll read the main.rs, cli test and README changes for the `score` alias.
The `score` alias is a clap `visible_alias` with a named test (`score_is_an_alias_of_eval`). Next I'll clone ireval at fb47fb2 into scratch and build it, reusing the existing scratch target directory so only the changed crate rebuilds.
Both builds are running. I'll check the call sites of the not-yet-measured agentprof hot paths (billing lookup, Codex `Thread::spawn`, T3 queries) at the new HEAD.
The Codex fixpoint re-parses each thread's `source` JSON on every pass. `lookup` has no early exit once the window is passed. Both are small-scale. I'll size them against the real data, read-only.
The agentprof test run finished. Reading its result.
Agentprof at 6a72921 passes: 65 unit tests and 7 integration tests, exit 0. Running clippy and fmt now.
Agentprof clippy (`-D warnings`) and fmt both pass at 6a72921. ireval at fb47fb2 passes its tests: 23 lib, 10 cli, 1 parity, 6 stats_parity. Running ireval clippy and fmt.
Clippy and fmt pass for ireval too. Next is the scan write-atomicity probe on the new binary. I'll reuse the arguments from the earlier probe run.
I'll build a small synthetic fixture under scratch (one Claude transcript, empty Codex and T3 inputs) and run the scan twice with a directory at `scan.json` in between. The first run is the baseline.
Baseline is good: one session, exit 0. Next the mixed-write probe: add a second session, replace `scan.json` with a directory, and rescan.
The probe confirms the rename-phase gap at 6a72921. The scan exits 2, `sessions.jsonl` and `findings.jsonl` are already replaced, `scan.json` is untouched, and a hidden `.tmp` file is left behind. Next I'll run `report` on this mixed directory and read the README sentence the code claims.
The README says `report` refuses files whose counts differ from `scan.json`. The probe above failed on the directory, not on the count check. To test the count check I'll restore an old `scan.json` file and run `report` again.
`report` does catch a mix whose session counts differ, as the README says. It does not catch a mix whose counts match. Next I'll check whether the earlier prefilter patch still applies at 6a72921.
The patch applies cleanly. Before measuring I need the exact inputs of the earlier full-scan runs, so I'll read them from the saved `scan.json`.
The earlier full runs used `--project ~/Documents/ChatGPT/masters` with default input paths. I'll build release binaries of both the new HEAD and the prototype in scratch, then rerun the same measurement.
The release builds are still compiling, since the background command returned early. I'll let them finish before any timing. Meanwhile I'll check the ireval probe scripts from the earlier audit so I can re-run them on the new binary.
The earlier ireval probes are scattered and partly broken, so I'll write one new probe script for fb47fb2. First I'll confirm the binary version and check the config keys and config-path recording at the new HEAD.
`config.rs` is unchanged between the two HEADs, and `settings_json` still omits the config path. I'll re-verify that with a probe after checking the CLI flags.
The `exit 1` came from the `ls` of the missing `rel-proto.log`, not from `ireval`. The release build is still compiling. I'll run the ireval probes now with the fb47fb2 debug binary, starting with duplicates, config, the tie-reorder finding and the `score` alias.
Duplicates, `check` and the `score` alias all behave as before. Next is the tie-reorder finding on the real run files, then the config probe.
None of the real run files trigger the finding. I'll count score ties in those files to see whether the check had any chance to fire.
The real runs have no tied scores, so the finding cannot fire on that data. Its verdict rests on the named unit and CLI tests. Next is the config-source probe.
The config applied (`missing` came from the TOML file), but the JSON output names no config file. I'll check whether the README documents that and find the tie CLI test name.
README line 142 documents the `[ireval]` table, but no line says the output records which file set the values. Checking the agentprof release build before timing anything.
Those extra `cargo` processes belong to another session's papers build, so I won't touch them. My agentprof release build is still compiling. Meanwhile I'll check the document drift lines in `docs/state-machines.md`.
Header line 4 claims agentprof 0.4.0, and line 404 says 0.3.0 added archive counts. I need the version history to check those claims, plus the §8 table format for the new rows.
The version history settles the drift question. Line 404 is right: 0.3.0 is commit e2a2ebb. The header's 0.4.0 is 18f44b2, and the current HEAD is 0.5.0. Checking the release build.
The release build is still running. While it finishes I'll read the `open_turns` sites in claude.rs and codex.rs so I can write a probe for them.
The README says `open_turns` counts turns still open. claude.rs:889 sets it to the number of tool calls without a result. A probe with an unanswered `tool_use` will show which one the output records.
The output shows `open_turns: 1` with one unanswered call. That does not separate "tool calls" from "turns". A second unanswered call in the same turn will.
Confirmed: `open_turns` is 2 with `turns` at 1, so it counts tool calls, not turns. Checking the release build again.
The base release binary is built and the prototype build is still running. Before timing, I'll read the remaining cheap code sites: T3 per-thread queries, the mentions_cwd test name, and the silent parse defaults.
T3 runs per-thread queries at lines 301, 379 and 401, and line 313 falls back to a default on bad JSON. I need the surrounding code to see whether statements are prepared inside the loop and whether a bad payload leaves any trace.
The three per-thread statements are prepared once and reused, so the pattern is three queries per thread, not a re-prepare. Next I'll check whether any counter records bad T3 payloads, then read the table names for a fixture.
The T3 schema is clear now. I'll build a small T3 fixture with one thread whose run and thread payloads are malformed, plus one valid control thread, and check what the scan records.
That was a stray `python3 -c 1` I left in the command. The guard blocked it. Rerunning without it.
The scan exits 0, and the `notes` field names only a timing gap. The model names are not in `sessions.jsonl`, so I'll look at them in `scan.json` and the session row.
The valid control also reports `model: unknown`. My fixture probably used the wrong key name. I'll check the serde field names on `RunPayload`.
The real key is `modelSelection`. My fixture used `model_selection`, which the parser silently ignored. I'll correct the control to the real key and re-run. That also shows the silent fallback when a key name is wrong.
macOS `sed -i` needs an empty suffix argument, so the edit did not happen and the chain stopped. The output above is the old run. Retrying with the correct flag.
Confirmed. A malformed run payload gives `model: unknown` with no counter, no finding, and no note about the parse failure. The only note is about missing timestamps. Checking the prototype build.
The prototype is still compiling. While it finishes, I'll count the real T3 rows the N+1 loop would touch for the masters project, read-only.
The masters project has 773 T3 threads. That means 2,319 per-thread queries against the real database. I'll check the index use and time those queries read-only, as a proxy for the SQL cost.
The index plan is `SEARCH ... USING INDEX` for all three queries, so the cost is about 0.2 s in Python for 2,319 queries. That is a small share of a 5 to 8 s scan. Checking the prototype build again.
The prototype binary is still compiling. Meanwhile I'll re-check the ireval `check` string-replace coupling at fb47fb2, since that finding is one of the open items.
The severity upgrade is covered by `check_reports_duplicates_in_unjudged_queries` (exit 1 and `error[...]`). The message text is not asserted, but the probe above shows the replacement working at fb47fb2. Checking the prototype build again.
Both release binaries are built. The machine is quiet now, so I'll run the full scan three times each on the base and prototype binaries, with identical arguments, and save outputs and timings to scratch.
The full-scan timings are noisy at 6a72921. Base ran 7.8 to 9.0 s and the prototype 6.3 to 8.2 s. Before running more repetitions, I'll check that the outputs match.
Outputs match: `sessions.jsonl` is byte-identical in all three pairs, and findings match once `at` is removed. The timing gap is within noise, so I'll run four more alternating pairs and record user and sys time too.
The base median is 8.39 s and the prototype median is 6.27 s, over seven pairs each. Both drift downward across runs, so I'll report medians and the spread. I'll also record the core count and the size of the Claude transcript set, since the prefilter cost scales with both.
The Claude prefilter reads all 3,920 transcript files (4.86 GB) on every scan. That read is the main cost, and the prototype only parallelizes it. Next is the ireval timing, which needs a release build. I'll check the compare flags first.
ireval is built in release. I'll time `eval` over all five runs and an all-pairs randomization `compare` at 100,000 trials, three runs each.
ireval is fast at this size: `eval` takes 0.03 to 0.04 s and the all-pairs randomization `compare` takes 0.06 to 0.07 s. Next I'll run the prototype's tests, clippy and fmt, since the timing claim depends on the prototype being correct.
The prototype passes its 65 unit and 7 integration tests, clippy with `-D warnings`, and fmt. Next I'll confirm the exact `needle` string for the `mentions_cwd` false-positive check.
The needle is the JSON key prefix `"cwd":"<root>` with the closing quote removed. I'll get the name of the test that covers the boundary cases.
The boundary test is `cwd_mentions_stop_at_path_boundaries` at scan.rs:1344. Last measurements: the Codex `source` re-parse cost, and the billing lookup bound.
Some Codex `source` values are plain strings like `vscode`, not JSON. The Rust code handles that with `.ok()?`. I'll mirror that in the timing script.
The Codex re-parse costs about 6 ms for three passes, and the billing lookup bound is about 1.8 million cheap steps. Both are small. One more item: `last_ts` in the Codex parse loop, which the summary lists as skipped by `continue`.
The skipped `last_ts` updates are the `session_meta` and inherited-history paths. The inherited lines are copied from the parent before the fork, and the comment at scan's codex.rs:741 says so. So that is by design. Last, the silent-default sites in claude.rs.
The `unwrap_or_default` at claude.rs:684 and :784 turns a whole content array into an empty list when one field has the wrong type. Plain-string content is normal, so that part is by design. A malformed block inside an array would drop every block in it. I'll check the `Block` field types to build that probe.
`Block.is_error` is `Option<bool>`, so a string value fails the whole `Vec<Block>`. I'll compare a valid and a malformed version of the same two-call transcript.
The output says "Claude V2 adapter has no active turn for this tool request." I don't recognise that as agentprof text, so I'll check whether the repo contains it before going further.
The repo does not contain that text, and the probe directory was never created, so the command was refused before it ran. I'll record this as a setup issue. I'll rebuild the fixture from a Python script instead of a shell heredoc.
The same message came back. The command this time included a grep pattern containing `tool`, so I'll split it up. First I'll run the scans alone.
Both scans exit 0. I'll read the session rows with a short uv script rather than shell grep.
No row has `id: s5`. I'll print the ids and a sample row to see the actual key.
Both outputs have zero sessions, so the scan found no transcripts. I'll check the stderr and the candidate counts in `scan.json`.
The fixture is wrong, not agentprof. `json.dumps` writes `"cwd": "..."` with a space, and the `cwd` needle requires `"cwd":"` with no space, as real transcripts have. I'll rewrite the files with compact separators and re-run.
The same adapter message came back. It looks intermittent rather than tied to the command text. I'll split the work into smaller calls and retry.
The probe confirms the silent loss. The valid transcript shows `open_turns` 0 with no notes. The malformed one shows `open_turns` 2 and "2 tool call(s) without a result", although both results exist. The parse failure is never reported.

Next I'll re-anchor the ireval `file:line` refs at fb47fb2, since main.rs and eval.rs moved. I'll also confirm that the trading repo is still at 911e485.
The ireval refs are re-anchored at fb47fb2, and the trading HEAD is still 911e485. I'll get the final agentprof line numbers at 6a72921 for the sites the report cites.
The main anchors are set. Before I write section F, I need the current line numbers for the ireval duplicate policy and the eval.rs duplicate refusal.
I have the anchors I need. Before writing the report I'll clean up: remove the large build directories in my scratch, keep the probe scripts and logs, and confirm no process of mine is still running.
Five build directories total about 2.8 GB. I'll remove those exact paths, then confirm the probe scripts and logs are still there.
Cleanup is done, and the probe scripts, fixtures and logs are kept. One item from the summary still needs re-checking at 6a72921: the Codex rollout state. I'll count the rollout files and the `transcript-missing` findings from the new full run.
The `transcript-missing` count matches the 468 Codex rows. Only 1 of 1680 rollout paths exists on disk, which needs a closer look before I report it.
The Codex finding is data state, not a code defect: 1680 rollout paths are recorded, and only one file remains under `~/.codex/sessions`. Next I'll re-verify the key `file:line` refs at the current HEADs and read the §8 table format.
The §8 table columns are `#`, machine, section and fix, and the header still says 0.4.0. I'll verify the remaining refs I have not yet re-checked (claude.rs, codex.rs, scan.rs, t3.rs, report.rs, the ireval main.rs precedence lines and the trading script).
The refs match the current HEADs, including the ireval precedence and `check` replace lines and the trading `def` lines. Before I write the hot-path section, I need the exact prefilter change in the prototype.
The prototype parallelizes the prefilter read and re-reads only matching files. It does not avoid reading unmatched files. I'll confirm that `a.jobs` already exists in the 6a72921 source and that no test covers the prototype's ordering.
The source already has a `jobs` setting and a worker pool at scan.rs:664-685. I need to see what that pool parallelizes, so the prototype follows the existing convention.
The existing Codex pool at scan.rs:664-685 uses the same pattern as the prototype. The `--jobs` help text at scan.rs:80 says "Codex rollout readers" only, so it would need updating if the Claude prefilter lands. Next I'll test whether `report` catches a same-count mix, which I have not probed yet.
The count check at report.rs:181 compares only counts. I'll run a same-count probe on a scratch copy of the earlier write-path output, using the installed 0.5.0 binary, and record which binary I used.
The unmodified `out` directory is the earlier rename-phase leftover, so it already mismatches. For a clean same-count test, I'll make `scan.json` agree with the counts in the copy first, then alter a row's value.
The same-count probe exited 0 on a copy whose `scan.json` I set to match. That confirms `report` checks only counts. Next I'll read the existing write-path test body, re-check the open-turns and content-loss probe outputs, and test the NaN path in ireval.
The test at scan.rs:1309 makes the second file's temp name a directory, so it fails in the staging phase and never reaches a rename. The Claude block probe confirms open_turns goes from 0 to 2 when the content parse drops results. Next I'll re-check the turns probe output and run the ireval NaN check.
The installed `ireval` is 0.1.0, which is stale, so it cannot test the 0.3.0 NaN path. I'll build the scratch clone at fb47fb2 with a scratch target directory and find the turns probe's session rows.
The turns probe shows `turns: 1` with `open_turns: 2` at the 6a72921 probe output, which confirms the mismatch. Next I'll run the NaN input against the fb47fb2 build.
The NaN run exits 2 at parse time with `score is NaN`, so the `expect` at eval.rs:113 is not reachable from file input. I'll remove the build directory I just created and check that no processes of mine remain.
The NaN check and scratch cleanup are done. The trading lock file is in my scope but is not in my record of files read, so I'll check it with a read-only command before writing the report.
The complete report is below. I re-checked the refs at agentprof 6a72921, ireval fb47fb2 and trading 911e485, and I made no edits or commits.

**Result.** At the current HEADs, the main findings hold. The Claude prefilter is the only hot path with a measurable saving (about 25% lower median, noisy). Four silent or misleading outputs are confirmed by probe: an interrupted scan can leave a mix that `report` misses when counts match, `open_turns` can exceed `turns`, and two parse failures drop data without a note. ireval's config source is not recorded, and its NaN path is safe. The archive shell lexer (archive.rs 330-827) was not read, so it has no verdict.

**Commit labels.** agentprof 6a72921 (current) and c03e843 (earlier numbers); ireval fb47fb2 (current) and ab89f80 (earlier); trading 911e485 (unchanged).

## A. Files read

agentprof (6a72921, 0.5.0):
- `src/scan.rs`: read 80-83, 324-326, 463-482, 628-700, 1265-1287, 1309-1318, 1344. NOT read: the rest (1365 lines).
- `src/claude.rs`: read 60-111, 204-214, 684, 784, 889-892. NOT read: the rest (1481 lines).
- `src/codex.rs`: read 76-100, 585-600, 640-760, 1040-1062, 1088-1095. NOT read: the rest (1932 lines).
- `src/detect.rs`: read 640-662. NOT read: the rest (930 lines).
- `src/report.rs`: read 165-190. The rest is diff only (911 lines).
- `src/session.rs`: read 314-319. NOT read: the rest (517 lines).
- `src/t3.rs`: read 128-135, 173-205, 216-262, 296-320, 370-404. NOT read: the rest (502 lines).
- `src/billing.rs`: read 42, 70-112. NOT read: 1-41 and 113-144.
- `src/archive.rs`: read 1-330 (Shell at 168, `run` at 216). NOT read: 330-827. No verdict on the shell lexer.
- NOT read: `src/util.rs`, `src/pricing.rs`, `src/main.rs` (diff +1 line only), `Cargo.toml`, `Cargo.lock`, `tests/scan.rs` (304 new lines).
- `README.md`: diff read, including line 163 and the archive section.

ireval (fb47fb2, 0.3.0):
- `src/eval.rs`: read 109-113, 138-140, 209-212, 356, 490, 564, 669, and the `Reorder` and `reorder_finding` diff. The rest was not re-read.
- `src/input.rs`: diff read; lines 108, 150, 161, 210 read.
- `src/main.rs`: read 184, 212, 266-268, 289-291, 316-325, 369-379, 388, 405, 599, 604-609. The rest was not read.
- `src/config.rs`: read 34-46, 62, 74, 110. The rest was not read (unchanged since ab89f80).
- `tests/cli.rs`: diff read. `README.md`: diff read.
- `src/compare.rs`, `src/stats.rs`, `src/rng.rs`: unchanged since ab89f80. Read there earlier: compare.rs 97-200, stats.rs 187-250.

trading (911e485):
- `scripts/archive.py`: read at 911e485. Def lines re-checked: `load_notes` 93, `check_markets` 134, `validate` 140, `check_links` 196, `render` 225, `main` 297.
- `scripts/test_archive.py`: read (3 tests).
- `scripts/archive.py.lock`: read at 911e485. It is a uv lockfile (`pyyaml` 6.0.3, `requires-python >=3.12`), not runtime state. `archive.py` has no references to "lock".

Build and test runs (current HEADs):
- agentprof 6a72921: 65 lib and 7 integration tests pass. Clippy and fmt pass. Logs: `agentprof-6a72921-test.log`, `-clippy.log`, `-fmt.log` in the scratch dir.
- ireval fb47fb2: 23 + 10 + 1 + 6 tests pass. Clippy and fmt pass. Logs: `ireval-fb47fb2-*.log`.
- Scratch root: `/private/tmp/claude-501/-Users-sunyizhe--t3-worktrees-research-factory-task-research-factory-4-1c1fdf969a284ada/1fb4fe71-1498-46b7-8a1e-3b6da0f588f3/scratchpad/agentprof-ireval-trading/`

## B. Machines, gaps and verdicts

### agentprof

**B1. Scan output set (`write_outputs`, scan.rs:1265-1287). Explicit or implicit: implicit. Maps to: new.**
- States: old set; staged (hidden `.<name>.<pid>.tmp`, `sync_all` at 1272); renamed in order (sessions, findings, scan.json at 1284); mixed.
- Transitions: stage (1266-1273); rollback of temp files on staging error (1275-1281); rename loop (1282-1285).
- Gap 1: a stop during the rename loop leaves a mix. No rollback exists for this phase. Verdict: **confirmed**. Probe `probes/write/`, 6a72921: rename failure, `report` exit 2, sessions and findings new, scan.json old, temp file left.
- Gap 2: `report` checks only counts (report.rs:181). Verdict: **confirmed**. Probe `probes/write/samecount2.out`: same-count copy, exit 0. The copy's `scan.json` counts were set by hand, so this shows the check's limit, not a real scan.
- Gap 3: the test at scan.rs:1309 (`a_scan_stopped_between_files_leaves_the_earlier_scan_whole`) forces a staging failure (a directory at the temp name). Its name overstates coverage. Verdict: **by test** (passes); the rename phase is not covered.
- Proposed change: write one scan id into all three files and compare ids in `report`, or swap a whole directory. Size M. Priority p3 (window is milliseconds).

**B2. Claude candidate read (scan.rs:463-482; `cwd_needle` claude.rs:108-111; `mentions_cwd` scan.rs:324-326).**
- States: candidate, read whole, prefilter hit or miss, basis, parsed.
- Gap: every candidate is read whole, even when it does not match. Verdict: **confirmed** as cost (see C1).
- Proposed change: parallel prefilter read (prototype). Size S. Priority p3.

**B3. Claude open turns (claude.rs:889-892; `session.rs:314-319`).**
- States: tool call pending, answered, open at end.
- Gap: `open_turns` counts unanswered tool calls, not turns. README:163 and session.rs:314-319 say turns. Verdict: **confirmed**. Probe `probes/turns/out/sessions.jsonl` (6a72921): `"turns":1 "open_turns":2`.
- Proposed change: count turns, or rename the field and fix README:163. Size S. Priority p3.

**B4. Claude message content parse (claude.rs:684, 784, `unwrap_or_default`).**
- States: content parsed, or dropped.
- Gap: a bad block drops every block in the message, with no note. Verdict: **confirmed**. Probe `probes/blocks/`: valid input `open_turns 0`, bad `is_error` string input `open_turns 2`.
- Proposed change: count dropped blocks and add a note. Size S. Priority p3.

**B5. T3 run payload (t3.rs:128-135 `modelSelection`, 312-314, defaults 319, 327).**
- States: payload parsed, or defaulted.
- Gap: a malformed payload gives model `unknown` with no note. Verdict: **confirmed**. Probe `probes/t3/`: malformed payload gives `unknown`; the valid control gives the model name.
- Proposed change: add a note on the fallback. Size S. Priority p3.

**B6. Codex rollout presence (codex.rs:589-600; rule at detect.rs:648-660).**
- States: rollout plain, zstd or missing.
- Verdict for the rule: **by design** (the rule text matches the state).
- Data state: 1680 thread rows point to rollouts. Only 1 `.jsonl` exists under `~/.codex/sessions`, and 0 under `~/.codex/archived_sessions`. 468 `transcript-missing` warnings repeat on every full scan (`runs6/full-base-1/findings.jsonl`).
- Proposed change: summarise repeated warnings, or leave as is. Size S. Priority p3. Owner decision (E4).

**B7. Codex spawn fixpoint (scan.rs:628-646 loop at 629; `Thread::spawn` codex.rs:81-97).**
- Gap: `source` JSON is re-parsed on each pass. Verdict: **no waste** (C2).

**B8. Codex turn and `last_ts` (codex.rs:1040-1062, final open turn at 1056; note at 1092-1094).**
- Read only. **Not probed.** No verdict.

**B9. Billing mode lookup (billing.rs:98-105, `WINDOW_MS` at 42; test `lookup_needs_same_cwd_and_window` at 112).**
- Gap: reverse scan with no early break. Verdict: **no waste** (C3).

**B10. Claude `mentions_cwd` boundary (scan.rs:324-326).**
- Verdict: **pinned by test** `cwd_mentions_stop_at_path_boundaries` (scan.rs:1344, passes).

**B11. Archive shell lexer (archive.rs:168 Shell, 216 `run`, read to 330).** **No verdict.** Not read past 330. Not probed.

### ireval

**I1. Duplicate policy (`Duplicates::Error` eval.rs:209-212; flag main.rs:289-291; `check` sets First at main.rs:599).**
- States: Error (default), First.
- Probes (fb47fb2): default run exit 2; `--duplicates first` exit 0; `check` exit 1 with `error[run-duplicate-doc]`.
- Test: `check_reports_duplicates_in_unjudged_queries` (tests/cli.rs ~132-156).
- Verdict: **by design** (README). Confirmed by probe.

**I2. Missing-query policy (main.rs:266-268).**
- Precedence: flag, then config, then Skip.
- Probe: config `missing = "zero"` applied.
- Test: `config_supplies_defaults_and_flags_override` (tests/cli.rs:220).
- Verdict: **confirmed**.

**I3. Config source (config.rs:62 `discover`, 74 `load`, 34-46 `KNOWN_KEYS`; settings_json main.rs:316-325).**
- Gap: the JSON settings output does not record the config path. Verdict: **confirmed** information loss.
- Proposed change: add `config_path` to `settings_json`. Size S. Priority p3.

**I4. Tie reorder (`trec_order` eval.rs:109-113; finding `tie-reorders-run` eval.rs:490; test helper eval.rs:669).**
- Verdict: **by design** (README row, named tests).
- Gap: no tied scores in the 5 real runs (0 tie lines). Not exercised on real data.
- Priority p3, no change required.

**I5. Input parse and NaN (`parse_run` input.rs:161; NaN check input.rs:210; `expect` at eval.rs:113).**
- Probe `probes/nan/` (fb47fb2 build, rebuilt in scratch): `run_nan.txt:2: score is NaN`, exit 2 for both `eval` and `check`. Control run exits 0.
- Verdict: **refuted** as a reachable panic. The parser rejects NaN before the `expect`.

**I6. `check` severity replace (main.rs:604-609).**
- Severity is covered by the test. The message text is not asserted.
- Priority p3.

**I7. `score` alias.** Verdict: **verified**. Probe exit 0; test `score_is_an_alias_of_eval` (tests/cli.rs, diff).

**I8. Output path (`out` main.rs:369-379; BrokenPipe at 375).** Read only. Not probed.

### trading (911e485, unchanged)

Earlier probes, run at 911e485 in an earlier context. Not re-run this time. Logs in `trading-probes/`.
- Missing-directory hypothesis: **refuted** (93 broken-link errors).
- INDEX crash window: **confirmed**. Self-healing. Test `test_index_rewrites_stale_index_after_rename`.
- Idea status transitions: not checked in `validate` (archive.py:140-193). Gap.
- Paper `status: read`: not validated. Gap.
- Tests: 3 only. Gap.
- YAML loader: 180-470 ms. CSafeLoader unavailable. Measured, not a defect.

### Document drift in `docs/state-machines.md` (research-factory)

- Line 4 (header) says `agentprof 0.4.0`. Current is 0.5.0 (6a72921). 0.4.0 is 18f44b2.
- Line 404 is correct: 0.3.0 is e2a2ebb, which added archive counting.
- Section 5.9 (lines 396-405) names agentprof as a session source. No agentprof machine section exists.
- No ireval mention exists.
- §8 table ends at row 32 (line 509).

## C. Hot paths

**C1. Claude candidate read (scan.rs:463-482).**
- Before (6a72921), full masters scan, real seconds: 7.81, 8.08, 8.95, 8.72, 8.39, 6.98, 9.36. Median 8.39.
- Corpus: 3920 Claude transcripts, 4.86 GB.
- Fix tested: prototype in `agentprof-proto-6a72921` (src/scan.rs, +47/-5). Parallel read of each candidate (jobs = cores, at most 8, same pattern as the Codex pool at scan.rs:664-685). Only matching files are read again.
- After: 8.24, 7.27, 6.27, 7.24, 5.53, 4.59, 3.30. Median 6.27 (about 25% lower). Ranges overlap.
- Output: `sessions.jsonl` identical in 3 pairs (cmp). Findings equal except `at`.
- Tests: 65 + 7 pass. Clippy and fmt pass.
- Earlier (c03e843): base 8.01, 8.87, 6.32 (mean 7.73); prototype 5.86, 5.78, 5.18 (mean 5.61).
- Caveat: this is parallel reading, not read avoidance. Unmatched files are still read whole.
- Doc drift if adopted: `--jobs` help (scan.rs:80) says "Codex rollout readers" only.

**C2. Codex spawn fixpoint (scan.rs:628-646).** About 6 ms for 3 passes over 1680 threads. Python proxy, not Rust. **No waste.**

**C3. Billing lookup (billing.rs:98-105).** Worst case about 1.8M steps. Negligible. **No waste.**

**C4. T3 per-thread queries (t3.rs:296-301, 370-401).** 2319 queries, all indexed. About 0.21 s Python proxy. Prepared statements at t3.rs:247-253. **No waste** at masters scale (773 threads, 866 runs). Not prototyped.

**C5. ireval eval (eval.rs:140).** 0.03-0.04 s on real inputs. **No waste.**

**C6. ireval all-pairs compare (compare.rs).** 100k randomization trials in 0.06-0.07 s on real inputs. **No waste.**

## D. Other points (item 4)

- **D1. Scan atomicity (B1).** Confirmed. p3.
- **D2. `open_turns` (B3).** Confirmed. p3.
- **D3. Claude content loss (B4).** Confirmed. p3.
- **D4. T3 fallback to `unknown` (B5).** Confirmed. p3.
- **D5. ireval config source (I3).** Confirmed. p3.
- **D6. `check` message untested (I6).** p3.
- **D7. Tie reorder not on real data (I4).** p3.
- **D8. Waits with no limit.** None found in agentprof's read code. The billing lookup has a fixed window (billing.rs:42). Not searched exhaustively.
- **D9. States with no test.** Rename phase (D1). Shell lexer beyond 330 (B11). Repeated `transcript-missing` (B6).

## E. Owner decisions

1. **Scan atomicity.** Options: (a) scan id in all three files, checked by `report`; (b) directory swap. Recommendation: (a). Design choice.
2. **Prefilter.** Options: (a) parallel read (prototype, about 25% lower median, small change); (b) read avoidance (needs a size or mtime index, larger change). Recommendation: (a) now, (b) later if needed. Design choice.
3. **Parse failures (B4, B5).** Options: (a) a note per session; (b) a finding with a count. Recommendation: (a) plus a count in the finding. Design choice.
4. **Repeated `transcript-missing` warnings (B6).** Options: keep per session, or one summary line. Recommendation: summary line. Design choice.

## F. Markdown for the document

### agentprof (6a72921, 0.5.0)

**Scan output set.** `write_outputs` (`src/scan.rs:1265`) stages three files, then renames them in order: sessions, findings, `scan.json` (`scan.rs:1284`).
- A stop during the rename phase leaves a mix.
- `report` (`src/report.rs:181`) compares counts only.
- The test at `scan.rs:1309` covers staging failure only.

**Claude candidate read.** Every candidate is read whole (`src/scan.rs:465`). The needle check is `mentions_cwd` (`scan.rs:324-326`).

**Claude open turns.** `open_turns` counts unanswered tool calls (`src/claude.rs:889`). README:163 says turns.

**Claude content blocks.** A failed parse drops all blocks in a message (`src/claude.rs:684`, `:784`).

**T3 run payload.** A failed parse gives model `unknown` (`src/t3.rs:312-314`).

**Codex transcript state.** A missing rollout raises `transcript-missing` (`src/detect.rs:648-660`).

### ireval (fb47fb2, 0.3.0)

**Duplicates.** Default `Error` refuses the run (`src/eval.rs:209-212`), with exit 2. `first` keeps the higher-ranked copy.

**Missing queries.** Flag beats config, which beats `skip` (`src/main.rs:266-268`).

**Config source.** Not recorded in JSON output (`src/main.rs:316-325`).

**Ties.** Ranked by score descending, then docid descending (`src/eval.rs:109-113`). Reported as `tie-reorders-run` (`src/eval.rs:490`).

**NaN.** Rejected at parse (`src/input.rs:210`), so the `expect` at `eval.rs:113` is not reached.

### Trading (911e485)

`scripts/archive.py` loads notes (`load_notes`, line 93), validates them (`validate`, line 140), checks links (`check_links`, line 196), renders (`render`, line 225) and runs (`main`, line 297). Idea status transitions and paper `status: read` are not validated. Three tests cover the INDEX rebuild after a rename.

### §8 rows (from 33)

| # | Machine | Section | Status |
|---|---|---|---|
| 33 | agentprof scan output set | new (5.9) | Open: rename-phase mix; `report` counts only (6a72921) |
| 34 | agentprof Claude candidate read | new (5.9) | Open: whole-file read; parallel prototype only (6a72921) |
| 35 | agentprof Claude open turns | new (5.9) | Open: counts tool calls, docs say turns (6a72921) |
| 36 | agentprof Claude content blocks | new (5.9) | Open: failed parse drops blocks silently (6a72921) |
| 37 | agentprof T3 run payload | new (5.9) | Open: fallback to `unknown`, no note (6a72921) |
| 38 | agentprof Codex rollout state | new (5.9) | By design; data state, repeated warnings (6a72921) |
| 39 | ireval duplicate policy | new | By design; exit 2 by default (fb47fb2) |
| 40 | ireval missing-query policy | new | Confirmed by probe (fb47fb2) |
| 41 | ireval config source | new | Open: not recorded in JSON (fb47fb2) |
| 42 | ireval tie reorder | new | By design; not on real data (fb47fb2) |
| 43 | trading archive INDEX and status fields | new | INDEX self-heals; status unchecked (911e485) |

## Work to file (parent)

Each item is a sub-task or proposal, under task `research-factory-414`. I did not file anything.
- agentprof scan atomicity: M, p3. Owner choice E1.
- agentprof Claude prefilter parallel read: S, p3. Owner choice E2. Update `--jobs` help too.
- agentprof `open_turns` meaning: S, p3.
- agentprof Claude block drop note: S, p3.
- agentprof T3 fallback note: S, p3.
- agentprof repeated Codex warnings: S, p3. Owner choice E4.
- agentprof archive shell lexer beyond line 330: not read, no verdict. Proposal to review.
- ireval config path in JSON: S, p3.
- ireval `check` message test: S, p3.
- Trading: status validation, more tests: S, p3.
- Document drift: header 0.4.0 to 0.5.0; add agentprof and ireval machines; §8 rows 33-43. S, p3.

## Setup feedback (parent to file)

- The Bash tool returned "Claude V2 adapter has no active turn for this tool request." on several commands. Those commands did not run. Setup `other`, activity `operate`. Workaround: split commands and write fixtures through `uv run python` scripts.
- The guard-runtimes hook blocked a bare `python3`. This was correct behaviour and my error. No feedback needed.

## Open questions

- Owner decisions E1-E4.
- The archive shell lexer (agentprof archive.rs 330-827) has no verdict.
- Trading earlier probes were not re-run in this context.

Cleanup: removed the ireval build directory I created (129 MB). Remaining scratch: probe scripts, fixtures, logs and source clones, about 156 MB. No process of mine is running.
