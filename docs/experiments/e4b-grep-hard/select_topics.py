"""Selects the E4b topics: the ones whose question gives grep the weakest handle.

Usage: uv run --no-project python select_topics.py <masters copy> <testset dir> [n] [--table]

The rule is in README.md ("Selection rule"). For each known-item or topical
topic with a grade-2 record, the handle is the smallest document frequency,
over the git-tracked Markdown files outside private/, of a term of the topic's
`need` that occurs in a grade-2 record. E4's topics leave the pool, except
T09 (failed in every E4 condition), which leads. Topics without a handle come
next, then larger handles. Prints the first
n topic ids (default 24), one per line; --table prints every pool topic with
its handle to stderr.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

FORCED = ["T09"]
# E4's topics (e4-delivery/README.md). Its cli answered 22 of 24 correctly; only
# T09 is known to have failed, so the others are at ceiling and leave the pool.
E4 = set(
    "K31 K32 K33 K34 K35 T14 K01 T01 K02 T02 K03 T03 K04 T04 K05 T05 K06 T06 "
    "K07 T07 K08 T08 K09 T09".split()
) - set(FORCED)
TOKEN = re.compile(r"[a-z0-9]+")
STOP = set(
    """
    the and for are but not you all any can her was one our out has have had his how its
    who what when where which why with this that these those from into onto than then them
    they their there here been being does did doing done also only such each more most
    other some very just over under about after before between both same own whether
    while would could should will shall may might must per via
    """.split()
)


def terms(text: str) -> set[str]:
    return {t for t in TOKEN.findall(text.lower()) if len(t) >= 3 and t not in STOP}


def main() -> None:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    root, testset = Path(args[0]), Path(args[1])
    n = int(args[2]) if len(args) > 2 else 24
    files = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z", "--", "*.md"],
        capture_output=True, check=True, text=True,
    ).stdout.split("\0")
    files = [f for f in files if f and not f.startswith("private/")]
    vocab: dict[str, set[str]] = {
        f: terms((root / f).read_text(encoding="utf-8", errors="replace")) for f in files
    }
    df: dict[str, int] = {}
    for words in vocab.values():
        for w in words:
            df[w] = df.get(w, 0) + 1

    topics = [json.loads(line) for line in (testset / "topics.jsonl").open()]
    good: dict[str, list[str]] = {}
    for line in (testset / "qrels-records.txt").open():
        topic, _, doc, grade = line.split()
        if grade == "2":
            good.setdefault(topic, []).append(doc)

    pool = []
    for i, t in enumerate(topics):
        if t["type"] not in ("known-item", "topical") or t["id"] not in good or t["id"] in E4:
            continue
        need = terms(t["need"])
        hits = {w for doc in good[t["id"]] if doc in vocab for w in need & vocab[doc]}
        handle = min((df[w] for w in hits), default=None)
        word = min(hits, key=lambda w: (df[w], w)) if hits else "-"
        pool.append((t["id"], handle, word, i))

    def key(row):
        tid, handle, _, i = row
        return (tid not in FORCED, handle is not None, -(handle or 0), i)

    pool.sort(key=key)
    print(f"corpus: {len(files)} files; pool: {len(pool)} topics", file=sys.stderr)
    if "--table" in sys.argv:
        for tid, handle, word, _ in pool:
            print(f"{tid}\t{handle if handle is not None else '-'}\t{word}", file=sys.stderr)
    for tid, *_ in pool[:n]:
        print(tid)


if __name__ == "__main__":
    main()
