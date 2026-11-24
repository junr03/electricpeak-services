# Transaction Import

A small Rust CLI for importing the 13-column financial transaction CSV export
shown in [examples/transactions.csv](examples/transactions.csv) into PostgreSQL.
The CSV parser and PostgreSQL driver are existing Rust libraries; the custom code
maps this export's fields and makes each file import atomic and repeatable.

```sh
PGHOST=localhost PGDATABASE=transactions PGUSER=transactions \
  PGPASSWORD_FILE=/path/to/password cargo run --locked -- export.csv
```

`transaction-import <file.csv>` also accepts `-` for standard input,
`--database-url`/`DATABASE_URL` for a PostgreSQL URL or connection string, and
`--password-file`/`PGPASSWORD_FILE` for a secret file. Without a URL, it uses
`PGHOST`, `PGPORT`, `PGUSER`, `PGDATABASE`, and `PGPASSWORD`. Defaults are
`localhost:5432` and user/database `transactions`. A password file overrides
other password settings; a trailing newline is removed. Connections have a
10-second timeout. This CLI uses plaintext PostgreSQL connections intended for
the isolated local container network (or an SSH tunnel); TLS is not implemented.

## Container use

From this directory, create a local password file, build, and start PostgreSQL:

```sh
install -d -m 700 secrets
(umask 077; openssl rand -hex 32 > secrets/database-password)
export IMPORT_UID="$(id -u)" IMPORT_GID="$(id -g)"
docker compose build importer
docker compose up -d --wait db
docker compose run --rm -T importer - < export.csv
```

Set `POSTGRES_PASSWORD_FILE` to use an existing host secret file instead.
PostgreSQL has no published ports and persists data in the `postgres-data`
volume. The importer exits after each file. `docker compose down` retains the
database; `docker compose down -v` deletes it. Password-file settings initialize
a new database only; changing the file does not rotate an existing role's
password. Coordinate rotation with `ALTER ROLE` in PostgreSQL.

Query imported rows or take a backup:

```sh
docker compose exec db psql -U transactions -d transactions \
  -c 'SELECT date, name, amount, excluded FROM transaction_import.transactions ORDER BY id;'
docker compose exec -T db pg_dump -U transactions -d transactions > transactions.sql
```

Protect backup files like the original financial CSV. Restore with `psql` into
an empty database before depending on a backup.

## Import behavior

- All 13 column names are required, in any order; extra or duplicate headers
  and rows with incorrect field counts fail the import.
- Dates become `DATE`, amounts become exact `NUMERIC` values, and `excluded`
  becomes `BOOLEAN`. Amounts must fit Rust Decimal's 96-bit coefficient and
  at most 28 fractional digits; values requiring rounding are rejected.
- Empty optional cells become SQL `NULL`. Account masks stay text, preserving
  leading zeroes. `tags` and `recurring` remain text because this export does
  not define their internal structure. Excluded and pending rows are retained.
- Schema initialization and all inserts commit together. Any error rolls back
  the file. SQL uses bound parameters. The database role needs permission to
  create the `transaction_import` schema and its tables on first use.
- SHA-256 of the original file bytes prevents importing the exact same file
  twice, even after renaming it. Repeated rows within a file are preserved.
  **Overlapping or modified exports can insert the same transaction again.**
  There is no stable transaction ID, so this CLI does not guess which purchases
  match or reconcile pending transactions with posted transactions. Use
  non-overlapping exports. Byte changes (including line endings) count as a new
  file.
- `transaction_import.imports` records the source basename (`-` for stdin),
  row count, hash, and import time. Rows reference that import and their
  one-based data-record number. Files are read into memory before insertion;
  this tool targets small personal exports.

## Development and releases

```sh
cargo fmt --all -- --check
cargo test --locked
cargo clippy --all-targets --locked -- --deny warnings
# Point this at a disposable test database:
TEST_DATABASE_URL='host=localhost user=transactions dbname=transactions' \
  cargo test --locked --test postgres -- --ignored
```

The PostgreSQL integration test checks exact values, duplicate rows/files,
invalid CSV, and rollback after an insert fails. It writes fixture rows into
the test database. The repository's `scripts/smoke-transaction-import.sh` tests
the actual container against an isolated PostgreSQL container without exposing
ports. CI builds and tests on PRs, then publishes an image on trusted pushes
and verifies an anonymous pull by digest.

Electricpeak deployment configuration lives in `electricpeak-sensitive`.
After publishing, promote the image's immutable digest there and enable the
optional deployment module with its 1Password password reference. No financial
exports or credentials belong in this application repository.
