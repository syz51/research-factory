"""Index closing transcripts: for every task_done, the turn structure after it."""
import json, re, sys, os
files = [l.strip() for l in open(sys.argv[1]) if l.strip()]
root = os.path.expanduser("~/.claude/projects")
FILE_TOOLS = {"mcp__board__task_add": "add", "mcp__board__ask_user": "ask",
              "mcp__board__task_note": "note", "mcp__board__feedback": "feedback"}
out = []
for rel in files:
    path = os.path.join(root, rel)
    seq = []  # (kind, data, ts)
    pending = {}
    for line in open(path, errors="replace"):
        try: r = json.loads(line)
        except Exception: continue
        t = r.get("type")
        if t not in ("user", "assistant") or r.get("isSidechain"): continue
        msg = r.get("message") or {}
        c = msg.get("content")
        ts = r.get("timestamp")
        if t == "user":
            if isinstance(c, str):
                if r.get("isMeta"): continue
                k = "hook" if c.startswith("Stop hook feedback") else ("cmd" if c.startswith("<command") or c.startswith("<local-command") else "user")
                if c.startswith("This session is being continued"): k = "compact"
                seq.append((k, c[:300], ts))
            elif isinstance(c, list):
                for b in c:
                    if b.get("type") == "tool_result" and b.get("tool_use_id") in pending:
                        kind, inp = pending.pop(b["tool_use_id"])
                        txt = b.get("content")
                        if isinstance(txt, list): txt = " ".join(x.get("text", "") for x in txt if isinstance(x, dict))
                        seq.append(("res-" + kind, {"input": inp, "result": (txt or "")[:400]}, ts))
                    elif b.get("type") == "text" and not r.get("isMeta"):
                        tx = b.get("text", "")
                        if tx.startswith("<system-reminder>") or tx.startswith("[Request interrupted"): continue
                        seq.append(("user", tx[:300], ts))
        else:
            for b in c or []:
                if b.get("type") == "tool_use":
                    n = b.get("name", ""); inp = b.get("input", {})
                    if n == "mcp__board__task_done":
                        pending[b["id"]] = ("done", inp)
                    elif n in FILE_TOOLS:
                        pending[b["id"]] = (FILE_TOOLS[n], inp)
                    elif n == "Bash":
                        cmd = inp.get("command", "")
                        m = re.search(r"\bboard (done|add|ask|note|feedback)\b", cmd)
                        if m and "--help" not in cmd:
                            pending[b["id"]] = (m.group(1), {"command": cmd[:400]})
                elif b.get("type") == "text":
                    seq.append(("text", b.get("text", ""), ts))
    out.append({"file": rel, "seq": seq})
with open(sys.argv[2], "w") as f:
    for o in out: f.write(json.dumps(o) + "\n")
print(len(out))
