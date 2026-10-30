#!/bin/bash
# wait for the first runner's phase 1 to end, stop it before its phase 2 runs anything,
# and start the reordered phase 2 (runner2.py) on the same results
OLD=$(pgrep -f "^python3 runner.py$")
until grep -q "phase 1 done" /data/out/progress.log; do sleep 0.2; done
pkill -STOP -P $OLD; kill -STOP $OLD
for c in $(pgrep -P $OLD); do kill -9 $c; done
kill -9 $OLD
pkill -9 -f "/data/work/cache-" 2>/dev/null
sleep 2
echo "$(date +%T) handover: first runner stopped after phase 1 ($(pgrep -fc 'tla-mc' || true) tla-mc processes left)" >> /data/out/progress.log
source /data/env.sh; cd /data
OUT=/data/out WORK=/data/work PHASES=2 SEQ_START=3000 exec python3 runner2.py
