#!/usr/bin/env bash
set -euo pipefail

image=${1:?usage: smoke-transaction-import.sh IMAGE}
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
fixture="$repo_root/services/transaction-import/examples/transactions.csv"
prefix="transaction-import-smoke-$$"
network="$prefix-network"
database="$prefix-db"
work=$(mktemp -d)

cleanup() {
  docker rm -fv "$database" >/dev/null 2>&1 || true
  docker network rm "$network" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

docker network create --internal "$network" >/dev/null
docker run -d --name "$database" --network "$network" --network-alias db \
  --tmpfs /var/lib/postgresql/data \
  -e POSTGRES_DB=transactions -e POSTGRES_USER=transactions \
  -e POSTGRES_PASSWORD=fixture-password \
  postgres:17-alpine@sha256:f9cf3e1f0066f0eb0c48707d864f0ac5941e9ff454a072077d97a4d278d07d73 >/dev/null
ready=false
for ((attempt = 0; attempt < 60; attempt++)); do
  if docker exec "$database" pg_isready -h 127.0.0.1 -U transactions -d transactions >/dev/null 2>&1; then
    ready=true
    break
  fi
  sleep 1
done
if [[ $ready != true ]]; then
  docker logs "$database"
  echo 'Test database did not become ready' >&2
  exit 1
fi

# Exercise the secret-file path with the same permissions as a runtime secret.
printf '%s\n' fixture-password > "$work/password"
chmod 600 "$work/password"
run_import() {
  docker run --rm -i --network "$network" --read-only \
    --user "$(id -u):$(id -g)" --cap-drop ALL --security-opt no-new-privileges:true \
    --mount "type=bind,source=$work/password,target=/run/secrets/database-password,readonly" \
    -e PGHOST=db -e PGUSER=transactions -e PGDATABASE=transactions \
    -e PGPASSWORD_FILE=/run/secrets/database-password "$image" -
}
query() {
  docker exec "$database" psql -X -U transactions -d transactions -Atc "$1"
}

[[ $(run_import < "$fixture") == 'Imported 1 transactions.' ]]
[[ $(run_import < "$fixture") == 'Already imported this exact file; no changes.' ]]
[[ $(query 'SELECT date, name, amount, status, category, excluded, transaction_type, account, account_mask FROM transaction_import.transactions') == '2026-01-15|Example Store|12.34|pending|Returns|t|regular|Example Card|0007' ]]
[[ $(query 'SELECT parent_category IS NULL AND tags IS NULL AND note IS NULL AND recurring IS NULL FROM transaction_import.transactions') == t ]]

# A database error on the second row must roll back the first row and audit row.
cat "$fixture" > "$work/db-error.csv"
printf '"2026-01-15","bad\0name",1.00,"pending",,,true,,"regular","Example Card","0019",,""\n' >> "$work/db-error.csv"
if run_import < "$work/db-error.csv"; then
  echo 'Expected PostgreSQL to reject a NUL in a text field' >&2
  exit 1
fi
[[ $(query 'SELECT count(*) FROM transaction_import.transactions') == 1 ]]
[[ $(query 'SELECT count(*) FROM transaction_import.imports') == 1 ]]

cat "$fixture" > "$work/invalid.csv"
printf 'invalid,row\n' >> "$work/invalid.csv"
if run_import < "$work/invalid.csv"; then
  echo 'Expected invalid CSV to fail' >&2
  exit 1
fi
[[ $(query 'SELECT count(*) FROM transaction_import.transactions') == 1 ]]

# Distinct identical rows must remain distinct within an export.
cat "$fixture" > "$work/two-rows.csv"
tail -n 1 "$fixture" >> "$work/two-rows.csv"
[[ $(run_import < "$work/two-rows.csv") == 'Imported 2 transactions.' ]]
[[ $(query 'SELECT count(*) FROM transaction_import.transactions') == 3 ]]
echo 'Transaction Import container smoke test passed.'
