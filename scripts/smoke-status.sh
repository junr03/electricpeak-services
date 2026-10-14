#!/usr/bin/env bash
set -euo pipefail
image=${1:?usage: smoke-status.sh IMAGE}
fixtures=$(mktemp -d)
container=""
cleanup() {
  if [[ -n "$container" ]]; then docker rm -f "$container" >/dev/null; fi
  rm -rf "$fixtures"
}
trap cleanup EXIT
now=$(date -u +%Y-%m-%dT%H:%M:%SZ)
printf '{"fixture":"status-smoke","generated_at":"%s"}\n' "$now" > "$fixtures/status.json"
chmod 755 "$fixtures"
chmod 644 "$fixtures/status.json"
container=$(docker run -d --read-only --cap-drop ALL --security-opt no-new-privileges:true \
  --network bridge -p 127.0.0.1::8080 --tmpfs /tmp --tmpfs /var/cache/nginx \
  -v "$fixtures:/srv/status-data:ro" "$image")
port=$(docker port "$container" 8080/tcp | head -1 | cut -d: -f2)
python3 - "$port" <<'PY'
import json
import sys
import time
import urllib.error
import urllib.request

base = f"http://127.0.0.1:{sys.argv[1]}"
for attempt in range(30):
    try:
        with urllib.request.urlopen(base + "/data/status.json", timeout=2) as response:
            assert response.headers["Cache-Control"] == "no-store"
            assert json.load(response)["fixture"] == "status-smoke"
        break
    except (OSError, urllib.error.URLError):
        time.sleep(1)
else:
    raise SystemExit("Status page did not become ready")
with urllib.request.urlopen(base + "/", timeout=5) as response:
    assert b"<html" in response.read().lower()
    assert "default-src 'none'" in response.headers["Content-Security-Policy"]
for asset in ("app.js", "style.css"):
    with urllib.request.urlopen(f"{base}/{asset}", timeout=5) as response:
        assert response.status == 200
try:
    urllib.request.urlopen(base + "/nginx.conf", timeout=5)
except urllib.error.HTTPError as error:
    assert error.code == 404
else:
    raise AssertionError("unexpected file served")
print("Status page image: page, assets, and status data passed")
PY
