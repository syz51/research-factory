"""Reads the E4 session transcripts and grades each answer.

Usage: uv run --no-project python measure.py <testset dir> <out dir> [--show]

<out dir>/<condition>/<topic>.jsonl is the stream-json transcript of one
session (run_session.sh). Prints one JSON object per session, with no text of
the archive: topic, condition, correct, the counts below, tokens and cost.

- correct: the final answer names a file with grade 2 for the topic in
  qrels-records.txt. A path counts with or without a line range, in full or
  as a suffix of the judged path (see `judged`).
- strict: correct, and the answer names no file that the topic grades 0
  (such as a replaced record, or a page cited only as context).
- main_tool_calls: tool calls of the main agent (subagent calls not counted).
- all_tool_calls: tool calls of the main agent and its subagents.
- main_context_tokens: the main agent's context at its last model call
  (input, cache read and cache creation tokens).
- searches: calls of `lab search` (Bash) or archive_search (MCP), anywhere.
- greps: Grep tool calls and Bash commands that run grep, rg, ag or find.
- reads: Read tool calls, archive_open calls, `lab open` and Bash cat/sed/head/tail.
- search_share: searches / (searches + greps); None without either.
- archivist: calls of the archivist subagent by the main agent.
"""

import json
import re
import sys
from pathlib import Path

PATH = re.compile(r"[\w./\-]+\.md")
GREP = re.compile(r"(^|[|;&(]|&&)\s*(git\s+grep|grep|egrep|rg|ag|find)\s")
LAB_SEARCH = re.compile(r"(^|[|;&(]|&&)\s*(\S*/)?lab\s+search\b")
LAB_OPEN = re.compile(r"(^|[|;&(]|&&)\s*(\S*/)?lab\s+(open|related)\b")
SHOW = re.compile(r"(^|[|;&(]|&&)\s*(cat|sed|head|tail|less|awk)\s")


def load_grades(testset: Path) -> dict[str, dict[str, int]]:
    grades: dict[str, dict[str, int]] = {}
    for line in (testset / "qrels-records.txt").open():
        topic, _, doc, grade = line.split()
        grades.setdefault(topic, {})[doc] = int(grade)
    return grades


def judged(answer: str, grades: dict[str, int]) -> set[str]:
    """The judged files that the answer names. Answers write paths in full,
    relative to a folder (`decisions/DR037-….md`) or as a bare name, so a
    named path matches the one judged file whose path ends with it; a suffix
    of several (a bare `README.md`) matches none."""
    out = set()
    for tok in PATH.findall(answer):
        tok = tok.removeprefix("./")
        hits = {d for d in grades if d == tok or d.endswith("/" + tok)}
        if len(hits) == 1:
            out |= hits
    return out


def session(path: Path, grades: dict[str, int]) -> dict:
    m = {
        "main_tool_calls": 0,
        "all_tool_calls": 0,
        "searches": 0,
        "greps": 0,
        "reads": 0,
        "archivist": 0,
        "main_context_tokens": 0,
        "cost_usd": None,
        "turns": None,
        "ok": False,
    }
    answer = ""
    for line in path.open():
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        kind = msg.get("type")
        if kind == "assistant":
            main = msg.get("parent_tool_use_id") is None
            usage = msg["message"].get("usage") or {}
            if main and usage:
                m["main_context_tokens"] = (
                    usage.get("input_tokens", 0)
                    + usage.get("cache_read_input_tokens", 0)
                    + usage.get("cache_creation_input_tokens", 0)
                )
            for c in msg["message"].get("content", []):
                if c.get("type") != "tool_use":
                    continue
                name, args = c["name"], c.get("input", {})
                m["all_tool_calls"] += 1
                m["main_tool_calls"] += main
                if name in ("Task", "Agent") and args.get("subagent_type") == "archivist":
                    m["archivist"] += 1
                elif name == "mcp__lab__archive_search":
                    m["searches"] += 1
                elif name in ("mcp__lab__archive_open", "Read"):
                    m["reads"] += 1
                elif name in ("Grep", "Glob"):
                    m["greps"] += 1
                elif name == "Bash":
                    cmd = args.get("command", "")
                    m["searches"] += bool(LAB_SEARCH.search(cmd))
                    m["greps"] += bool(GREP.search(cmd))
                    m["reads"] += bool(LAB_OPEN.search(cmd) or SHOW.search(cmd))
        elif kind == "result":
            answer = msg.get("result") or ""
            m["cost_usd"] = msg.get("total_cost_usd")
            m["turns"] = msg.get("num_turns")
            m["ok"] = msg.get("subtype") == "success"
    named = judged(answer, grades)
    good = any(grades[p] == 2 for p in named)
    wrong = any(grades[p] == 0 for p in named)
    total = m["searches"] + m["greps"]
    m["search_share"] = m["searches"] / total if total else None
    m["correct"] = bool(good and m["ok"])
    m["strict"] = bool(good and not wrong and m["ok"])
    m["_answer"] = answer
    m["_named"] = sorted(named)
    return m


def main() -> None:
    testset, out = Path(sys.argv[1]), Path(sys.argv[2])
    show = "--show" in sys.argv
    grades = load_grades(testset)
    for path in sorted(out.glob("*/*.jsonl")):
        topic, cond = path.stem, path.parent.name
        m = session(path, grades.get(topic, {}))
        row = {"topic": topic, "condition": cond}
        row.update({k: v for k, v in m.items() if not k.startswith("_")})
        print(json.dumps(row))
        if show:
            print("  named:", m["_named"], file=sys.stderr)
            print("  grade-2:", [d for d, g in grades.get(topic, {}).items() if g == 2], file=sys.stderr)


if __name__ == "__main__":
    main()
