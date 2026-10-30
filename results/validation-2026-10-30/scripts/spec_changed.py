#!/usr/bin/env python3
"""For each flint gate entry: did its cfg, or any module its root module EXTENDS or
INSTANCEs (transitively, within its directory), change between two flint commits?
  spec_changed.py FLINT_GIT OLD NEW ENTRIES.jsonl > changed.json"""
import json, re, subprocess, sys
G, OLD, NEW, ENT = sys.argv[1:5]
def show(rev, path):
    p = subprocess.run(["git", "-C", G, "show", f"{rev}:{path}"], capture_output=True, text=True)
    return p.stdout if p.returncode == 0 else None
def closure(rev, d, mod, seen):
    if mod in seen: return
    src = show(rev, f"{d}/{mod}.tla")
    if src is None: return
    seen[mod] = src
    for m in re.findall(r"\bEXTENDS\s+([\w\s,]+)", src):
        for x in re.split(r"[\s,]+", m.strip()):
            if x: closure(rev, d, x, seen)
    for x in re.findall(r"\bINSTANCE\s+(\w+)", src):
        closure(rev, d, x, seen)
out = {}
for l in open(ENT):
    r = json.loads(l)
    d, cfg, mod = r["dir"], r["cfg"], r["module"]
    a, b = {}, {}
    closure(OLD, d, mod, a); closure(NEW, d, mod, b)
    same = a == b and show(OLD, f"{d}/{cfg}") == show(NEW, f"{d}/{cfg}") and show(OLD, f"{d}/{cfg}") is not None
    out[f"{d}/{cfg}"] = not same
json.dump(out, sys.stdout)
