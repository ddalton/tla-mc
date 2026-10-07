#!/bin/bash
# 21:05Z: runner.sh hung after phase A — its bare `wait` also waited on the endless S3 sync loop. This finishes the
# job: phase A summary, B (TLC 1.7.4 on decided entries), C (gate_par), the AnsweredRecordSkipped rerun if it timed
# out, then waits for the RepairOverridesUI rerun (tlamc-rerun-mid) before DONE and shutdown. Waits on its own PIDs only.
export HOME=/root AWS_DEFAULT_REGION=us-west-1; source /root/.cargo/env; ulimit -n 1048576
BK=flint-tlamc-val-20261007; O=/data/out; F=/opt/flint; G=$F/formal/tla-mc-gate; T=/root/.cargo/bin/tla-mc
export TLAMC=$T FLINT_ROOT=$F GATE_WORK=/data/gate-work
log() { echo "$(date -u +%FT%TZ) $*" | tee -a $O/RESULTS.txt; }
cat $O/sweep-*.jsonl > $O/gate-sweep.jsonl; cat /data/exout/examples-*.jsonl > $O/examples.jsonl
log "phase A done (runner.sh hung in wait; resumed by runner-bc.sh): gate $(python3 -c "import json,collections;print(dict(collections.Counter(json.loads(l)['status'] for l in open('$O/gate-sweep.jsonl'))))") | examples $(python3 -c "import json,collections;print(dict(collections.Counter(json.loads(l)['got'] for l in open('$O/examples.jsonl'))))")"
python3 - <<PY
import json
rows=[l for l in open("$O/gate-sweep.jsonl")]
for k in range(24): open(f"/data/tlc-in-{k}.jsonl","w").writelines(rows[k::24])
PY
log "phase B: TLC 1.7.4 (24 shards x 8 workers, 600 s)"
pids=()
for k in $(seq 0 23); do (cd /data && TLC_JAR=/data/tla2tools-1.7.4.jar TLC_WORKERS=8 python3 /opt/tlc_side.py /data/tlc-in-$k.jsonl /data/tlcout/tlc-$k.jsonl 600 > $O/tlc-$k.log 2>&1) & pids+=($!); done
wait "${pids[@]}"
cat /data/tlcout/tlc-*.jsonl > $O/tlc.jsonl; log "phase B done: $(wc -l < $O/tlc.jsonl) TLC runs, $(python3 -c "import json;print(sum(1 for l in open('$O/tlc.jsonl') if json.loads(l).get('tlc_distinct')))") with a count"
log "phase C: gate_par (24 jobs)"
python3 $G/gate_par.py $F $O/gate-sweep.jsonl $O/tlc.jsonl,$G/liveness-vs-tlc-54-2026-09-26.jsonl,$G/liveness-vs-tlc-18-2026-09-26.jsonl $O/gate-par.jsonl 24 > $O/gate-par.log 2>&1
python3 $G/gate_eval.py $O/gate-par.jsonl > $O/gate-eval.txt 2>&1
log "phase C done: $(head -1 $O/gate-eval.txt)"
if python3 -c "import json,sys; sys.exit(0 if any(json.loads(l)['cfg']=='LeanImmutableAnsweredRecordSkipped.cfg' and json.loads(l)['status']=='timeout' for l in open('$O/gate-sweep.jsonl')) else 1)"; then
  C=LeanImmutableAnsweredRecordSkipped.cfg; log "rerun: $C timed out at 8 workers; compiled, 184 workers, -queue-mem 90000, 1800 s"
  t0=$(date +%s); (cd $F/lean/formal && TLAMC_CACHE=/data/tlamc-cache-$C timeout 1800 $T -compile -workers 184 -checkpoint 0 -metadir /data/ars-meta -fpmem 32000 -queue-mem 90000 -config $C LeanSubtree.tla > $O/rerun-$C.out 2>&1); rc=$?; rm -rf /data/ars-meta
  log "rerun lean/formal/$C: rc $rc | $(grep -oE '(Invariant|Action property|Temporal property) [A-Za-z_0-9]+ is violated|No error has been found' $O/rerun-$C.out | head -1) | $(grep -oE 'progress: depth [0-9]+, [0-9]+ generated, [0-9]+ distinct|[0-9]+ distinct states found' $O/rerun-$C.out | tail -1) | $(( $(date +%s) - t0 ))s"
fi
while systemctl is-active --quiet tlamc-rerun-mid; do sleep 10; done
log DONE; echo DONE > $O/DONE; aws s3 cp $O/ s3://$BK/out/ --recursive --quiet
shutdown -h +5
