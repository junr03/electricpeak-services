#!/usr/bin/env bash
set -euo pipefail
image=${1:?usage: smoke-rawbackup.sh IMAGE}
fixtures=$(mktemp -d)
container=""
cleanup() {
  if [[ -n "$container" ]]; then docker rm -f "$container" >/dev/null; fi
  rm -rf "$fixtures"
}
trap cleanup EXIT
mkdir -p "$fixtures/data/jobs" "$fixtures/reconcile/requests"
printf '%s\n' '{"fixture":"rawbackup-smoke"}' > "$fixtures/data/status.json"
container=$(docker run -d --read-only --network bridge -p 127.0.0.1::8765 \
  -v "$fixtures/data:/data:ro" -v "$fixtures/reconcile:/reconcile" "$image")
port=$(docker port "$container" 8765/tcp | head -1 | cut -d: -f2)
python3 - "$port" "$fixtures" <<'PY'
import json
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

base = f"http://127.0.0.1:{sys.argv[1]}"
for attempt in range(30):
    try:
        with urllib.request.urlopen(base + "/api/status", timeout=2) as response:
            assert json.load(response) == {"fixture": "rawbackup-smoke"}
        break
    except (OSError, urllib.error.URLError):
        time.sleep(1)
else:
    raise SystemExit("Raw Backup did not become ready")
with urllib.request.urlopen(base + "/", timeout=5) as response:
    assert b"<html" in response.read().lower()
with urllib.request.urlopen(base + "/api/jobs", timeout=5) as response:
    assert json.load(response) == {"jobs": []}
request = urllib.request.Request(base + "/api/reconcile", method="POST")
with urllib.request.urlopen(request, timeout=5) as response:
    assert response.status == 202
    assert json.load(response)["state"] == "queued"
assert len(list((Path(sys.argv[2]) / "reconcile/requests").glob("*.json"))) == 1
try:
    urllib.request.urlopen(request, timeout=5)
except urllib.error.HTTPError as error:
    assert error.code == 409
else:
    raise AssertionError("duplicate reconciliation was accepted")
print("Raw Backup image: dashboard, status, jobs, and reconciliation passed")
PY
