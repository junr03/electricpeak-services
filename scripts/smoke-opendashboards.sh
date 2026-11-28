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
  -v "$fixture/state/database-password:/run/secrets/database-password:ro" \
  --entrypoint sh "$postgres_image" -c 'sleep 3; exec docker-entrypoint.sh postgres' >/dev/null
# Start immediately: application startup owns database readiness.
docker run -d --init --user "$smoke_uid:$smoke_gid" --name "$app" --network "$network" \
  --read-only --cap-drop ALL --security-opt no-new-privileges:true \
  --tmpfs /tmp --tmpfs "/app/.next/cache:uid=$smoke_uid,gid=$smoke_gid" \
  -p 127.0.0.1::3000 -e APP_ORIGIN=https://dashboards.example.invalid \
  -e LLM_BASE_URL=http://unused-inference:11434/v1 -e LLM_MODEL=fixture-model \
  -v "$fixture/state:/data:ro" "$image" >/dev/null
port=$(docker port "$app" 3000/tcp | head -1 | cut -d: -f2)
cat > "$fixture/probe.py" <<'PY'
import pathlib
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        return None

opener = urllib.request.build_opener(NoRedirect)
base = f"http://127.0.0.1:{sys.argv[1]}"
password = pathlib.Path(sys.argv[2]).read_text().strip()
headers = {"Host": "dashboards.example.invalid", "Origin": "https://dashboards.example.invalid"}
def request(path, data=None, extra=None):
    try:
        return opener.open(urllib.request.Request(base + path, data=data, headers={**headers, **(extra or {})}), timeout=10)
    except urllib.error.HTTPError as error:
        return error

for attempt in range(60):
    try:
        if request("/login").status == 200:
            break
    except OSError:
        pass
    time.sleep(1)
else:
    raise SystemExit("OpenDashboards did not become ready")

for cookie in ("", "opendashboards_session=forged"):
    assert request("/api/artifacts/missing", extra={"Cookie": cookie}).status == 401
assert request("/auth/login", b"password=incorrect").status == 401
login = request("/auth/login", urllib.parse.urlencode({"password": password}).encode(),
                {"Content-Type": "application/x-www-form-urlencoded"})
assert login.status == 303, login.read()
set_cookie = login.headers["Set-Cookie"]
assert "HttpOnly" in set_cookie and "Secure" in set_cookie and "SameSite=strict" in set_cookie
cookie = set_cookie.split(";", 1)[0]
for path in ("/apps", "/connections"):
    response = request(path, extra={"Cookie": cookie})
    assert response.status == 200, (path, response.status, response.read())
    assert b"<html" in response.read().lower()
print("OpenDashboards: password login, signed session, forgery rejection and app pages passed")

PY
python3 "$fixture/probe.py" "$port" "$fixture/state/admin-password"
[[ $(docker exec "$database" psql -U opendashboards -d opendashboards -Atc \
  'SELECT count(*) FROM drizzle.__drizzle_migrations') == 6 ]]
docker exec "$database" psql -U opendashboards -d opendashboards -c \
  "CREATE TABLE smoke_retention (value text); INSERT INTO smoke_retention VALUES ('preserved');" >/dev/null
# Restart exercises idempotent migrations with the same database and keys.
docker restart "$app" >/dev/null
port=$(docker port "$app" 3000/tcp | head -1 | cut -d: -f2)
python3 "$fixture/probe.py" "$port" "$fixture/state/admin-password"
[[ $(docker exec "$database" psql -U opendashboards -d opendashboards -Atc \
  'SELECT value FROM smoke_retention') == preserved ]]
[[ "$before" == "$(sha256sum "$fixture/state/"*)" ]]
printf '[{"host":"*.invalid","port":5432}]\n' > "$fixture/invalid-destinations.json"
if timeout 45 docker run --rm --user "$smoke_uid:$smoke_gid" --network "$network" --read-only --cap-drop ALL \
  -e APP_ORIGIN=https://dashboards.example.invalid \
  -e LLM_BASE_URL=http://unused-inference:11434/v1 -e LLM_MODEL=fixture-model \
  -e OPENDASHBOARDS_PRIVATE_DESTINATIONS_FILE=/run/config/private-destinations.json \
  -v "$fixture/state:/data:ro" \
  -v "$fixture/invalid-destinations.json:/run/config/private-destinations.json:ro" \
  "$image" > "$fixture/invalid-destinations.log" 2>&1; then
  echo 'Container unexpectedly started with an invalid private destination rule' >&2
  exit 1
fi
grep -Fq 'Invalid operator private destinations' "$fixture/invalid-destinations.log"
if docker run --rm --user "$smoke_uid:$smoke_gid" --network none --read-only --cap-drop ALL \
  -e APP_ORIGIN=https://dashboards.example.invalid -v "$fixture/state:/data:ro" \
  "$image" > "$fixture/missing-inference.log" 2>&1; then
  echo "Container unexpectedly started without an inference endpoint" >&2
  exit 1
fi
grep -Fq 'LLM_BASE_URL and LLM_MODEL' "$fixture/missing-inference.log"
echo 'OpenDashboards: persistent credentials, migrations and missing-inference rejection passed'
