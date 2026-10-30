#!/bin/bash
# tla-mc 0.2.0 validation, 2026-10-30: setup only; the runner is started over ssh
shutdown -h +300
sysctl -w vm.max_map_count=16777216
exec > /var/log/tlamc-setup.log 2>&1
set -x
dnf -y install gcc git python3 java-21-amazon-corretto-headless tar gzip
mkdir -p /data && chown ec2-user:ec2-user /data
cat > /data/setup.sh <<'EOS'
set -x
curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
source ~/.cargo/env
cd /data
cargo install tla-mc --version 0.2.0 --locked --root /data/v020 > /data/install-020.log 2>&1 &
cargo install tla-mc --version 0.1.2 --locked --root /data/v012 > /data/install-012.log 2>&1 &
fetch() { mkdir -p $1 && git -C $1 init -q && git -C $1 remote add origin $2 && git -C $1 fetch -q --depth 1 origin $3 && git -C $1 checkout -q FETCH_HEAD; }
fetch /data/flint https://github.com/ddalton/flint 037ef46e1b27fadcccc137347f6ca89a64e8bf9d
fetch /data/examples https://github.com/tlaplus/Examples bce9f215e212691b22d4396f5481f6fac873d004
fetch /data/community https://github.com/tlaplus/CommunityModules 1805e9d5e1cef05d74806309fb87788e69b48f26
mkdir -p /data/tlaps && curl -sSfL -o /data/tlaps/TLAPS.tla https://raw.githubusercontent.com/tlaplus/tlapm/main/library/TLAPS.tla
curl -sSfL -o /data/tla2tools-1.7.4.jar https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar
wait
/data/v020/bin/tla-mc -version; /data/v012/bin/tla-mc -version
touch /data/SETUP-DONE
EOS
chown ec2-user:ec2-user /data/setup.sh
sudo -u ec2-user -i bash /data/setup.sh > /data/setup.log 2>&1
