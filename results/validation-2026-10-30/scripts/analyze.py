#!/usr/bin/env python3
"""Summarize the 2026-10-30 run: verdicts, counts against 0.1.2 / TLC (today's and 10-07's),
Examples against the manifests, and the speedups.
  analyze.py RESULTS.jsonl TLC_1007.jsonl SWEEP_1007.jsonl EXAMPLES_1007.jsonl"""
import json, math, sys, collections
R = [json.loads(l) for l in open(sys.argv[1])]
T7 = {(r["dir"], r["cfg"]): r for r in map(json.loads, open(sys.argv[2]))}
S7 = {(r["dir"], r["cfg"]): r for r in map(json.loads, open(sys.argv[3]))}
X7 = {r["model"]: r for r in map(json.loads, open(sys.argv[4]))}
by = collections.defaultdict(dict)
tlc, ex, builds = {}, [], []
for r in R:
    if r["kind"] in ("interp", "compiled"):
        by[(r["dir"], r["cfg"])][(r["kind"], r["version"])] = r
    elif r["kind"] == "tlc":
        tlc[(r["dir"], r["cfg"])] = r
    elif r["kind"] == "examples":
        ex.append(r)
    elif r["kind"] == "build":
        builds.append(r)
print(f"# records: {len(R)}; entries with a result: {len(by)}")

def final(k, v):
    """the verdict of record for version v: compiled where it ran (the big ones), else interp"""
    d = by[k]
    return d.get(("compiled", v)) or d.get(("interp", v))

c = collections.Counter(); bad = []
cnt = collections.Counter(); cbad = []
for k in sorted(by):
    a, b = final(k, "0.1.2"), final(k, "0.2.0")
    if b is None:
        bad.append(f"{k}: no 0.2.0 result"); continue
    c[(b["status"], "compiled" if ("compiled", "0.2.0") in by[k] else "interp")] += 1
    if b["status"] != "agree" and b["status"] != "timeout":
        bad.append(f"{k[1]}: 0.2.0 {b['status']} {b.get('err')}")
    if a and a["status"] != b["status"]:
        bad.append(f"{k[1]}: 0.1.2 {a['status']} 0.2.0 {b['status']}")
    exact = b["gate_kind"] in ("strict_run", "liveness_mutation_run")
    if exact and b["status"] == "agree":
        refs = {}
        if a and a["status"] == "agree": refs["0.1.2"] = a.get("distinct")
        t = tlc.get(k)
        if t and t["status"] == "done" and t.get("violated") is None and b["gate_kind"] == "strict_run": refs["TLC today"] = t.get("distinct")
        t7 = T7.get(k)
        if t7 and t7.get("tlc_distinct") and not t7.get("tlc_violated") and t7.get("tlc_rc") == 0 and b["gate_kind"] == "strict_run": refs["TLC 10-07"] = t7["tlc_distinct"]
        s7 = S7.get(k)
        if s7 and s7.get("status") == "agree" and s7.get("distinct"): refs["0.1.0 10-07"] = s7["distinct"]
        i2 = by[k].get(("interp", "0.2.0"))
        if i2 and i2 is not b and i2["status"] == "agree": refs["0.2.0 interp"] = i2.get("distinct")
        for name, val in refs.items():
            cnt[(name, val == b.get("distinct"))] += 1
            if val != b.get("distinct"):
                cbad.append(f"{k[1]} ({b['gate_kind']}): 0.2.0 {b.get('distinct')} vs {name} {val}")
print("verdicts of record (0.2.0):", dict(c))
print("verdict problems:", len(bad)); print("\n".join("  " + x for x in bad))
print("exact counts, 0.2.0 against:", {f"{n} {'same' if s else 'DIFF'}": v for (n, s), v in sorted(cnt.items())})
print("\n".join("  " + x for x in cbad))
tl = collections.Counter(r["status"] for r in tlc.values()); print("TLC runs:", dict(tl))

# Examples
g = collections.Counter(("accepted" if r["accepted"] else r["got"]) for r in ex)
print(f"Examples: {len(ex)} models:", dict(g))
was = sum(1 for r in X7.values() if r["got"] == r["want"] and (r.get("want_distinct") is None or r["distinct"] == r["want_distinct"]))
print(f"  10-07 (0.1.0, Examples 6a7b26a): {len(X7)} models, {was} accepted")
lost = [r["model"] for r in ex if not r["accepted"] and r["model"] in X7 and X7[r["model"]]["got"] == X7[r["model"]]["want"] and (X7[r["model"]].get("want_distinct") is None or X7[r["model"]]["distinct"] == X7[r["model"]]["want_distinct"])]
print("  accepted on 10-07, not now:", lost)
wrong = [f"{r['model']}: want {r['want']} {r.get('want_distinct')}, got {r['got']} {r.get('distinct')}" for r in ex if not r["accepted"] and r["got"] in ("success", "safety failure", "liveness failure", "action failure")]
print("  a result other than the manifest's:", wrong)
print("  not accepted, by reason:", dict(collections.Counter(r["got"] for r in ex if not r["accepted"])))
print("  builds:", dict(collections.Counter(b["status"] for b in builds)))

# speed
def gm(xs): return math.exp(sum(math.log(x) for x in xs) / len(xs)) if xs else float("nan")
for kind in ("interp", "compiled"):
    for grp in ("formal", "lean/formal"):
        rows = []
        for k, d in by.items():
            a, b = d.get((kind, "0.1.2")), d.get((kind, "0.2.0"))
            if k[0] == grp and a and b and a["status"] == b["status"] == "agree" and a["secs"] >= 1:
                rows.append((k[1], a, b))
        if not rows: continue
        sp = [a["secs"] / b["secs"] for _, a, b in rows]
        rss = [b["rss_gb"] / a["rss_gb"] for _, a, b in rows if a["rss_gb"] > 0.05]
        print(f"{kind} {grp}: {len(rows)} entries of 1 s or more on 0.1.2: geo-mean {gm(sp):.2f}x (min {min(sp):.2f}, max {max(sp):.2f}); "
              f"sum {sum(a['secs'] for _, a, _ in rows):.0f}s -> {sum(b['secs'] for _, _, b in rows):.0f}s; RSS geo-mean x{gm(rss):.2f}, max 0.2.0 {max(b['rss_gb'] for _, _, b in rows):.1f} GB")
