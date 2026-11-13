Now check.rs.
Now signals.rs.
Build should be done. Let me check and time the hot paths on masters (read-only commands first).
Now let me write the check.rs changes.
Now the edits to `Run::read` and loading.
Now replay.rs callers.
Mutation caught. Restoring the fast path and rerunning.
Now the duplicate-on-crash probe, in a scratch fixture.
Reads files only; evaluate is read-only. Measuring `lab evaluate` in masters before and after.
I fixed four hot paths in lab and one in the guard hook, and committed them in two commits. I also gave verdicts on 8 gaps in the loop and check machines: 6 confirmed, 1 confirmed but working as designed, 1 by design. Each verdict rests on a probe or a named test. Nothing was pushed and no PR is open. No reviewer agent ran on the diff. That review is still needed before a push.

## A. Files read

- **lab source:** src/check.rs, change.rs, replay.rs, main.rs, lesson_loop.rs, signals.rs, lessons.rs, brief.rs, retro.rs and worktree.rs.
- **Read in part:**
  - rules.rs (the table and its tests).
  - config.rs (`LoopConfig` and `min_version`).
  - evaluate.rs (`Verdict`, `measured`, `new_units` and the function list).
- **Not read:** stats.rs and finding.rs. Their only appearance is a sampled hot spot (`stats::cluster_permutation`, 13% of `lab evaluate` samples).
- **Scripts:** guard-check-runs.sh and board-notify.sh.
- **Read only:** the masters `.claude/settings.json` hooks and `archive-gate.sh`.
- **Document:** docs/state-machines.md header, §5, §7 and §8.

## B. Machines and gaps

Line numbers are at commit 62ed9d6.

**Lesson** (lessons.rs:27). States are observed, proposed, adopted, verified and retired.
- `transition` (:199) refuses moves back (:214), except verified → adopted and reopening a retired lesson.
- Verified is allowed only from an active state (:208).

**Loop run.** The loop runs in this order:
1. `checkout` (lesson_loop.rs:392) can redirect to the main checkout, or skip a bare repository, a worktree whose main checkout has no config, or a checkout that is not `[lab.loop] home` (:426).
2. `Lock::acquire` (:456).
3. The `--if-older` check runs under the lock (:467).
4. The measure commands run, with a 600 s limit (:24).
5. `evaluate_and_send` runs: `State::load` (:598), `plan` (:599), then a notify per signal with a 60 s limit (:625).
6. `signals.json` is saved after all sends (:653).
7. `last.json` is saved (:539, and :534 on error).

**Signal send state** (signals.rs:475, `plan` :565). States are pending (new, repeat, reopened), sent and resolved. `sent` (:636) clears pending. `failed` (:648) keeps it pending.

**Check run.**
- A check's status is `failed` by default (check.rs:400). It becomes `ok` at exit 0 or 1 with every line parsed (:462). There is no `findings` status.
- A run that repeats the newest one is not stored (`repeats` :976). Otherwise `store` (:1180) writes it with `create_new`, adding a suffix on a name clash (100 tries).
- Replay worktrees use a `Drop` guard (replay.rs:250). `remove_leftovers` (:198) removes those of dead pids.

| # | Gap | Type | Verdict and evidence | Change, size, priority |
|---|---|---|---|---|
| 1 | Signal state is saved only after all sends (lesson_loop.rs:653). A loop killed after a successful notify, or a notify that files a task and then hits the 60 s limit, sends the signal again. board-notify.sh then runs `board add` again. | state lost on crash | **Confirmed.** Probe `probes/crash/probe.sh`: 2 notify calls, and the first ref `T-1` is lost. The timeout path is the same retry: existing test `a_failed_send_is_retried_with_its_flags`. | Save `signals.json` after each send. Make board-notify find an open task by signal id before `board add`. Size M, p2. |
| 2 | `run_one` has no time limit (check.rs:391, `.output()`). It also waits for stdout EOF, so a child the checker leaves running holds `lab check`. | wait with no limit | **Confirmed.** Probe `probes/hang/probe.sh`: a checker that exits at once took 20 s, and a hung checker was still running after 30 s. | A per-check timeout with a process-group kill, as `version_of` and `wait_for` already have. Size M, p2. |
| 3 | `lessons::save` uses a fixed temp name and no lock (lessons.rs:365). A `lab lesson set` reads, changes and writes with no lock. | two writers | **Confirmed.** Probe `probes/lesson/probe.sh`, 40 rounds: 5 rounds lost a History line, and 25 rounds had one command fail with `No such file or directory`. | A unique temp name plus a lock on the lessons directory. Size S, p3. |
| 4 | The foreground `--background` start rotates loop.log outside the lock (lesson_loop.rs:136). Two starts at once rename twice. | two writers | **Confirmed.** Probe `probes/rotate/probe.sh`: 94 of 100 concurrent rounds lost the old log generation, against 0 of 100 in the one-start control. | Rotate in the child under the lock. Size S, p3. |
| 5 | A signal whose detail does not change is never sent again, even after its board task is closed while the condition stays. | state with no exit | **Confirmed, working as designed.** Probe `probes/unchanged/probe.sh` (renotify_hours = 0, 4 loops, 1 call). Existing test `dedupe_sends_new_changed_and_reopened_signals_once`. | Owner decision (E3). |
| 6 | An unreadable `signals.json` makes every loop fail (exit 2, no sends). `last.json` is replaced in the same case. | state with no exit | **Confirmed.** Probe `probes/corrupt/probe.sh`: 3 runs, exit 2 each, 0 notify calls. The error is in `last.json`. | Owner decision (E4). Name the fix in the error. Size S, p3. |
| 7 | A lesson file edited by hand can go from observed to verified with no History line, and `load` accepts it. | unguarded transition | **Confirmed.** `probes/lesson/probe.sh` probe 2: `lab lesson list` exits 0 and shows `verified`. | Owner decision (E5). |
| 8 | Replay worktree after kill -9. | state lost on crash | **By design.** `remove_leftovers` handles it. No leftover `lab-replay-*` in `$TMPDIR` after about 10 `--new-only` runs. The remaining edge is a reused pid, which keeps a leftover. | none |

**Document drift:**
- The header says lab 0.22.0. The code is 0.25.1 and the installed lab is 0.25.0.
- §5.1 says 11 observed. Masters now has 17 observed, 5 adopted and 3 verified (`lab lesson list`).
- The §5.2 diagram shows "skip (disabled, background session)". No such skip exists in the code. The real skips are checkout redirect or skip, `home`, too recent, and lock held.
- The §5.5 status list says "ok, findings, failed". The code has only `ok` and `failed`.
- §5.4 refs are still right: evaluate.rs:38, and verified only from an active status at lessons.rs:208. The document cites :207.
- §5.7 is now explicit: a dismissed candidate stays dismissed (main.rs:1315, search/learn.rs:114, test `a_dismissed_candidate_does_not_return`).

## C. Hot paths

The machine load average was 8 to 30, so wall times are noisy. I report instruction counts where they apply.

| Site | Before | Fix | After | Check |
|---|---|---|---|---|
| `lab brief` in masters (432 runs, 141 MB) | 0.54–0.78 s, 8.0–8.1 G instructions, 411 MB | `history` skips measure lines. The schema is read without a JSON Value tree. Runs load in parallel (`parallel_map`). | 0.18–0.20 s, 4.87–4.89 G instructions, 259 MB | Output byte-identical (`cmp`). Measures test extended. |
| `lab evaluate` in masters | 1.02–1.35 s, 15.7–15.9 G instructions | Parallel run loading | 0.69–0.92 s, 15.1 G instructions | Output byte-identical |
| `lab check` in the masters clone (5 checks) | 2.84–3.00 s (load 11–12) | Checks run in parallel, each after its own version command. `git status` runs while the checks run. | 1.91–2.21 s | Output equal apart from the ms column. New test `checks_run_at_once_and_keep_their_order` fails when I force one thread. |
| `lab check --new-only` in the clone, one changed file | 16.4 / 17.2 / 20.2 s in alternating pairs | The merge base's run is made while the work tree's run is (`change::base`, main.rs:849) | 9.8 / 12.1 / 16.9 s | Existing tests in tests/change.rs and tests/cli.rs pass |
| guard-check-runs.sh, per call, 40 calls each | bash1 48.7 ms, bash2 82.5 ms, bash3 51.6 ms, edit 16.1 ms | One jq call, and an early exit when no deleting or writing command word and no checks path appear | 8.4 / 8.6 / 51.2 / 6.1 ms | 7 new refused cases and 2 allowed cases. `/bin/rm -rf .research` fails when `/` is removed from the fast-path set. shellcheck exit 0. |

Sites I measured but did not fix:
- **evaluate per-lesson work.** `check_runs` rebuilds each run's key sets for every lesson: 51% of samples, with BTreeSet insert at 45%. Dropping `Evidence` at exit takes 27%. Running the lessons in parallel gave no stable gain (0.52–0.83 s against 0.69–0.71 s), so I reverted it. Proposal: cache each run's measured key sets across lessons. Size M, p3.
- **`--new-only` base run:** `git worktree remove` takes 6.7 s and linking 25,614 ignored-file symlinks takes 2.4 s. Proposal: remove the worktree in the background, or reuse it. Size M, p3.
- `retro::recurrences` was 11% of the before-brief samples. I did not change it.

## D. Other points

- **Lost information.** The masters Stop hook runs `lab loop --background >/dev/null 2>&1; exit 0`. A failed start, such as the rotation error, is never seen. This is masters' setup. Size S, p3.
- **Limits.** Local git calls have no limit (worktree.rs:35, :275, replay.rs:24, check.rs:528). This is by design, since local git ends and `--no-optional-locks` avoids index.lock waits. `analyse` has no limit, but it runs in-process.
- **Meaning changes from my fix:**
  - `duration_ms` per check now overlaps under contention, so the per-check times no longer add up to the run's time.
  - docs/interfaces.md needs to say that checks run at the same time. Size S. It is outside my files.
  - `lab archive done` uses the same `run_checks` (archive track), so its checks run in parallel too.
- **No test covers these states:**
  - a notify interrupted mid-loop;
  - concurrent `lesson set`;
  - log rotation;
  - a corrupt `signals.json`.
  The probes above cover them.

## E. Owner decisions (recommended option first)

1. **Parallel checks by default:**
   - Keep. It assumes checkers are safe to run at the same time, which they are in masters: they wrote no files (`find -newer`).
   - Or add a `[lab] check_jobs` cap.
   - To reverse, replace `parallel_map` in `run_checks` (check.rs:639) with a sequential map.
2. **Duplicate signal tasks (gap 1):** save after each send, plus a board-notify lookup by id. The alternative is to save after each send only.
3. **Closed task with the condition still present (gap 5):**
   - board-notify could check the `ref` status when lab sends again after `renotify_hours`.
   - Or accept it as is.
4. **Corrupt `signals.json` (gap 6):**
   - Keep failing, and name the fix in the error.
   - Or move the file aside and file every signal again.
5. **Hand-edited lesson status (gap 7):**
   - Accept, since git history is the audit.
   - Or make `load` require a History line for each move away from observed.
6. **Check timeout default (gap 2):** 600 s, the same as the measure commands.

## F. Replacement markdown

```markdown
## 5. Loop machines (lab)

### 5.1 Lesson

States (`src/lessons.rs:27`): `observed`, `proposed`, `adopted`, `verified`, `retired`. Masters has 17 observed, 5 adopted, 3 verified (lab lesson list, 14 November 2026).

`transition` (`src/lessons.rs:199`) moves status forward. Two moves back are allowed (`:214`): `verified → adopted`, and a retired lesson reopening at any status. `verified` needs an active status (`:208`). Re-adoption resets `adopted` and `reviewed`. Fixed by research-factory-217.

Gaps:
- Two `lab lesson set` runs on one lesson race: `save` uses the fixed temporary name `<id>.md.tmp` and no lock (`:365`). Confirmed: in 40 concurrent rounds, 5 lost a History line and 25 had one command fail. Proposed (S, p3).
- A hand edit can move a lesson from observed to verified without a History line; `load` checks field shape only. Confirmed by probe. Owner decision.

### 5.2 Loop run

hook fires ─► checkout: main checkout redirect, or skip (bare, no config in the main checkout, not `[lab.loop] home`) ─► lock held: stop ─► too recent (`--if-older`, under the lock): stop ─► measure (600 s limit each) ─► evaluate ─► plan ─► notify each due signal (60 s limit each) ─► save signals.json ─► save last.json

Explicit, with a file lock (`src/lesson_loop.rs:456`); `checkout` is at `:392`, `home` at `:426`. An unreadable `last.json` cannot show a recent loop, so the loop runs and replaces it (lab 0.17.0, 217).

Gaps:
- `signals.json` is saved once, after all sends (`:653`). A loop killed after a successful notify, or a notify that files a task and then times out, sends the signal again, and `board-notify.sh` files a second task. Confirmed by probe (2 notify calls, first ref lost). Proposed (M, p2).
- An unreadable `signals.json` fails every loop (exit 2, no sends); the error is in `last.json`. Confirmed by probe. Owner decision.
- `--background` rotates `loop.log` outside the lock (`:136`). Two starts at once lose the old generation (94 of 100 concurrent rounds, 0 of 100 single). Proposed (S, p3).

### 5.3 Signal

| Machine | States | Defect |
|---|---|---|
| Signal kind | fixed list | None |
| Send state (`src/signals.rs:475`, `plan` `:565`) | pending (`new`, `repeat`, `reopened`), sent, resolved | A notice still pending when its signal resolves is dropped and reported (`unsent`). A signal never sent that comes back is `new`. Fixed by 217. A signal whose detail does not change is never sent again, even when its task was closed while the condition stays (probe; test `dedupe_sends_new_changed_and_reopened_signals_once`). Owner decision. |
| Board task for a signal | none, filed, repeated | `signals.json` keeps the task id that `board-notify.sh` returns as `ref`; repeats go to that task while it is open. Fixed by 217. `board-notify.sh` does not look up an open task by signal id before `board add`, so a resend after a crash or timeout files a duplicate (5.2). |

### 5.4 Evaluation

`Verdict` (`src/evaluate.rs:38`): `Collecting`, `Improved`, `Worse`, `Inconclusive`, `Unmeasurable`. Explicit. A lesson moves to `verified` only from an active status (`src/lessons.rs:208`); the verdict informs that move and does not force it.

### 5.5 Check run

| Machine | States | Defect |
|---|---|---|
| Check status | failed (default, `src/check.rs:400`), ok (exit 0 or 1 with every line parsed, `:462`) | A disagreeing exit adds `check-exit-mismatch`. Fixed by 217. Since research-factory-414 the checks of one run run at the same time, each after its version command; the run keeps the configured order, and `duration_ms` per check overlaps. A check has no time limit and waits for its stdout to close: a checker that leaves a child running holds `lab check` (probe: 20 s for a checker that exits at once). Proposed (M, p2). |
| Baseline | none, recorded | explicit |
| Finding severity | Error, Warn, Info | (unchanged) |
| Stored check run **(implicit)** | stored, committed, superseded and committed; not stored when it repeats the newest stored run (`repeats`, `:976`); stored with a suffix on a name clash (`store`, `:1180`) | (unchanged text). |

### 5.6 Replay and worktree

| Machine | States | Note |
|---|---|---|
| Replay worktree | created, checked, removed | `Drop` guard (`src/replay.rs:250`) on each exit path; a run killed by a signal leaves it, and the next replay removes worktrees of dead pids (`remove_leftovers`, `:198`). Since 414 the `--new-only` merge-base run runs while the work tree's run does. |
| (other rows unchanged) | | |

### 5.7 Retro candidate

`found ─► shown ─► filed (a lesson's metric covers the rule) or dismissed`. Explicit: `lab retro dismiss` stores the decision and a dismissed candidate does not return (`src/main.rs:1315`; test `a_dismissed_candidate_does_not_return`). Fixed by 205.

### 5.8 and 5.9: unchanged.
```

§7.5: replace "**(implicit)**: nothing compares…" with this text. "Explicit since 207: `[lab] min_version` (`src/config.rs:86`). `lab brief` says first when the installed lab is older (`version_note`, `src/brief.rs:545`)." I did not check §7.1–§7.3. Their contents are in the archive, search and experiment tracks.

§8: change row 17 to "Fixed by 217; duplicate on a resend after a crash or timeout: proposed". Add these rows:
- 33 | Signal state on crash | 5.2 | Proposed
- 34 | Lesson file writers | 5.1 | Proposed
- 35 | Check with no time limit | 5.5 | Proposed

## G. Worktree and checks

- **Worktree:** /Users/sunyizhe/Developer/research-factory/.claude/worktrees/agent-a811ecd1e2c8c9e39
- **Branch:** `worktree-agent-a811ecd1e2c8c9e39`, based on 133b118.
- **Commits:**
  - `d81db6a` covers check.rs, change.rs, main.rs and replay.rs.
  - `62ed9d6` covers guard-check-runs.sh and tests/guard_check_runs.rs.
- **Checks:** `cargo fmt --check` exit 0, `cargo clippy --all-targets -- -D warnings` exit 0, `cargo test` exit 0 (15 suites ok), shellcheck exit 0.
- **Scratch:** the probes are in /private/tmp/claude-501/-Users-sunyizhe--t3-worktrees-research-factory-task-research-factory-4-1c1fdf969a284ada/1fb4fe71-1498-46b7-8a1e-3b6da0f588f3/scratchpad/lab-check/probes. The bench logs are in /private/tmp/claude-501/-Users-sunyizhe--t3-worktrees-research-factory-task-research-factory-4-1c1fdf969a284ada/1fb4fe71-1498-46b7-8a1e-3b6da0f588f3/scratchpad/lab-check. I removed the build directory, the binaries and the 9.9 GB masters clone. No lab process remains, and no `lab-replay-*` directory is left.

**Work to file:** gaps 1–4 and 6 from B, the two hot-path proposals from C, the interfaces.md note and the masters Stop hook from D, and the §5/§7.5/§8 document update from F.

**Setup friction:**
- Worktree isolation refused 3 commands:
  - a heredoc Python patch whose regex held the word `git`;
  - a probe command with `cd $P/...` and a heredoc;
  - a `cd ~/Documents/ChatGPT/masters && … git log`.
  I wrote files with Write and Edit and ran them by literal path. I dropped the git call.
- `sample` caught another track's process named `lab-before`. I sampled a uniquely named copy instead.

**Open:** the guard fast path reaches masters only after this merges, because masters runs the script through the skill symlink.
