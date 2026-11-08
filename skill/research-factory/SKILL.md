---
name: research-factory
description: Run research work in a project that has a research.toml, using the research-factory tools (lab, papers, claimcheck, prov, ireval, agentprof) so that every reference is verified, every claim matches its source, every result is traceable, and recurring problems become lessons. Use when writing or checking literature reviews, citations, experiment results or evaluation statistics in such a project, and at the start and end of each research session there.
---

# Research factory

Accuracy comes before speed. A value you cannot establish is reported as unknown, never guessed.

## Start of a session

1. Run `lab brief` and follow every rule under "Rules from lessons". Each is an instruction to follow, whether it was learned from a failure or from a practice that worked.
2. Note the errors the latest checks left open; fix those in the area you are working on. If the brief has a "Lesson measurements" section, it shows each lesson's verdict from the last `lab loop`; act on `→ verify` and `→ revise or retire` lines as in step 3 of "Before reporting done", and if it says the loop has not run for over a day, tell the user the hooks may not be running.
3. If the brief has an Archive section, read the start page it names. Look records up by id or a few words with `lab archive find` rather than reading indexes or notes whole, and read a large file by section (search for the heading, then read that range).

## While working on the archive

- Write each fact once. Append a log entry to the current month's log file; update a record's front matter in place (status, `superseded_by`); link to a record instead of copying its notes into other files. Never edit an index: `lab archive update` generates them.
- Take new ids from `lab archive next <PREFIX>`.
- When you move files out of the project, such as to cloud storage, add their paths to the project's `[[lab.archive.offloaded]]` list (lab 0.9.1 or later) so links to them are not reported broken. Never add a path to the list to silence a link to a file that was not moved.
- Never edit a sealed file (the log, session reports). To correct one, append a dated correction. If an edit is unavoidable, such as removing personal data, ask a person, then record it with `lab archive seal --accept <path> --note "<why>"`.

## While working

- **Sources:** register each new source in the reference registry with `papers` before citing it, and verify its metadata (`papers verify`). Save its extracted text locally (`papers read`) and set its `depth` honestly: `metadata`, `abstract`, `method`, `full`, `artifact` or `reproduced`.
- **Claims:** copy quotations from the extracted text, never from memory. Every number taken from a source must appear in that source's text. Claim only what the depth you reached supports: an abstract does not support a statement about the method.
- **Experiments:** run them under `prov run` so the command, code version, inputs and outputs are recorded. Score retrieval with `ireval`, compare systems with its paired randomization test, correct for multiple comparisons, and report effect sizes and confidence intervals at a precision the data supports.
- **Judgement calls**, such as whether a paraphrase is fair or a source is authoritative: give your verdict with the passage it rests on, and present it as a judgement for a person to confirm.

## Before reporting done

1. If the project has an archive, run `lab archive update` first. Then run `lab check --new-only`. It lists the errors that are not in the baseline. Fix every one, or show that the finding is wrong and correct the checker or its configuration. Re-running the check does not clear it. Never delete or edit stored findings, and never run `lab baseline set` to make the gate pass; accepting findings into the baseline is a person's decision.
2. If something went wrong or worked unusually well in this session, record it: `lab lesson add --id <kebab-id> --title "<short title>" --kind bad|good --rule "<one imperative sentence>" --evidence <path:line or finding fingerprint>`. Add `--metric tool:rule` when a checker or `agentprof` rule measures it.
3. Periodically, or when the brief or a signal asks, run `lab retro` and `lab evaluate`. Propose lessons for recurring rules; since lab 0.7.0 a rule's findings in one root session and its subagents count as one recurrence. Adopt a lesson only after reading its evidence. Mark it `verified` only when `lab evaluate` says `improved`, citing its numbers in the note; `collecting` and `inconclusive` mean wait, not success. For a check-run lesson, review the findings `lab evaluate` lists as introduced since adoption, fix them, and record the ones that cannot be fixed (such as findings in append-only files) with `lab lesson review <id> --count <n> --note "<what was found>"` (lab 0.6.0 or later). Its verdict needs intervals between clean, measured check runs (lab 0.8.0 or later, with the checker run with `--measure`); while it says `no baseline` or method `descriptive`, decide on the introduced findings with `--force` and a note. A person can supply the missing baseline by replaying commits from before adoption with `lab check --at <first>^..<last>` (lab 0.9.0 or later; `<first>..<last>` would leave out `<first>`); list git-ignored inputs, such as source texts, as the check's `uncommitted_inputs` first. Intervals across a checker version change are left out, so expect fewer intervals after a checker update. When `lab evaluate` or a `lesson-introduced` signal says the checker version changed (lab 0.9.3 or later), check whether the introduced findings come from the update before fixing text (`lab check --at` on the earlier run's commit shows what today's checker finds there). Fix those that new text caused; `lab lesson review` records all listed findings at once, so run it only when every remaining one comes from the update or cannot be fixed. claimcheck 0.2.0 and later print two units, which need lab 0.9.2 or later; an `uncited-claim` lesson then starts a new baseline in uncited sentences. Revise or retire a lesson whose verdict is `worse`, and give an `unmeasurable` one a metric that can show an effect (a finding rule, or a numeric session field such as `agentprof:session.max_prompt_tokens`) or decide on other evidence with `--force` and a note.

## Running the loop from hooks

`lab loop` runs the `[lab.loop] measure` commands (such as `agentprof scan`), evaluates every lesson and sends each new or changed signal to the `notify` command; `lab brief` shows its results. To keep it running, add Claude Code hooks that start it at the start of each session and at the end of each turn (in `.claude/settings.json`):

```json
{
  "hooks": {
    "SessionStart": [{ "hooks": [{ "type": "command", "command": "cd \"$CLAUDE_PROJECT_DIR\" && lab loop --background --if-older 15" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "cd \"$CLAUDE_PROJECT_DIR\" && lab loop --background --if-older 15" }] }]
  }
}
```

`--background` returns at once; the detached loop does nothing when the last one finished less than 15 minutes ago or another is running, and writes its output to `.research/lab/loop/loop.log`. To file signals as tasks on a work board, set `notify` to the script `scripts/board-notify.sh` in this skill: it reads one `rf.signal/1` object on stdin and prints the task's reference.

## Reporting

State which checks ran and their results, which findings remain and why, and every value left unknown. Do not describe a claim as verified unless a check or a person verified it.
