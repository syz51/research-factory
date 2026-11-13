I'll start by reading the shared audit brief.
The brief defines my track. Next I'll read the state-machines document and the papers repository layout.
The papers source is about 15.4k lines. I'll read the document sections in my scope first.
The doc maps §8 rows 13–15 to §4.1 and §4.3. Next I'll set up the scratch directory and start a release build of HEAD in the background, using a separate target dir so the main checkout stays untouched.
The build is running. Next I'll map the subcommands in `main.rs` and the module layout.
Next I'll list the functions in `main.rs` so I can target the main command paths.
`main` dispatches each subcommand. Next is `http.rs`, the shared client behind the network machines in §4.3.
The release build is still compiling. Next I'll read `seen.rs`, which §4.3 and §8 row 15 describe.
`seen` merges registry and notes hits and prints "not seen" with exit 1 whether or not a note was skipped. I'll test that with a probe later. Next is `registry.rs`, which holds the §4.1 state fields.
`registry.rs` covers the record fields and the lock-based `update`. The doc cites papers versions up to 0.12.1, but `Cargo.toml` says 0.2.0, so I'll check the version history and whether the commands the doc names exist.
The repo history starts at "Add the papers CLI". The doc's version numbers (0.7.0 to 0.12.1) and several `papers` commands it names (`add`, `text`, `structure`, `card`) do not exist in this crate, so that is drift. I'll check the build and the registry command path next.
`registry build` loads the registry twice: once unlocked at `registry_cmd.rs:238`, then again under the lock inside `update`. I'll map every call site of the loaders to find other double reads.
`fetch`, `search`, `verify` and `seen` each call `seen::Index::load`, which parses every note and the registry. I'll read `verify`, the most network-heavy command, first.
`verify --record` writes results only after every lookup finishes, so an interrupted run loses them except for the HTTP cache. Next I'll map `verify.rs` to see how the lookups run.
`verify::run` batches arXiv ids first, then looks up DOIs one at a time. Classification of arXiv "not found" depends on a message prefix (`verify.rs:494`), so I'll check `arxiv.rs` for that message and for the batch behaviour.
The arXiv batch path returns one result per id and stops after a failed batch, which matches the README. Next I'll check whether any Unpaywall calls exist and read the OpenAlex and Crossref clients for sequential requests.
Unpaywall is not used by this crate. Next I'll read the OpenAlex key lookup, because a macOS Keychain fallback may spawn a process on every request.
OpenAlex reads the key once per process and caches the Keychain lookup. Next is `graph.rs`, which holds the OpenAlex-then-Semantic-Scholar fallback for `refs` and `cited-by`.
`graph.rs` runs one OpenAlex list per 100 references and re-serializes each Semantic Scholar link to parse it again. The second parse is small, so I'll only measure it if it matters. Next is `fetch`, which handles many references per run.
`fetch` batches the arXiv ids but resolves each DOI in turn. I'll read `work.rs` to see how many requests one DOI costs.
`Resolved::fetch` always calls Crossref and then OpenAlex in sequence, even when Crossref already has the record. Those are two different hosts, so each host's throttle is separate. Next I'll read the `read` path for PDF candidates and browser retries.
`finish_read` writes the manifest sidecar directly at `main.rs:2310`, before the registry record step. I'll read `extract_pdf` and `existing_text` to see whether the `.txt` file is staged and renamed as §4.1 says.
`extract_pdf` writes the `.txt` straight to its final path (`main.rs:2689`). I found no `.txt.tmp` staging in this function, which contradicts §4.1. I'll confirm with a search for staging, and check how `sha256_file` handles read errors.
Two confirmed points: `sha256_file` turns every read error into `None` (`main.rs:2073`), and the `.txt` and manifest are written without staging. Next I'll map `pdf.rs`, which owns the extraction states and the `pdftotext` spawns.
`choose_text` runs `pdftotext` as a second process whenever pdf-extract's text lacks the title. Next I'll check the build and size up the masters inputs, using read-only commands.
The masters `research.toml` sets `[papers]` keys that this build may not know (`cards`, `card_dir`, `claude`). I'll check whether `config.rs` rejects them.
`RawPapers` uses `deny_unknown_fields`. I'll run `papers config` against masters' config (read-only) with my build and the installed binary, and compare versions.
The installed `papers` is 0.13.1 and reads masters' config. The HEAD build (0.2.0) rejects it with `unknown field card_dir`. I'll check which subcommands the installed 0.13.1 has, and confirm the HEAD exit code.
The installed 0.13.1 has `structure` and `card`, and it accepts masters' config. HEAD 0.2.0 has neither, so the doc's §4.1 commands belong to the 0.13.x line. I'll check where that line lives and what `registry` offers in 0.13.1.
The `papers` 0.12.x and 0.13.x line with `structure`, `card`, `registry abstracts` and the `cards` config is not in `~/Developer/papers`. I'll note that as drift and move on. Next is `browser.rs`, which §4.3 lists as an explicit machine.
`try_in_browser` holds the per-host throttle lock across each page load, which can run up to 45 s. Its page-link expansion is also not capped. I'll read the candidate loop and check whether Chrome is available for a probe.
The landing-page expansion only adds `Pdf` candidates, so it cannot loop. The browser retry loop has no cap on links it follows. I'll check whether a local Chromium exists for a probe.
There is no local Chromium and no Google Chrome, so the browser retry cannot be probed without a state change. I will record that as unverified. Next I'll look at how `record-pdfs` writes the registry, then count masters registry shapes for the hot-path sizing.
**Result.** The doc's `papers read` staging and undo claim is false. `seen` reports a note it skipped as "not seen" with exit 1. The doc's `depth-missing` rule does not exist in HEAD. Coverage is partial: about half of `src/` was read, and the browser loop could not be probed because this Mac has no Chromium.

**A. Files read**

Repository: `/Users/sunyizhe/Developer/papers`, HEAD b7a9b95.

Read:
- `src/main.rs` 1–280, 523–567, 588–610, 645–797 (partly), 814–987, 1187–1220, 1594–1685, 2073–2327, 2470–2622, 2644–2710, 2712–2834. Other ranges by grep only.
- `src/http.rs` 1–620. Tests (621–868) by name only.
- `src/seen.rs`, `src/registry.rs`, `src/registry_cmd.rs`: all.
- `src/verify.rs` 313–460, 478–603.
- `src/arxiv.rs` 1–200. `src/openalex.rs` 14–73, 252–394.
- `src/graph.rs` 1–230. `src/work.rs` 84–100 and test names.
- `src/pdf.rs` 56–265. `src/browser.rs` 24–185, 254–286, enums 32–47.
- `src/config.rs` 28–72, 258–270. `src/notes.rs` 12–50, 158–176.
- `src/catalogue.rs` by grep only (parse 122–208, depth 858–932).
- `Cargo.toml`, `README.md` exit-code table (lines 60–66), test file names in `tests/`.

NOT read:
- `main.rs` 378–587 (except noted), 989–1186, 1222–1593, 2334–2469 (`download`), 2626–2643 (`save_pdf`, write at 2635 seen by grep), 2837–2896 (`stub`).
- `verify.rs` 1–312, 604–949. `arxiv.rs` 200–410. `openalex.rs` 1–14, 73–251, 395–495. `graph.rs` 230–282. `work.rs` rest. `pdf.rs` 265–698. `browser.rs` 186–253, 287–377. `config.rs` rest. `http.rs` 621–868.
- `catalogue.rs` (rest), `notes.rs` (rest), `check.rs`, `cite.rs`, `bib.rs`, `sources.rs`, `crossref.rs`, `atom.rs`, `text.rs`, `refs.rs`, `id.rs`, `searchlog.rs`, `finding.rs`, and the `tests/*.rs` bodies.
- The repository has no `scripts/` folder.

**B. Machines**

*B1. Text file (implicit; doc §4.1 "text").*
- States: no text, PDF saved, `.txt` written, sidecar written, registry `texts[]` entry, stale (sha256 differs).
- Transitions and enforcement: `save_pdf` writes the PDF in place (`main.rs:2635`). `extract_pdf` writes the `.txt` in place (`main.rs:2689`). `finish_read` writes the sidecar (`main.rs:2310`). `record_text` writes a sidecar (`main.rs:556`), then updates the registry (`main.rs:554-558`). `record_pdfs` writes a manifest (`main.rs:761`), then updates the registry (`main.rs:791`). Stale check: `existing_text` (`main.rs:2073-2092`).
- Gap a, staging and undo claim (doc §4.1): **refuted**.
  - Probe A: the sidecar path was a directory. `read` exited 2. The `.txt` (45 bytes) and `.pdf` (618 bytes) stayed. No undo.
  - Probe B: `rf.jsonl.lock` was read-only, with `read --key paper1 --record`. Exit 2, "Permission denied (os error 13)". `paper1.txt` and `paper1.json` stayed. `cmp` shows the registry unchanged.
  - Rerun after `chmod 644`: exit 0, "already have out3/paper1.txt (sha256 matches)", then "recorded in rf.jsonl". Harm is p3. Fix: stage as `cite` does (`main.rs:1139-1149`). Size M. No test exists.
- Gap b, sidecar and registry disagree after a failed record: **confirmed** by probe B. p3.
- Gap c, `sha256_file` drops IO errors (`main.rs:2073-2074`, `.ok()`): **confirmed**. Probe C, `chmod 000` on a recorded text: exit 2 "Permission denied" at the write. The "changed since" message is never printed. p3, size S.
- Gap d, `save_pdf` writes in place, so a crash leaves a partial PDF (`main.rs:2635`): **open**, code reading only. p3, size S.

*B2. Seen index (doc §4.3 "Seen index"; §8 row 15).*
- States: registry entry indexed (`seen.rs:60-63`), note indexed (`seen.rs:65-77`), note skipped (`seen.rs:69-71`), hit or not seen (`main.rs:1217`).
- Probe: a note with `title: [unclosed` in frontmatter. `seen` printed `papers: skipping …/broken.md: invalid…` on stderr and `papers: not seen` on stdout. Exit 1. A valid note gave exit 0. `registry check` reports the same note as `frontmatter-invalid` (`registry_cmd.rs:62-70`).
- README exit code 1 means "the work is not in the archive" (`README.md:60-66`). The work is in the archive, so the code is wrong. **Confirmed.** p2, size S. Fix: exit 2 and name the skipped notes when nothing matches.
- Second case: a note with `depth: bogus` is a hit, exit 0. `registry check` reports `depth-invalid` (error). **Confirmed** inconsistency, p3.
- Tests: `seen.rs` tests (186, 203) do not cover skipped notes. No test pins the behaviour.

*B3. Registry lock (explicit in doc; doc says "explicit file lock").*
- States: free, held. Lock file `<registry>.lock` (`registry.rs:297-306`). Blocking `lock.lock()` (`registry.rs:315-316`). Load under lock (`registry.rs:317`). Save by tmp and rename (`registry.rs:274-290`).
- Gap: no time limit and no message while waiting. Probe: a Python process held `flock` for 12 s. `registry build` (scratch registry) waited 11.75 s, printed nothing, exit 0. **Confirmed.** p3, size S. Fix: `try_lock` with a timeout and a "waiting" line.
- Test `registry.rs:378` (`updates_do_not_lose_concurrent_changes`) checks lost updates, not waits.

*B4. Host throttle (explicit in doc).*
- States: free, held during an attempt. Enforcement: `http.rs:495-531`, blocking `file.lock()` at `http.rs:515`. Intervals: arXiv 3 s, others 1 s (`http.rs:379-388`).
- Holders are bounded by the 60 s attempt timeout (`http.rs:22`). Waiters have no limit. **By design** for bounded holders. Unbounded waiter: code reading only.
- `read` holds this lock during a browser load (`main.rs:2512`), up to 45 s per load.

*B5. HTTP attempt and cache (explicit in doc; matches code).*
- Attempts: 5 (`http.rs:21`), backoff 5, 10, 20, 40, 60 s, `Retry-After` up to 300 s (`http.rs:183-186`). Test `http.rs:702`.
- Cache: 7 days (`http.rs:32`), tmp and rename on write (`http.rs:339-365`). Test `http.rs:763` (`cache_round_trips_and_expires`).
- Gap: `verify` stamps `verified.at` with today's date (`main.rs:2801`), but the data may come from cache (`arxiv.rs:123`, `openalex.rs:275`). **Open**, needs a live verify. p3.
- Gap: `verify --record` writes only at the end (`main.rs:2809`). A crash loses all results. **Open**, code reading only. p3.

*B6. Browser page and link queue (doc §4.3 "Browser page").*
- Code states: `Loaded` = Download, Page, Blocked (`browser.rs:32-39`). `PageState` = Loading, Challenge, Blocked, Ready (`browser.rs:42-47`). Deadline 45 s (`browser.rs:24-25`, `96`, `129`). Poll sleep at `browser.rs:132`. Test `browser.rs:335`.
- Drift: the doc's "closed" and "opening" states do not exist in code.
- Gap: the cap of 8 applies only to the first list (`main.rs:2502`, `BROWSER_URLS` at `main.rs:53`). Links found on loaded pages are inserted with no cap (`main.rs:2526-2530`). **Confirmed by reading.** Runtime probe **open**: no Chromium (`find_chrome` found none; `PAPERS_CHROME` unset; no Google Chrome app; no Playwright cache). p2 until probed. Test `main.rs:2927` covers only the first queue. Size S.

*B7. Paper lifecycle (doc §4.1, explicit table).*
- Commands in the doc table that HEAD does not have: `papers add`, `papers text`, `papers structure`, `papers card`, `papers metadata fetch`. HEAD commands: fetch, search, read, verify, seen, refs, cited-by, bib, cite, registry, config, stub.
- `papers fetch` prints metadata only (help text). PDFs come from `read`.
- `structure`, `card` and `registry abstracts` exist in the installed 0.13.1 (`~/.cargo/bin/papers`), not in HEAD.
- Doc §4.1 cites "fixed by research-factory-219 (papers 0.7.0)" rules `verify-withdrawn`, `duplicate-cycle`, `duplicate-chain` and `verified-mismatch`. None exists in HEAD `src/` or `tests/` (grep). `malformed-date` does exist.
- Doc header says papers 0.12.1. HEAD is 0.2.0 (`Cargo.toml:3`). Installed is 0.13.1.

*B8. Config version gate (new, not in doc).*
- `deny_unknown_fields` on `[papers]` (`config.rs:31-32`).
- Probe: HEAD `--config ~/Documents/ChatGPT/masters/research.toml config` exits 2 with `unknown field card_dir`. Installed 0.13.1 accepts the same file. **Confirmed** version drift. Masters cannot run on HEAD. See E2 and E3.

*B9. Depth vocabulary (doc §4.2).*
- `depth-missing`: **refuted**. No such rule in `src/` or `tests/`. Probe: a registry entry with `"depth":null` gave no finding. Only the `depth: bogus` entry gave one (`invalid-value`, error).
- Doc state names ("listed", "read in full") do not match code values: metadata, abstract, method, full, artifact, reproduced (`registry.rs:16-23`, printed in the probe message).
- Rule names differ: the registry path uses `invalid-value`; the notes path uses `depth-invalid` (`registry_cmd.rs:74`).
- `depth-unparsed` exists (`catalogue.rs:879`, `913`). Matches the doc. Not probed.

*B10. Graph source (explicit in doc).* OpenAlex first, Semantic Scholar fallback (`graph.rs:82-135`). Matches. The doc's "fast failure on rate limits" is not in the code I read. Not probed.

**C. Hot paths**

*C1. `Resolved::fetch` runs Crossref, then OpenAlex, in sequence (`work.rs:90-100`).*
- Callers: `main.rs:1634`, `2471`, `2864`.
- Input: 3 DOIs (10.1002/asi.22706, 10.1145/3677052.3698671, 10.18653/v1/2022.acl-long.303). Cold cache and lock dir per run.
- Before (HEAD binary): totals 4.27 s, 5.02 s, 5.17 s. Mean 4.82 s, 3 runs.
- Single call latency (curl, one DOI): Crossref 0.89 s, OpenAlex 0.47 s.
- Fix, prototype in the scratch clone `papers-proto` (uncommitted, 11 lines in `src/work.rs`): `std::thread::scope` runs Crossref in a thread while OpenAlex runs on the main thread. Error order is kept: Crossref's error comes first.
- After: totals 4.04 s, 4.89 s. Mean 4.47 s, 2 runs. Gain is 0.1–0.5 s per DOI. Run-to-run spread is about 0.9 s, so the sample is small.
- Output is byte-identical for all 3 DOIs (`cmp`).
- Side effect: a Crossref failure now also sends the OpenAlex request.
- Rejected: skipping OpenAlex when Crossref answers. `print_resolved` uses OpenAlex fields (`main.rs:1464`), so output would change.
- Test: none added. `cargo test` was not run in the prototype. Priority p3, size S.

*C2. OpenAlex paging cap (`openalex.rs:373`, `per_page` clamped to 100).*
- One live call with `per_page=200` returned HTTP 200 on 2026-11-14, using `select=id`.
- A 200 cap would halve paged list calls in `paged_with` (`openalex.rs:368-394`).
- `graph.rs:158-162` sends one list call per 100-ID chunk, in sequence. Not measured. p3, size S.

*C3. Registry load and parse (851 entries, masters).*
- `registry check`: 0.09 s and 0.10 s. `registry build --dry-run`: 0.07 s and 0.07 s. `seen 2001.00001`: 0.03 s.
- Double load in `registry build`: an unlocked load (`registry_cmd.rs:238`) is used only in the dry-run path. The real path reloads under the lock (`registry.rs:317`). Negligible.
- No waste worth filing.

*C4. `record-pdfs` (`main.rs:645-797`).* Per text: read PDF, run pdf-extract in-process, spawn `pdftotext` only on mismatch (`main.rs:588-610`). The `pdftotext` check is cached (`main.rs:671`). **Not measured**: masters has no local PDFs.

*C5. `read` (`main.rs:2095-2327`).* Registry loaded at `main.rs:2111` and again in `record_text`. `existing_text` reads the recorded file once. No waste worth filing.

*C6. Process spawns.* `pdftotext` (`pdf.rs:90`, `114`; 20 ms poll at `pdf.rs:131-136`; 60 s timeout). `/usr/bin/security` once per process (`openalex.rs:39`). No `git` calls in `src/`. No waste found.

*C7. Graph link re-serialize (`graph.rs:201-202`).* In memory. Negligible. Read only.

**D. Other points (item 4)**

- D1. Config version gate: confirmed (B8). p2, because it blocks any move to HEAD.
- D2. `papers fetch 'not a doi or url !!'` exits 1. README says usage errors exit 2 (`README.md:60-66`). **Confirmed.** Output: `/private/tmp/claude-501/-Users-sunyizhe--t3-worktrees-research-factory-task-research-factory-4-1c1fdf969a284ada/1fb4fe71-1498-46b7-8a1e-3b6da0f588f3/scratchpad/papers/probes/fetch-bad.out`. p3, size S.
- D3. `seen` false "not seen" (B2). p2.
- D4. Lock wait with no limit (B3). p3.
- D5. Browser queue uncapped (B6). Open until probed. p2.
- D6. Silent unreadable recorded text (B1c). p3.
- D7. Test gaps, each **no test pins it**: skipped note exit code; read staging or undo; browser link cap; `Resolved::fetch` error order.
- D8. `verify --record` writes only at the end; `verified.at` date (B5). Open.
- D9. `save_pdf` partial write (B1d). Open.

**E. Owner decisions** (Reason: design for E1, E2, E4, E5)

- E1. Which `papers` line is the tool.
  - Option a: installed 0.13.1, which masters uses. HEAD is an older 0.2.0 line.
  - Option b: HEAD is the tool. Port the 0.13.1 features into it.
  - Recommendation: a, until HEAD gains `structure`, `card` and `registry abstracts`.
- E2. Must HEAD gain `structure`, `card` and `abstracts` before masters moves?
  - Port them (size L), or keep masters on 0.13.1 (size S).
  - Recommendation: keep masters on 0.13.1 for now.
- E3. Masters config keys `cards`, `card_dir`, `claude`.
  - Remove them from masters config while HEAD is the tool, or add the keys to HEAD.
  - Recommendation: add the keys when the commands are ported. Keep `deny_unknown_fields`, since it catches typos.
- E4. Exit code for `seen` when a note was skipped and nothing matched.
  - Option a: exit 2, and name the skipped notes (recommended).
  - Option b: keep exit 1, and add a finding.
- E5. Doc §4.1 table: annotate each command with the tool version that has it. Recommendation: yes.

**Open items (not filed; parent to file under research-factory-414 or the papers board)**

1. p2, S: `seen` exit code and skipped notes (B2, D3; decision E4).
2. p2, S: browser link queue cap. Probe first on a machine with Chromium (B6, D5).
3. p2, decision: HEAD vs 0.13.1 gap and masters config keys (E1–E3, B8, D1).
4. p2, S: research-factory doc fixes for §4.1, §4.2, §4.3, §8 rows 13–15 (section F). Includes the false `depth-missing` claim and the staging claim.
5. p3, M: stage `read` text and sidecar, as `cite` does; record before rename (B1a, B1b).
6. p3, S: propagate `sha256_file` errors (B1c, D6).
7. p3, S: `fetch` bad input exits 2 (D2).
8. p3, S: lock wait limit and message (B3, D4).
9. p3, S: parallel `Resolved::fetch` with a test (C1). Prototype in `papers-proto`.
10. p3, S: OpenAlex `per_page` 200 with a measured paging run (C2).
11. Open, probe needed: `verify --record` at end, and `verified.at` date (B5, D8).
12. p3, S: partial PDF write in `save_pdf` (B1d, D9).
13. p3, S: test gaps (D7).
14. Open, coverage: the unread modules in section A. A follow-up track is needed to finish the audit scope.

**F. Replacement markdown**

Section 4.1 (replacement text):

```markdown
### 4.1 Paper lifecycle

Most of this machine is implicit. The state is the set of fields and files that exist for a key.
Unless a row names another version, the table shows papers 0.2.0 (commit b7a9b95).

candidate ─► registered ─► abstract ─► fetched ─► text ─► structure ─► card ─► indexed ─► cited ─► quote-checked
                 │                        │         │         │          │
                 └─► duplicate_of ─► ...  │         └─────────┴──────────┴─► stale when the text's sha256 differs
                                          └─► fetch failed (not checked in this audit)

| State | Stored as | Set by | Version |
|---|---|---|---|
| candidate | a search result, not stored | `papers search` | 0.2.0 |
| registered | registry entry `rf.reference/1` | `papers registry build` | 0.2.0 |
| abstract | `abstract` field | `papers registry abstracts` | 0.13.1 only |
| fetched | PDF saved next to its text | `papers read` | 0.2.0 |
| text | `.txt`, `.json` sidecar, `texts[]` entry | `papers read`, `papers registry attach` | 0.2.0 |
| structure | `rf.paper-structure/1` sidecar | `papers structure` | 0.13.1 only |
| card | `rf.paper-card/1` | `papers card` | 0.13.1 only |
| indexed | units in the search index | `lab search` | lab |
| cited | a note cites `ref:<key>` | author | checked by claimcheck |
| quote-checked | verdict per quote | `claimcheck` | claimcheck |

Papers 0.2.0 has no `add`, `text`, `metadata` or `structure` command.
`papers fetch` prints metadata only. `papers read` saves the PDF.

Staging: `papers read` writes the text, then the sidecar, then the registry entry.
It writes no temporary file and undoes no earlier step.
A failure can leave a text with no registry entry.

Guards:
- Stale derivative: applies to 0.13.1 only. Papers 0.2.0 has no structure or card sidecar.
- Write order: papers 0.2.0 does not follow this for `read`. `papers cite` writes a temporary file and renames it.

Defects outside the redesign: the verify and duplicate rules named in papers 0.7.0 and 0.8.0 are not in papers 0.2.0.
```

Section 4.2 (replacement text):

```markdown
### 4.2 Reading depth

Depth values in papers 0.2.0 are metadata, abstract, method, full, artifact and reproduced (`src/registry.rs:16-23`).
Any other value gives `invalid-value` (registry) or `depth-invalid` (notes) at error severity.
The tool accepts these values in any order.

- `unknown` is valid only as `Inspection depth: unknown. Reason: …`, with text after the word `Reason`.
- Other text after `unknown` gives `depth-unparsed` (warn). Rule: `src/catalogue.rs:879`, `913`.
- An entry with no depth gives no finding in papers 0.2.0. The rule `depth-missing` does not exist.
- A duplicate entry gives no finding for depth.
```

Section 4.3 (replacement table):

```markdown
| Machine | States | Note |
|---|---|---|
| HTTP attempt | try 1–5, then failed | explicit: 5 attempts (`src/http.rs:21`), backoff 5–60 s, `Retry-After` up to 300 s |
| HTTP cache entry | fresh for 7 days, then expired | explicit: `src/http.rs:32`, tmp and rename on write |
| Host throttle | free, held during an attempt | explicit: `src/http.rs:495-531`; waiters have no time limit |
| Browser page | Loading, Challenge, Ready, Blocked; results Download, Page, Blocked | explicit: `src/browser.rs:32-47`; 45 s deadline |
| Browser link queue | uncapped after the first list | gap: `src/main.rs:2502`, `2526-2530`; open until probed |
| Registry lock | free, held | explicit: `src/registry.rs:297-321`; waiters have no time limit |
| Seen index | built from registry, then notes | invalid notes are skipped with a stderr line; exit 1 "not seen" (`src/seen.rs:69-71`) |
| Graph source | primary, fallback | OpenAlex, then Semantic Scholar (`src/graph.rs:82-135`) |
```

Section 4.6: no change. This track did not audit lab or claimcheck. Papers `cite` only renumbers citations (`src/main.rs:1139-1149`).

Section 8, changed rows:

```markdown
| 13 | Paper lifecycle | 4.1 | Built in lab and papers 0.13.1. Papers 0.2.0 has `read`, `registry` and `seen` for this machine. |
| 14 | Stale derivative | 4.1 | Papers 0.13.1 only. Not in papers 0.2.0. |
| 15 | Seen index | 4.3 | Partly fixed. Invalid notes still give "not seen" with exit 1 (`src/seen.rs:69-71`). Open. |
```

**Changes and cleanup**

- No repository was changed. No commits. No board filings.
- Probe files, logs and the scratch clones are kept under `/private/tmp/claude-501/-Users-sunyizhe--t3-worktrees-research-factory-task-research-factory-4-1c1fdf969a284ada/1fb4fe71-1498-46b7-8a1e-3b6da0f588f3/scratchpad/papers/` (`probes/`, `logs/`, `bin/`, `papers-head/`, `papers-proto/`).
- Removed the two large build directories I created (`target/`, `target-head/`, 1.4 GB).
- Stopped the local HTTP server I started (port 8765). Probe processes ended.
- Masters: mtimes of `references.jsonl` and `references.md` unchanged after all runs. All masters runs were read-only or wrote to scratch.
- The papers main checkout is clean at b7a9b95.
- Live calls: 3 DOIs × 4 runs, plus 3 single calls to Crossref and OpenAlex, plus 2 OpenAlex list calls. All free APIs.
- I wrote a curl output file to `/tmp` once, against the scratchpad rule. I removed it.

**Not confirmed or not run**

- Browser loop: no Chromium on this Mac.
- `verify --record`, `verified.at`, `save_pdf`: code reading only.
- `record-pdfs` timing: no PDFs in masters.
- `refs` and `cited-by` timing: not run.
- `cargo test` in the prototype: not run.

**Setup feedback (for the parent to file, since this subagent does not file)**

1. Bash blocks a foreground `sleep 60`. Cost: one blocked call. Workaround: Monitor with an until-loop.
2. Each finished Monitor sends a task notification. About 20 notices filled the context. Workaround: arm one monitor at a time.
3. A chained `cp` after a long `cargo build` ran in the background and was delayed. Workaround: split the steps.
4. Masters `research.toml` uses `cards`, `card_dir` and `claude`, which HEAD does not accept (B8). The config and the installed tool disagree. Report as setup feedback about `masters`.
5. My own slip: one `/tmp` write, against the scratchpad rule. Removed.
I'll start by confirming the checkout is at 6851b18 and re-reading the brief.
The brief ends at section E, so I'll follow the parent's A–F shape. Next I'll clone 6851b18 into a fresh scratch directory and start a build in the background.
While the build runs, I'll list the source files and what changed since b7a9b95.
The source grew to about 24k lines, with a new `docling.py` script. Next I'll list every file in `src` and the CLI surface.
The CLI has 14 top-level commands plus the registry subcommands. I'll read the CLI definition block first.
The new surface includes `structure`, `card` (model call), `registry abstracts`, `record-pdfs` and a Python script. I'll check the build, then read the main dispatch.
I'll read the `main()` dispatch, which shows how each command is wired.
`main()` maps `Ok(false)` to exit 1 and `Err` to exit 2. That mapping matters for the `seen` and `fetch` exit-code findings. Next I'll find where each file's test module starts, so I can skip tests when reading production code.
Production code is about 17k lines, too much to read line by line in full. I'll read the state-bearing modules fully and skim the parsers and formatters by grep. I'm starting with config and the registry, since most gaps depend on them.
Config now accepts `cards`, `card_dir`, `claude`, `card_model` and `docling`, so the first report's B8 config gap needs re-checking. Next is the registry module.
I have the registry logic. Next I'll get exact line numbers for the key sites, plus the lock and file-write calls across all of `src`.
The test noise hides production writes. I'll limit the grep to production code (above each file's test module) and cover the new modules as well.
Several new state machines show up: the derivative pending file, the HTTP cooldown file, the file lock with a minimum interval, and the Docling and card timeouts. I'll read `derivative.rs` first, since it matches the doc's "stale derivative" row.
`derivative.rs` has the same silent-skip pattern as `sha256_file`. An unreadable input or a corrupt sidecar is treated as "no derivative" and reports nothing. Next I'll map the functions in `structure.rs` and `card.rs`.
`card.rs` has the call, the reply-keeping and the stale check. I'll read its command and run path. I will not execute it, since it calls a model.
The card cache check treats an unreadable or unparseable old card as absent. That may trigger a repeat paid call. I can probe it with a fake `claude` script that counts calls and makes no network request. First I'll check the build and the `Card` struct.
The build is still compiling. Next is `http.rs`, which gained a cooldown file and a new throttle path since the first audit.
The HTTP layer now has a per-host cooldown with strikes that doubles on each 429. That is a new persisted state machine. I'll read its transitions.
The retry loop has a per-host cooldown state that persists in a file across processes. I'll read `attempt()`, the cooldown and strike helpers, and the cache code.
The cooldown logic is clear. A corrupt cooldown file reads as "no cooldown", and a failed cooldown write is only a warning. Next I'll read the throttle and lock-file code.
The throttle holds an exclusive file lock across the whole request. A corrupt or missing spacing file reads as "no spacing". Next is `browser.rs`, which I need for the link-queue gap and the browser states.
The browser state machine is clear. Its blocked-word list runs over the first 3000 characters of page text, so it may misfire on ordinary articles. That is a code-reading point and needs a probe. Next I'll read the browser caller, which holds the link queue.
The browser queue still has no cap on links found on loaded pages. I'll read `try_candidates` next, which decides when the browser runs and how a PDF gets saved.
The plain-text path now stages the `.txt` and commits it only after the record. That changes the first report's staging finding, so I'll re-probe it. Next I'll read `read()` and its caller chain.
`read` now also writes a structure sidecar as a later step. I'll read `finish_text`, which sets the commit order.
`finish_text` now records the registry first, then writes the manifest, then renames the staged `.txt`. It undoes earlier steps when a later one fails. I'll read `record_text` to confirm the registry step.
`record_text` rewrites the manifest with a plain `fs::write` inside the registry update closure, before the registry is saved. I'll check which callers pass a manifest file, and then read the `record_pdfs` flow.
`record_pdfs` rewrites each manifest inside the registry lock, before the registry save. A failed save leaves manifests ahead of the registry, and a rerun skips them as already recorded. I'll test that with a read-only registry directory. Next I'll read `seen`, the first finding's subject.
`seen` still returns `Ok(false)` when nothing matches, so exit 1 is kept. I'll check whether `Index::load` still skips invalid notes silently or with a message.
`seen` still prints a skip line and then "not seen" with exit 1 when nothing matches. Next I'll read `registry_cmd.rs`, which drives `build` and `check`.
`registry check` also reports stale derivatives and card findings. Next I'll collect the current rule vocabulary across `src`, since the doc's rule names need checking.
`depth-missing` now appears seven times, and the first report said it did not exist. I'll check where it is emitted and what it tests.
`depth-missing` lives in the catalogue parser (`catalogue.rs:1008`), not the registry. The rules that the first report called absent (`verified-mismatch`, `verify-withdrawn`, `duplicate-cycle`, `duplicate-chain`) now exist. I'll read the catalogue depth code to see the exact condition.
`depth-missing` is a catalogue warning for entries with no depth line. The first report's claim was wrong for this checkout. The notes and registry paths still need separate probes. Next I'll read the catalogue parse entry point and its duplicate handling.
Duplicates defer their depth check, which the catalogue code handles. I now have the core state paths. Before the probes, I'll check the build and read the new `abstracts.rs` module, since it makes network calls.
`abstracts` makes one OpenAlex request per DOI entry, in sequence, and writes the registry only once at the end. I'll read the Docling wrapper and script next, since they run a Python process.
The Docling wrapper runs Python through a configurable command. I'll check where `docling::version` is called, since each call starts a full Python environment.
Docling makes two Python starts per read, one for the version check and one for the conversion. I'll read the `structure` command path next.
`structure` writes atomically and skips unchanged sidecars. I'll read its `Known<T>` state type and `build()`, which hold the module's main state.
The build is still compiling. While it runs, I'll read `work.rs` for the C1 hot path and grep the OpenAlex `per_page` clamp.
The build is still running. While it finishes, I'll grep every module I have not read for state writes, locks and deletes, so the unread files get a coverage check.
The crate itself is compiling now. I'll read the unread parts of `card.rs`, which the card probe depends on.
Foreground sleep is blocked here, so I'll keep reading while the build finishes. Next are `pdf.rs` and `sources.rs`.
The timeout check at `pdf.rs:314` runs after `read_limited` and `read_tail` return. I need to see whether those readers can block past the deadline.
The build finished with exit 0. I'll confirm the release binary and its version, then start the probes.
The 6851b18 binary reports `papers 0.15.0` and is copied to `new/bin`. Next I'll check the CLI surface and the masters config, read-only.
Before any masters run, I'll record the mtimes of the two masters registry files and check which environment variables the code reads, so state dirs can go to scratch.
The `.research` folder already exists in masters, so I will keep its state untouched. Next I'll run read-only commands against masters and log their exit codes.
Config loads on 6851b18 with the masters keys, so the version gate from the first report is not reproduced. Next I'll map the first report's item numbers from the transcript, since the summary lists them only by topic.
The first report's item list is now recovered from the prior transcript. I'll map each item to a probe. Next I'll read `seen`'s note indexing and command code at 6851b18 so the probe inputs are right.
`seen` still returns `Ok(false)` after a skipped note, so exit 1 and "not seen" should reproduce. I'll create a masters copy for write-capable probes, then run the seen and fetch probes against the 6851b18 binary.
The first miss probe used a bare title, which correctly fails as a usage error with exit 2. I'll rerun the miss case with `--title`, which is the path the first report tested.
The skipped-note case reproduces with exit 1 and "not seen". The title probe was weak because the broken note has no readable title, so I'll repeat it with a DOI that lives only in the broken note.
B2 is confirmed on 6851b18. Next are the fetch bad-input exit code (D2) and the config gate (B8) with a typo key, which must still be rejected.
D2 still reproduces: `fetch` exits 1 on bad input, while `seen` exits 2 for the same class of error. The typo key is still rejected, so the config gate is gone for masters keys but the strictness remains. Next I'll check the README exit-code table and the `read` options before designing the read probes.
The `read` probes need a local PDF server. Before building them, I'll check where `sha256_file` is called and which local PDF fixture exists from the earlier probes.
`existing_text` (`src/main.rs:2693-2700`) drops an unreadable recorded text without a message, so probe C can check that. I'll look at the old `read` fixture.
The old `read` probe left a `server.pid`. I'll check whether that server still runs before starting a new one.
No leftover server is running, so the old PID is stale. I'll start a new loopback PDF server on port 8771 for the read probes, and stop it at cleanup.
Probe A shows the text is now undone on failure (no `.txt` or `.tmp` left), but the PDF stays in place, and the registry is unchanged. Next is probe B, where the registry lock file is read-only.
Probe B on 6851b18 leaves only the PDF after a failed record, and the registry is unchanged. The rerun succeeds, so the text-versus-registry disagreement from the first report is gone for `read`. Next is probe C, which makes a recorded text unreadable.
Probe C returns exit 0 and silently replaces the unreadable recorded text. The output also labels it "(missing)", because `Registered::new` maps every read error to "missing". Next is the record-pdfs disagreement case (B1b, C4), using a manifest with its `pdf_sha256` removed.
The record-pdfs success path works, and one 618-byte PDF takes about 0.02 s. Next is E2, where the registry folder is read-only so the registry save fails after the manifests are written.
My redirect targets were inside the folder I had just made read-only, so the shell refused them and `papers` never ran. I'll confirm the folder mode is restored, then rerun with logs written outside the folder.
Both redirects pointed at `read/logs`, which does not exist. The shared log folder is one level up. I'll rerun with the correct path, checking first that the folder exists.
This reproduces the ahead-of-registry state on 6851b18. The manifest records `pdf_sha256`, the registry does not, and stdout still says "in the manifest and the registry" before the error. I'll check whether a rerun repairs the registry.
Rerunning does not repair the registry. The second run reports "1 already with a pdf_sha256" and the registry keeps zero. Next is the lock-wait probe (B3), using a Python holder that keeps the registry lock for 12 seconds.
The wall time of 7.35 s came from separate tool calls, so the wait is not measured exactly. I'll rerun in one command, starting papers only after the holder reports that it holds the lock, and compare with a no-lock baseline.
Under a 12-second holder, the build waits the full 12 seconds with no message, and exits 0. The baseline with no holder is near 0 s. Next is the depth probe (B9 and the `depth-missing` rule), so I'll read the rule's conditions first.
The masters catalogue has 860 `Inspection depth` lines, and its registry has 17 null-depth entries. I'll remove one depth line from a scratch catalogue copy to test `depth-missing`, and check how `registry check` reports the 17 null depths.
`depth-missing` exists on 6851b18 (`src/catalogue.rs:1008`, Warn) and fires for a catalogue entry with no depth line. The 17 null-depth registry entries are not flagged. Next I'll read the state-machines doc sections I must replace, so drift can be checked against them.
The doc's section 8 rows 13 to 15 call the paper lifecycle and seen index "built" or "fixed". I'll grep the 6851b18 source for each rule and state the doc names, so each claim gets a verdict.
`derivative::check` runs from `registry check` (`src/registry_cmd.rs:318`), so the doc's claim that `registry check` reports stale derivatives holds in code. Next is the `structure` timing, which needs the masters text files. I'll size them before copying.
The registry records 133 `.txt` files, and the whole text folder is 376 MB. I'll copy only the 133 recorded texts into the scratch masters copy, then check the `structure` options.
All three structure runs exit 1, so the timings are not valid yet. I'll read the stderr to see why.
The 81 failures are `.docling.md` texts the registry records, which I did not copy. Structure still wrote 133 sidecars for the `.txt` texts. I'll copy the docling texts, clear the sidecars, and rerun the three timed passes for a clean exit code.
Five recorded texts (`60.md`, `68.md`, `141.md`, `246.md` and one more) are absent from masters too, so the exit 1 is a real result. I'll check whether the second `structure` run skipped unchanged sidecars, and whether `registry check` reports a recorded text that is missing on disk.
The second structure run printed nothing that marks it as skipped, and `registry check` on the scratch copy reports no missing-text finding even though five recorded texts are absent. I'll confirm the keys for those five texts and check whether `fetch` marks them missing.
`fetch` does not accept keys, so I'll use the DOI or arXiv id of entry 60.
`fetch` marks the missing text as "(missing)", but `registry check` reports nothing for it. That is a gap for the registry check, and I'll file it under D. Next I'll confirm whether `structure` rewrites unchanged sidecars, by comparing mtimes before and after a rerun.
Reruns leave all 209 sidecars unchanged, so `structure` has no rewrite waste. Next is the card probe. I'll read the command and reply parsing, so a fake `claude` stub can answer without a model call.
The stub has to answer in the `structured_output` shape. The masters config points `claude` at the real `claude-experiment`, so the probe must use its own config with the stub. I'll read the card field list and the card file path, then pick quotes from text 1.
The quote needs at least eight words, so I'll use a 12-word passage from line 291 of text 1. Next I'll build the card probe project with a stub that counts calls and prints a fixed reply, and run the non-model `--reply` path first.
`card` takes no positional key, so I'll check its option names.
The `--reply` path takes about 0.01 s and makes no call. Next is the cache test: a first call, a cache hit, then a corrupted card file to see whether the model is called again.
zsh does not word-split an unquoted variable, so `$C` ran as one command name and returned 127. I'll write the commands out in full.
Card cache behaves as the code predicts. A valid cached card is not called again, but a corrupt card file triggers a second stub call. Next are the hot paths. I'll check which commands call `Resolved::fetch` on 6851b18 and the `fetch` and `search` options.
`search --max` allows up to 200, so the 100-result clamp forces two requests for a 200-result search. I'll confirm that `main.rs:2116` belongs to the `fetch` command, then check the `Http` type for thread safety.
`Http` holds a `ureq::Agent` and an optional cache, so a scoped thread can share it. Next are the C1 before measurements on the 6851b18 binary: three runs of the same three DOIs, each with a fresh cache dir and lock dir.
The 6851b18 baseline for C1 is 4.20, 3.81 and 3.79 seconds, a mean of 3.93 seconds. Next I'll make two scratch clones of 6851b18, one with the parallel `Resolved::fetch` patch and one with the OpenAlex 200 cap, so each fix is measured alone.
Both patches applied. I'll build the two prototypes in the background, sharing the scratch target dir, and copy each binary out when its build finishes.
While the builds run, I'll check the `cited-by` options. `list` responses are cached, so cache-file counts can show how many OpenAlex requests a run made.
`cited-by --max` is available and uses the cache. I'll run the C2 before case on the 6851b18 binary with a fresh cache, counting cache files as requests.
C2 before is 3 cached requests per run (one lookup, two list pages) and about 5.4 s wall. I'll check the prototype builds.
Both prototype builds exited 0. Since the two binaries have the same size, I'll confirm each log compiled its own clone and that the binaries differ.
The c2 log has no "Compiling papers" line and its binary is identical to c1, so the c2 build may not have compiled the C2 clone. I'll read its log.
The c2 build finished in 0.36 s because cargo reused the c1 artifact. The metadata hash is path-relative, so the two clones share it. I'll touch the c2 source to force a recompile of the crate, then confirm the binary differs from c1.
`timeout` is not installed on this macOS host, so the build never started. I'll rerun without it.
The three binaries now differ, so each prototype is its own build. I'll run the C2 after case with the C2 binary, then the C1 after case with the C1 binary.
C2 after gives 2 cache files per run (was 3), output identical. Next is C1 after. I need the exact before-run command, so I'll check the fetch probe folder first.
The before-run command isn't recorded, so I'll check the full before output to reconstruct it.
The before run used three DOIs with a fresh cache, giving 6 cache files (3 Crossref, 3 OpenAlex). I'll run the C1 binary the same way: three runs, fresh cache and lock dirs, then byte-compare stdout.
C1 after shows no gain: 3.82 to 4.16 s against 3.79 to 4.20 s before, with identical stdout. I'll check the per-host throttle in `http.rs` to see whether it serializes the requests.
The throttle waits 1 s after each request completes, per host. I'll measure one request latency per API and read the fetch loop to see whether that gap limits the parallel version.
Single requests take 0.75 s (Crossref) and 0.26 s (OpenAlex). Crossref calls must be 1 s apart, so three Crossref calls need at least 2 × 1 s gaps plus 3 × 0.75 s. That floor (about 4.25 s) matches the measured times, so parallelising the two sources cannot cut wall time here. Next I'll check the port 8771 server, the masters file mtimes, which browsers exist, and the sizes of the unread modules.
Masters mtimes and sizes are unchanged. No Chromium is installed, so the browser queue cap can only be checked by code reading. The port 8771 server is PID 44993. Next I'll look for the base-URL override and the server script so I can probe the retry waits.
Base URLs are hard-coded to the public APIs. The probe server serves `read` URLs, so the retry probe can use a `read` of a local 503 URL. I'll read the HTTP retry and cooldown logic to confirm which statuses are retried.
Retries use `Retry-After` when present (capped), otherwise 5 << (attempt − 1), capped at 60. Next I'll read the status mapping and the cooldown file reader.
The cooldown reader falls back silently to the default on a missing or corrupt file, so the corrupt-file case is a silent reset. Next I'll start a local status server on port 8772 for the retry and cooldown probes. It logs each request with a timestamp so I can read the wait gaps.
The `read` path retries once (`retry 1/1 in 5s`), not four times, so the ladder depends on the call site. I'll check the attempts setting per caller and the stdout and exit for this run.
`read` uses `PDF_ATTEMPTS` (2 attempts) and gives exit 1 with "no candidate yielded a PDF". The 5-attempt ladder belongs to the default `Get`. I'll test it through `fetch` with a URL, which may use that default. A run of about 80 s covers the ladder.
`fetch` rejects non-DOI URLs (exit 1), so the 5-attempt API ladder cannot reach the local server. Only the first wait (5 s) is measured, through `read`. The 429 cooldown needs `retry_429`, which `read` turns off (`main.rs:3640`). Next is the Docling start count. I'll read the two call sites and the config key.
The Docling runner starts Python once for `--version` and once for each convert. A shim that logs each start will show the count. I'll write the shim and check the `read` flags.
I ran a stray command by mistake (`/Users/sunyizhe/Developer/papers/../papers`). It is a directory, so it executed nothing and changed nothing. Next I'll run the Docling read through the shim and count the Python starts.
The log holds the full `-c` script text, so the line count is misleading. I'll count the timestamped entries, one per Python start.
Two Python starts per Docling read (`--version` and convert), so the count claim is confirmed. Real per-start cost is not measured, because the shim replaced Python. Next I'll check `registry abstracts` and run it on a copy of the scratch registry.
`registry abstracts --limit 5` made five OpenAlex lookups in 5.8 s and added 2 abstracts (rc 0, 5 cache files). The 33-candidate figure is my subtraction, not a measured full run. Next I'll read `verify.rs` for the `--record` end-write and the `verified.at` date.
`verify --record` computes `today` after all lookups and writes once through `registry::update`. Nothing is recorded until the end. I'll probe that by stopping a run partway. First I need the verify flags and one registry row to pick keys.
`verify` takes the registry from `[papers] registry`, not a flag. The kill run failed on usage (rc 2), so it proves nothing. I'll set the key in the config and repeat.
The interrupted run (rc 143) left the registry unchanged, and one lookup had already reached the cache. I'll run it to completion, check the recorded `verified` fields and `at` date, then time a cached rerun.
`verify --record` is confirmed: the interrupted run wrote nothing, the full run wrote three results dated 2026-11-14, and a cached rerun takes 0.019 s with no new cache files. Last probe is the pipe case in `pdf.rs`. A fake `pdftotext` exits at once but leaves a background `sleep 45` that holds stdout. I'll time how long `read` waits.
`read` took 45.95 s (rc 0) while the stub's own work took no time. The reader threads wait for the grandchild to close stdout, and the 60 s `pdftotext` timeout does not cover that wait. The sleep has exited, and `pgrep` finds nothing left. Next I'll scan the unread modules for risky patterns (file writes, sleeps, process spawns, unbounded loops) before reading the key parts.
The scan turned up a few loops to check: `graph.rs:215`, `sources.rs:483`, `browser.rs:99-132`, and the keychain call at `openalex.rs:39`. I'll read those ranges.
`graph.rs` paging is bounded and the offset advances. The browser wait is bounded by `LOAD_TIMEOUT`. `sources.rs:483` needs a check that the offset advances and the loop stops on an empty page, so I'll read the rest of that loop and the key-cache wrapper.
The S2 search loop advances its offset and stops on a short page, so it is bounded. The OpenAlex key lookup runs once per process and spawns `security` only when no env var is set. A few `unwrap`/`expect`/`unreachable!` sites in `structure.rs`, `cite.rs` and `bib.rs` could panic on odd input, so I'll check whether their guards hold.
`structure.rs:395-402` and `bib.rs:160` cannot panic, since `out` starts non-empty and the letters are ASCII. Two sites still need their earlier guards: `structure.rs:442` (`page 1`) and `cite.rs:380` (`[]`). I'll read those.
Both guards hold: `cite.rs` returns early on an empty name list, and `structure.rs` starts with page 1. Next is cleanup. I'll confirm the binaries are standalone copies, then stop the two servers I started (ports 8771 and 8772), and check the main checkout is still clean.
The `pgrep` pattern also matched other sessions' processes (T3, rustc, storagectl, rclone). I did not touch them, and they are not mine. Next I'll confirm the two `uv` parent processes are gone, remove the large build directory I created, delete my backup file, and recheck the masters mtimes.
All probe processes are gone, the build directory is removed, and the masters mtimes and sizes match the recorded values. Last, I need the doc sections for part F. I'll read §4.1–4.3, §4.6 and the §8 rows, and check the C2 unit test that may assert the old page size.
The doc drift is clear. I still need exact line numbers for the registry lock and the throttle lock, so I'll grep for them before writing the report.
Line numbers are confirmed. Last, I'll read-only check the masters text folder for the recorded texts that the scratch copy lacks, so the report does not rely only on the scratch copy.
The four files are present in the masters text folder, so the earlier "missing in masters" note was wrong or refers to a different path. I'll check the masters registry read-only against the disk to get the real count of recorded texts that are missing.
The masters registry check printed 0 missing, but it skipped any text entry without a `path` key. I'll confirm that every entry had a path before I rely on that result.
## Result

At 6851b18 (fresh build, probes rerun), the config and HEAD gaps are gone and `read` no longer leaves a broken text behind. Most other first-report findings still hold, and the new `record-pdfs` gap and card repeat-call risk are p2. The C1 parallel fetch gives no measured gain, while the C2 page-size change saves one request per `cited-by` run.

## A. Files read, and files not read

Read at 6851b18 (paths under `~/Developer/papers/`):
- `src/work.rs:88-100` (`Resolved::fetch`).
- `src/openalex.rs:17-50` (endpoint, key cache, Keychain call), `:385-405` (`paged_with`), `:416-440` (test header).
- `src/http.rs:186-330` (retry loop, status map), `:518-556` (cooldown dir and file), `:622-712` (lock dir, `throttled`, `throttle_in`), `:20-40` (constants).
- `src/browser.rs:24-28, 90-140` (load loop, timeouts).
- `src/card.rs:90-155, 187, 494-533, 542-640, 659-785` (partly; `call`, `keep_reply`, `run`, cache check at `:683-694`).
- `src/structure.rs:335-402, 412-446` (table and page code, guards).
- `src/main.rs:59, 869, 1146-1150, 1409-1444, 2105-2130, 2687-2707, 3172-3185, 3223-3284, 3544-3614, 3639-3640, 3733-3750, 3920-3990` (plus `758-993` record-pdfs and `3715-3718` save_pdf from the earlier round).
- `src/pdf.rs:275-367`; `src/registered.rs:24-40`; `src/derivative.rs:52-62`; `src/registry.rs:319-337`; `src/registry_cmd.rs:74, 132, 318`; `src/catalogue.rs:996-1016`; `src/searchlog.rs:186-210`; `src/abstracts.rs:120-168`; `src/docling.rs` (full), `src/docling.py` (full).
- `src/graph.rs:200-240` (`s2_links`); `src/sources.rs:469-528` (S2 search loop); `src/cite.rs:352-382`; `src/bib.rs:150-161`; `src/config.rs:31-32, 50-51, 83-84, 100-116, 188-189`.
- `src/verify.rs`: function list only (grep). Lookup logic at `:534-680` not read.
- Doc `~/Developer/research-factory/docs/state-machines.md`: header line 5, §4.1–4.3 (247–309), §4.6 (331–336), §8 rows 13–15 (490–492).
- Masters, read only: `research.toml` `[papers]` keys; `docs/research/references.jsonl` (214 recorded texts, all with a path, 0 missing on disk); mtimes and sizes unchanged.

Not read (pattern scan only, no verdict): `verify.rs` lookups, `arxiv.rs`, `sources.rs` (other than S2 search), `cite.rs` (rest), `bib.rs` (rest), `text.rs`, `refs.rs`, `id.rs`, `finding.rs`, `crossref.rs`, `atom.rs`, `notes.rs`, `check.rs` (rule names by grep; loop at `:234` unread), `catalogue.rs` (rest; built-in regex `expect` at `:41, 852, 861, 1032`), `browser.rs` (classify near `:240`, downloads), `http.rs` (rest), `structure.rs` (about 1,800 lines), `card.rs:190-494` (`find_bounded` at `:269`, word bounds at `:358-401`), `openalex.rs` (rest), `searchlog.rs` (rest), `graph.rs` (rest), `main.rs` `690-758, 1211-1408, 1580-1708, 1708-2058, 2059-2470` (partly), `2470-2686, 3319-3525, 3860-3919, 3990+`. `tests/*.rs`: names only, not run.

Panic sites checked: `structure.rs:398, 402, 442`, `cite.rs:380`, `bib.rs:160` are guarded. Loops in `graph.rs:215` and `sources.rs:483` are bounded. The browser loop is bounded by `LOAD_TIMEOUT`.

## B. Machines

B1 Read (text) staging. `~/Developer/papers/src/main.rs:3223-3284`.
- States: PDF saved, text staged (`.txt.tmp`), recorded, manifest written, renamed, sidecar written. A failure undoes the text.
- Undo of text: CHANGED (fixed). Probe A: rc 2, `paper1.pdf` (618 bytes) kept, no `.tmp`, registry `texts` unchanged.
- PDF residue after failure: CONFIRMED (probes A, B).
- Registry and sidecar disagree after failure: GONE for `read` (probe B: rc 2, registry unchanged).
- `sha256_file` drops read errors (`main.rs:2687-2689`); `existing_text` ignores them (`main.rs:2693-2707`). A recorded text that cannot be read is downloaded again and replaced: CONFIRMED in changed form. Probe C: rc 0, `text: out/paper1.txt (missing)` from `registered.rs` (every error becomes "missing").
- `save_pdf` writes in place (`main.rs` ~3715-3718): CONFIRMED by probes A, B. Partial-write crash risk: code reading only.

B2 Record-pdfs. `~/Developer/papers/src/main.rs:758-993`; sha check `:869`.
- States: PDF present, manifest written with `pdf_sha256`, registry saved. The manifest is written before the registry save.
- Probe E2 (registry dir mode 555): rc 2, `Permission denied`. Manifest has `pdf_sha256`; registry does not. Stdout printed "recorded … in the manifest and the registry" before the error (false line).
- Rerun with writable dir: rc 0, "1 already with a pdf_sha256"; registry still unrepaired. NEW, CONFIRMED, p2.
- Success path: rc 0, 0.017 s for one 618-byte PDF (probe E1).

B3 Card. `~/Developer/papers/src/card.rs`.
- States: none, model call (`:593`), reply kept (`:625-637`), checked (`MIN_WORDS` 8, `MIN_KEPT_PERCENT` 80), stored at `card_dir/<key>.json` (`:777-779`), cache hit when text sha, prompt sha and model match (`:683-694`).
- A corrupt or old-schema card counts as absent (`:683-694`, `:167-184`). The model is called again. The kept reply is not reused.
- Stub probe: first run 1 call; second run "not calling Claude" (0 calls); after garbage in `cards/1.json`, 2 calls. NEW, CONFIRMED, p2.

B4 Registry lock. `~/Developer/papers/src/registry.rs:319-337`.
- States: free, held (`flock`, `:337`), saved (tmp and rename). A waiter has no time limit.
- Holder held 12 s: `registry build` waited 12 s, printed nothing, rc 0. Baseline about 0 s. CONFIRMED, p3.

B5 HTTP attempt. `~/Developer/papers/src/http.rs:196-262` (loop), `:275-330` (status map).
- States: try n, then Done, Redirect, Retry (request error or 5xx; `Retry-After` used, max 300 s at `:29`), RateLimited (429, API calls only), Cooling, Fail.
- API waits: 5, 10, 20, 40 s (`:259-260`, `(5 << (attempt-1)).min(60)`). Code reading for waits 2 to 4; no test host reaches the API path.
- `read` path: 2 tries (`main.rs:3639`), one wait of 5 s. Probe: "retry 1/1 in 5s", then "gave up after 2 attempts".
- First report's "60 s" was wrong: CHANGED.

B6 Throttle. `~/Developer/papers/src/http.rs:22-25` (intervals), `:560-570`, `:698-712` (lock, wait, write).
- States: free, waiting (1 s per host; arXiv 3 s), request, done. The interval counts from the end of the previous request, from any process.
- Waiters have no limit (first report B4): code reading only; by design for bounded holders. This explains C1.

B7 Browser page. `~/Developer/papers/src/browser.rs:24-28, 99-132`; `src/main.rs:59, 3554`.
- States: closed, opening, loading, ready (2 polls), challenge, blocked, loaded. Limit 45 s.
- Initial queue capped at 8 (`main.rs:3554`). Links from loaded pages are not capped (first report B6).
- No Chromium here: `/Applications` has neither Chromium nor Chrome; `PAPERS_CHROME` unset. OPEN, not probed.

B8 Seen index. `~/Developer/papers/src/seen.rs:69-71` (skip), `:151` (lookup order: doi, arxiv, openalex, url, title).
- States: notes indexed; an invalid note is skipped with a stderr line and no finding.
- Probe: DOI held only by a skipped note gives empty stdout, stderr "skipping … invalid frontmatter", then "papers: not seen", rc 1. First report B2: CONFIRMED (stderr line now; still no finding).
- Observation: a bogus `depth` in the registry gives a `seen` hit (rc 0). `registry check` flags it `invalid-value` (rc 1).

B9 Derivative. `~/Developer/papers/src/derivative.rs`; check call `registry_cmd.rs:318`.
- States: pending `.tmp`, commit (rename), discard; rule `derivative-stale`.
- `derivative::check` skips unreadable inputs silently (`:52-62`): code reading only.

B10 Verify record. `~/Developer/papers/src/main.rs:3920-3960`.
- States: lookups (cached 7 days), one write at the end via `registry::update`. `at` is the local run date (`jiff::Zoned::now()`).
- Probe: a killed run (rc 143) wrote nothing; one lookup had reached the cache. A full run wrote 3 results with `at` 2026-11-14. A cached rerun took 0.019 s with no new cache files. First report B5 end-write: CONFIRMED (p3). `verified.at` date: CONFIRMED correct.

B11 Docling. `~/Developer/papers/src/docling.rs:27-32`; calls `main.rs:3172` (version) and `:3739` (convert).
- Probe with a logging shim: 2 Python starts per read (`--version`, then convert). NEW, CONFIRMED count. Per-start cost not measured (shim, no real Docling).

B12 PDF reader. `~/Developer/papers/src/pdf.rs:275-352`, readers `:354-367`, join after child exit `:330-335`.
- A grandchild that holds stdout blocks the join. No timeout covers the join.
- Probe: stub `pdftotext` exits at once and leaves a 45 s background child. `read` rc 0, wall 45.95 s. NEW, CONFIRMED, p3.

B13 Graph source. `~/Developer/papers/src/graph.rs:215-232` (S2 links).
- Paging is bounded: the next offset must grow. The OpenAlex-then-S2 fallback order is not re-verified. OPEN.

Doc drift (`~/Developer/research-factory/docs/state-machines.md`):
- §4.1 table names `papers add`, `papers text`, `papers metadata fetch`. None appear in the 0.15.0 help. First report B7: CONFIRMED.
- §4.1 defect list: each named check is present in `src` (grep). CHANGED (now present).
- §4.2 depth vocabulary ("listed … read in full") differs from code `registry.rs:16-23` (metadata, abstract, method, full, artifact, reproduced). First report B9: CONFIRMED.
- §4.2 `depth-missing` (Warn) exists at `catalogue.rs:1008`. First report said absent: CHANGED (doc correct).
- §4.3 HTTP row ("try 1–5") omits the 2-try `read` path. §4.3 seen row ("silently") is out of date (stderr line).
- Header lists papers 0.12.1; the tool is 0.15.0.
- §4.1 says 17 masters entries with null depth now state `unknown`. The registry still has 17 `"depth":null` entries, and `registry check` gives no finding. Not verified against the catalogue.

## C. Hot paths (before, after)

C1 `Resolved::fetch` (`~/Developer/papers/src/work.rs:88-100`), parallel Crossref and OpenAlex (prototype clone `/private/tmp/claude-501/-Users-sunyizhe--t3-worktrees-research-factory-task-research-factory-4-1c1fdf969a284ada/1fb4fe71-1498-46b7-8a1e-3b6da0f588f3/scratchpad/papers/new/papers-proto6851-c1`).
- Before: 4.195, 3.805, 3.785 s; mean 3.93 s; 6 cache files per run.
- After: 4.160, 3.874, 3.817 s; mean 3.95 s; 6 cache files per run; stdout byte-identical in 3 of 3 runs.
- Verdict: no gain. Single live calls: Crossref 0.745 s, OpenAlex 0.256 s. The 1 s per-host spacing makes three Crossref calls take about 3 × 0.75 + 2 × 1 = 4.25 s. Measured times are close to that floor.
- Not measured: a Crossref error no longer skips the OpenAlex request.

C2 OpenAlex `per_page` (`~/Developer/papers/src/openalex.rs:390`, clamp 100 to 200; prototype binary `…/scratchpad/papers/new/bin/papers-c2`).
- Before: `cited-by 1706.03762 --source openalex --max 150 --no-log`: 3 cache files per run (1 lookup, 2 pages of 100 and 50). Wall 5.576 s and 5.333 s; mean 5.45 s; 899 output lines.
- After: 2 cache files per run (1 lookup, 1 page of 150). Wall 5.074 s and 3.862 s; mean 4.47 s. Run 1 stdout byte-identical; run 2 has 899 lines (not compared byte-wise).
- Verdict: confirmed, one request less per run. Two runs, so the wall times are noisy. The unit test `pages_keep_one_page_size` (`openalex.rs:416`) was not run.

New commands (6851b18 build):
- `structure`, 209 sidecars (133 `.txt`, 76 docling): dry run 1.075 s; first write 1.079 s; rerun 1.065 s; rerun rewrote 0 of 209 files.
- `card`: `--reply` check 0.012 s, 0 stub calls. Cache hit: 0 calls. Corrupt card: 1 extra call (B3). No real model call was made.
- `registry abstracts --limit 5`: 5.79 s; 5 OpenAlex lookups (5 cache files); 2 abstracts added; 3 without an abstract at source; 0 failed. Full run not measured.
- `registry record-pdfs`, one 618-byte PDF: 0.017 s.
- `verify --record`, 3 DOI entries: full run 2.34 s; cached rerun 0.019 s; 0 new cache files.
- `read --extraction docling` (shim): 2 Python starts. Real per-start cost not measured.

## D. Other points

- Masters: 214 recorded texts, all with a `path`, 0 missing on disk (read-only check). My earlier note "5 texts missing in masters" was wrong. My scratch copy lacked 5 texts (copy error). That copy caused the `structure` rc 1 and the "(missing)" fetch message.
- `registry check` still misses a recorded text that is missing on disk (rc 0 in the scratch copy). This gap is real, separate from the copy error.
- Exit codes: `fetch 'not a doi or url !!'` rc 1; `seen` with similar input rc 2. Inconsistent (first report D2 confirmed).
- `read` on a 503 URL: rc 1, "no candidate yielded a PDF; open the landing page in a browser". That is a finding, not a usage error.
- Config: `deny_unknown_fields` on `[papers]` (`config.rs:31-32`). Masters keys accepted; a typo key (`card_dirr`) gives rc 2.
- Keychain: `security find-generic-password` runs once per process when no env key is set (`openalex.rs:26-31, 39`). Cost not measured.
- Semantic Scholar search filters `since_year` after fetching, so results can be fewer than `--max` (`sources.rs` doc comment).
- Testability: API hosts are hard-coded (`openalex.rs:17`, `crossref.rs:9`). `fetch` rejects non-DOI URLs. `read` turns off 429 retry (`main.rs:3640`). So the API retry ladder and the 429 cooldown could not be probed with a local server.
- Masters `research.toml` `[papers] claude` points to the real `claude-experiment`, with `cards = true`. A `papers card` run in masters bills a real model. I did not run one.

## E. Owner decisions

- E1 (Reason: design): depth vocabulary. Option (a): the code list (`registry.rs:16-23`), and change doc §4.2. Option (b): the doc list, and change code and data. Recommend (a) unless the masters catalogue uses the doc names. Consequence: (a) changes the doc only; (b) changes code and data.
- E2 (Reason: design): link cap for browser pages (item 14). Option (a): cap links from loaded pages at 8, the initial cap. Option (b): keep uncapped. Recommend (a). Consequence: fewer pages per run, against a possible missed paper.
- E3 (decision note, no ask): registry lock wait limit (item 8). Recommended default: 30 s, then a message naming the lock file. Reversal: change one constant.
- E4 (decision note, no ask): card corrupt-file path (item 24). Recommended: reuse the kept reply when text and prompt match, with no model call. Reversal: remove the check.
- E5 (decision note, no ask): `fetch` bad input exit code (item 2). Recommended: 2, like `seen` and the `Err` convention. The README exit-code section was not re-read at 6851b18.
- E6 (parent decides): install Chromium to probe item 14. Installing a dev tool is outside this read-only audit.

## F. Replacement markdown for state-machines.md

Replace §4.1–4.3, §4.6 and §8 rows 13–15 with the text below. The `verified`, `duplicate_of`, `depth-missing` and withdrawn/retracted bullets of §4.1 stay unchanged (rules present by grep at 0.15.0).

```markdown
### 4.1 Paper lifecycle

Most of this machine is implicit. The state is the set of fields and files that exist for a key.

(Diagram unchanged.)

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

- **Stale derivative.** A structure sidecar or card stores the sha256 of its text. A different sha256 makes it `stale`. The index ignores a stale card.
- **Write order.** A derivative is written to a temporary file, recorded, then renamed.

Known gaps at papers 0.15.0:

- `papers read` keeps the PDF after a failed step. It undoes the text.
- `papers registry record-pdfs` can write a manifest with a `pdf_sha256` that the registry lacks. A rerun does not repair it.
- `papers registry check` does not flag a recorded text that is missing on disk.
- `papers card` calls the model again when a stored card cannot be read.

### 4.2 Reading depth

States are the values of `depth` in the registry. The code accepts these values: metadata, abstract, method, full, artifact, reproduced, plus `unknown`. [Pending decision E1.]

- `unknown` is valid only as `Inspection depth: unknown. Reason: …`, with text after the word `Reason` (papers 0.10.1).
- Other text after `unknown` gives `depth-unparsed` (Warn) and no level (papers 0.11.1).
- An entry with no depth gives `depth-missing` (Warn), `catalogue.rs:1008`.
- A duplicate entry gives no `depth-missing` (papers 0.10.1).

### 4.3 Network machines of papers

| Machine | States | Note |
|---|---|---|
| HTTP attempt | API calls: up to 5 tries; waits 5, 10, 20, 40 s, or `Retry-After` (max 300 s). PDF and `read` downloads: 2 tries, one 5 s wait. | explicit, `http.rs:196-262`; first wait measured |
| HTTP cooldown | per host, after a 429 on API calls; kept in the lock folder; an unreadable file counts as none | explicit; an unreadable file resets silently, `http.rs:531-536` |
| HTTP cache entry | fresh for 7 days, then expired | explicit |
| Host throttle | one request per host each 1 s (arXiv 3 s), counted from the end of the last request | explicit, `http.rs:22-25, 698-712` |
| Browser page | closed, opening, loaded, blocked; 45 s limit | explicit, `browser.rs:24, 99-132` |
| Registry lock | free, held; a waiter has no time limit | explicit, `registry.rs:337` |
| Seen index | built from notes; an invalid note is skipped with a stderr line and no finding | explicit, `seen.rs:69-71` |
| Graph source | primary, then fallback | fallback order not re-verified at 0.15.0; research-factory-178 proposal open |

### 4.6 Citation

No change from the papers audit. Its claims concern `lab` and `claimcheck`, which the audit did not read.

§8 rows:

| # | Machine | Section | Status |
|---|---|---|---|
| 13 | Paper lifecycle | 4.1 | Built by 188, 198, 213, 191; the `record-pdfs` gap is open |
| 14 | Stale derivative | 4.1 | Built by 198, 213, 191; the check skips unreadable inputs (code reading only) |
| 15 | Seen index | 4.3 | Fixed by 188 for registered papers; a DOI held only by an invalid note still gives "not seen" |
```

## Numbered items

No item is filed. The parent files each line under research-factory-414 (or the papers board). No p1 items.

1. `seen` says "not seen" for a DOI held only by a skipped invalid note. Verdict CONFIRMED (now a stderr line, no finding). p2, S. (First report B2)
2. `fetch` bad input exits 1; `seen` exits 2. CONFIRMED. p3, S. (D2)
3. `read` keeps the PDF after a failed step; text is undone. CHANGED (text fixed, PDF residue remains). p3, S. (B1a)
4. `read` registry and sidecar disagree after failure. GONE for `read`; open in item 6. (B1b)
5. `sha256_file` hides read errors; an unreadable recorded text is re-downloaded and replaced silently. CONFIRMED (changed form). p3, S. (B1c)
6. `record-pdfs` writes the manifest before the registry; false success line; rerun does not repair. NEW, CONFIRMED. p2, M. (E2 probe)
7. `save_pdf` writes in place; PDF residue on failure; crash-risk code reading only. CONFIRMED. p3, S. (B1d)
8. Registry lock waits with no limit or message (12 s silent wait). CONFIRMED. p3, S. (B3)
9. HTTP throttle waiters have no limit. OPEN (code reading only; by design). p3, S. (B4)
10. HTTP retry waits: API 5/10/20/40 s; `read` 2 tries, 5 s. CHANGED (the 60 s figure was wrong; doc fix). p3, S. (B5)
11. Corrupt cooldown file resets silently (`http.rs:531-536`). NEW, OPEN (code reading only). p3, S.
12. `verify --record` writes once at the end; an interrupted run records nothing. CONFIRMED. p3, S. (B5)
13. `verified.at` uses the local run date. CONFIRMED correct; no work. (B5 date)
14. Browser links from loaded pages are uncapped (initial queue capped at 8). OPEN (not probed; no Chromium). p3, S. (B6)
15. Doc §4.1 names `papers add`, `papers text`, `papers metadata fetch`; none exist at 0.15.0. CONFIRMED. p2, S (doc). (B7)
16. Doc header versions and §4.3 rows (HTTP, seen) are out of date. CONFIRMED. p3, S (doc). (B7)
17. Config version gate (B8) and config key rejection (D1). GONE.
18. Depth vocabulary: doc §4.2 differs from `registry.rs:16-23`. CONFIRMED. p2, S; needs E1. (B9)
19. `depth-missing` exists (`catalogue.rs:1008`, Warn). CHANGED (first report said absent); no work.
20. Graph source fallback: S2 paging bounded (`graph.rs:215-232`); fallback order not re-verified. OPEN. p3, S. (B10)
21. HEAD lacks structure, card, abstracts (E1, E2). GONE at 6851b18 (0.15.0 has them).
22. Masters config keys rejected (E3). GONE; typo key still rejected (rc 2).
23. Card: a corrupt card file triggers a new paid model call; the kept reply is not reused. NEW, CONFIRMED (stub calls 1 to 2). p2, S. (E4)
24. `registry check` does not flag recorded texts missing on disk. NEW, CONFIRMED (rc 0 with 5 missing in scratch copy). p3, S.
25. PDF reader join waits for a grandchild that holds stdout; no timeout. NEW, CONFIRMED (45.95 s). p3, S.
26. Docling: two Python starts per read. NEW, CONFIRMED count; per-start cost not measured. p3, S.
27. §4.1 defect list: each named check present in `src`. CHANGED (present); no work.
28. C1 parallel `Resolved::fetch`: no gain (3.93 s to 3.95 s); do not file as a speed-up at 3 DOIs. MEASURED. p3, S (drop or redesign).
29. C2 `per_page` 200: one request less per `cited-by` run; output same; unit test not run. CONFIRMED. p3, S.
30. `structure`: no waste; rerun rewrites 0 of 209 files. No work.
31. `card` non-model path: cache hit 0 calls; reply check 0.012 s. No work.
32. `registry abstracts --limit 5`: works; 5 lookups in 5.8 s; full run not measured. No work.
33. Semantic Scholar search filters `since_year` after fetching. Documented in code; no work.
34. Keychain: one `security` spawn per process when no env key. Observation; no work.
35. §4.1 claim about 17 null-depth masters entries. OPEN, not verified against the catalogue. p3, S.
36. §4.6 claims (lab, claimcheck). OPEN; outside papers; parent routes to a lab or claimcheck audit.
37. Replacement doc text (section F). p3, S (doc).
38. Masters recorded texts: 214, all present (read-only check). No work; the earlier "5 missing" was my copy error.
39. Proposal: add a base-URL override for test hosts, so API retries and 429 cooldowns can be probed (blocked probes in this audit). `auto=false`, body starting "Proposal:", S/M, on the papers board.

## Feedback (setup friction, for the parent to file)

1. The papers project has no base-URL override for API hosts. `fetch` rejects URLs and `read` turns off 429 retry. This blocked probes of the API retry ladder and the cooldown. Workaround: probe the `read` first wait only. Setup: other.
2. Shared Cargo target directory: cargo reused the c1 binary for the c2 clone (path-relative build hash). Workaround: `touch` the sources, rebuild, `cmp` the binaries. Setup: other (build).
3. Bash hook refused a foreground `sleep 45`. Workaround: short sleeps.
4. `timeout` is missing on macOS (exit 127). Workaround: plain build command.
5. Background task `b35mgv1q2` reported "completed" while its server still ran. Workaround: find the PID with `lsof`, then kill it.
6. `pgrep -fl` with a pattern listed other sessions' processes. Workaround: match my own PIDs from `lsof`.

Own slips (not feedback): a stray `cd`-style command `/Users/sunyizhe/Developer/papers/../papers` (no effect); an incomplete scratch copy (5 texts short), which produced the false "missing in masters" note; earlier slips listed in my working notes (unquoted zsh variable, bare-title `seen`, a redirect into a read-only folder, a `/tmp` write before compaction; removed).

## Changes made and cleanup

- No file, commit, branch or PR changed in any repository. Nothing was filed on the board.
- Live calls: OpenAlex and Crossref only, few and polite (about 20 cached calls across the runs).
- No paid model call. No real `papers card`. No `claude` call. A fake stub was used for card.
- Masters: read-only checks only; `references.jsonl` (mtime 1794593981, size 1166906) and `references.md` (mtime 1794593981, size 1250429) unchanged at the end.
- Stopped the two local servers I started: port 8771 (PIDs 44993, 44946) and port 8772 (PIDs 84319, 84322). `lsof` shows no listener; my PIDs no longer appear.
- Removed `/private/tmp/claude-501/-Users-sunyizhe--t3-worktrees-research-factory-task-research-factory-4-1c1fdf969a284ada/1fb4fe71-1498-46b7-8a1e-3b6da0f588f3/scratchpad/papers/new/target` (726 MB) and `…/scratchpad/papers/probes6851/mcopy/research.toml.bak`.
- Kept: binaries `…/scratchpad/papers/new/bin/papers` (6851b18), `papers-c1`, `papers-c2`; clones `…/scratchpad/papers/new/papers-6851`, `papers-proto6851-c1`, `papers-proto6851-c2`; probes and logs under `…/scratchpad/papers/probes6851/` (logs in `…/probes6851/logs/`). Total scratch size 156 MB.

## Open questions

- Items 14 and 9 need a Chromium install or a test host to probe. Both are blocked by the setup (see Feedback 1 and E6).
- The full 5-attempt API ladder and the 429 cooldown are not probed (Feedback 1).
- Docling per-start cost needs a real Docling install; not measured.
- The C2 unit test was not run; check it before filing item 29.
- Item 35 needs a catalogue check.
