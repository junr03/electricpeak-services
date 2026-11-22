#!/usr/bin/env bash
set -euo pipefail
image=${1:?usage: smoke-opendashboards.sh IMAGE}
postgres_image=postgres:17-alpine@sha256:f9cf3e1f0066f0eb0c48707d864f0ac5941e9ff454a072077d97a4d278d07d73
fixture=$(mktemp -d)
smoke_uid=$(id -u)
smoke_gid=$(id -g)
network="opendashboards-smoke-$$"
database="opendashboards-db-$$"
app="opendashboards-app-$$"
cleanup() {
  docker rm -f "$app" "$database" >/dev/null 2>&1 || true
  docker network rm "$network" >/dev/null 2>&1 || true
  rm -rf "$fixture"
}
trap cleanup EXIT
chmod 755 "$fixture"
mkdir "$fixture/state"
chmod 700 "$fixture/state"
printf '%s\n' smoke-only-not-a-real-api-key > "$fixture/api-key"
chmod 644 "$fixture/api-key"
docker run --rm --user "$smoke_uid:$smoke_gid" --network none --read-only --cap-drop ALL --security-opt no-new-privileges:true \
  -v "$fixture/state:/data" "$image" init
before=$(sha256sum "$fixture/state/"*)
docker run --rm --user "$smoke_uid:$smoke_gid" --network none --read-only --cap-drop ALL --security-opt no-new-privileges:true \
  -v "$fixture/state:/data" "$image" init
[[ "$before" == "$(sha256sum "$fixture/state/"*)" ]]
[[ $(stat -c %a "$fixture/state/encryption-key") == 600 ]]
docker network create "$network" >/dev/null
docker run -d --name "$database" --network "$network" --network-alias opendashboards-db \
  -e POSTGRES_USER=opendashboards -e POSTGRES_DB=opendashboards \
  -e POSTGRES_PASSWORD_FILE=/run/secrets/database-password \
  -v "$fixture/state/database-password:/run/secrets/database-password:ro" "$postgres_image" >/dev/null
for _ in $(seq 1 40); do
  if docker exec "$database" pg_isready -U opendashboards >/dev/null 2>&1; then break; fi
  sleep 1
done
docker run -d --init --user "$smoke_uid:$smoke_gid" --name "$app" --network "$network" \
  --read-only --cap-drop ALL --security-opt no-new-privileges:true \
  --tmpfs /tmp --tmpfs "/app/.next/cache:uid=$smoke_uid,gid=$smoke_gid" \
  -p 127.0.0.1::3000 -e APP_ORIGIN=https://dashboards.example.invalid \
  -v "$fixture/state:/data:ro" -v "$fixture/api-key:/run/secrets/thesys-api-key:ro" "$image" >/dev/null
port=$(docker port "$app" 3000/tcp | head -1 | cut -d: -f2)
cat > "$fixture/probe.py" <<'PY'
import base64
import pathlib
import sys
import time
import urllib.error
import urllib.request

base = f"http://127.0.0.1:{sys.argv[1]}"
password = pathlib.Path(sys.argv[2]).read_text().strip()
authorization = "Basic " + base64.b64encode(f"admin:{password}".encode()).decode()
for attempt in range(60):
    try:
        with urllib.request.urlopen(urllib.request.Request(base + "/apps", headers={"Authorization": authorization}), timeout=5) as response:
            assert response.status == 200
            assert b"<html" in response.read().lower()
        break
    except (OSError, urllib.error.URLError):
        time.sleep(1)
else:
    raise SystemExit("OpenDashboards did not become ready")
for path in ("/apps", "/connections", "/api/frontend-token"):
    for headers in ({}, {"Authorization": "Basic YWRtaW46d3Jvbmc="}):
        try:
            urllib.request.urlopen(urllib.request.Request(base + path, headers=headers), timeout=5)
        except urllib.error.HTTPError as error:
            assert error.code == 401, (path, error.code)
            assert error.headers["WWW-Authenticate"].startswith("Basic ")
        else:
            raise AssertionError(f"unauthenticated request accepted: {path}")
with urllib.request.urlopen(urllib.request.Request(base + "/connections", headers={"Authorization": authorization}), timeout=10) as response:
    assert b"encrypted on this server" in response.read()
print("OpenDashboards: login, app, connections and encrypted storage passed")
PY
python3 "$fixture/probe.py" "$port" "$fixture/state/admin-password"
[[ $(docker exec "$database" psql -U opendashboards -d opendashboards -Atc \
  'SELECT count(*) FROM drizzle.__drizzle_migrations') == 5 ]]
docker exec "$database" psql -U opendashboards -d opendashboards -c \
  "CREATE TABLE smoke_retention (value text); INSERT INTO smoke_retention VALUES ('preserved');" >/dev/null
# Restart exercises idempotent migrations with the same database and keys.
docker restart "$app" >/dev/null
port=$(docker port "$app" 3000/tcp | head -1 | cut -d: -f2)
python3 "$fixture/probe.py" "$port" "$fixture/state/admin-password"
[[ $(docker exec "$database" psql -U opendashboards -d opendashboards -Atc \
  'SELECT value FROM smoke_retention') == preserved ]]
[[ "$before" == "$(sha256sum "$fixture/state/"*)" ]]
if docker run --rm --user "$smoke_uid:$smoke_gid" --network none --read-only --cap-drop ALL \
  -e APP_ORIGIN=https://dashboards.example.invalid -v "$fixture/state:/data:ro" \
  "$image" > "$fixture/missing-key.log" 2>&1; then
  echo "Container unexpectedly started without its API key" >&2
  exit 1
fi
echo 'OpenDashboards: persistent credentials, migrations and missing-key rejection passed'
