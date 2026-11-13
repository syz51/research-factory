# Shared brief: audit of board task research-factory-414

You do one track of a 6-track audit. The parent agent integrates the tracks, writes the
committed document and files the board tasks. You do not file on the board; you report.

## The four items (apply them to your track's code only)

1. Implicit state machines. Find every set of states that the code moves through without a
   named type, table or check: a status, phase or flag field; a file, marker or lock on disk;
   a process, session or connection lifecycle; a retry, lease or timeout; a cache and what
   makes it stale; a multi-step protocol. For each machine give its states, its transitions,
   where each transition is enforced (`file:line`), and its gaps: an unguarded transition, a
   state with no exit, a state lost on a crash or restart, two writers, or one state kept in
   two places that can disagree. Also explicit machines that the document already lists:
   check each one against today's code (states, transitions, `file:line` refs still right).
2. Verdicts. Check each gap with a probe (a command you run, with its output), a new test,
   or an existing test that pins the behaviour (name it). Give each gap a verdict:
   confirmed, by design (cite the document line or test), or refuted. A code reading alone
   is NOT a verdict. Keep probes small; put probe files in your own scratch subdirectory.
3. Sequential work on hot paths. The tools are synchronous Rust. Look for process spawns in
   sequence, repeated git calls, files read or parsed twice, network calls in sequence, the
   same data read or parsed twice. Measure each site (wall time, call counts via a probe,
   `strace`-like counting is unavailable on macOS; use timing, `GIT_TRACE=1`-style logs,
   counters, or `hyperfine` if installed) before and after.
4. Other important points: failures that lose information, waits with no time limit,
   states with no test.

## Context

- The model document is `docs/state-machines.md` in research-factory (read it from
  `/Users/sunyizhe/Developer/research-factory/docs/state-machines.md` or your worktree).
  It is from lab 0.22.0 and older tool versions; code has moved since.
- The large real project for measurement is masters at `~/Documents/ChatGPT/masters`
  (12 GB, 9266 tracked files). NEVER write into it: no command that stores runs, writes
  indexes, caches, `.research/` state or git state there. Read-only commands are fine.
  For a command that writes, use `git clone --local ~/Documents/ChatGPT/masters <scratch>/masters`
  and copy the untracked inputs it needs, or point the tool's state dir at scratch.
  First read the code to learn what a command writes.
- Installed tool binaries are in `~/.cargo/bin` (lab, papers, claimcheck, prov, agentprof,
  ireval). Prefer building the repository at its current HEAD with
  `CARGO_TARGET_DIR=<your scratch>/target` so you measure today's code.
- Python only via `uv run`. No node/npm.
- Never run a paid model call (no `papers card`, no claude/Anthropic API, no experiment).
  Network calls to free public APIs (OpenAlex, arXiv, Crossref) are fine in small numbers.
- Never touch other repositories' main checkouts except to read. Your scratch directory:
  `/private/tmp/claude-501/-Users-sunyizhe--t3-worktrees-research-factory-task-research-factory-4-1c1fdf969a284ada/1fb4fe71-1498-46b7-8a1e-3b6da0f588f3/scratchpad/<your track name>/`.
  Remove large build dirs there when you finish (keep probe scripts and logs).

## Report (your final message; keep it dense, no file dumps)

A. Files read: list every source file of your scope, marked read or NOT read (with why).
B. Machines: for each, a block:
   - Name, explicit/implicit, document section it maps to (or "new").
   - States; transitions with enforcement `file:line`.
   - Gaps, each: description, verdict (confirmed / by design / refuted), evidence (probe
     command + key output line, or test name), and for confirmed: proposed change, size
     S/M/L, priority (p1 harm now; p2 each hit wastes a paid run or >10 min; p3 otherwise).
   - Document drift: refs or claims in `docs/state-machines.md` that no longer match.
C. Hot paths: for each site: `file:line`, what runs in sequence, measurement before
   (command, numbers, input size), the fix, measurement after, and the test or check.
   Sites you examined and found no waste in: one line each.
D. Other points (item 4), each with evidence and a verdict.
E. Owner decisions: anything only the user can decide, with options and your recommendation.
