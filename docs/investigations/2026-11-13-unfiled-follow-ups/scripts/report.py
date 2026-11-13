"""Tables and the case appendix for the record. Usage: report.py <scratch dir> <out dir>"""
import json, glob, collections, sys, math, os
S, O = sys.argv[1], sys.argv[2]
P1, CUT = "2026-11-07T07:33", "2026-11-12T03:09"
J = {}
for f in sorted(glob.glob(f"{S}/judge/out-*.jsonl")) + sorted(glob.glob(f"{S}/judge/rout-*.jsonl")):
    for l in open(f): o = json.loads(l); J[o["case"]] = o
s = json.load(open(f"{S}/data/sample.json")); late = {r["task"]: r for r in json.load(open(f"{S}/data/late.json"))}
cls = {r["task"]: r for r in json.load(open(f"{S}/data/classified.json"))}
nt = {c["case"]: c for c in json.load(open(f"{S}/data/notask-cases.json"))}
seq = {}
for l in open(f"{S}/data/events.jsonl"):
    e = json.loads(l)
    if e["type"] == "task_done" and e["task"] not in seq: seq[e["task"]] = e["seq"]
BAD = {"late-hook", "late-prompted", "never"}
per = lambda t: "post" if late[t]["at"] >= CUT else "pre"
per3 = lambda t: "P0" if late[t]["at"] < P1 else ("P1" if late[t]["at"] < CUT else "P2")
def wilson(k, n, z=1.96):
    if not n: return (0, 0)
    p = k / n; d = 1 + z * z / n; c = (p + z * z / (2 * n)) / d; h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (round(100 * (c - h)), round(100 * (c + h)))
def lv(o):
    it = o["items"]; st = {i["status"] for i in it}
    return {"any": bool(it), "chat": bool(st & BAD), "close": any(i["status"] in BAD and not (i.get("turn") or 0) for i in it),
            "never": "never" in st, "after": bool(st & {"late-same-turn", "late-hook", "late-prompted"})}
out = []
w = out.append
# 1. mechanical table
w("## T1 mechanical\n| Board | Closed | Transcript found | Late task or ask | of which within 10 min | Trigger: same turn | owner prompt | compaction | Late note on closed task |\n|---|--:|--:|--:|--:|--:|--:|--:|--:|")
agg = collections.defaultdict(collections.Counter)
for t, r in late.items():
    a = agg[r["board"]]; a["closed"] += 1
    th = [h for h in r["hits"] if h[0] in ("task", "ask")]; nn = [h for h in r["hits"] if h[0] == "note-on-closed"]
    if th: a["late"] += 1; a["le10"] += min(h[2] for h in th) <= 600
    if nn: a["note"] += 1
    c = cls.get(t)
    if c and c.get("found"): a["found"] += 1
    if c:
        for k in {k for _, k in c.get("trig", [])}: a[k] += 1
for b, a in sorted(agg.items(), key=lambda x: -x[1]["closed"]):
    f = a["found"] if b != "board-feedback" else "not read"
    w(f"| {b} | {a['closed']} | {f} | {a['late']} | {a['le10']} | {a['same-turn']} | {a['user']} | {a['compact']} | {a['note']} |")
T = collections.Counter()
for a in agg.values(): T.update(a)
w(f"| all | {T['closed']} | {T['found']} | {T['late']} | {T['le10']} | {T['same-turn']} | {T['user']} | {T['compact']} | {T['note']} |")
# 2. sample A per board and period
w("\n## T2 sample A\n| Board | Period | Sampled | Any open item | Left in chat (any turn) | Left in chat in closing turn | Never filed |\n|---|---|--:|--:|--:|--:|--:|")
tab = collections.defaultdict(collections.Counter); tot = collections.defaultdict(collections.Counter)
for t in s["A"]:
    L = lv(J[t]); k = (late[t]["board"], per(t)); tab[k]["n"] += 1; tot[per(t)]["n"] += 1; tot[per3(t)]["n"] += 1
    for x, v in L.items(): tab[k][x] += v; tot[per(t)][x] += v; tot[per3(t)][x] += v
for (b, p), c in sorted(tab.items()):
    w(f"| {b} | {p} | {c['n']} | {c['any']} | {c['chat']} | {c['close']} | {c['never']} |")
for p in ("pre", "post", "P0", "P1", "P2"):
    c = tot[p]; n = c["n"]
    w(f"| all | {p} | {n} | {c['any']} | {c['chat']} ({round(100*c["chat"]/n)}%, {wilson(c['chat'],n)}) | {c['close']} ({round(100*c["close"]/n)}%, {wilson(c['close'],n)}) | {c['never']} ({round(100*c["never"]/n)}%, {wilson(c['never'],n)}) |")
# population weights
pop = collections.Counter(per(t) for t in late if t in cls and cls[t].get("found") and t in {json.loads(l)["task"] for l in open(f"{S}/data/replies.jsonl")})
w(f"\npopulation with replies: {dict(pop)}")
for x in ("chat", "close", "never"):
    r = sum(tot[p][x] / tot[p]["n"] * pop[p] for p in ("pre", "post")) / sum(pop.values())
    w(f"weighted {x}: {100*r:.0f}%")
# 3. where and turn of unfiled items in A
wc = collections.Counter((i["where"], "closing" if not (i.get("turn") or 0) else "later", i["status"]) for t in s["A"] for i in J[t]["items"] if i["status"] in BAD)
w("\n## T3 unfiled items in A (where, turn, status)"); [w(f"- {k}: {v}") for k, v in sorted(wc.items())]
wp = collections.Counter((per(t), i["where"], "closing" if not (i.get("turn") or 0) else "later") for t in s["A"] for i in J[t]["items"] if i["status"] in BAD)
[w(f"- {k}: {v}") for k, v in sorted(wp.items())]
w(f"- all items, all judged: {dict(collections.Counter(i['where'] for o in J.values() for i in o['items']))}")
w(f"- unfiled items, all judged: {dict(collections.Counter(i['where'] for o in J.values() for i in o['items'] if i['status'] in BAD))}")
w(f"- statuses, all judged: {dict(collections.Counter(i['status'] for o in J.values() for i in o['items']))}")
# 4. B
kinds = collections.Counter((x.get("kind"), x.get("prompt")) for t in s["B"] for x in J[t].get("late", []))
w(f"\n## T4 B late kinds: {dict(kinds)}")
w(f"B asked: {dict(collections.Counter(i.get('asked') for t in s['B'] for i in J[t]['items'] if i['status']=='late-prompted'))}")
w(f"B cases {len(s['B'])}, with a prompted miss: {sum(any(x.get('kind')=='miss' and x.get('prompt') not in ('same-turn',) for x in J[t].get('late',[])) for t in s['B'])}")
# 5. notask
nk = collections.Counter()
for c, v in nt.items():
    if c not in J: continue
    o = J[c]; L = lv(o)
    nk[("flagged" if v["flagged"] else "unflagged", o.get("thread_kind"), "never" if L["never"] else ("fp" if o.get("detector_fp") else "ok"))] += 1
w("\n## T5 notask"); [w(f"- {k}: {v}") for k, v in sorted(nk.items(), key=str)]
open(f"{O}/tables.md", "w").write("\n".join(out) + "\n")
# appendix
A, B, D = set(s["A"]), set(s["B"]), set(s["D"])
ap = ["| Case | Sets | Period | Done event | Session | held | late same turn | late after prompt | late after hook | never | no work | Late filings (kind, prompt) |", "|---|---|---|--:|---|--:|--:|--:|--:|--:|--:|---|"]
for t in sorted(J, key=lambda x: (x.startswith("notask"), x)):
    o = J[t]; st = collections.Counter(i["status"] for i in o["items"])
    lt = "; ".join(f"{x['id']} ({x.get('kind')}, {x.get('prompt')})" for x in o.get("late", []))
    if t.startswith("notask"):
        f = nt[t]["file"]; sid = os.path.basename(f)[:-6]
        sets = ("flagged" if nt[t]["flagged"] else "unflagged") + f", {o.get('thread_kind')}" + (", detector false positive" if o.get("detector_fp") else "")
        ap.append(f"| {t} | {sets} | – | – | {sid} | {st['held']} | {st['late-same-turn']} | {st['late-prompted']} | {st['late-hook']} | {st['never']} | {st['no-work']} | {lt} |")
    else:
        sets = ",".join(x for x, ss in (("A", A), ("B", B), ("D", D)) if t in ss)
        f = cls.get(t, {}).get("file") or ""; sid = os.path.basename(f)[:-6] if f else "–"
        ap.append(f"| {t} | {sets} | {per3(t)} | #{seq[t]} | {sid} | {st['held']} | {st['late-same-turn']} | {st['late-prompted']} | {st['late-hook']} | {st['never']} | {st['no-work']} | {lt} |")
mech = ["| Task | Done event | Late task or ask (minutes after done) | Trigger |", "|---|--:|---|---|"]
for t, r in sorted(late.items()):
    th = [h for h in r["hits"] if h[0] in ("task", "ask")]
    if not th: continue
    trig = "; ".join(f"{i} {k}" for i, k in cls.get(t, {}).get("trig", [])) or ("triage board" if r["board"] == "board-feedback" else "–")
    hs = "; ".join(f"{h[1] if h[0]=='task' else 'ask on '+h[1]} ({h[2]/60:.0f})" for h in th)
    mech.append(f"| {t} | #{seq[t]} | {hs} | {trig} |")
open(f"{O}/cases-judged.md", "w").write("\n".join(ap) + "\n")
open(f"{O}/cases-mechanical.md", "w").write("\n".join(mech) + "\n")
print(len(ap) - 2, len(mech) - 2)
