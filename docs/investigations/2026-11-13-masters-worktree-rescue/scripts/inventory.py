"""Inventory the files a worktree removal would lose, against the main checkout.

Usage: inventory.py <main checkout> <out dir> <worktree>...
Lists untracked and ignored files (git ls-files -o) and tracked files with uncommitted
edits (git diff HEAD). Files inside a nested repository that git reports as one
directory entry are listed one by one. Writes <out dir>/<worktree name>.tsv with:
status, size, sha256, path.
Status: in-main (same path, same sha256 in main), unique, dup:<worktree> (same path and
sha256 already kept from an earlier worktree), skip:<reason> (rebuildable or not a file).
"""
import hashlib, subprocess, sys
from pathlib import Path

SKIP_PARTS = {".venv": "venv", "__pycache__": "pycache", ".ruff_cache": "ruff-cache",
              ".pytest_cache": "pytest-cache", "node_modules": "node-modules"}
# Runtime locks of lab and the reference registry; empty or a process id.
RUNTIME_LOCKS = {".research/lab/search/lock", ".research/lab/search/board.lock",
                 "docs/research/references.jsonl.lock"}

def skip_reason(rel: str) -> str | None:
    parts = rel.split("/")
    for p in parts[:-1]:
        if p in SKIP_PARTS:
            return SKIP_PARTS[p]
    if parts[-1] == ".DS_Store":
        return "ds-store"
    if rel == ".research/lab/search/index.db":
        return "search-index"
    if rel in RUNTIME_LOCKS:
        return "lock-file"
    return None

def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()

def git_paths(wt: Path, *args: str) -> list[str]:
    out = subprocess.run(["git", "-C", str(wt), *args, "-z"], check=True, capture_output=True).stdout
    return [raw.decode() for raw in out.split(b"\0") if raw]

def candidates(wt: Path) -> set[str]:
    paths = set(git_paths(wt, "diff", "--name-only", "HEAD"))
    for rel in git_paths(wt, "ls-files", "-o"):
        p = wt / rel
        if rel.endswith("/") and p.is_dir() and not p.is_symlink():
            # A nested repository: git lists only its directory.
            paths.update(str(f.relative_to(wt)) for f in p.rglob("*") if f.is_file() or f.is_symlink())
        else:
            paths.add(rel.rstrip("/"))
    return paths

main, out = Path(sys.argv[1]), Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
kept: dict[tuple[str, str], str] = {}
for wt in map(Path, sys.argv[3:]):
    rows = []
    for rel in candidates(wt):
        p = wt / rel
        if p.is_symlink() or not p.is_file():
            rows.append(("skip:not-regular-file", 0, "-", rel)); continue
        size = p.stat().st_size
        reason = skip_reason(rel)
        if reason:
            rows.append((f"skip:{reason}", size, "-", rel)); continue
        digest = sha256(p)
        m = main / rel
        if m.is_file() and not m.is_symlink() and m.stat().st_size == size and sha256(m) == digest:
            status = "in-main"
        elif (rel, digest) in kept:
            status = f"dup:{kept[(rel, digest)]}"
        else:
            status = "unique"; kept[(rel, digest)] = wt.name
        rows.append((status, size, digest, rel))
    with (out / f"{wt.name}.tsv").open("w") as f:
        f.write("status\tsize\tsha256\tpath\n")
        for r in sorted(rows, key=lambda r: r[3]):
            f.write("\t".join(map(str, r)) + "\n")
    print(wt.name, len(rows), flush=True)
