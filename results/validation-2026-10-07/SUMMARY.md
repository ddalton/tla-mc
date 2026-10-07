# Validation of tla-mc 0.1.0, 2026-10-07

The crate as published (`cargo install tla-mc --version 0.1.0 --locked`), run on a spot
AWS c8g.48xlarge (192 Graviton4 cores, 371 GB) against flint's TLA+ gate (flint `04765c80`)
and the tlaplus/Examples corpus (`6a7b26a`). TLC is v1.7.4, the version flint's CI pins.

## flint's gate: 446 entries (`gate-sweep.jsonl`, `tlc.jsonl`, `gate-par.jsonl`)

| step | result |
|---|---|
| every entry through tla-mc's interpreter (8 workers, 40 min cap) | **443 agree with the gate's expected verdict, 0 disagree**, 3 undecided at the cap |
| TLC 1.7.4 on the 443 decided entries (8 workers, 10 min cap) | 435 with a distinct-state count |
| interpreter AND compiled checker on the 443 (`gate_par.py`, 4 workers each; the last two compiled at 192 workers) | every verdict agrees with the gate and between the engines: 130 must-hold runs, 293 planted invariant bugs, 18 planted liveness bugs — every planted bug caught by both engines |
| distinct states, must-hold runs with a TLC count | **126 identical** (TLC = interpreter = compiled); 2 differ — both `SYMMETRY` + `VIEW` worlds, where with several workers the state kept for a symmetric group depends on timing (TLC's own 8-worker count differs from its 1-worker one; the compiled checker's 246,151 on LeanScopedSyncHolds is TLC's exact 1-worker count) |

ForgeSync.cfg, undecided by tla-mc before this run: 89,146,934 distinct, exactly TLC's count
(flint's 2026-09-28 gate); 37 min interpreted at 8 workers, **71 s compiled at 192** (TLC: ~76 min).

**Undecided**: LeanImmutableRenameHolds, LeanImmutableRepairOverridesUI and
LeanImmutableAnsweredRecordSkipped. All three model the same spec, rewritten on 2026-09-26
(flint `f6f6a892`), and no checker has decided them since, so their expected verdicts are
unverified. Compiled reruns reached depth 20 (221M distinct, RenameHolds) and depth 22
(422M, RepairOverridesUI) without a verdict and were stopped; a flint follow-up checks them
with TLC.

How to read the raw files: `gate_par.py` marks a caught planted bug "DIFF" when the gate's
expectation is a whole violation line and the run reports a name (`gate_eval.py` applies the
gate's rule); ForgeSyncLive's "DIFF" compares against a 2026-09-26 TLC count of an older spec
(today's TLC gives 1,781,559, as tla-mc does); `RESULTS.txt`'s "DISAGREE (rc 143)" lines are
runs stopped by hand.

## tlaplus/Examples: 165 exhaustive-search models (`examples.jsonl`)

| | models |
|---|---|
| accepted | **115** (103 hold, 12 violations) — **every one matches TLC's recorded result** (verdict and distinct states) |
| refused with a message | 47 — temporal shapes not yet supported, `ACTION_CONSTRAINT`, modules TLC implements in Java, enumerating an infinite set |
| errors | 2 — YoYoAllGraphs and MCEWD687a enumerate `Seq(S)`, which TLC tests membership in without listing |
| timeout (600 s) | 1 — MultiPaxos_MC |

Up from 112 accepted at the previous sweep (2026-09-29); CoffeeCan, rejected then, now passes.

`scripts/` has the runners as they ran, including the ones that took over when the first
runner hung (a bare `wait` also waited on its background upload loop).
