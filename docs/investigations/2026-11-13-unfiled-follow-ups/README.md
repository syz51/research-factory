# Work closed with unfiled follow-ups: an audit of every board

Board task research-factory-242. Data from 2026-11-06 (the first board event) to 2026-11-13 07:32Z. Written on 2026-11-13.

## Question

How often do agents close work while follow-up items sit only in chat, a record or a note, on no board task? Where do these items sit? Which existing or proposed mechanism would catch which kind of case?

## Answer in short

- 1,734 tasks were closed on 11 boards. 982 of them are on boards other than the feedback triage board, and 950 of these have a matched closing transcript.
- 190 closed tasks got a related task or ask from the closing agent after `task_done`. 123 of them got it within 10 minutes.
- In a random sample of 108 closed tasks, 52 left at least one open item in chat at the end of a turn. Weighted to all closes, that is about 51%. 32 of the 108 (about 32% weighted) still have an item that no task holds.
- Most unfiled items come after the close. 97 of the 116 unfiled items in the sample first appear in a turn after `task_done`, mostly in answers to the owner's next prompts.
- 74 of the 116 sit in prose with no heading. The heading-based Stop hook flagged 1 of the 84 sampled turns that left an item in chat.
- After 2026-11-12 03:09Z, when the id rule and the Stop hook arrived, the share fell from 55% (40 of 73) to 34% (12 of 35). Almost all remaining items (25 of 28) are prose in later turns.
- Threads with no task: 36 of 246 sessions were flagged. 25 of the 36 hold an item that no task holds today, 18 of them delegated children that handed items to their parent with no id.

## Recommendation

One change, filed as **dotfiles-535**: every reply that ends a tool-using turn in a repository with a board ends with an `Open items:` label. Each entry names a task or ask id, or says `none` or `no action: <reason>`. The open-items Stop hook checks that label section instead of guessing headings. It blocks once when the label is missing or an entry has no id.

Why this one:

- It acts on every turn, so it covers the 97 of 116 items that appear after the close. The `task_done` reminder (agent-board-76) and a `task_done` declaration see only the closing turn.
- It does not depend on how the agent words or places an item. Prose items (74 of 116) and unusual labels ("Waits on you", "Not filed", "Next") pass the current heading patterns.
- It runs in every Claude Code session in a board repository, so it also covers threads with no task, reviews and delegated children.

Cost:

- One line per tool-using reply.
- One blocked stop when the line is missing or an entry has no id. A block makes the agent's second answer the only message T3 shows, so the block text must ask for the full reply again.
- It cannot see an item that the agent leaves out of the line. research-factory-374 measures the effect one week after the change ships, against the baseline in this record.

## Method

### Data

- **Board events.** [scripts/dump_log.sh](scripts/dump_log.sh) pages `board log --json` into one file: 46,181 events, seq 1 to 46,216. `board ls --json --detailed` gave each task's parent, title and body.
- **Transcripts.** Claude Code session files under `~/.claude/projects`, main sessions only (no subagent side chains). [scripts/tx.py](scripts/tx.py) reads the user and assistant lines. It finds each `task_done` call (MCP tool or `board done` in Bash), the prompts after it, and each board filing call. It marks a Stop hook prompt, a context compaction and an owner or parent prompt apart. 883 transcripts hold a `task_done`.
- No transcript text leaves this machine. The committed files hold task ids, counts, session ids and short paraphrases only.

### Definitions

- **Closed task.** The first `task_done` event of a task.
- **Mechanical late filing** ([scripts/late.py](scripts/late.py)). Within 3 days after the close, the same actor id creates a task whose parent is the closed task or whose title or body names it, or opens an ask on it. A note by the same actor on the closed task counts apart, as a late note. A close in the last 3 days has a shorter window.
- **Trigger** ([scripts/classify.py](scripts/classify.py)). The strongest prompt kind seen in the closing transcript between the close and a late filing, in this order: owner or parent prompt, Stop hook, compaction, none (same turn). The walk has no time limit and does not stop at the next close. The judges set the prompt of each judged late filing again from the turns.
- **Turns** ([scripts/replies.py](scripts/replies.py)). The closing turn is the turn that holds `task_done` (turn 0). Later turns are all turns within 24 hours after it, up to another task's close. The detector read all of them; the case files show the judges up to five.
- **Open item.** Work that remains or was not done, run, read, verified or tested; a limit or caveat that needs work; a follow-up or suggested change; a step or decision that waits on the owner. The full rule is in [judge-brief.md](judge-brief.md).
- **Left in chat.** An item that no task, ask or note held at the end of the turn in which it first appeared. Its status is `late-prompted` (filed after a later prompt), `late-hook` (filed after a Stop hook) or `never`.
- **Never filed.** No task held the item on 2026-11-13, after up to two board searches per item.

### Periods

The cut at 2026-11-12 03:09Z is the time the open-items Stop hook was installed on the Air. The pre period splits at the merge of the first filing reminder:

| Period | From (UTC) | Filing rule and checks in force |
|---|---|---|
| P0 | 2026-11-06 | No filing rule. |
| P1 | 2026-11-07 07:33 | CLAUDE.md: put everything still to do on the board (dotfiles 0e8f8b0, 06:44). Board MCP instructions and a Stop reminder after `task_done` (agent-board PR #31, 07:33). From 2026-11-11 14:24 the reminder went silent after any proposal or ask in the session (agent-board-151, PR #180). |
| P2 | 2026-11-12 03:09 | Each open item in a report names its task or ask (dotfiles-427, 09f632d, 01:15). The reminder went silent only after a filing about the closed task (agent-board-455, PR #190, 01:26). Lab open-item rule for records with a `task` field, on in masters (research-factory-241, lab 0.13.0, about 02:35). open-items.js Stop hook installed (dotfiles-426, 03:09). |
| P3 | 2026-11-13 05:33 | The reminder moved into the output of `task_done` and of a release; the board's Stop hook no longer blocks for them (agent-board-76, PR #196). Not measured apart: about 2 hours of data. Its 5 non-triage closes are counted in P2. One is in sample A (agent-board-316) and left no item. |

Times are commit or merge times. A deploy can come later.

### Samples and judges

[scripts/sample.py](scripts/sample.py) draws the samples with seed 242 from the 950 closes with a matched transcript:

- **A**, random: 70 closes from P0 and P1 and 35 from P2, proportional to each board's closes, at least one per board. Rounding gave 73 and 35.
- **B**, late filings: the 81 closes whose late filing followed an owner prompt or had no matched prompt.
- **D**, detector flags: closes where open-items.js, replayed with the 2026-11-13 config ([scripts/detect.ts](scripts/detect.ts)), flags a reply. 17 cases that were only in D were not judged.
- **No-task sessions**: the 36 flagged sessions and 7 of 14 drawn unflagged sessions, out of 246 sessions that made board calls but claimed no task.

[scripts/build_cases.py](scripts/build_cases.py) wrote one case file per case. Six Opus subagents, then three, judged the cases with [judge-brief.md](judge-brief.md): 217 closed cases and 43 no-task cases, 260 in all. Each judge listed the open items, the turn and place where each first appears, and the task that holds each. [scripts/report.py](scripts/report.py) builds the tables below and the case lists. Rates for all closes weight P0 and P1 against P2 by their closes with transcripts: 777 and 173.

## Counts per board

### Mechanical counts, all closes

| Board | Closed | Transcript found | Late task or ask | of which within 10 min | Trigger: same turn | owner prompt | compaction | Late note on closed task |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| board-feedback | 752 | not read | 43 | 13 | – | – | – | 98 |
| research-factory | 274 | 271 | 41 | 27 | 15 | 26 | 0 | 51 |
| agent-board | 268 | 258 | 32 | 24 | 18 | 13 | 0 | 56 |
| dotfiles | 251 | 237 | 31 | 26 | 13 | 14 | 1 | 50 |
| vps | 89 | 88 | 22 | 18 | 10 | 5 | 1 | 10 |
| t3code | 59 | 58 | 18 | 13 | 11 | 8 | 1 | 22 |
| email-agent | 33 | 31 | 2 | 2 | 2 | 0 | 0 | 13 |
| codex-commit | 5 | 5 | 1 | 0 | 0 | 1 | 0 | 3 |
| 17lands-zh | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 1 |
| dispatch-live-check | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 |
| dispatch-live-check-2 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| all | 1,734 | 950 | 190 | 123 | 69 | 67 | 3 | 304 |

The trigger columns count tasks; a task can have filings with more than one trigger. No mechanical late filing followed a Stop hook prompt; the judges found one. The board-feedback board is the triage board: closing a report there routes it, and the routed task often comes after the close. Its closes were counted but not judged.

### Judged random sample A

"Left in chat" counts closes with at least one item left in chat in any turn. "Closing turn" counts those with such an item that first appeared in the closing turn.

| Board | Period | Sampled | Any open item | Left in chat | Closing turn | Never filed |
|---|---|--:|--:|--:|--:|--:|
| agent-board | P0, P1 | 23 | 22 | 11 | 5 | 8 |
| agent-board | P2 | 2 | 2 | 1 | 0 | 1 |
| dotfiles | P0, P1 | 19 | 19 | 7 | 2 | 5 |
| dotfiles | P2 | 5 | 4 | 2 | 1 | 1 |
| research-factory | P0, P1 | 13 | 13 | 7 | 3 | 5 |
| research-factory | P2 | 25 | 23 | 8 | 2 | 5 |
| t3code | P0, P1 | 4 | 4 | 3 | 0 | 2 |
| t3code | P2 | 3 | 3 | 1 | 0 | 0 |
| vps | P0, P1 | 8 | 8 | 7 | 1 | 1 |
| email-agent | P0, P1 | 3 | 3 | 2 | 1 | 1 |
| codex-commit | P0, P1 | 1 | 1 | 1 | 1 | 1 |
| 17lands-zh | P0, P1 | 1 | 1 | 1 | 0 | 1 |
| dispatch-live-check | P0, P1 | 1 | 1 | 1 | 0 | 1 |

| Period | Sampled | Left in chat (95% CI) | Closing turn (95% CI) | Never filed (95% CI) |
|---|--:|---|---|---|
| P0 | 12 | 8, 67% (39–86) | 2, 17% (5–45) | 7, 58% (32–81) |
| P1 | 61 | 32, 52% (40–64) | 11, 18% (10–29) | 18, 30% (20–42) |
| P0 and P1 | 73 | 40, 55% (43–66) | 13, 18% (11–28) | 25, 34% (24–46) |
| P2 | 35 | 12, 34% (21–51) | 3, 9% (3–22) | 7, 20% (10–36) |
| all, weighted | 108 | 51% | 16% | 32% |

The per-board cells are small. Read them as cases, not as rates. The P2 closes are recent: an item may still be filed, so "never filed" there is an upper bound.

### All judged closed cases

Of the 162 judged closes from P0 and P1 (samples A, B and D), 96 left an item in chat and 50 hold an item never filed. Of the 55 from P2, 32 and 13. B and D select for problems, so these are not rates.

Across all 260 judged cases, the judges listed 1,467 items: 856 held, 115 filed late in the same turn, 223 filed after a later prompt, 185 never filed, and 88 needing no work.

## Where unfiled items sit

Sample A holds 116 items left in chat:

| Place | Closing turn | Later turn | All |
|---|--:|--:|--:|
| Prose, no heading | 8 (all never) | 66 (44 filed after a prompt, 22 never) | 74 |
| Under a heading or bold label | 11 (4 filed after a prompt, 7 never) | 31 (21 filed after a prompt, 10 never) | 42 |
| All | 19 | 97 | 116 |

- In P2, 25 of the 28 items are prose in later turns.
- Across all judged cases, unfiled items sat 218 times under a heading, 188 times in prose and 2 times only in the `task_done` summary. The judges placed no item only in a record, a handoff or a note.
- Records: in the known cases research-factory-190 and -203, items also sat in a record's Limits or Open questions sections. The lab open-item rule now checks such records in masters (research-factory-241).

Why the heading hook misses them: open-items.js reads only sections under its heading patterns (Limits, Open items, Next steps, "not done" and similar). In the sampled miss turns the items were in running text, or under labels such as "Waits on you", "Not filed", "Proposals" or "Next" that the patterns do not match. An item that names any task id passes, even an unrelated one.

## Cases after close

- 97 of the 116 unfiled items in sample A first appear after `task_done`.
- In sample B, the judges classified 144 late filings. 69 were items that already sat in the closing reply, the summary or a record. 40 of those were filed only after an owner prompt, 18 in the same turn after `task_done`, 6 after a compaction, 2 after another `task_done`, and 3 with no matched prompt. 75 were new work, 69 of them brought by the owner's later prompt.
- 27 of the 81 B cases had at least one missed item that was filed only after an owner prompt. In 56 of the 143 items filed after a prompt, the prompt asked whether the work was filed or named the item.
- Known cases: research-factory-209 (research-factory-216, -218 and vps-245, about 9 minutes after the close), research-factory-203 (research-factory-223 and -224 at about 6 minutes, then -226 to -230, board-feedback-661, research-factory-241 and -242 after owner prompts), research-factory-190 (research-factory-244 to -248 at about 13 minutes).

## Threads without a task

| Thread kind | Flagged, item never filed | Flagged, false positive | Flagged, items filed | Unflagged, item never filed | Unflagged, clean |
|---|--:|--:|--:|--:|--:|
| Delegated child | 18 | 3 | 1 | 1 | 1 |
| Review | 2 | 6 | 0 | 0 | 4 |
| Owner chat | 4 | 0 | 0 | 0 | 0 |
| Investigation | 1 | 1 | 0 | 0 | 0 |
| Other | 0 | 0 | 0 | 0 | 1 |

- The 18 delegated children reported items to their parent by message, with no id. No task holds them today, so the parent did not file them either.
- Most flagged reviews were false positives: a review lists findings under headings that the hook reads as open items.

## Which mechanism covers which case

Y: covers it. P: partly. N: does not.

| Mechanism | Item in the closing reply, under a heading | Item in the closing reply, in prose | Item only in a record | Item in a later turn after the close | Lost in a compaction | Thread with no task | Delegated child | Subagent (Agent tool) |
|---|---|---|---|---|---|---|---|---|
| CLAUDE.md filing rule and the id rule (dotfiles-427), text only | P | P | P | P | P | P | P | P |
| Board Stop reminder after `task_done` (agent-board-455; removed by agent-board-76) | P: generic question, closing turn | P | N | N | N | N | N | N |
| Reminder in the output of `task_done` and of a release (agent-board-76) | P: generic, closing turn | P | N | N | N | N | N | N |
| A declaration in `task_done` (proposed in the task) | Y | Y, if the agent lists it | P | N | N | N | N | N |
| Lab open-item rule on records (research-factory-241) | N | N | Y, masters records with a `task` field | N | N | N | N | N |
| open-items.js Stop hook on the last reply (dotfiles-426) | P: listed headings only; 1 of 84 miss turns flagged | N | N | P: headings only | N | P: headings only | P: headings only | N |
| board-feedback-661 text changes: file when found, decisions become blocked tasks with an ask (dotfiles-429, open) | P | P | P | P | Y, if followed | P | P | P |
| Stop reminder for sessions with no task (agent-board-477, dropped 2026-11-13) | – | – | – | – | – | P | P | N |
| **Recommended: `Open items:` line on every tool-using reply, checked at Stop (dotfiles-535)** | Y | Y, if the agent lists it | P: the reply must name the record's items | Y | P | Y | Y | N: the parent's reply carries the line |

Subagents started with the Agent tool get no Stop hook. Their reports go to the parent, whose reply must list the items. The audit read only Claude Code transcripts; other runtimes, if any, are among the 32 unmatched closes.

## Limits

- Transcript matching found 950 of 982 non-triage closes. The other 32 were counted but not judged; their sessions ran elsewhere or left no file here. No action: the share is 3%.
- The mechanical count links a filing to a close by actor id and a named id. It misses filings by another agent and filings that do not name the closed task, and it can include another task's filings by the same actor. No action: the judged samples measure the rate; the mechanical count only finds cases.
- One judge per case. The judges' "heading" and "prose" labels were not checked against each other. No action: a second judge pass would cost about as much as the first, and the main result (most items after close) does not depend on the label.
- The P2 sample covers about 28 hours, and P3 was not measured. research-factory-374 measures the next period.
- The mechanical trigger takes the strongest prompt kind since the close, not the prompt just before the filing. It can label a hook-prompted filing as owner-prompted. No action: the judges set the prompt of each judged late filing again, and they found 1 hook-triggered filing.
- The detector replay used the 2026-11-13 hook and config on all replies. It shows what today's hook would catch, not what the hook did at the time. No action: no other version was installed for long.
- Judges found 185 items never filed, across the boards. They are filed for triage: agent-board-530 and -531, dotfiles-533 and -534, research-factory-372 and -373, vps-250, t3code-160, email-agent-71, 17lands-zh-3 and codex-commit-7. The research-factory list also holds items from the trading project, which has no board.

## Files

- [cases-judged.md](cases-judged.md): each judged case with its samples, period, `task_done` event, session id, item counts by status, and late filings with their kind and prompt.
- [cases-mechanical.md](cases-mechanical.md): each close with a mechanical late filing, its `task_done` event, the late task or ask with minutes after the close, and the trigger.
- [judge-brief.md](judge-brief.md): the brief the judges followed.
- [scripts/](scripts/): the pipeline, in this order: `dump_log.sh`, `late.py`, `tx.py`, `classify.py`, `replies.py`, `notask.py`, `detect.ts`, `sample.py`, `build_cases.py`, then the judges, then `report.py` and `recall.py`. To rerun, with `D` the data directory and `S` its parent:

  ```sh
  cd ~/.claude/projects
  find . -name '*.jsonl' -newermt 2026-11-06 -not -path '*/subagents/*' > $S/all-files.txt
  rg -l -F 'mcp__board__task_done' $(cat $S/all-files.txt) > $S/done-files.txt    # plus files with `board done `
  rg -l -F '<board agent=' $(cat $S/all-files.txt) > $S/board-files.txt
  rg --files-without-match -e 'mcp__board__task_claim' -e 'board claim ' -e 'board take ' $(cat $S/board-files.txt) > $S/notask-files.txt
  cd -
  zsh dump_log.sh $D                     # events.jsonl
  python late.py $D                      # late.json
  python tx.py $S/done-files.txt $D/tx.jsonl
  python classify.py $D                  # classified.json
  python replies.py $D                   # replies.jsonl
  bun detect.ts $D                       # detect.jsonl
  python sample.py $D                    # sample.json
  # board show <id> --json for each sampled task, into $D/show/<id>.json
  python build_cases.py $D               # cases/<id>.md
  python report.py $S <out dir>          # tables.md and the case lists
  python recall.py $S
  # no-task sessions
  python tx.py $S/notask-files.txt $D/tx-notask.jsonl
  python notask.py $D                    # replies-notask.jsonl
  bun detect.ts $D replies-notask.jsonl detect-notask.jsonl
  ```

  Run Python with `uv run --no-project python`.
