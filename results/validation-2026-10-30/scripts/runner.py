#!/usr/bin/env python3
"""tla-mc 0.2.0's full validation, and 0.1.2 against it, on one c8g.48xlarge (2026-10-30).

Phase 1, packed onto the cores (each job reserves the threads it runs; longest first by
the 2026-10-07 times, the two versions of an entry side by side):
  interp    every flint gate entry under BIG_PRIOR s on 2026-10-07, 0.2.0 and 0.1.2,
            8 workers, sweep.py's verdict rules, INTERP_CAP s; an entry either version
            does not finish in that joins the big ones
  tlc       TLC 1.7.4, 8 workers, on the entries 0.2.0 decided in under TLC_UNDER s
            (TLC is not run on the big ones)
  examples  every exhaustive-search model of tlaplus/Examples, 0.2.0, 8 workers,
            examples-gate.py's rules, EX_CAP s
  build     each big entry's compiled checker, per version (its own cache), untimed
Phase 2, one at a time on an otherwise idle machine: every big entry through both
versions' compiled checker at all the cores, the order alternating, BIG_CAP s
(UNDECIDED_CAP for the three no checker has decided). Every tla-mc run has
TLAMC_LEVEL_PROFILE set, so a run stopped at its cap still says how far it got.

  runner.py        ($FLINT_ROOT $EXAMPLES $TLAMC_LIB $TLC_JAR $TLAMC_020 $TLAMC_012 $OUT $WORK
                    $PRIOR_GATE $PRIOR_EXAMPLES; optional $ONLY_CFGS, $ONLY_MODELS, $PHASES)
"""
import glob, json, os, re, shlex, shutil, signal, subprocess, sys, threading, time

E = os.environ
ROOT, EX, LIB, JAR, OUT, WORK = E["FLINT_ROOT"], E["EXAMPLES"], E["TLAMC_LIB"], E["TLC_JAR"], E["OUT"], E["WORK"]
BIN = {"0.2.0": E["TLAMC_020"], "0.1.2": E["TLAMC_012"]}
CORES = int(E.get("CORES") or os.cpu_count())
BIG_PRIOR = float(E.get("BIG_PRIOR", "60"))
INTERP_CAP = float(E.get("INTERP_CAP", "120"))
TLC_UNDER = float(E.get("TLC_UNDER", "20"))
TLC_CAP = float(E.get("TLC_CAP", "300"))
EX_CAP = float(E.get("EX_CAP", "600"))
BIG_CAP = float(E.get("BIG_CAP", "1200"))
UNDECIDED_CAP = float(E.get("UNDECIDED_CAP", "180"))
MIN_AVAIL_GB = float(E.get("MIN_AVAIL_GB", "40"))
JOB_RSS_GB = float(E.get("JOB_RSS_GB", "120"))
TLC_MAX = int(E.get("TLC_MAX", "10"))
PHASES = E.get("PHASES", "1,2").split(",")
UNDECIDED = {"LeanImmutableRenameHolds.cfg", "LeanImmutableRepairOverridesUI.cfg", "LeanImmutableAnsweredRecordSkipped.cfg"}
VIOL = re.compile(r"(?:Invariant|Temporal property|Action property|Property) (\S+)(?: \(for [^)]*\))? is violated")
os.makedirs(f"{OUT}/logs", exist_ok=True)
os.makedirs(WORK, exist_ok=True)
LOCK = threading.RLock()
RES = open(f"{OUT}/results.jsonl", "a")
PROG = open(f"{OUT}/progress.log", "a")


def say(*a):
    with LOCK:
        PROG.write(time.strftime("%H:%M:%S ") + " ".join(str(x) for x in a) + "\n"); PROG.flush()


def gate_entries():
    """flint's gate, parsed as sweep.py parses it"""
    out = []
    for script, d in [("scripts/check-tla.sh", "formal"), ("lean/formal/check.sh", "lean/formal")]:
        V = {}
        for line in open(f"{ROOT}/{script}").read().replace("\\\n", " ").splitlines():
            m = re.match(r"\s*([A-Z][A-Za-z0-9_]*)=(\w+)\s*$", line)
            if m:
                V[m.group(1)] = m.group(2)
            m = re.match(r"\s*(strict_run|mutation_run|liveness_mutation_run)\s+(.*)", line)
            if not m:
                continue
            try:
                args = shlex.split(re.sub(r"\$(\w+)", lambda v: V.get(v.group(1), v.group(0)), m.group(2)))
            except ValueError:
                continue
            kind = m.group(1)
            want = args[3] if kind == "mutation_run" and len(args) > 3 else None
            out.append(dict(dir=d, gate_kind=kind, module=args[0], cfg=args[1], want=want))
    return out


def examples_models():
    out = []
    for f in sorted(glob.glob(f"{EX}/specifications/*/manifest.json")):
        for mod in json.load(open(f)).get("modules", []):
            for mo in mod.get("models", []):
                if mo.get("mode") == "exhaustive search":
                    out.append(dict(module=mod["path"], model=mo["path"], want=mo["result"],
                                    want_distinct=mo.get("distinctStates"), tlc_runtime=mo.get("runtime")))
    return out


def mem_avail_gb():
    try:
        for l in open("/proc/meminfo"):
            if l.startswith("MemAvailable:"):
                return int(l.split()[1]) / 1048576
    except OSError:
        pass
    return 1e9


def vmrss_gb(pid):
    try:
        for l in open(f"/proc/{pid}/status"):
            if l.startswith("VmRSS:"):
                return int(l.split()[1]) / 1048576
    except OSError:
        pass
    return 0.0


def execute(job):
    """run the job's command in its own process group; rc -1 at its cap, -2 past JOB_RSS_GB"""
    log = f"{OUT}/logs/{job['id']}.log"
    env = dict(os.environ, **job.get("env", {}))
    meta = f"{WORK}/meta-{job['id']}"
    cmd = [meta if a == "@META" else a for a in job["cmd"]]
    res, done = {}, threading.Event()
    with open(log, "w") as lf:
        lf.write("# " + " ".join(cmd) + "\n# cwd " + job["cwd"] + "\n"); lf.flush()
        t0 = time.time()
        p = subprocess.Popen(cmd, cwd=job["cwd"], stdout=lf, stderr=subprocess.STDOUT, env=env, start_new_session=True)

        def reap():
            _, st, ru = os.wait4(p.pid, 0)
            res.update(t=time.time(), st=st, ru=ru)
            p.returncode = os.waitstatus_to_exitcode(st)  # reaped here: Popen must not wait on a reused pid
            done.set()
        threading.Thread(target=reap, daemon=True).start()
        peak, killed = 0.0, None
        while not done.wait(1.0):
            peak = max(peak, vmrss_gb(p.pid))
            if job.get("stop_on") and job["stop_on"] in open(log, errors="replace").read():
                killed = 0  # a build job: the checker is built, its run is not wanted
            elif time.time() - t0 > job["cap"]:
                killed = -1
            elif peak > JOB_RSS_GB:
                killed = -2
            if killed is not None:
                try:
                    os.killpg(p.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                done.wait()
                break
    shutil.rmtree(meta, ignore_errors=True)
    rc = killed if killed is not None else os.waitstatus_to_exitcode(res["st"])
    peak = max(peak, res["ru"].ru_maxrss / 1048576)
    out = open(log, errors="replace").read()
    return rc, res["t"] - t0, peak, out


def progress(out):
    lv = re.findall(r"^level (\d+): (\d+) states", out, re.M)
    return dict(levels=int(lv[-1][0]) + 1 if lv else None, states_seen=sum(int(n) for _, n in lv) if lv else None)


def judge_gate(job, rc, out):
    m = re.search(r"(\d+) distinct states found", out)
    v = VIOL.search(out)
    kind, want = job["gate_kind"], job["want"]
    if rc == -1:
        st = "timeout"
    elif rc == -2:
        st = "killed"
    elif rc == 2:
        st = "unsupported"
    elif kind == "liveness_mutation_run":
        st = "agree" if rc == 12 and v else "DISAGREE"
    elif kind == "strict_run":
        st = "agree" if rc == 0 else "DISAGREE"
    else:
        st = "agree" if rc == 12 and v and (want is None or re.search(want, v.group(1)) or want in out) else "DISAGREE"
    err = ""
    if rc not in (0, 12, -1):
        lines = [l for l in out.strip().splitlines() if not l.startswith("level ") and not l.startswith("# ")]
        err = (lines[-1] if lines else "")[:200]
    return dict(status=st, distinct=int(m.group(1)) if m else None, violated=v.group(1) if v else None, err=err,
                built_now="tla-mc: building a checker" in out, **progress(out))


def judge_tlc(rc, out):
    m = re.findall(r"(\d+) distinct states found", out)
    v = re.search(r"Invariant (\S+) is violated", out)
    other = "temporal" if "Temporal properties were violated" in out else "deadlock" if "Deadlock reached" in out else None
    return dict(status="timeout" if rc == -1 else "killed" if rc == -2 else "done", distinct=int(m[-1]) if m else None,
                violated=v.group(1) if v else other)


def judge_example(job, rc, out):
    m = re.search(r"(\d+) distinct states found", out)
    distinct = int(m.group(1)) if m else None
    if rc == -1:
        got = "timeout"
    elif rc == -2:
        got = "killed"
    elif rc == 2:
        got = "unsupported"
    elif rc == 0:
        got = "success"
    elif re.search(r"^Error: Temporal propert", out, re.M):
        got = "liveness failure"
    elif re.search(r"^Error: Action property", out, re.M):
        got = "action failure"
    elif re.search(r"^Error: (Invariant|Property|Deadlock)", out, re.M):
        got = "safety failure"
    else:
        got = "error"
    want, wd = job["want"], job["want_distinct"]
    ok = (got == want or (got == "action failure" and want in ("safety failure", "liveness failure"))) and (wd is None or distinct == wd)
    tail = [l for l in out.splitlines() if l.startswith("tla-mc:") or l.startswith("Error")]
    return dict(got=got, distinct=distinct, accepted=ok, err=(tail[-1] if tail else "")[:200], **progress(out))


# ---------------------------------------------------------------- phase 1
prior = {}
for l in open(E["PRIOR_GATE"]):
    r = json.loads(l)
    prior[(r["dir"], r["cfg"])] = r["secs"]
prior_ex = {}
for l in open(E["PRIOR_EXAMPLES"]):
    r = json.loads(l)
    prior_ex[r["model"]] = r["secs"]
only = set(E["ONLY_CFGS"].split(",")) if E.get("ONLY_CFGS") else None
only_ex = set(E["ONLY_MODELS"].split(",")) if E.get("ONLY_MODELS") else None
ENTRIES = [e for e in gate_entries() if only is None or e["cfg"] in only]
for e in ENTRIES:
    e["prior"] = prior.get((e["dir"], e["cfg"]))
BIG = {(e["dir"], e["cfg"]) for e in ENTRIES if (e["prior"] or 0) >= BIG_PRIOR}
say(f"start: {len(ENTRIES)} gate entries, {len(BIG)} big by the 10-07 times; {CORES} cores; {mem_avail_gb():.0f} GB available")
LEVEL = {"TLAMC_LEVEL_PROFILE": "1"}
pending, running, used, seq = [], {}, 0, [0]
interp_done = {}  # (dir, cfg) -> {version: record}
build_queued = set()


def add(job):
    seq[0] += 1
    job["id"] = f"{seq[0]:04d}-{job['kind']}-{job.get('version', 'tlc')}-{job.get('cfg') or os.path.basename(job['model'])}"
    pending.append(job)


def gate_job(e, kind, version, cap, workers, est, extra_env=None):
    j = dict(e, kind=kind, version=version, cap=cap, threads=min(workers, CORES), est=est,
             cwd=f"{ROOT}/{e['dir']}", env=dict(LEVEL, **(extra_env or {})))
    j["cmd"] = [BIN[version]] + (["-compile"] if kind in ("build", "compiled") else []) + \
        ["-workers", str(workers), "-metadir", "@META", "-config", e["cfg"], e["module"] + ".tla"]
    return j


def queue_build(e):
    k = (e["dir"], e["cfg"])
    if k in build_queued:
        return
    build_queued.add(k)
    for v in BIN:
        # the build, then the checker stops after its first level
        j = gate_job(e, "build", v, 1800, 1, 30, {"TLAMC_CACHE": f"{WORK}/cache-{v}", "TLAMC_STOP_AFTER_SECS": "0"})
        j["stop_on"] = "tla-mc: built in"
        add(j)


def finished(job, rec):
    """what a finished job adds to the queue"""
    k = (job.get("dir"), job.get("cfg"))
    if job["kind"] == "interp":
        interp_done.setdefault(k, {})[job["version"]] = rec
        if rec["status"] == "timeout" and k not in BIG:
            BIG.add(k)
            say("joins the big ones (interp cap):", job["cfg"], job["version"])
            queue_build(job)
        if job["version"] == "0.2.0" and rec["status"] in ("agree", "DISAGREE") and rec["secs"] < TLC_UNDER:
            t = dict((x, job[x]) for x in ("dir", "cfg", "module", "gate_kind", "want"))
            t.update(kind="tlc", cap=TLC_CAP, threads=8, est=rec["secs"] * 8, cwd=job["cwd"], env={},
                     cmd=["java", "-XX:+UseParallelGC", "-Xmx8g", "-XX:MaxDirectMemorySize=8g", "-cp", JAR, "tlc2.TLC",
                          "-workers", "8", "-metadir", "@META", "-config", job["cfg"], job["module"] + ".tla"])
            add(t)


def run_job(job):
    global used
    try:
        rc, secs, peak, out = execute(job)
        rec = {x: job.get(x) for x in ("id", "kind", "version", "dir", "cfg", "module", "gate_kind", "want", "model",
                                       "want_distinct", "tlc_runtime", "threads", "cap", "prior")}
        rec.update(rc=rc, secs=round(secs, 3), rss_gb=round(peak, 3))
        if job["kind"] == "tlc":
            rec.update(judge_tlc(rc, out))
        elif job["kind"] == "examples":
            rec.update(judge_example(job, rc, out))
        elif job["kind"] == "build":
            b = re.search(r"tla-mc: built in ([\d.]+)s", out)
            rec.update(status="built" if b else "build-failed", build_secs=float(b.group(1)) if b else None,
                       err="" if b else "\n".join(out.strip().splitlines()[-3:])[:300])
        else:
            rec.update(judge_gate(job, rc, out))
    except Exception as ex:
        rec = dict(id=job["id"], kind=job["kind"], status="harness", err=repr(ex))
    with LOCK:
        RES.write(json.dumps(rec) + "\n"); RES.flush()
        used -= job["threads"]
        del running[job["id"]]
        finished(job, rec)
    say("done", job["id"], rec.get("status") or rec.get("got"), f"{rec.get('secs')}s", f"{rec.get('rss_gb')}GB",
        rec.get("distinct"), rec.get("violated") or "", rec.get("err") or "")


def drain():
    global used
    while True:
        with LOCK:
            if not pending and not running:
                return
            pending.sort(key=lambda j: -j["est"])
            for j in list(pending):
                if used + j["threads"] > CORES:
                    continue
                if running and mem_avail_gb() < MIN_AVAIL_GB:
                    break
                if j["kind"] == "tlc" and sum(1 for r in running.values() if r["kind"] == "tlc") >= TLC_MAX:
                    continue
                pending.remove(j)
                used += j["threads"]
                running[j["id"]] = j
                threading.Thread(target=run_job, args=(j,), daemon=True).start()
        time.sleep(0.3)


if "1" in PHASES:
    for e in ENTRIES:
        k = (e["dir"], e["cfg"])
        if k in BIG:
            queue_build(e)
        else:
            for v in BIN:
                add(gate_job(e, "interp", v, INTERP_CAP, 8, e["prior"] if e["prior"] is not None else 30))
    for mo in examples_models():
        if only_ex is not None and mo["model"] not in only_ex:
            continue
        tla = f"{EX}/{mo['module']}"
        d = os.path.dirname(tla)
        j = dict(mo, kind="examples", version="0.2.0", cap=EX_CAP, threads=8, est=prior_ex.get(mo["model"], 30),
                 cwd=d, env=dict(LEVEL, TLAMC_LIB=LIB),
                 cmd=[BIN["0.2.0"], "-workers", "8", "-metadir", "@META", "-config",
                      os.path.relpath(f"{EX}/{mo['model']}", d), os.path.basename(tla)])
        add(j)
    say(f"phase 1: {len(pending)} jobs")
    drain()
    say("phase 1 done")

# ---------------------------------------------------------------- phase 2
if "2" in PHASES:
    big = [e for e in ENTRIES if (e["dir"], e["cfg"]) in BIG]
    big.sort(key=lambda e: (e["dir"], e["cfg"]))
    say(f"phase 2: {len(big)} big entries, compiled, {CORES} workers, alone")
    for i, e in enumerate(big):
        order = ["0.1.2", "0.2.0"] if i % 2 == 0 else ["0.2.0", "0.1.2"]
        for v in order:
            cap = UNDECIDED_CAP if e["cfg"] in UNDECIDED else BIG_CAP
            add(gate_job(e, "compiled", v, cap, CORES, 0, {"TLAMC_CACHE": f"{WORK}/cache-{v}"}))
            j = pending.pop()
            running[j["id"]] = j
            used += j["threads"]
            run_job(j)
    say("phase 2 done")
say("DONE")
open(f"{OUT}/DONE", "w").write("DONE\n")
