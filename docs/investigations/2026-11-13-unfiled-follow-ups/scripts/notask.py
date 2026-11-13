import json, sys
D = sys.argv[1]
out = open(f"{D}/replies-notask.jsonl", "w")
for line in open(f"{D}/tx-notask.jsonl"):
    o = json.loads(line); seq = o["seq"]
    nuser = sum(1 for k, *_ in seq if k == "user")
    files = [d for k, d, t in seq if k.startswith("res-") and k != "res-done"]
    turn, last, t_last = 0, None, None
    for k, d, t in seq + [("user", "", None)]:
        if k in ("user", "hook", "compact"):
            if last: out.write(json.dumps({"task": o["file"], "board": "notask", "done_at": t_last or "", "file": o["file"], "turn": turn, "reply": last, "nuser": nuser, "nfiled": len(files)}) + "\n")
            last = None; turn += 1; continue
        if k == "text" and d.strip(): last, t_last = d, t
