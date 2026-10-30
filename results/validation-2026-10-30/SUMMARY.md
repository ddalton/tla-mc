# tla-mc 0.2.0: the full validation (2026-10-30)

The crates as published (`cargo install tla-mc --version 0.2.0 --locked`, and 0.1.2
beside it), on a spot AWS c8g.48xlarge (192 Graviton4 cores, 371 GB), against flint's
TLA+ gate at `037ef46e` and tlaplus/Examples at `bce9f21` (CommunityModules
`1805e9d`; the commits the pull-request gate pins, flint's two commits later with no
model moved). No gate entry's cfg or modules changed since the 2026-10-07 validation
(flint `04765c80`; `scripts/spec_changed.py`), so TLC's counts from that day still
apply. TLC is v1.7.4. 50 minutes on the machine.

| leg | what ran |
|---|---|
| interpreter, 8 workers, up to 24 runs at a time, the two versions of an entry side by side | the 412 gate entries under 60 s on 10-07, through 0.2.0 and 0.1.2 |
| compiled checker (`-compile`), all 192 workers, one run at a time on an otherwise idle machine | the other 34 through both versions; the three no checker has decided last, 0.2.0 first, 180 s each |
| TLC 1.7.4, 8 workers | the 394 entries 0.2.0 decided in under 20 s (TLC was not run on the slow ones) |
| tlaplus/Examples, 0.2.0 interpreter, 8 workers, 600 s cap | all 191 exhaustive-search models |

## flint's gate: 446 entries

| | |
|---|---|
| verdict as the gate expects | **443 of 446** (412 interpreted, 31 compiled), **0 disagree**; LeanImmutableRenameHolds, LeanImmutableRepairOverridesUI and LeanImmutableAnsweredRecordSkipped still undecided, as since 2026-09-26 |
| verdict as 0.1.2's | 446 of 446 |
| exact distinct counts (must-hold and liveness runs) | as 0.1.2: 148 of 149; as TLC today: 101 of 102; as TLC on 10-07: 126 of 128 |
| TLC's verdict as the gate expects | 390 of 394; the other 4 TLC did not parse, run beside other TLC processes in one directory (a NullPointerException in its module table); on 10-07 TLC found each one's planted bug, as 0.2.0 did today |

The differing counts are the two `SYMMETRY` + `VIEW` worlds 10-07 already noted, where
with several workers the state kept for a symmetric group depends on timing, for every
checker: LeanBarrierLeaseSyncOverlayHolds (TLC 455,930 on 10-07 and 455,427 today;
tla-mc 0.1.0 456,165, 0.1.2 456,506, 0.2.0 456,475) and LeanScopedSyncHolds (0.1.2 and
0.2.0 246,151, TLC's exact 1-worker count; 246,199 from TLC at 8 workers on 10-07).

## Speed, 0.1.2 → 0.2.0

Entries that took at least 1 s on 0.1.2 and agree on both:

| leg | entries | geometric-mean speedup | range | total | peak RSS |
|---|---:|---:|---|---|---|
| interpreter, flint (`formal/`) | 47 | **2.02×** | 1.03–4.32× | 821 → 427 s | ×1.48, 1.5 GB at most |
| interpreter, lean (`lean/formal/`) | 63 | **1.21×** | 0.71–1.50× | 834 → 690 s | ×1.27, 3.1 GB at most |
| compiled at 192 workers, flint | 11 | **1.59×** | 1.20–3.88× | 111 → 69 s | ×1.24, 9.7 GB at most |
| compiled at 192 workers, lean | 20 | **1.42×** | 1.01–1.90× | 198 → 148 s | ×1.28, 77.7 GB at most |

The two below 1× are mutation runs, which stop at their first violation: 0.2.0's
workers happened to explore more before reaching it (687,194 states against 550,904;
180,277 against 108,874).

The largest models, compiled, 192 workers:

| model | distinct | 0.1.2 | 0.2.0 | |
|---|---:|---:|---:|---:|
| ForgeSync | 89,146,934 | 49.3 s | 34.9 s | 1.4× |
| LeanImmutablePendingAdoptionDropped (its bug found) | 71,214,513 | 33.8 s | 29.1 s | 1.2× |
| LeanCoreHoldsSmall | 49,042,820 | 18.8 s | 11.2 s | 1.7× |
| ForgeSyncCode | 30,277,694 | 17.6 s | 11.7 s | 1.5× |
| FlintExtentsCommit | 25,819,382 | 8.0 s | 4.4 s | 1.8× |
| LeanRemovalCrashHolds | 20,384,119 | 14.0 s | 7.4 s | 1.9× |
| LeanImmutableHolds | 17,580,288 | 12.5 s | 7.5 s | 1.7× |
| LeanBarrierLeaseOrphanConverges | 10,398,809 | 22.0 s | 21.8 s | 1.0× |
| LeanRefineProbe | 1,965,383 | 23.8 s | 20.1 s | 1.2× |
| FlintReplicationBounce | 1,822,000 | 16.1 s | 4.1 s | 3.9× |

On 10-07 the interpreter took 37 min on ForgeSync at 8 workers, and TLC ~76 min.

Where the gain is small, the cost is in evaluating the spec, not in storing states:
workers were 91–98% busy on all three of these, but 0.2.0 checked 4.46M distinct
states/s on LeanCoreHoldsSmall against 480k on LeanBarrierLeaseOrphanConverges and
98k on LeanRefineProbe (`logs/`). The series made storing, hashing and decoding a state
cheaper; most of lean's models spend their time in their own actions and invariants.

The three undecided, 180 s each: 0.2.0 reached depth 19 (152–159M distinct) in
57–67 s, 0.1.2 in 77–87 s (1.2–1.4×). Depth 20 (221M) took both about 170 s: by then
40M queued states were on disk, and the disk set the pace. Peak RSS 109 GB (0.2.0),
114 GB (0.1.2).

## tlaplus/Examples: 191 exhaustive-search models

| | models |
|---|---:|
| accepted: verdict and distinct states as the manifest records | **124** |
| a result other than the manifest's | **0** |
| refused with a message | 60 |
| not finished in 600 s | 5: MultiPaxos_MC (as on 10-07) and four added since, TLC's recorded runtimes 50 min to 7 h or unknown (MCKVSSafetyLarge, SlushLarge, c1cs, EnvironmentController) |
| stopped at 120 GB | 1: CarTalkPuzzle Model_3, added since, an `ASSUME` whose manifest result is "unknown" |
| crashed | 1: DiningPhilosophers.cfg, below |

On 10-07, 115 of 165 were accepted. Of those 165, 0.2.0 accepts the same models except
DiningPhilosophers.cfg, and 10 of the 26 added since. YoYoAllGraphs and MCEWD687a,
errors on 10-07, are now refused with a message (enumerating `Seq(S)`).

**DiningPhilosophers.cfg** was replaced upstream since 10-07 (the PlusCal spec it was
is now DiningPhilosophersPCal.cfg, accepted). The new one has `INVARIANT NoDeadlock`
with `NoDeadlock == ENABLED Next`, and tla-mc panics loading it, before any state is
checked:

```
thread 'main' panicked at src/eval.rs:378:35:
index out of bounds: the len is 0 but the index is 1
  <tla_mc::eval::Program>::eval_ref ... <tla_mc::compile::Compiler>::finish ... tla_mc::cli::load_spec
```

0.1.2 panics the same way (`eval.rs:364`): not a 0.2.0 regression. Without that
invariant both versions check the model, `NoLivelock` and `NoStarvation` under strong
fairness included, to the manifest's 58 distinct states.

## The files

`results.jsonl` has one record per run (`kind`: interp, compiled, build, tlc,
examples); `progress.log` the run as it went; `logs/` the runs quoted here;
`scripts/` the runners. `runner.py` ran the first phase; the compiled checkers it
built one at a time per version (cargo locks a target directory), so `parbuild.py`
built the remaining 39 in parallel, each in its own cache, and the waiting builds
were stopped (the 33 `build-failed` records); `handover.sh` then stopped
`runner.py` at the end of its first phase and started `runner2.py`, which ran the
compiled phase with the undecided last. `analyze.py results.jsonl` with the 10-07
files reproduces the numbers above.
