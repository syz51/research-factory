# Research data that existed only in masters worktrees

Board task research-factory-363. Inventory taken on the Air on 2026-11-13, about 23:10 JST.

## Question

The masters repository has no remote. Which git-ignored or untracked files in its worktrees under `~/.t3/worktrees/masters` exist nowhere else? A worktree cleanup would lose them.

## Answer in short

- 19 masters worktrees and the research-factory coordinator worktree were checked.
- 18 masters worktrees and the coordinator worktree held files that the main checkout lacks. In all, 393 files, about 124 MB.
- One file is large: a 103 MB partial embedding file from the E1 bakeoff (`t3-3f8cbcf2`).
- The rest are small. They are run records, E5 output, untracked lab check records, paper caches, source texts, lab search boards and lab seals.
- The 2026-11-12 archive-search bakeoff output is not at risk. Main holds all 67 of its output files with equal sha256. The 875 MB folder in `t3-3d22d7ca` is 822 MB of `.venv`.
- `rf-166-check-errors` has no untracked files. Its one commit, dfc65dd, is not on main. Main's 2afed4b holds the same change with its own check records. The commit is saved as a patch.
- Each of the 393 manifest copies matches the sha256 of its source. The patch matches its own sha256. No worktree, branch or file was removed.

## Where the copies are

`air:masters/worktree-rescue-2026-11-13/` (on the Air: `~/Pool/masters/worktree-rescue-2026-11-13/`):

- `masters/<worktree>/<path>`: the files that main lacks, at their worktree paths.
- `masters/rf-166-check-errors-…/dfc65dd.patch`: the commit that is only on that branch.
- `research-factory/coordinator-research-fa-…/`: 12 `.research/papers` files of the coordinator worktree.
- `manifests/<repo>--<worktree>.tsv`: every untracked or ignored file with its status, size and sha256.

The destination is `air`, not `cloud`. masters AGENTS.md ("Confidentiality and safety") rules out plain uploads of research material, and `pro` was offline.

## Method

1. `scripts/inventory.py` lists each worktree's untracked and ignored files (`git ls-files -o`) and its tracked files with uncommitted edits (`git diff HEAD`). It lists the files of a nested repository one by one. No worktree had uncommitted edits to tracked files.
2. For each file, it computes the sha256 and gives one status:
   - `in-main`: the main checkout has the same path with the same sha256.
   - `unique`: main lacks the path, or its sha256 differs.
   - `dup:<worktree>`: an earlier worktree already gave the same path and sha256. The file is copied once.
   - `skip:<reason>`: a rebuildable file or not a regular file. The rebuildable files are under `.venv`, `__pycache__`, `.ruff_cache`, `.pytest_cache` or `node_modules`, or are `.DS_Store` or `.research/lab/search/index.db`. The runtime locks are `.research/lab/search/lock`, `.research/lab/search/board.lock` and `docs/research/references.jsonl.lock`. They are empty or hold a process id. The only files that are not regular are the interpreter symlinks in `.venv/bin`.
3. `scripts/stage.sh` copies the `unique` rows and the manifests to a stage folder. Then `storagectl put <stage> air:masters/` sends the folder.
4. `scripts/verify.sh` hashes each copy on the pool and its live source. It compares both with the manifest.

## Results by worktree

| Worktree | Unique files | Size | What |
|---|---:|---:|---|
| cards-docling-286 | 28 | 0.9 MB | run records 15, paper cache and replies 8, source texts 4, lab seal |
| chore-lab-mcp | 2 | 0.6 MB | 1 untracked lab check record, lab seal |
| chore-paper-preprocessi | 187 | 4.0 MB | paper HTTP cache 186, lab seal |
| exp-e5-sessions-board | 28 | 2.4 MB | E5 run output 26, 1 lab check record, lab seal |
| rf-166-check-errors | 0 | 0 | commit dfc65dd saved as a patch |
| t3-24408cbc | 21 | 5.8 MB | 20 untracked lab check records, lab seal |
| t3-3853c686 | 3 | 1.1 MB | lab search board and log, lab seal |
| t3-3befe7fd | 4 | 1.2 MB | lab search board and log, lab seal, paper search log |
| t3-3d22d7ca | 1 | 0 | lab seal |
| t3-3f8cbcf2 | 2 | 103 MB | E1 bakeoff partial embeddings, lab seal |
| t3-4470bcb4 | 1 | 0 | lab seal |
| t3-50484c1c | 3 | 1.1 MB | lab search board and log, lab seal |
| t3-6bbea333 | 3 | 1.1 MB | lab search board and log, lab seal |
| t3-81955246 | 7 | 0.03 MB | agentprof output 3, lab loop state 4 |
| t3-9e67b28e | 3 | 1.2 MB | lab search board and log, lab seal |
| t3-afa7bffb | 2 | 0 | lab search log, lab seal |
| t3-ca4acb1f | 1 | 0 | lab seal |
| t3-df8870cd | 77 | 1.6 MB | source texts 39, paper cache 38 |
| t3-e7658687 | 8 | 2.8 MB | 7 untracked lab check records, lab seal |
| research-factory coordinator | 12 | 0.1 MB | paper HTTP cache 11, paper search log |

Sizes of 0 are under 0.01 MB. The board notes on research-factory-363 give the same results per worktree.

## Verification

`bash scripts/verify.sh ~/Pool/masters/worktree-rescue-2026-11-13` printed:

```
copies matching manifest: 393, mismatched: 0, sources changed since inventory: 0
```

It exited 0.

A later rerun gave the same counts, except 1 source changed: the lab seal of the live worktree `t3-9e67b28e`. The fixed `inventory.py` rerun gave the same unique files for every other worktree.

`dfc65dd.patch` is in no manifest. Its sha256 on air equals that of `git format-patch -1 dfc65dd --stdout`: `68d63770c80fbaa8dc55df01d2627e7e8718256a3bccdd399490f843487c91d3`.

## Limits

- `t3-9e67b28e` was a live worktree (research-factory-378). It committed during the copy. Its first inventory listed 7 files, which it then committed or which main received. Its manifest was taken again, and its 3 new unique files were sent. The 7 first-snapshot files also stay on air. No manifest lists them, so `verify.sh` does not check them.
- Other live worktrees can gain files after this inventory. Run the scripts again before a worktree is removed.
- The air pool is on the same SSD as the worktrees. The copy survives a worktree removal, not a loss of the Air. The masters backup reads only the main checkout. vps-245 covers adding the worktrees and `~/Pool/masters` to the backup.
- No file was copied into the masters main checkout. Lab seals and search boards differ per worktree, and copying them would overwrite main's.
