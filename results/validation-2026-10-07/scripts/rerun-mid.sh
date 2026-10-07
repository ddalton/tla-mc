#!/bin/bash
# User ~21:00Z: the 180 GB run outgrew memory (RSS 320 GB at depth 23, 42 GB left); restarted at 90 GB.
# disk-bound at ~450 MB/s), 1800 s cap; the checker is already built (same -compile cache, so no build).
export HOME=/root AWS_DEFAULT_REGION=us-west-1; source /root/.cargo/env; ulimit -n 1048576
O=/data/out; F=/opt/flint; T=/root/.cargo/bin/tla-mc; C=LeanImmutableRepairOverridesUI.cfg
log() { echo "$(date -u +%FT%TZ) $*" | tee -a $O/RESULTS.txt; }
log "rerun-mid: $C, compiled, 184 workers, -queue-mem 90000, 1800 s"
rm -rf /data/rn3-meta; t0=$(date +%s)
(cd $F/lean/formal && TLAMC_CACHE=/data/tlamc-cache-$C timeout 1800 $T -compile -workers 184 -checkpoint 0 -metadir /data/rn3-meta -fpmem 32000 -queue-mem 90000 -config $C LeanSubtree.tla > $O/rerun-mid-$C.out 2>&1); rc=$?
rm -rf /data/rn3-meta
# the gate's expectation for this mutation_run is the whole line "Invariant Inv_HITLDurable is violated"
if grep -q "Invariant Inv_HITLDurable is violated" $O/rerun-mid-$C.out; then j="AGREE (Inv_HITLDurable violated, as the gate expects)"
elif [ $rc -eq 124 ]; then j="TIMEOUT (1800 s, 184 workers)"
else j="DISAGREE (rc $rc: $(grep -oE '(Invariant|Action property|Temporal property) [A-Za-z_0-9]+ is violated|No error has been found' $O/rerun-mid-$C.out | head -1))"; fi
log "rerun-mid lean/formal/$C: $j | $(grep -oE 'progress: depth [0-9]+, [0-9]+ generated, [0-9]+ distinct|[0-9]+ distinct states found' $O/rerun-mid-$C.out | tail -1) | $(( $(date +%s) - t0 ))s"
