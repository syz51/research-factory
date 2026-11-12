"""Grades the E4b sessions as E4 did, and adds token counts and run order.

Usage: uv run --no-project python measure.py <testset dir> <out dir>

<out dir>/<condition>/<topic>-<rep>.jsonl is one session's stream-json
transcript (run_session.sh); <out dir>/order.log lists the session starts.
Grading and the counts are E4's `session()` (../e4-delivery/measure.py):
`correct` means the answer names a file that the topic grades 2 in
qrels-records.txt. Added per session, from the result message:
input, output, cache-read and cache-creation tokens (all model calls), and
`position`: the session's start rank among its topic's 4 sessions (1-4).
Prints one JSON object per session, with no archive text.
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "e4-delivery"))
from measure import load_grades, session

TOKENS = ["input_tokens", "output_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"]


def main() -> None:
    testset, out = Path(sys.argv[1]), Path(sys.argv[2])
    grades = load_grades(testset)
    position: dict[tuple[str, str, str], int] = {}
    seen: dict[str, int] = {}
    for line in (out / "order.log").open():
        _, _, cond, topic, rep = line.split()
        seen[topic] = seen.get(topic, 0) + 1
        position[(cond, topic, rep)] = seen[topic]
    for path in sorted(out.glob("*/*.jsonl")):
        cond = path.parent.name
        topic, rep = path.stem.rsplit("-", 1)
        m = session(path, grades.get(topic, {}))
        usage = {}
        for line in path.open():
            if '"type":"result"' in line:
                usage = json.loads(line).get("usage") or {}
        row = {"topic": topic, "rep": int(rep), "condition": cond,
               "position": position.get((cond, topic, rep))}
        row.update({k: v for k, v in m.items() if not k.startswith("_")})
        row.update({k: usage.get(k, 0) for k in TOKENS})
        print(json.dumps(row))


if __name__ == "__main__":
    main()
