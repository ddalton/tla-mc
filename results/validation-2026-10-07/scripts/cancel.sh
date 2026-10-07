#!/bin/bash
# User ~21:10Z: cancel both LeanImmutable reruns (their gate expectations are unverified since the 2026-09-26 protocol
# change, so neither result could be judged). Waits for runner-bc.sh to log "phase C done", stops it before its
# AnsweredRecordSkipped rerun, then finishes: DONE, upload, shutdown.
export HOME=/root AWS_DEFAULT_REGION=us-west-1
BK=flint-tlamc-val-20261007; O=/data/out
log() { echo "$(date -u +%FT%TZ) $*" | tee -a $O/RESULTS.txt; }
until grep -q "phase C done" $O/RESULTS.txt; do sleep 1; done
systemctl stop tlamc-bc; pkill -f "AnsweredRecordSkipped"; sleep 2; pkill -9 -f "AnsweredRecordSkipped"; rm -rf /data/ars-meta
log "reruns cancelled (user): LeanImmutableAnsweredRecordSkipped and LeanImmutableRepairOverridesUI — expectations unverified since the 2026-09-26 protocol change (f6f6a892); recorded as undecided"
log DONE; echo DONE > $O/DONE; aws s3 cp $O/ s3://$BK/out/ --recursive --quiet
shutdown -h +5
