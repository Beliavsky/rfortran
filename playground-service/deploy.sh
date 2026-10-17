#!/bin/sh
# Executed by the dedicated SSH deployment account on the Linux execution host.
set -eu
cd /opt/rfort-playground
image=${1:?Supply an immutable compiler image tag}
case "$image" in ghcr.io/*-playground:*) ;; *) echo 'Unexpected image name' >&2; exit 1;; esac
docker pull "$image"
python3 smoke.py "$image"
cp image.env image.env.previous
printf 'RFORT_PLAYGROUND_IMAGE=%s\n' "$image" > image.env.new
mv image.env.new image.env
sudo -n systemctl restart rfort-playground.service
ready=false
for attempt in 1 2 3 4 5; do
    if python3 - "$image" <<'PY'
import json, sys, urllib.request
with urllib.request.urlopen('http://127.0.0.1:8080/api/health', timeout=3) as response:
    health = json.load(response)
    assert health['compiler'] == 'rfort' and health['image'] == sys.argv[1]
PY
    then ready=true; break; fi
    sleep 2
done
if [ "$ready" != true ]; then
    cp image.env.previous image.env
    sudo -n systemctl restart rfort-playground.service
    echo 'Health check failed; previous image restored.' >&2
    exit 1
fi
