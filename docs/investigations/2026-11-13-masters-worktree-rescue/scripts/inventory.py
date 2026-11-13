"""Inventory untracked and ignored files in each worktree against its main checkout.

Usage: inventory.py <main checkout> <out dir> <worktree>...
Writes <out dir>/<worktree name>.tsv with: status, size, sha256, path.
Status: in-main (same path, same sha256 in main), unique, dup:<worktree> (same path and
sha256 already kept from an earlier worktree), skip:<reason> (rebuildable cache).
"""
import hashlib, os, subprocess, sys
from pathlib import Path

SKIP_PARTS = {".venv": "venv", "__pycache__": "pycache", ".ruff_cache": "ruff-cache",
              ".pytest_cache": "pytest-cache", "node_modules": "node-modules"}

def skip_reason(rel: str) -> str | None:
    parts = rel.split("/")
    for p in parts[:-1]:
        if p in SKIP_PARTS:
            return SKIP_PARTS[p]
    name = parts[-1]
    if name == ".DS_Store":
        return "ds-store"
    if rel == ".research/lab/search/index.db":
        return "search-index"
    if name.endswith(".lock") or rel == ".research/lab/search/lock":
        return "lock-file"
    return None

def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()

main, out = Path(sys.argv[1]), Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
kept: dict[tuple[str, str], str] = {}
for wt in map(Path, sys.argv[3:]):
    files = subprocess.run(["git", "-C", str(wt), "ls-files", "-o", "-z"],
                           check=True, capture_output=True).stdout.split(b"\0")
    rows = []
    for raw in filter(None, files):
        rel = raw.decode()
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
