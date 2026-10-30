#!/usr/bin/env python3
"""The tlaplus/Examples gate: every model in LIST through tla-mc, against the
result its manifest records (TLC's). A model that is refused, times out, or
gives another verdict or distinct count fails the gate: tla-mc never checks
less than the cfg asks, and what it accepts it gets exactly right.

  examples-gate.py EXAMPLES BIN LIST        the gate; exits 1 on any failure
  examples-gate.py EXAMPLES BIN             a survey: every exhaustive-search
                                            model, with the time it took, to
                                            build LIST from (see gate/README.md)
"""
import glob, json, os, re, shutil, subprocess, sys, time

EX, BIN = os.path.abspath(sys.argv[1]), os.path.abspath(sys.argv[2])  # the models run from their own directories
LIST = sys.argv[3] if len(sys.argv) > 3 else None
WORKERS = os.environ.get("WORKERS", "4")
TIMEOUT = float(os.environ.get("TIMEOUT", "300"))
META = os.path.abspath(os.environ.get("META", "examples-gate-meta"))

models = []  # (module path, model dict)
for f in sorted(glob.glob(f"{EX}/specifications/*/manifest.json")):
    for mod in json.load(open(f)).get("modules", []):
        for mo in mod.get("models", []):
            if mo.get("mode") == "exhaustive search":
                models.append((mod["path"], mo))
if LIST:
    want = [l.split()[0] for l in open(LIST) if l.strip() and not l.startswith("#")]
    by_path = {mo["path"]: (m, mo) for m, mo in models}
    missing = [p for p in want if p not in by_path]
    if missing:
        sys.exit(f"not in the Examples manifests (Examples moved? pin it, or refresh the list): {missing}")
    models = [by_path[p] for p in want]


def run(module, mo):
    tla = os.path.join(EX, module)
    d = os.path.dirname(tla)
    cfg = os.path.relpath(os.path.join(EX, mo["path"]), d)
    t0 = time.time()
    try:
        p = subprocess.run([BIN, "-workers", WORKERS, "-metadir", META, "-config", cfg, os.path.basename(tla)],
                           cwd=d, capture_output=True, text=True, timeout=TIMEOUT)
        o, rc = p.stdout + p.stderr, p.returncode
    except subprocess.TimeoutExpired:
        o, rc = "TIMEOUT", -1
    finally:
        shutil.rmtree(META, ignore_errors=True)
    secs = time.time() - t0
    m = re.search(r"(\d+) distinct states found", o)
    distinct = int(m.group(1)) if m else None
    if rc == -1:
        got = "timeout"
    elif rc == 2:
        got = "unsupported"
    elif rc == 0:
        got = "success"
    elif re.search(r"^Error: Temporal propert", o, re.M):
        got = "liveness failure"
    elif re.search(r"^Error: Action property", o, re.M):
        # `[][A]_v` found violated by a step; TLC finds the same violation in
        # its liveness pass when the property is a whole Spec (a refinement)
        got = "action failure"
    elif re.search(r"^Error: (Invariant|Property|Deadlock)", o, re.M):
        got = "safety failure"
    else:
        got = "error"
    tail = [l for l in o.splitlines() if l.startswith("tla-mc:") or l.startswith("Error")]
    return got, distinct, secs, (tail[-1] if tail else "")[:200]


failed = 0
for module, mo in models:
    got, distinct, secs, tail = run(module, mo)
    want, want_distinct = mo["result"], mo.get("distinctStates")
    verdict_ok = got == want or (got == "action failure" and want in ("safety failure", "liveness failure"))
    ok = verdict_ok and (want_distinct is None or distinct == want_distinct)
    if LIST:
        mark = "ok  " if ok else "FAIL"
        failed += not ok
        why = "" if ok else f"  want {want} {want_distinct}, got {got} {distinct} {tail}"
        print(f"{mark} {mo['path']} {secs:.1f}s{why}", flush=True)
    else:
        print(f"{'accepted' if ok else got:10} {secs:7.1f}s {mo['path']} {'' if ok else tail}", flush=True)
if LIST:
    print(f"{len(models) - failed} of {len(models)} as the manifests record" + (f"; {failed} FAILED" if failed else ""))
    sys.exit(1 if failed else 0)
