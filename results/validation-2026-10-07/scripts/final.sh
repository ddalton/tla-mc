#!/bin/bash
# 21:03Z: RepairOverridesUI alone, after runner-bc.sh (phases B, C, the AnsweredRecordSkipped rerun) has finished —
# side by side with phase B's 24 TLC JVMs it would run the box out of memory. Waits for tlamc-bc to exit (it calls
# shutdown -h +5 on its way out), replaces that shutdown, runs, then shuts down. Logs DONE2.
export HOME=/root AWS_DEFAULT_REGION=us-west-1; source /root/.cargo/env; ulimit -n 1048576
BK=flint-tlamc-val-20261007; O=/data/out; F=/opt/flint; T=/root/.cargo/bin/tla-mc; C=LeanImmutableRepairOverridesUI.cfg
log() { echo "$(date -u +%FT%TZ) $*" | tee -a $O/RESULTS.txt; }
while systemctl is-active --quiet tlamc-bc; do sleep 5; done
sleep 2; shutdown -c; shutdown -h +45
log "final: $C alone, compiled, 184 workers, -queue-mem 90000, 1800 s (shutdown re-armed +45)"
rm -rf /data/rn4-meta; t0=$(date +%s)
(cd $F/lean/formal && TLAMC_CACHE=/data/tlamc-cache-$C timeout 1800 $T -compile -workers 184 -checkpoint 0 -metadir /data/rn4-meta -fpmem 32000 -queue-mem 90000 -config $C LeanSubtree.tla > $O/final-$C.out 2>&1); rc=$?
rm -rf /data/rn4-meta
if grep -q "Invariant Inv_HITLDurable is violated" $O/final-$C.out; then j="AGREE (Inv_HITLDurable violated, as the gate expects)"
elif [ $rc -eq 124 ]; then j="TIMEOUT (1800 s, 184 workers)"
else j="DISAGREE (rc $rc: $(grep -oE '(Invariant|Action property|Temporal property) [A-Za-z_0-9]+ is violated|No error has been found' $O/final-$C.out | head -1))"; fi
log "final lean/formal/$C: $j | $(grep -oE 'progress: depth [0-9]+, [0-9]+ generated, [0-9]+ distinct|[0-9]+ distinct states found' $O/final-$C.out | tail -1) | $(( $(date +%s) - t0 ))s"
log DONE2; aws s3 cp $O/ s3://$BK/out/ --recursive --quiet
shutdown -h +5
