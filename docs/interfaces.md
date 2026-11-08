# Shared interfaces

The research factory is a set of small, independent Rust CLIs. They share only the contracts below: one project config file, one finding format, and one reference-registry format. Every tool must work alone; none links another tool's crate.

Version every record with a `schema` field. A reader rejects a major version it does not know and ignores unknown fields.

## 1. Project config: `research.toml`

- Each tool locates the project by walking up from the current directory to the first `research.toml`. `--config <path>` overrides the search. Relative paths in the file resolve against the file's directory.
- Each tool owns one table named after it and ignores the others: `[papers]`, `[agentprof]`, `[claimcheck]`, `[prov]`, `[ireval]` and `[lab]`. The top-level `[project]` table is shared:

```toml
[project]
name = "my-thesis"
timezone = "Asia/Tokyo"        # date display; stored timestamps are UTC RFC 3339
state_dir = ".research"        # tool-owned state (logs, caches, ledgers); safe to gitignore except where a tool says otherwise
```

- A table's sub-tables belong to the same tool: `lab` owns `[lab.archive]`, `[[lab.archive.index]]`, `[[lab.archive.budget]]` and `[[lab.archive.offloaded]]` (section 6), and `[lab.evaluate]` and `[lab.loop]` (section 7).
- A tool with no `research.toml` and no `--config` still runs with defaults and explicit flags.
- A tool never writes `research.toml`, except `lab init`, which creates it.

## 2. Findings: `rf.finding/1` (JSON Lines)

Every checker supports `--format jsonl` and then writes one finding per line on stdout. Human-readable text is the default format. The exit code is 0 when no finding has severity `error`, 1 when at least one does, and 2 for usage or I/O failure.

```json
{"schema":"rf.finding/1","tool":"claimcheck","rule":"quote-not-found","severity":"error","subject":"docs/survey.md:42","message":"quoted text not found in the source text of reference 177","evidence":[{"path":"sources/text/177.txt","locator":"best match line 310, similarity 0.71"}],"data":{"reference":"177"},"at":"2026-11-07T03:00:00Z"}
```

| Field | Meaning |
| --- | --- |
| `tool` | Binary name |
| `rule` | Stable kebab-case id; documented in the tool's README |
| `severity` | `error` (a claim, record or result is wrong or unsupported), `warn` (likely problem, needs a person), `info` (measurement) |
| `subject` | What the finding is about: `path:line`, a reference key, a session id, a run id |
| `message` | One sentence |
| `evidence` | Zero or more `{path, locator}` pointers; `path` is relative to the project root when inside it. A checker may leave it out or write `null`, read as none; `lab check` stores it in every record, `[]` when empty (since lab 0.6.1) |
| `data` | Tool-specific object; optional |
| `at` | UTC RFC 3339 time the finding was produced |

## 3. Reference registry: `rf.reference/1` (JSON Lines)

`papers` owns the registry. Other tools only read it. The default location is `references.jsonl` next to `research.toml`; `[papers] registry = "<path>"` overrides it.

One line per catalogue entry, ordered by `key`. The key is the archive's stable identifier: the catalogue number as a string (`"177"`) for numbered catalogues, or the note id for one-note-per-paper archives.

```json
{"schema":"rf.reference/1","key":"177","type":"paper","title":"Utilizing Metadata for Better Retrieval-Augmented Generation","authors":["Raquib Bin Yousuf","Shengzhe Xu"],"year":2026,"venue":"arXiv preprint","ids":{"doi":null,"arxiv":"2601.11863","dblp":null,"openalex":null,"url":"https://arxiv.org/abs/2601.11863"},"version":"v1","accessed":"2026-10-31","depth":"method","texts":[{"path":"sources/text/2601.11863.txt","sha256":"…","origin":"https://arxiv.org/pdf/2601.11863v1"}],"duplicate_of":null,"source":{"path":"docs/research/references.md","line":3015},"verified":{"at":"2026-11-07","against":"arxiv","status":"match"}}
```

- `type`: `paper`, `dataset`, `software`, `web`, `standard` or `other`.
- `depth`: how far the archive inspected the source, from shallowest to deepest: `metadata`, `abstract`, `method`, `full`, `artifact`, `reproduced`. `method` covers reading the method or evaluation sections; `full` is the full text; `artifact` means released code or data was inspected; `reproduced` means a result was rerun independently.
- `texts`: local extracted-text copies a checker may quote against. Paths are relative to the project root. The `sha256` is of the text file. `origin` is the URL the text came from. The optional `retrieved` is the UTC time `papers read` downloaded it (RFC 3339).
- `duplicate_of`: the key of the surviving entry when two keys describe the same work. Keys are never renumbered or deleted.
- `verified`: the last metadata check against an index, or null.
- Optional fields are omitted when empty, and readers keep fields they do not know. `papers` adds `authors_truncated` (the source lists only the first authors), `author_variants` (an index's misspellings of the first author, accepted by `verify`) and `citekey`; its README describes them.
- Unknown values are `null`, never invented.

## 4. Provenance records: `rf.run/1` and `rf.manifest/1`

`prov` owns these. Other tools may read them. Their schemas are in the `prov` README.

## 5. Agent-session records: `rf.session/1`

`agentprof` owns these. Their schema is in the `agentprof` README. `lab retro` reads `agentprof`'s `rf.finding/1` findings (the `findings.jsonl` that `agentprof scan --out <dir>` writes, named by `[lab] agent_findings` or `--agent-findings`), so agent-session problems join the same pool as checker findings. `lab retro`, `lab brief` and `lab loop` count a rule's agent findings once per session tree: a session without a `parent_id` together with every session below it, within one `source`. A finding whose `subject` is not a session in the file counts on its own. They read the sessions file only when there are agent findings, and `lab retro --agent-findings FILE` reads the `sessions.jsonl` next to FILE. Without a readable sessions file each distinct finding counts, and they say why. `lab evaluate` also reads `source`, `id`, `role`, `start` and `parent_id` from the `sessions.jsonl` next to it (or `[lab] agent_sessions`), to measure a lesson about an agent-session rule per session within strata of the same `source` and `role` (a missing `role` is `unknown`). Within a stratum, the sessions below one root session (following `parent_id` up, within one `source`) form one session group, on the root's side of the adoption date; a session without a `parent_id` is a group of its own. It matches a finding to its session by the finding's `subject`, `source:id`. A lesson metric whose rule starts with `session.` names a numeric field of the record by its dotted path, such as `session.max_prompt_tokens` or `session.total.tokens.output`; a missing, `null` or non-numeric value leaves that session out.

## 6. Archive records and seals: front matter and `rf.archive-seal/1`

`lab` owns these. A record is a Markdown file that a `[[lab.archive.index]]` includes. It may start with YAML front matter: a first line `---`, YAML, and a line `---`. Everything after it is the record's body. Every field is optional, unknown fields are ignored, and a single string is accepted where a list is expected.

```markdown
---
id: D012
title: Use BM25 as the first-stage retriever
date: 2026-11-07
status: accepted
summary: BM25 beat the dense baseline on the pilot queries.
aliases: [lexical baseline]
supersedes: [D009]
decisions: [D010]
evidence: [docs/research/followups/2026-11-06-pilot/README.md]
tags: [retrieval]
---
```

| Field | Meaning |
| --- | --- |
| `id` | Stable identifier, such as `H158` or `D012`. When absent, the index derives it from the file name (`id_from`, `id_prefix`, `trim_leading_zeros`), the folder name for `README.md`, or else the file stem. |
| `title` | When absent, the first `# ` heading after the front matter, without a leading copy of the record's id and its separator (`# H158 — Moved` gives `Moved`). |
| `date` | `YYYY-MM-DD`. When absent, the first date in the file's path. |
| `status` | One of `[lab.archive] statuses` (default: `current`, `draft`, `proposed`, `accepted`, `open`, `resolved`, `superseded`, `rejected`, `withdrawn`, `historical`). The `inactive_statuses` (default: `superseded`, `rejected`, `withdrawn`, `historical`) mark records that no longer apply. |
| `summary` | One line shown in the index. |
| `aliases` | Other names `lab archive find` matches. |
| `supersedes`, `superseded_by`, `decisions` | Ids of other records. Each must exist, and the two supersession lists must agree. |
| `evidence` | Paths, relative to the project root, that support the record. |
| `tags` | Free labels. |

Each index file is generated: its first line is ``<!-- Generated by `lab archive update` from front matter. Do not edit by hand. -->``, and it lists one record per line.

The seal manifest (`[lab.archive] seal_manifest`, default `<state_dir>/lab/archive-seal.json`) records the append-only files. Each entry holds the length and SHA-256 of the file's body, so front matter, such as `status` or `superseded_by`, can change without breaking the seal. A body that still starts with the sealed bytes was appended to; any other change is an edit, which `lab archive seal --accept <path> --note <why>` records in `history`. The file is pretty-printed JSON with keys in a fixed order and a trailing newline:

```json
{
  "schema": "rf.archive-seal/1",
  "files": {
    "docs/research/log/2026-11.md": { "bytes": 18234, "sha256": "…" }
  },
  "history": [
    { "at": "2026-11-07T03:00:00Z", "path": "docs/research/log/2026-10.md", "old_sha256": "…", "new_sha256": "…", "note": "removed a student number pasted by mistake" }
  ]
}
```

An offloaded list (`[[lab.archive.offloaded]]`, since lab 0.9.1, whose older versions reject the key) records files moved out of the project, such as to cloud storage, that the archive still links. `list` is a committed plain-text file, a relative path below the project root without `.` or `..`, configured once. It holds one path per line, written like a link from the project root: a leading `/` or `./`, `.`, `..`, a trailing `/` and a byte-order mark are resolved or ignored, blank lines are skipped, and a line that leaves the root is an error. The list is not compressed. `location` says where the files went, such as `cloud:archive/2026-11-08/masters/`. `lab archive check` does not report a link as `broken-link` when its missing target is a listed path, lies in a listed directory, or is a directory that holds a listed path. It reports one `archived-link` info finding per list instead, with the list as its subject and `data` `{"links": N, "location": "…"}`. Its message names the location and changes only when the count reaches or leaves 0, so the finding keeps its fingerprint while links come and go; the text output adds the count. A missing target that no list holds, or one outside the project root, stays a `broken-link` error. A configured list that is missing or unreadable, or holds a line that leaves the root, stops the check with an error (exit 2). Only links honour the list: a sealed file moved out still fails `sealed-missing`.

## 7. Check runs, lesson evaluations and loop signals: `rf.labcheck/1`, `rf.evaluation/2`, `rf.signal/1`, `rf.loop/1`

`lab` owns these. [factory.md](factory.md#how-a-lesson-is-evaluated) explains the method.

```toml
[lab.evaluate]                 # all optional; these are the defaults
min_sessions = 10              # session groups, or check-run intervals, needed on each side
alpha = 0.05                   # two-sided; each direction is tested at alpha/2
baseline_days = 90             # baseline: observations this many days before adoption; 0 = all; at most 36500

[lab.loop]
measure = [["agentprof", "scan", "--out", ".research/agentprof", "--timezone", "Asia/Tokyo"]]
notify = ["sh", "scripts/notify.sh"]   # optional
renotify_hours = 24            # at most 87600
```

**`rf.evaluation/2`**: one per active lesson, printed by `lab evaluate --format jsonl` and stored in `last.json`. Version 2 (lab 0.4.0) counts agent sessions in session groups: `unit` `session-group`, and `sessions` on each side. lab 0.4 still reads version 1 records in an older `last.json`; lab 0.3 cannot read version 2. lab 0.5.0 adds `introduced` for check-run metrics, and lab 0.6.0 its `reviewed` and `latest_run`; older readers ignore them. lab 0.8.0 adds the unit `interval`, the method `conditional-binomial`, `measure`, and `findings` and `new_units` on each side; lab 0.7 and older cannot read a `last.json` that has the new unit or method, so update lab on every machine that shares the state directory.

```json
{"schema":"rf.evaluation/2","lesson":"read-by-section","lesson_status":"adopted","metric":"agentprof:file-reread","unit":"session-group","method":"cluster-permutation","verdict":"collecting","reason":"3 of 10 session groups after adoption, 540 of 10 before, in comparable strata","adopted":"2026-11-07","before":{"n":540,"sessions":612,"positives":5,"rate":0.008169934640522876,"mean":null,"median":null},"after":{"n":3,"sessions":4,"positives":0,"rate":0.0,"mean":null,"median":null},"p_lower":null,"p_upper":null,"strata":[{"source":"claude","role":"main","before":540,"after":3}],"excluded":12,"sessions_needed":295,"min_sessions":10,"alpha":0.05,"baseline_days":90}
```

| Field | Meaning |
| --- | --- |
| `metric` | `tool:rule`, or `null` for a lesson without one |
| `unit` | `session-group` (a root session, or the subagents below one root session), `run` (check runs without intervals) or `interval` (between two clean, measured check runs); `null` when nothing measures the metric. Version 1 records say `session` |
| `method` | `stratified-hypergeometric` (a finding rule, every group a single session), `van-elteren` (a `session.` field, every group a single session), `cluster-permutation` (either, when a group has several sessions), `descriptive` (check runs without measured intervals, described by `introduced`, with the verdict `collecting`), `conditional-binomial` (check-run intervals, see "Check-run denominators" below) or `null` |
| `verdict` | `collecting`, `improved`, `worse`, `inconclusive` or `unmeasurable` |
| `reason` | Why the verdict is `collecting` or `unmeasurable`. Otherwise `null`, except that a check-run lesson with a verdict notes a reviewed run that is no longer stored, intervals left out for another unit, or (since lab 0.9.0) intervals left out across a checker version change |
| `before`, `after` | `n` observations (session groups or check runs), `sessions` (the agent sessions in those groups; `null` for check runs), and `positives` (sessions with the finding) and `rate` (per session) for a rule, or `mean` and `median` (per session, or per check run); over the contributing strata only, so 0 when no stratum contributes yet. For intervals: `n` intervals, `findings` (introduced in them), `new_units` and `rate` (`findings` per new unit) |
| `p_lower`, `p_upper` | One-sided p-values for a decrease and an increase after adoption; `null` without a test |
| `strata` | Each contributing `(source, role)` with its `before` and `after` session-group counts |
| `excluded` | Session groups in strata without groups on both sides |
| `measure` | Intervals only: the `rf.measure/1` unit they count, such as `citing-sentence`; left out otherwise |
| `sessions_needed` | For a rule that is collecting or inconclusive: about how many more session groups without the finding would show a decrease |
| `introduced` | Check runs only, when a complete run exists since adoption: `since` (the first such run's `at`, UTC, or, after `lab lesson review`, the reviewed run's), `since_date` (its date in the project's time zone), `reviewed` (the reviewed run's file name when `since` is that run; left out otherwise), `latest` (the latest complete run's `at`), `latest_run` (its file name), `count`, and `findings`, one per message and data with more findings in the latest run than in the `since` run: `message`, `added` (the excess), `subjects` (where the new ones are: the latest run's subjects for that key whose fingerprint the `since` run lacks, or all of them when none does; at most ten) and `more` (subjects left out). Since lab 0.9.3, `version_changed` names each required check whose recorded `version` differs between the `since` run and the latest run, as `{"<check>": ["<old>", "<new>"]}`, since the checker update alone can add or remove findings; an unknown version on either side is no change, and the field is left out when nothing changed. It does not change `count`. The `lab evaluate` line and the `lesson-introduced` signal body then say the checker version changed, so a reviewer can tell findings from the update apart and record those with `lab lesson review`. Left out otherwise |

**`rf.signal/1`**: something to act on, sent to the notify command.

```json
{"schema":"rf.signal/1","project":"my-thesis","id":"lesson-verifiable:read-by-section","kind":"lesson-verifiable","lesson":"read-by-section","title":"Verify lesson read-by-section: its metric improved","body":"**Lesson `read-by-section`: …**\n\n…\n```sh\nlab lesson set read-by-section verified --note \"…\"\n```\n","detail":"improved","repeat":false,"reopened":false,"ref":null,"first_seen":"2026-11-20T03:00:00Z","at":"2026-11-20T03:00:00Z"}
```

| `kind` | `id` | When |
| --- | --- | --- |
| `lesson-verifiable` | `lesson-verifiable:<lesson>` | An adopted lesson's verdict is `improved`: verify it |
| `lesson-worse` | `lesson-worse:<lesson>` | An adopted or verified lesson's verdict is `worse`: revise the rule or retire it |
| `lesson-introduced` | `lesson-introduced:<lesson>` | An adopted or verified check-run lesson has findings introduced (unless another signal above is sent for it; `lesson-verifiable` then lists them) since adoption, or since its last review; the body lists them. Its detail is the run they are counted from, the messages and their counts, so a change, including a review, sends it again (once `renotify_hours` after the last send) |
| `lesson-unmeasurable` | `lesson-unmeasurable:<lesson>` | An adopted lesson's verdict is `unmeasurable` (a verified one stays verified on its other evidence): choose a metric that can show an effect, or decide on other evidence with `--force` and a note |
| `uncovered-rules` | `uncovered-rules` | `lab retro` finds recurring rules without a lesson; one signal lists them all |
| `measure-failed` | `measure-failed:<index>` | A `measure` command failed; the body has its command, status and last output lines |

`title` is imperative and at most 72 characters; `body` is standalone Markdown with the numbers, the method and the `lab` commands to act. `id` is stable, and `detail` is a short summary whose change makes the signal worth sending again. `repeat` marks a changed signal sent again and `reopened` one that had been resolved; both carry the `ref` the notify command returned for the first send. A notify command should add to that ref while it is still open and file anew when it is closed or gone. `first_seen` is when `lab loop` first produced the id.

**Notify protocol.** For each signal to send, `lab loop` runs the `notify` program once, from the project root, with the signal as one JSON object on stdin, and waits up to 60 seconds. Exit 0 means sent: the first non-empty line of stdout (trimmed, at most 200 characters), if any, becomes the signal's `ref`, such as a task id, and is passed back in later sends. Any other exit, or a timeout, leaves the signal unsent; the next loop retries it. A program with a directory part resolves against the project root, as for checks.

**Dedupe.** `<state_dir>/lab/loop/signals.json` (`rf.loop-signals/1`) keeps, per id, `first_seen`, `last_notified`, `detail_hash`, `ref`, `resolved`, `error` and `pending`. A signal is sent when its id is new, when it was resolved and appears again at least `renotify_hours` after its last send (`reopened`; sooner it is flapping and stays quiet), when its detail changed and it was last sent at least `renotify_hours` ago (`repeat`), and on every loop until a send succeeds. An id missing from a loop is resolved. Without a `notify` command the state is still kept, and the signals stay `pending`.

**`rf.loop/1`**: `<state_dir>/lab/loop/last.json`, written atomically at the end of each loop, also after a `lab` error (in `errors`).

```json
{"schema":"rf.loop/1","started":"2026-11-20T03:00:00Z","finished":"2026-11-20T03:00:41Z","measure":[{"argv":["agentprof","scan","--out",".research/agentprof"],"ok":true,"status":"exit 0"}],"evaluations":[],"signals":[{"id":"uncovered-rules","kind":"uncovered-rules","status":"unchanged"}],"errors":[]}
```

A measure's `status` is `exit N`, `killed by a signal`, `timed out after 600 s` or `could not start: …`; a signal's is `notified`, `pending` (no notify command), `failed` (retried next loop) or `unchanged`. The same directory holds `lock` (an exclusive OS file lock held while a loop runs and released when its process exits, so a loop that died never blocks the next; the file names the holder's pid) and `loop.log` (the measure commands' output and the background loop's own; moved to `loop.log.1` past 1 MiB). A measure or notify command that times out is killed with its whole process group. An error after the measure commands, such as an unreadable signal state, is recorded in `last.json`, so `--if-older` backs off and the brief shows it. `lab loop` exits 0, 1 when a measure or notify command failed, and 2 on a `lab` error.

**`rf.labcheck/1`**: one file per `lab check` run in `<state_dir>/lab/checks/`, named after the time it was stored (`<YYYYMMDDTHHMMSS.mmmZ>.jsonl`, with `-1`, `-2` on a clash). The first line is the run header; each further line is an `rf.labcheck.item/1` record with the `check`, the finding's `fingerprint` and the `finding` itself, or, since lab 0.8.0, an `rf.labcheck.measure/1` record (see "Check-run denominators" below). Since lab 0.7.0, readers skip a further line whose `schema` is another one, so a later version can add record kinds; older versions refuse such a run.

```json
{"schema":"rf.labcheck/1","at":"2026-11-20T03:00:00Z","commit":"04d97b1c…","dirty":false,"checks":[{"name":"claims","command":["claimcheck","check","--format","jsonl"],"exit":1,"status":"ok","findings":12,"unparsed":0,"duration_ms":5321,"version":"claimcheck 0.2.0"}]}
```

| Field | Meaning |
| --- | --- |
| `at` | UTC start time of the run |
| `commit` | `git rev-parse HEAD` of the repository holding the project root when the run started (since lab 0.7.0); `null` outside git or before the first commit. Runs stored by older versions have neither field and load as `null` |
| `dirty` | Whether files under the project root differed from `commit` when the run started (since lab 0.7.0): tracked changes, or untracked files that git does not ignore, whatever `status.showUntrackedFiles` says, leaving out the state directory. A project root that the repository ignores is always dirty, since no commit holds its files. Checks read the work tree, so a dirty run's findings belong to `commit` plus uncommitted text; `null` when `commit` is. lab reads both with `git --no-optional-locks status`, so a background run never holds the index lock; a git failure other than "not a git repository" is printed and leaves both `null` |
| `replayed` | `true` for a run of a past commit by `lab check --at` (since lab 0.9.0; see "Check-run denominators" item 6); left out otherwise. Its `commit` is that commit, `dirty` is `false` and `at` is the commit's committer time, while the file name stays the time it was stored |
| `checks` | Each check that ran: `name`, `command`, `exit` (`null` when it could not start or was killed), `status` (`ok` for exit 0 or 1, else `failed`), `findings`, `unparsed` (stdout lines that were neither findings nor valid `rf.measure/1` records), `duration_ms`, `version` and, in a replayed run, `uncommitted_inputs` |
| `version` | Since lab 0.9.0: the checker's version, the first non-empty stdout line (at most 200 characters, not a JSON object) of the check's `version` command, run where the check runs (the project root, or a replayed commit's copy of it), with its program resolved as the check's, and a 10-second limit. Without `version` in `[[lab.checks]]`, the command is `<program> --version` when the program is a bare name and not an interpreter or wrapper (any name starting with `python`, and `sh`, `bash`, `zsh`, `dash`, `ksh`, `fish`, `env`, `uv`, `uvx`, `pipx`, `poetry`, `pdm`, `hatch`, `conda`, `node`, `npm`, `npx`, `pnpm`, `yarn`, `bun`, `bunx`, `deno`, `perl`, `ruby`, `bundle`, `cargo`, `go`, `java`, `make`, `just`, `timeout`, `nice`, `nohup`, `time`, `xargs`, `docker`, `podman`); `version = []` means none. `null` when unknown: no command, or one that failed, timed out, kept its output open after it exited or printed no version line (lab then prints a warning). Runs stored by older versions have no field and load as `null`. lab 0.8.0 and older refuse a `[[lab.checks]]` entry with `version` or `uncommitted_inputs` |
| `uncommitted_inputs` | A replayed run's paths, relative to the project root, that the check read from the current work tree because the commit lacked them; left out when empty |

**Check-run denominators: `rf.measure/1`.** Since lab 0.8.0, a check-run lesson can get a verdict. A count of introduced findings says nothing without the amount of new text it came from: ten introduced findings in one new sentence and in a thousand are different results. So a checker can report what it checked, and `lab evaluate` compares findings introduced per new unit of text.

1. *The denominator record.* A checker that supports a `--measure` flag prints one `rf.measure/1` line per unit it checks beside its findings. Add the flag to the check's `run` in `research.toml`; checkers without it keep their current output.

   ```json
   {"schema":"rf.measure/1","tool":"claimcheck","unit":"citing-sentence","count":412,"keys":["5f1c09a2e4b7d013", "…"]}
   ```

   `unit` names what is counted (for claimcheck, `citing-sentence`, sentences that cite a reference, and since claimcheck 0.2.0 `uncited-sentence`, the checked sentences that do not; each sentence is in one of the two), and `count` how many the run saw. `keys`, optional, are the sorted, distinct first 16 lowercase hex digits of the SHA-256 of each unit's normalized text; there are at most `count` of them. They let lab count new units exactly, as it keys introduced findings by message and data rather than by path. `lab check` prints what each check measured and stores each record as an `rf.labcheck.measure/1` line, `{"schema":"rf.labcheck.measure/1","check":"claims","measure":{…}}`, after the findings. It does not count these lines as `unparsed`. A check that did not run to completion stores no measure, and a run that is not clean (`dirty` not `false`) stores no keys, since it can never end an interval. A line whose schema starts with `rf.measure/` but is not a valid version 1 record (no tool or unit, a malformed, unsorted or repeated key, or more keys than `count`) counts as `unparsed` and fails the check. lab 0.7.0 skips stored measure lines; older versions refuse such a run.
2. *New units.* Between two runs, a check's new units are the keys in the later run that the earlier one lacks. Without keys on both sides, they are the count's increase, at least 0. That misses text that replaced deleted text, while the findings in it still count, so revising text raises the rate; checkers should print keys. A run's new units are the sum over the required checks (those that report the metric's tool).
3. *Intervals.* A run is an interval end when it is complete, has `dirty` `false` and every required check measured the metric's tool in it. Only the last such run on each `commit`, in the order runs were stored, counts, so a commit is measured once and uncommitted text never is; a replay of a commit replaces its earlier runs. Ordered by `at`, consecutive ends form an interval with `introduced` (counted as for `introduced` above, between the interval's two runs) and `new_units`. An interval belongs to the side of adoption of its later run's date, within `baseline_days` before adoption. Intervals with no new units are left out, with the findings introduced in them. So are intervals across a change of a check's unit, and intervals in another unit than the latest interval's, so a renamed unit gets a new baseline instead of mixing rates. Since lab 0.9.0, so are intervals across a change of a required check's version, since a checker change moves the counts by itself; the reason counts them (`N interval(s) across a checker version change left out`). Each interval end takes, per check, the last known version up to it, and an interval crosses a change when a check has a known version at both ends and they differ. So a `null` version, as in runs stored before lab 0.9.0, still forms intervals, and an unknown version between two known ones does not hide their change. A check's records in one unit add their counts and keys. Since lab 0.9.2, each check-run lesson counts one unit per check, chosen by its metric, never by record order. A rule with its own unit counts only that unit: `claimcheck:uncited-claim` counts `uncited-sentence`, since it fires only on sentences that do not cite, and `claimcheck:evidence-label` counts `uncited-sentence` for now, though it also fires on paragraphs that cite. A run whose check did not print the rule's unit, such as one from claimcheck before 0.2.0, ends no interval for that rule. Any other rule counts the one unit its check printed, or, when the check printed several, its tool's unit (`citing-sentence` for claimcheck); a check that printed several units of a tool without one ends no interval. An interval across a change of a check's unit is left out, and the reason counts it (`N interval(s) across a change of unit left out`). So when claimcheck starts printing `uncited-sentence`, an `uncited-claim` lesson gets a new baseline, which `lab check --at` can replay. lab 0.9.1 and older end no interval at a run whose check printed several units, so with claimcheck 0.2.0 they give no check-run verdict. Each interval is one observation counted against `min_sessions`.
4. *The comparison.* The rate on each side is Σ introduced / Σ new units over its intervals. Given the total introduced `N = a + b`, adoption changing nothing makes the after count `b` binomial with `N` trials and probability `U_after / (U_before + U_after)`, from the new units on each side. `p_lower` is `P(X ≤ b)` and `p_upper` is `P(X ≥ b)`. The verdict is `collecting` until both sides have `min_sessions` intervals, `improved` when `p_lower < alpha/2`, `worse` when `p_upper < alpha/2`, and otherwise `inconclusive`. The evaluation then has `unit` `interval`, `method` `conditional-binomial`, `measure` (the unit's name), and on each side `n` intervals, `findings` (Σ introduced), `new_units` and `rate`. It keeps `introduced` and its `lesson-introduced` signal with any verdict.
5. *Without intervals.* With no interval yet, the lesson stays `collecting` with method `descriptive`, and the reason says what a verdict needs. With fewer than `min_sessions` intervals before adoption, it stays `collecting`, and the reason starts `no baseline:`. It is not `unmeasurable` as for sessions: replaying past commits (below) can still supply the baseline.
6. *Replaying past commits.* `lab check --at <commit>`, repeatable and accepting a range `A..B` (the commits `git rev-list --reverse A..B -- .` lists from the project root: those that changed the project, without `A` itself, so `A^..B` includes it), rebuilds the baseline of lessons adopted before their checker printed a denominator, such as masters' fetch-cited-texts and link-own-measurements. For each commit, lab checks it out into a temporary git worktree (`git worktree add --detach` in the system temporary directory, with hooks off), runs the selected checks and their `version` commands in the commit's copy of the project root, with only the program (`run[0]`) resolved against the current project root, and stores the run with that `commit`, `dirty` `false`, `at` the commit's committer time and `"replayed": true`; the file name stays the store time. It removes the worktree and its registration when the commit is done, also when a step fails or panics, and before replaying removes the worktrees that a `lab` process stopped by a signal left (named `lab-replay-<pid>-…`, with that process gone); other worktrees are never pruned. A commit without the project root is skipped with a note, and a replay in which no commit has it is an error. Each run is printed as it is stored. A replay with `--only` warns that a run counts only when every configured check reporting the metric's tool ran in it. Since `lab evaluate` orders interval ends by `at`, replayed runs slot in before adoption. They serve only as interval ends: `introduced`, the per-run means, `lab brief`, `lab retro` and `lab baseline set` leave them out, since they check past text again. A check whose inputs no commit holds, such as git-ignored source texts, cannot be replayed faithfully. Listed as `uncommitted_inputs` in `[[lab.checks]]`, those that the commit lacks are linked from the current work tree, as are the entries of a listed directory that the commit holds only part of, such as its `.gitignore`, and the stored check names the inputs it linked in `uncommitted_inputs`; unlisted, they are missing and the check sees no such files. The links point into the work tree, so a check that writes to such an input changes the work tree. lab 0.8.0 and older ignore `replayed` and read such a run as a live one, so update lab on every machine that shares the run directory before replaying.
7. *Limits.* Intervals are treated as independent, and findings as independent within them. Interval ends are ordered by time, not by commit ancestry: when runs from several branches share the run directory, an interval from a branch tip to another branch's commit counts the other branch's text and findings again. When one interval holds most findings, a permutation test over whole intervals, like the session-group test, should replace the binomial one. Text written between adoption and the first run after it falls in an interval that straddles adoption; its later run assigns it to the after side. A checker or checker-version change between two runs moves counts by itself; intervals across a recorded version change are left out, but a checker without a known version, or a change in its configuration or in `research.toml`, goes unseen. Replayed runs use today's checkers on past text, so an interval between the last replayed run and the first live run usually crosses a version change and is left out. A replay that read uncommitted inputs from today's work tree may count findings that the commit's text did not have, or miss some.

## 8. Conventions every repository follows

- Rust edition 2024, `rust-version = "1.99"`, stable toolchain. MIT OR Apache-2.0 licence files.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass, and a GitHub Actions workflow runs them.
- Network access happens only in explicit commands, goes through a client that spaces requests per host and retries 429 and 5xx with backoff, and identifies itself with a generic user agent (`<tool>/<version>`). No email address or personal data in requests.
- Tests use synthetic fixtures. Nothing from a private project, no personal data and no absolute home-directory paths enter a repository.
- Errors name the file and line or the record they concern. No silent fallbacks: a value a tool cannot establish is reported as unknown.
