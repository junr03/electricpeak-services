#!/usr/bin/env bash
set -euo pipefail
builder=${1:?usage: smoke-opendashboards-private-source.sh BUILDER_IMAGE}
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
fixture=$(mktemp -d)
network="dashboards-source-smoke-$$"
database="dashboards-source-db-$$"
postgres=postgres:17-alpine@sha256:f9cf3e1f0066f0eb0c48707d864f0ac5941e9ff454a072077d97a4d278d07d73
cleanup() {
  docker rm -fv "$database" >/dev/null 2>&1 || true
  docker network rm "$network" >/dev/null 2>&1 || true
  rm -rf "$fixture"
}
trap cleanup EXIT
chmod 755 "$fixture"
openssl req -new -x509 -newkey rsa:2048 -nodes -days 1 -subj /CN=Fixture-CA \
  -addext basicConstraints=critical,CA:TRUE -keyout "$fixture/ca.key" -out "$fixture/ca.crt" 2>/dev/null
openssl req -new -newkey rsa:2048 -nodes -subj /CN=analytics-db \
  -keyout "$fixture/server.key" -out "$fixture/server.csr" 2>/dev/null
printf '%s\n' 'subjectAltName=DNS:analytics-db' 'extendedKeyUsage=serverAuth' > "$fixture/server.ext"
openssl x509 -req -in "$fixture/server.csr" -CA "$fixture/ca.crt" -CAkey "$fixture/ca.key" \
  -CAcreateserial -days 1 -extfile "$fixture/server.ext" -out "$fixture/server.crt" 2>/dev/null
docker network create --internal "$network" >/dev/null
docker run -d --name "$database" --network "$network" --network-alias analytics-db --network-alias metadata-db \
  --tmpfs /var/lib/postgresql/data -v "$fixture:/fixture:ro" \
  -e POSTGRES_DB=transactions -e POSTGRES_USER=transactions -e POSTGRES_PASSWORD=fixture-admin \
  --entrypoint sh "$postgres" -c \
  'cp /fixture/server.key /tmp/server.key; chown postgres:postgres /tmp/server.key; chmod 600 /tmp/server.key; exec docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/fixture/server.crt -c ssl_key_file=/tmp/server.key' >/dev/null
ready=false
for ((attempt=0; attempt<60; attempt++)); do
  if docker exec "$database" pg_isready -h 127.0.0.1 -U transactions >/dev/null 2>&1; then ready=true; break; fi
  sleep 1
done
[[ $ready == true ]] || { docker logs "$database"; exit 1; }
sql() { docker exec -i "$database" psql -X -v ON_ERROR_STOP=1 -U transactions -d transactions "$@"; }
sql >/dev/null <<'SQL'
CREATE SCHEMA transaction_import;
CREATE TABLE transaction_import.transactions (amount numeric NOT NULL);
INSERT INTO transaction_import.transactions VALUES (12.34);
SQL
if [[ -n ${READER_ROLE_SQL:-} ]]; then
  sql < "$READER_ROLE_SQL" >/dev/null
else
  sql >/dev/null <<'SQL'
CREATE ROLE opendashboards_reader LOGIN;
GRANT USAGE ON SCHEMA transaction_import TO opendashboards_reader;
GRANT SELECT ON ALL TABLES IN SCHEMA transaction_import TO opendashboards_reader;
ALTER DEFAULT PRIVILEGES FOR ROLE transactions IN SCHEMA transaction_import GRANT SELECT ON TABLES TO opendashboards_reader;
SQL
fi
reader_password=$(openssl rand -hex 32)
printf '%s\n%s\n' "$reader_password" "$reader_password" | sql --command '\password opendashboards_reader' >/dev/null
sql --command 'CREATE TABLE transaction_import.future_import (amount numeric);' >/dev/null
address=$(docker inspect --format "{{(index .NetworkSettings.Networks \"$network\").IPAddress}}" "$database")
printf '[{"host":"analytics-db","port":5432,"addresses":["%s/32"]},{"host":"metadata-db","port":5432,"addresses":["%s/32"]}]\n' "$address" "$address" > "$fixture/allowed.json"
printf '[]\n' > "$fixture/denied.json"
probe() {
  docker run --rm --network "$network" --cap-drop ALL --security-opt no-new-privileges:true \
    -v "$fixture/ca.crt:/fixture/ca.crt:ro" -v "$fixture/$1:/fixture/destinations.json:ro" \
    -e NODE_EXTRA_CA_CERTS=/fixture/ca.crt -e NODE_ENV=production \
    -e OPENDASHBOARDS_SELF_HOSTED=true -e OPENDASHBOARDS_PRIVATE_DESTINATIONS_FILE=/fixture/destinations.json \
    -e DATABASE_URL=postgres://metadata:fixture@metadata-db/metadata \
    -e SOURCE_TEST_HOST=analytics-db -e "SOURCE_TEST_ADDRESS=$address" \
    -e "SOURCE_TEST_PASSWORD=$reader_password" -e "SOURCE_TEST_EXPECT_BLOCKED=$2" \
    "$builder" pnpm exec vitest run --config self-hosted/private-source.vitest.config.mts
}
probe denied.json true
probe allowed.json false
if [[ -n ${IMPORTER_COMPATIBILITY_IMAGE:-} ]]; then
  sql --command 'CREATE DATABASE compatibility;' >/dev/null
  docker run --rm -i --network "$network" --read-only --cap-drop ALL \
    -e PGHOST=analytics-db -e PGUSER=transactions -e PGDATABASE=compatibility -e PGPASSWORD=fixture-admin \
    "$IMPORTER_COMPATIBILITY_IMAGE" - < "$repo_root/services/transaction-import/examples/transactions.csv"
fi
echo 'OpenDashboards: private-source refusal, TLS discovery, queries, and read-only permissions passed.'
