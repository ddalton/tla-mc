# tla-mc 0.2.0: a partial re-validation (2026-10-29)

What changed since 0.1.2: the 26-commit performance series (PR #2, htejun),
which rewrites evaluation and storage paths — invariant conjuncts evaluated
only when a variable they read changed, fingerprints rehashed only for the
variables a step assigned, inline scalar key lookups, per-worker decode caches,
a queue encoding that carries each compound value's length (format 2; 0.1.2
checkpoints are refused), compiled fairness actions, a parallel liveness graph
load, `-pin` (Linux) — plus tabs counted to the next multiple of 8 columns as
SANY does (PR #1) and a clearer refusal of parameterized instances (PR #4).

The check: 0.1.2 (`ed024df`) and the series (`5bc76c1`, then rebased as
`2127e4a..45ae2f8`) side by side on flint's gate, current tree, on an 8-core
M1 (8 GB), `-workers 8` (`run-val.sh`):

| leg | entries | verdict as 0.1.2 | same distinct states |
|---|---|---|---|
| interpreter, the 343 entries under 5 s on 10-07 | 343 | 343 | 87 of 87 exact-count (strict + liveness) |
| interpreter, the 8 slowest models | 8 | 8 | 7 of 7 exact-count |
| `-compile`, a fresh cache | 4 | 4 | 4 of 4 |

Mutation entries stop at their first violation, so their counts move with the
workers' interleaving (34 of 256 equal), as on 10-12.

Wall time, 0.1.2 → 0.2.0 (`slow-and-compiled.log`):

| model | distinct | interpreter | `-compile`, cached build |
|---|---|---|---|
| FlintTierSession | 10,698,103 | 40.8 → 23.7 s | 18.5 → 9.1 s |
| FlintReplicationBounceRaceFixed | 7,767,872 | 49.8 → 26.1 s | |
| FlintCompositionWitness | 6,368,894 | 30.8 → 13.5 s | |
| LeanImmutableSyncQueueShippedInvs | 2,522,269 | 48.8 → 43.0 s | 29.2 → 21.8 s |
| LeanBarrierLeaseSameBytesVerified | 2,400,030 | 48.1 → 37.9 s | |
| FlintTierSessionLive (liveness) | 2,603,207 | 30.8 → 18.7 s | 23.5 → 7.4 s |
| ForgeSyncLive (liveness) | 1,781,559 | 44.7 → 12.9 s | |
| FlintTierSessionNoFence (liveness mutation) | 140,307 | | 0.7 → 0.2 s, same property violated |

Peak RSS rose 10–40% on every model (the per-worker value caches), to 1.1 GB
at most. The series' own figures, on a 2×48-core EPYC at 48–192 workers, are
larger (117 → 32 s on a 42.4M-state model).

Not re-run: the 103 slower gate entries, the TLC side, and tlaplus/Examples.
