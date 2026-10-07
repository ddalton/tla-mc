#!/bin/bash
# 2026-10-07 tla-mc 0.1.0 validation (user-approved): the crates.io build against flint's gate (04765c80) and
# tlaplus/Examples. A: gate sweep (20 shards x 8 workers, 2400 s/entry) + Examples sweep (4 shards x 8, 600 s);
# B: TLC 1.7.4 (CI's) on every decided entry (24 x 8 workers, 600 s); C: gate_par interp + generated (24 jobs).
export HOME=/root AWS_DEFAULT_REGION=us-west-1; source /root/.cargo/env; ulimit -n 1048576
BK=flint-tlamc-val-20261007; O=/data/out; mkdir -p $O; F=/opt/flint; G=$F/formal/tla-mc-gate
log() { echo "$(date -u +%FT%TZ) $*" | tee -a $O/RESULTS.txt; }
( while true; do aws s3 cp $O/ s3://$BK/out/ --recursive --quiet; sleep 60; done ) &
log "start; nproc $(nproc); mem $(free -g | awk '/Mem/{print $2}') GB"
cargo install tla-mc --version 0.1.0 --locked > $O/install.log 2>&1 || { log "cargo install FAILED"; }
export TLAMC=/root/.cargo/bin/tla-mc FLINT_ROOT=$F GATE_WORK=/data/gate-work
log "tla-mc: $(ls -la $TLAMC | awk '{print $5}') bytes, $(cargo install --list | grep '^tla-mc')"
curl -sSfL -o /data/tla2tools-1.7.4.jar https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar
cd /data && git clone -q --depth 1 https://github.com/tlaplus/Examples.git && git clone -q --depth 1 https://github.com/tlaplus/CommunityModules.git
mkdir -p /data/tlaps && curl -sSfL -o /data/tlaps/TLAPS.tla https://raw.githubusercontent.com/tlaplus/tlapm/main/library/TLAPS.tla
log "Examples $(git -C /data/Examples log -1 --format=%h), CommunityModules $(git -C /data/CommunityModules log -1 --format=%h)"
# warm cargo's registry for the generated crates (one codegen + fetch)
(cd $F/formal && $TLAMC -codegen /data/warm -config FlintReplication.cfg FlintReplication.tla >/dev/null 2>&1 && cd /data/warm && cargo fetch >/dev/null 2>&1); rm -rf /data/warm

# A. gate sweep, sharded (sweep.py SHARD=k/n, each shard its own TMPDIR for metadirs); Examples alongside
mkdir -p /data/tmp-{0..19} /data/exout /data/tlcout
log "phase A: gate sweep (20 shards) + Examples sweep (4 shards)"
for k in $(seq 0 19); do (cd /data && SHARD=$k/20 TMPDIR=/data/tmp-$k python3 /opt/sweep.py $TLAMC 2400 $O/sweep-$k.jsonl > $O/sweep-$k.log 2>&1) & done
for k in 0 1 2 3; do (cd /data && SHARD=$k/4 WORKERS=8 TIMEOUT=600 TLAMC_LIB=/data/CommunityModules/modules:/data/tlaps python3 /opt/examples_run.py /data/Examples $TLAMC /data/exout/examples-$k.jsonl > $O/examples-$k.log 2>&1) & done
wait
cat $O/sweep-*.jsonl > $O/gate-sweep.jsonl; cat /data/exout/examples-*.jsonl > $O/examples.jsonl
log "phase A done: gate $(python3 -c "import json,collections;print(dict(collections.Counter(json.loads(l)['status'] for l in open('$O/gate-sweep.jsonl'))))") | examples $(python3 -c "import json,collections;print(dict(collections.Counter(json.loads(l)['got'] for l in open('$O/examples.jsonl'))))")"

# B. TLC 1.7.4 on the decided entries
python3 - <<PY
import json
rows=[l for l in open("$O/gate-sweep.jsonl")]
for k in range(24): open(f"/data/tlc-in-{k}.jsonl","w").writelines(rows[k::24])
PY
log "phase B: TLC 1.7.4 (24 shards x 8 workers, 600 s)"
for k in $(seq 0 23); do (cd /data && TLC_JAR=/data/tla2tools-1.7.4.jar TLC_WORKERS=8 python3 /opt/tlc_side.py /data/tlc-in-$k.jsonl /data/tlcout/tlc-$k.jsonl 600 > $O/tlc-$k.log 2>&1) & done
wait
cat /data/tlcout/tlc-*.jsonl > $O/tlc.jsonl; log "phase B done: $(wc -l < $O/tlc.jsonl) TLC runs, $(python3 -c "import json;print(sum(1 for l in open('$O/tlc.jsonl') if json.loads(l).get('tlc_distinct')))") with a count"

# C. interpreter + generated checker on every agreeing entry
log "phase C: gate_par (24 jobs)"
python3 $G/gate_par.py $F $O/gate-sweep.jsonl $O/tlc.jsonl,$G/liveness-vs-tlc-54-2026-09-26.jsonl,$G/liveness-vs-tlc-18-2026-09-26.jsonl $O/gate-par.jsonl 24 > $O/gate-par.log 2>&1
python3 $G/gate_eval.py $O/gate-par.jsonl > $O/gate-eval.txt 2>&1
log "phase C done: $(head -1 $O/gate-eval.txt)"
log DONE; echo DONE > $O/DONE; aws s3 cp $O/ s3://$BK/out/ --recursive --quiet
shutdown -h +5
