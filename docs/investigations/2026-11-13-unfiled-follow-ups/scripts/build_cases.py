"""Write one compact Markdown case file per sampled item for the judges."""
import json, re, sys, glob, collections, random, os
from datetime import datetime, timedelta
D = sys.argv[1]; CUT = "2026-11-12T03:09"
ts = lambda s: datetime.fromisoformat(s.replace("Z", "+00:00"))
sample = json.load(open(f"{D}/sample.json"))
late = {r["task"]: r for r in json.load(open(f"{D}/late.json"))}
cls = {r["task"]: r for r in json.load(open(f"{D}/classified.json"))}
tasks = {}
for f in glob.glob(f"{D}/tasks-*.json"):
    for t in json.load(open(f)): tasks[t["id"]] = t
ev = [json.loads(l) for l in open(f"{D}/events.jsonl")]
claims = collections.defaultdict(list)
created_by = collections.defaultdict(list)
for e in ev:
    if e["type"] in ("task_claimed", "task_taken"): claims[(e["task"], e["actor"])].append(e["at"])
    if e["type"] in ("task_created", "ask_created"): created_by[e["actor"]].append(e)
txs = [json.loads(l) for l in open(f"{D}/tx.jsonl")]
def done_id(d):
    tid = d["input"].get("id") or ""
    if not tid:
        m = re.search(r"board done ([a-z0-9-]+-\d+)", d["input"].get("command", "")); tid = m.group(1) if m else ""
    return tid
idx = collections.defaultdict(list)
for fi, o in enumerate(txs):
    for i, (k, d, t) in enumerate(o["seq"]):
        if k == "res-done" and done_id(d): idx[done_id(d)].append((fi, i, t))
def ttl(tid):
    t = tasks.get(tid); return f'{tid} "{t["title"][:90]}" parent={t.get("parent")} proposal={t.get("proposal")}' if t else tid
os.makedirs(f"{D}/cases", exist_ok=True)
for tid in sample["all"]:
    r = late[tid]; A, T = r["actor"], ts(r["at"])
    c = [x for x in idx.get(tid, []) if x[2] and abs((ts(x[2]) - T).total_seconds()) < 900]
    fi, i, t0 = min(c, key=lambda x: abs((ts(x[2]) - T).total_seconds()))
    seq = txs[fi]["seq"]
    sh = json.load(open(f"{D}/show/{tid}.json"))
    cl = min(claims.get((tid, A), [r["at"]]))
    before = [e for e in created_by[A] if cl <= e["at"] <= r["at"]]
    after = [e for e in created_by[A] if r["at"] < e["at"] <= (T + timedelta(hours=24)).isoformat().replace("+00:00", "Z")]
    L = [f"# Case {tid}", f"board: {r['board']} · closed {r['at']} · period {'post' if r['at'] >= CUT else 'pre'} (open-items Stop hook live from 2026-11-12T03:09Z) · closing agent {A}",
         f"title: {sh['task']['title']}", f"transcript: ~/.claude/projects/{txs[fi]['file']}",
         "", "## task_done summary", (sh["task"].get("summary") or "(none)")[:1500],
         "", "## Filed by the closing agent between its claim and task_done"] + [f"- {e['type']} {ttl(e['task']) if e['type']=='task_created' else 'ask on '+str(e['task'])} at {e['at']}" for e in before] + \
        ["", "## Filed by the closing agent within 24 h after task_done"] + [f"- {e['type']} {ttl(e['task']) if e['type']=='task_created' else 'ask on '+str(e['task'])} at +{int((ts(e['at'])-T).total_seconds()//60)} min" for e in after]
    L += ["", f"mechanical: late filings related to this task (parent or body names it): {r and [h[1] for h in r['hits'] if h[0] in ('task','ask')]}; trigger per transcript: {cls.get(tid, {}).get('trig')}"]
    turn, last, prompt = 0, None, "(task_done in this turn)"
    blocks = []
    for k, d, t in seq[i + 1:]:
        if k == "res-done" and done_id(d) != tid and turn > 0: break
        if t and ts(t) > ts(t0) + timedelta(hours=24): break
        if k in ("user", "hook", "compact"):
            if last is not None or turn == 0: blocks.append((turn, prompt, last or ""))
            last = None; turn += 1; prompt = f"[{k}] " + (d if isinstance(d, str) else "")[:400]
            if turn > 5: break
            continue
        if k in ("res-add", "res-ask", "res-note", "res-feedback"):
            m = re.findall(r"\b[a-z0-9]+(?:-[a-z0-9]+)*-\d+\b|ask #\d+", d["result"][:200])
            last = (last or "") + f"\n[filed: {k[4:]} -> {', '.join(m[:3])}]"
        if k == "text" and d.strip(): last = (last or "") + "\n" + d
    if last is not None: blocks.append((turn, prompt, last))
    for turn, p, rep in blocks:
        cap = 7000 if turn == 0 else 3500
        L += ["", f"## Turn {turn} after task_done" + (" (the closing turn)" if turn == 0 else ""), f"prompt: {p}", "reply:", rep.strip()[:cap] + (" …[cut]" if len(rep.strip()) > cap else "")]
    open(f"{D}/cases/{tid}.md", "w").write("\n".join(L) + "\n")
# no-task sessions: flagged ones plus a seeded random 14 unflagged
random.seed(242)
flag = set(json.load(open(f"{D}/notask-flagged.json")))
rows = [json.loads(l) for l in open(f"{D}/replies-notask.jsonl")]
files = sorted({r["file"] for r in rows})
unflag = random.sample([f for f in files if f not in flag], 14)
det = collections.defaultdict(dict)
for l in open(f"{D}/detect-notask.jsonl"):
    x = json.loads(l); det[x["file"]][x["turn"]] = x["unfiled"]
txn = {json.loads(l)["file"]: json.loads(l) for l in open(f"{D}/tx-notask.jsonl")}
nt = []
for n, f in enumerate(sorted(flag) + unflag):
    seq = txn[f]["seq"]; name = f"notask-{n:02d}"; nt.append({"case": name, "file": f, "flagged": f in flag})
    first = next((d for k, d, t in seq if k == "user"), "")
    filed = [re.findall(r"\b[a-z0-9]+(?:-[a-z0-9]+)*-\d+\b|ask #\d+", d["result"][:200])[:2] for k, d, t in seq if k in ("res-add", "res-ask", "res-feedback", "res-note")]
    L = [f"# Case {name} (a thread with no claimed task)", f"transcript: ~/.claude/projects/{f}", f"detector flagged: {f in flag}", f"first prompt: {first[:500]}", f"filed in the session (in order): {filed}"]
    turn, last, prompt = 0, None, "(start)"
    turns = []
    for k, d, t in seq + [("user", "", None)]:
        if k in ("user", "hook", "compact"):
            if last: turns.append((turn, prompt, last))
            last = None; turn += 1; prompt = f"[{k}] " + (d if isinstance(d, str) else "")[:300]; continue
        if k in ("res-add", "res-ask", "res-note", "res-feedback"):
            m = re.findall(r"\b[a-z0-9]+(?:-[a-z0-9]+)*-\d+\b|ask #\d+", d["result"][:200]); last = (last or "") + f"\n[filed: {k[4:]} -> {', '.join(m[:2])}]"
        if k == "text" and d.strip(): last = (last or "") + "\n" + d
    keep = [x for x in turns if det[f].get(x[0] - 1 if False else x[0])] or []
    flagged_turns = {t for t, u in det[f].items() if u}
    show = [x for x in turns if x[0] in flagged_turns or x[0] == turns[-1][0]] if turns else []
    for t, p, rep in show[-4:]:
        L += ["", f"## Turn {t}", f"prompt: {p}", "reply:", rep.strip()[:4000]]
    open(f"{D}/cases/{name}.md", "w").write("\n".join(L) + "\n")
json.dump(nt, open(f"{D}/notask-cases.json", "w"), indent=0)
print(len(os.listdir(f"{D}/cases")))
