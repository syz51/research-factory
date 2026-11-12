"""Round 2 of E4b: the screening queue and the topic selection (README, "Round 2").

Usage:
  uv run --no-project python screen.py queue <testset dir>
      prints the screening queue, `cli <topic> <k> <k>` for phrasing k = 1, 2 of
      each pool topic (known-item or topical, with a grade-2 record); the rep
      number is the phrasing number
  uv run --no-project python screen.py select <testset dir> <measure output> [n]
      prints the round-2 topics.txt, `<topic> <phrasing>` lines: topics with a
      wrong screening session, both phrasings wrong first, then test-set order;
      at most n (default 24). Counts go to stderr.
"""

import json
import sys
from pathlib import Path


def pool(testset: Path) -> list[str]:
    good = {line.split()[0] for line in (testset / "qrels-records.txt").open() if line.split()[3] == "2"}
    topics = [json.loads(line) for line in (testset / "topics.jsonl").open()]
    return [t["id"] for t in topics if t["type"] in ("known-item", "topical") and t["id"] in good]


def main() -> None:
    mode, testset = sys.argv[1], Path(sys.argv[2])
    ids = pool(testset)
    if mode == "queue":
        for t in ids:
            for k in (1, 2):
                print("cli", t, k, k)
        return
    rows = [json.loads(line) for line in open(sys.argv[3])]
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 24
    wrong = {(r["topic"], r["rep"]) for r in rows if r["condition"] == "cli" and not r["correct"]}
    done = {(r["topic"], r["rep"]) for r in rows if r["condition"] == "cli"}
    picked = []
    for t in ids:
        ks = [k for k in (1, 2) if (t, k) in wrong]
        if ks:
            picked.append((len(ks) < 2, ids.index(t), t, ks[0]))
    picked.sort()
    print(f"pool {len(ids)} topics; screened {len(done)} of {2 * len(ids)} prompts; "
          f"wrong {len(wrong)}; topics with a wrong prompt {len(picked)}; "
          f"both wrong {sum(not p[0] for p in picked)}", file=sys.stderr)
    for _, _, t, k in picked[:n]:
        print(t, k)


if __name__ == "__main__":
    main()
