"""Selects the E4 topics from the masters archive search test set.

Usage: uv run --no-project python select_topics.py <testset dir> [n]

A topic is eligible when it is a known-item or topical topic (the paper texts
of passage topics are git-ignored and not in the scratch copy, and reference
topics ask for registry rows, not archive notes) and it is not at ceiling: the
grep baseline run (runs/grep.run) does not rank a grade-2 record first. The
first n eligible topics in test-set order are printed, one id per line, the
present-state topics (K31-K35, T14) first, then known-item and topical
topics in turn.
"""

import json
import sys
from pathlib import Path

PRESENT_STATE = ["K31", "K32", "K33", "K34", "K35", "T14"]


def main() -> None:
    testset = Path(sys.argv[1])
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 24
    topics = [json.loads(line) for line in (testset / "topics.jsonl").open()]
    grades: dict[tuple[str, str], int] = {}
    for line in (testset / "qrels-records.txt").open():
        topic, _, doc, grade = line.split()
        grades[(topic, doc)] = int(grade)
    first: dict[str, str] = {}
    for line in (testset / "runs" / "grep.run").open():
        topic, _, doc, rank, *_ = line.split()
        if rank == "1":
            first[topic] = doc
    eligible = [
        t["id"]
        for t in topics
        if t["type"] in ("known-item", "topical")
        and grades.get((t["id"], first.get(t["id"], "")), 0) < 2
    ]
    ordered = [t for t in PRESENT_STATE if t in eligible]
    # Then known-item and topical topics in turn, in test-set order.
    known = [t for t in eligible if t.startswith("K") and t not in ordered]
    topical = [t for t in eligible if t.startswith("T") and t not in ordered]
    for pair in zip(known, topical):
        ordered += pair
    longer = known if len(known) > len(topical) else topical
    ordered += longer[min(len(known), len(topical)) :]
    print(f"eligible: {len(eligible)} of {len(topics)}", file=sys.stderr)
    for topic in ordered[:n]:
        print(topic)


if __name__ == "__main__":
    main()
