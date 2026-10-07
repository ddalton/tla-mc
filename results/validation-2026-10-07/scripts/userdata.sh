#!/bin/bash
shutdown -h +300
sysctl -w vm.max_map_count=16777216
exec > /var/log/flint-userdata.log 2>&1
export HOME=/root AWS_DEFAULT_REGION=us-west-1
dnf -y install gcc tar gzip git python3 java-21-amazon-corretto-headless
curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
mkdir -p /data /opt/flint && aws s3 cp s3://flint-tlamc-val-20261007/payload/ /opt/ --recursive && tar -xzf /opt/flint.tgz -C /opt/flint
systemd-run --unit=tlamc-val --property=KillMode=process bash /opt/runner.sh
