# OpenDashboards container

Packages the independent MIT-licensed [junr03/opendashboards fork](https://github.com/junr03/opendashboards)
at `9c9279cdbe4e1da9138632571919b4a9bdf586b1`, pinned by archive checksum and frozen
pnpm lockfile. Node and PostgreSQL images are digest-pinned. Inference orchestration,
dashboard generation/repair, QuickJS execution, dashboard versions, chat history,
password login and signed sessions are supplied by the fork. No Thesys key or
OpenUI Cloud/Gateway service is required.

This package retains the existing operator-mounted private destination rules,
state-file initialization and legacy encrypted credential reader. It uses the
existing `self-hosted-admin` identity and encryption key, preserving ownership and
saved connection credentials. New credentials use the fork's native AES-GCM format;
old records are read without rewriting them. Reverting to the older image cannot
read newly written credentials or locally generated dashboards/history.

## Runtime contract

- Port: 3000, HTTP behind an HTTPS reverse proxy; no host publication needed.
- `APP_ORIGIN`: exact external HTTPS origin, without a trailing slash or path.
- `LLM_BASE_URL` and `LLM_MODEL`: required, operator-owned OpenAI-compatible endpoint
  and actual model ID. Optional `LLM_DASHBOARD_MODEL` selects another model on that
  endpoint. Optional `LLM_API_KEY_FILE` loads a token from a read-only secret mount.
  The image does not download weights or provision an inference server.
- `/data`: persistent credential state, read-only for the application. Run the
  same image with `init` and a writable mount before starting PostgreSQL.
  Init creates random 256-bit `database-password`, `admin-password`, `encryption-key`
  and `session-secret` files with mode 0600. Existing files are never overwritten;
  an upgrade adds only the missing session signing key. Run as UID 1000, GID 100.
- PostgreSQL 17 at `opendashboards-db:5432` (override `DATABASE_HOST`), database and
  user `opendashboards`. Mount `database-password` as `POSTGRES_PASSWORD_FILE`.
  This database stores application state, separately from analytics/transactions.
- `OPENDASHBOARDS_PRIVATE_DESTINATIONS_FILE`: optional operator JSON rules file.
  It is validated once at startup using the fork's destination policy. Missing or
  invalid configured files fail closed. No file means the typed settings apply.
- Sign in at `/login` using the existing `admin-password` (no username field).
  Passwords are never printed by the image. Login issues a 12-hour HttpOnly,
  SameSite=Strict, Secure session cookie. This remains a single-operator service.

The entrypoint applies additive migrations before starting. Use Docker's init
process. A read-only filesystem needs tmpfs at `/tmp` and `/app/.next/cache`.
Init needs no network. The app connects only to its PostgreSQL, configured model
endpoint and selected data sources; product telemetry is off by default.

Back up PostgreSQL and `/data`, keeping an additional separate backup of the
credential encryption key. Changing the database password file alone does not
update an initialized database role. Changing `session-secret` signs out sessions.
Old Cloud dashboards and messages require explicit archive/import as described in
the fork's `docs/self-hosting.md`; no automatic Cloud fallback remains.

## Validation

```sh
docker build -t opendashboards:test services/opendashboards
bash scripts/smoke-opendashboards.sh opendashboards:test
docker build --target build -t opendashboards:source-tests services/opendashboards
bash scripts/smoke-opendashboards-private-source.sh opendashboards:source-tests
```

The build tests native/legacy encrypted credentials and operator policy loading,
then performs a production Next.js build. The smoke test uses disposable PostgreSQL,
checks init retention, native login/session rejection, app pages, six additive
migrations, restart retention, invalid policy and absent inference configuration.
The TLS fixture exercises private source queries without production data.
The fork's own browser suite covers generation, edits, local history and filters
with a model stand-in. Actual model quality/capacity needs separate validation.
