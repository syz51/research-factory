# Item ledger for research-factory-414 (one line per subagent item -> outcome)

## papers, first report (stale checkout b7a9b95, 0.2.0) — superseded by re-audit on 6851b18
P1-1 seen exit code skipped notes -> re-verify on 6851b18
P1-2 browser link queue cap -> re-verify
P1-3 HEAD vs 0.13.1 gap / masters config keys -> no work: artifact of stale checkout (origin/main 0.15.0 has the commands)
P1-4 doc fixes §4.1-4.3, §8 13-15 -> this PR (redo from re-audit)
P1-5 read staging/undo -> re-verify
P1-6 sha256_file drops IO errors -> re-verify
P1-7 fetch bad input exits 1 -> re-verify
P1-8 registry lock wait no limit -> re-verify
P1-9 parallel Resolved::fetch -> re-verify
P1-10 OpenAlex per_page 200 -> re-verify
P1-11 verify --record at end / verified.at -> re-verify
P1-12 save_pdf partial write -> re-verify
P1-13 test gaps -> re-verify
P1-14 unread modules -> re-audit covers them
P1-F1 setup: foreground sleep blocked -> board-feedback-787
P1-F2 setup: Monitor notifications flood context -> board-feedback-787
P1-F3 setup: chained cp after cargo build delayed -> board-feedback-787
P1-F4 masters config vs HEAD -> no work: stale checkout artifact
P1-F5 /tmp slip -> no work: agent's own slip, file removed
STALE checkouts -> board-feedback-786

## lab-search (worker, commit 053e764)
S1 killed build leaves index.db.tmp-<pid> (confirmed probe, S p2) -> PROPOSAL
S2 stage `failed` never occurs; no integrity check; vectors sentence (drift) -> this PR doc
S3 refresh-budget stop has no test (by design) -> PROPOSAL tests
S4 open_current maps non-busy errors to silent rebuild (code only) -> PROBE then proposal
S5 MCP archive_open/related drop notices (confirmed, S p3) -> PROPOSAL
S6 0.25.1 main-checkout info line (drift) -> this PR doc
S7 retro.json decisions lost under parallel dismiss 20->11 (confirmed, S p2) -> PROPOSAL
S8 thesaurus append/retire same unlocked pattern (unprobed) -> in S7 proposal as acceptance check
S9 --refresh-board waits board.lock unbounded (confirmed, S p3) -> PROPOSAL
S10 corrupt board.json reported as "no export cached" (confirmed, S p3) -> PROPOSAL
S11 no test for 120 s board export timeout -> PROPOSAL tests (same as S3)
S12 linked worktree: lab related loses cited-by, lab open exits 2 for paper texts (confirmed, M p2) -> PROPOSAL
S13 graph::registry_path ignores project.registry (confirmed, S p2) -> PROPOSAL
S14 topics batch refreshes once (fixed) -> this PR
S15 notes+files walk notes once (fixed) -> this PR
S16 notes walk through ~69k ignored entries, 0.3-1.5 s per query/mcp call -> OWNER ASK (E1)
S17 registry read 3x per query, ~5 ms -> no work: 5 ms
S18 read_papers skips unreadable sidecars silently -> PROPOSAL silent-loss
S19 numeric registry keys skipped graph.rs:374 -> PROPOSAL silent-loss
S20 similar::new_files treats git error as no files -> PROPOSAL silent-loss
S21 files scope drops notes listing notices -> PROPOSAL silent-loss
S22 E2 graph stage partial when no paper edges -> decision default yes, in S13 proposal
S23 E3 drop failed/vectors from §3.1 -> decision: doc describes code; this PR
S24 doc version line 0.22.0 -> this PR doc
S25 setup: isolation hook refused compound git clone|tail and heredoc+cd -> board-feedback-788
S26 setup: lease/edit hook warnings named another checkout's edits -> board-feedback-788

## lab-archive (worker, commit fdc6974)
A1 G1 deleted manifest-sealed file has no exit from missing (probe g1, S p3) -> PROPOSAL
A2 G2 update seals invalid front matter with write_indexes=false; done refuses (probe g2, S p3) -> PROPOSAL
A3 G3 archive check writes git-seal cache; comment says read-only (probe g3, S p3) -> decision keep write; PROPOSAL fix comment
A4 G4 non-mapping front matter -> misleading sealed-edited (probe g4, S p3) -> PROPOSAL
A5 G5 renumber renames first, no rollback, prints changed before write (probe g5, M p2) -> PROPOSAL
A6 G6 reserve lock waits unbounded (probe g6, S p2) -> PROPOSAL
A7 G7 unparsable reserved-ids file reset silently, D003 twice (probe g7, S p3) -> PROPOSAL
A8 G8 board export drops stderr; bg child holds 30 s; only direct child killed (probe g8, S p2) -> PROPOSAL
A9 G9 non-terminal status moves unguarded; masters rejected not terminal (probe g9) -> OWNER ASK
A10 superseded with no successor caught only via reason_statuses -> by design: finding note
A11 board export --limit 1000 could cut list silently (code only) -> PROPOSAL (probe first)
A12 doc drift §2, §7.1, §8 -> this PR doc
A13 kept-link seal check 17->3 spawns (fixed) -> this PR
A14 sealed_at for --new-only 20->2 spawns (fixed) -> this PR
A15 untracked scan 1.06-1.54 s -> 0.08-0.11 s (fixed) -> this PR
A16 Blobs::get answers blobs only (fixed with test) -> this PR
A17 --new-only replay 25-36 s in change.rs/worktree.rs -> see lab-check report
A18 similar::new_files second full ls-files 0.7-1.7 s -> no work: measured on masters, full scan 0.27-0.33 s vs scoped 0.27 s; scoping saves too little
A19 board export sequential with file walk 0.5-0.7 s -> FIXED this PR (export started before walk; median 1.64 -> 0.91 s)
A20 stale_derivatives errors only to stderr mod.rs:617-620 -> PROPOSAL silent-loss
A21 setup: isolation hook refused git with vars/loops/xargs/heredoc -> board-feedback-788 note
A22 hyperfine not installed -> no work: /usr/bin/time and git traces sufficed
A23 index --print, next --peek, update measured, no waste -> no work: 0.12-0.36 s, no repeated work
A24 lab archive done not measured: runs every external check -> no work here: its parts are measured by the archive, papers and claimcheck tracks

## claimcheck-prov (explorer; claimcheck 0eae562, prov 8743e9d)
C1 prov placeholder before interrupt guard -> refuted at 8743e9d (fixed 9ea76bb): finding note
C2 prov kill between create and write leaves 0-byte .json (debug probe, S p3) -> PROPOSAL
C3 prov incomplete record has no exit; verify/rerun exit 2 (probe, S-M p3) -> PROPOSAL (decision E1 default: cleanup command)
C4 prov records without status read as complete -> by design (test run_record_names_its_status...): finding note
C5 prov interrupt guard: SIGTERM/SIGHUP not caught -> by design: finding note
C6 prov output drain waits for EOF unbounded, grandchild 5.1 s (probe, S-M p2) -> PROPOSAL (decision E2 default: time-bound + finding)
C7 prov preflight 7-8 sequential spawns, dup git --version; 50 -> 38 ms prototype (p3) -> PROPOSAL hot path
C8 prov import path-only entry gives no finding (probe, S p3) -> PROPOSAL
C9 prov manifest completeness matches doc -> this PR doc
C10 claimcheck unclosed quotation dropped silently (probe, S p2) -> PROPOSAL
C11 claimcheck first sha wins for shared text path (probe, S p2) -> PROPOSAL (decision E4 default: error)
C12 claimcheck unreadable checked file aborts run (probe, M p2) -> PROPOSAL (decision E5 default: report and continue)
C13 claimcheck number verdict: not-found number searched twice (C2, 67 of 140 nearby calls) -> PROPOSAL hot path
C14 claimcheck kept quotes all states tested -> no work: tests rules.rs:2011,2058,2087,2147
C15 claimcheck eager index build, lazy prototype -29% instructions, RSS -24% (p3 M) -> PROPOSAL hot path
C16 claimcheck registry double parse Value then from_value (not measured, S p3) -> PROPOSAL hot path (measure first)
C17 claimcheck check_quote repeated finds, 2 quotes in masters -> no work: no measurable waste
C18 claimcheck align row alloc, 8 calls -> no work: negligible
C19 claimcheck canonicalize per file, not measured -> no work: examined, cheap syscall per file
C20 prov git status cost 0.35 s; untracked=all claim refuted -> refuted: finding note
C21 prov record read double parse 6.5 ms -> no work: negligible
C22 prov snapshot double hashing -> by design: finding note
C23 prov/manifest rename without fsync (code only) -> PROPOSAL p3 (no probe possible; note)
C24 lineage exit 0 with skipped records -> by design (tests/cli.rs:1856-1862), decision E3 keep: finding note
C25 doc drift §4.4/4.5/§8 row 16/header -> this PR doc
C26 lab check --new-only runs every check again at merge base -> see lab-check report (dedupe with A17)
C27 unread parts of prov run.rs/manifest/import/lineage/walk/compare/dupes/finding and claimcheck check.rs/source/matcher/main/tests -> TASK coverage
C28 setup: stale checkouts -> board-feedback-786 (note #6599)
C29 setup: git clone --local takes local branch tips -> board-feedback-786 (note #6599)
C30 setup: guard-runtimes refused python3 -c -> no work: worked as designed
C31 setup: timeout absent on macOS -> no work: no cost beyond one rerun; coreutils not needed
C32 setup: "Claude V2 adapter has no active turn" on 3 Bash calls -> board-feedback-790
C33 open: masters text path shared by two refs with different shas? -> include in C11 proposal as check

## agentprof-ireval-trading (explorer; agentprof 6a72921, ireval fb47fb2, trading 911e485)
G1 agentprof rename phase leaves mixed scan set (probe, M p3) -> PROPOSAL (decision E1 default: scan id)
G2 agentprof report checks counts only (probe) -> same PROPOSAL as G1
G3 agentprof test name scan.rs:1309 overstates coverage -> same PROPOSAL as G1 (add rename-phase test)
G4 agentprof Claude candidates read whole; parallel prefilter 8.39 -> 6.27 s median (S p3) -> PROPOSAL hot path (decision E2 default: parallel now)
G5 agentprof open_turns counts tool calls not turns (probe, S p3) -> PROPOSAL
G6 agentprof bad content block drops whole message silently (probe, S p3) -> PROPOSAL silent-loss agentprof
G7 agentprof T3 malformed payload -> model unknown, no note (probe, S p3) -> same PROPOSAL as G6
G8 agentprof codex transcript-missing x468 repeats each scan -> by design rule; PROPOSAL summarise (decision E4 default: summary line)
G9 agentprof codex spawn fixpoint -> no work: ~6 ms
G10 agentprof codex turn/last_ts not probed -> COVERAGE task
G11 agentprof billing lookup no early break -> no work: negligible
G12 agentprof mentions_cwd boundary -> by design: pinned by test cwd_mentions_stop_at_path_boundaries
G13 agentprof archive shell lexer archive.rs 330-827 not read -> COVERAGE task
G14 ireval duplicate policy -> by design (probe + test): finding note
G15 ireval missing-query precedence -> verified by probe + test: no work
G16 ireval config source not in JSON (S p3) -> PROPOSAL
G17 ireval tie reorder not on real data -> by design (README, tests): finding note
G18 ireval NaN panic -> refuted by probe: finding note
G19 ireval check message text not asserted (p3) -> PROPOSAL (with G16, ireval)
G20 ireval score alias -> verified: no work
G21 ireval output path / BrokenPipe not probed -> COVERAGE task
G22 trading missing-directory hypothesis -> refuted: finding note
G23 trading INDEX crash window self-heals -> by design (test_index_rewrites_stale_index_after_rename): finding note
G24 trading idea status transitions not validated -> PROPOSAL trading
G25 trading paper status: read not validated -> same PROPOSAL trading
G26 trading only 3 tests -> same PROPOSAL trading
G27 trading YAML loader 180-470 ms, no CSafeLoader -> no work: measured, not a defect
G28 agentprof T3 per-thread queries 0.21 s -> no work
G29 ireval eval and compare 0.03-0.07 s -> no work
G30 doc: header agentprof 0.5.0, new agentprof/ireval/trading sections, §8 rows -> this PR doc
G31 agentprof/ireval unread code (scan, claude, codex, detect, session, t3, billing, report rest, util, pricing, main, tests; ireval eval/main/config rest) -> COVERAGE task
G32 setup: "Claude V2 adapter has no active turn" -> board-feedback-790 note
G33 setup: guard-runtimes blocked python3 -> no work: correct

## papers re-audit (6851b18) and lab-check (d81db6a, 62ed9d6, merged)
NOT YET LEDGERED: papers.md "Numbered items" 1-39 and lab-check.md B gaps 1-8, C sites, D points, E1-E6. Each needs one outcome line here.
