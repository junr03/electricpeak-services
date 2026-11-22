# OpenDashboards container

Packages MIT-licensed [thesysdev/opendashboards](https://github.com/thesysdev/opendashboards)
at `4c49e8d207fbb7407e0a0d1897dc101f09add3f0`, with its frozen pnpm lockfile.
The Dockerfile verifies the upstream archive checksum. Node and PostgreSQL are
digest-pinned. Upstream has no application container and its production providers
expect Supabase gateway tokens and Vercel OIDC/AWS KMS, so this package adds two
small adapters: a single-user HTTP Basic login and Node's AES-256-GCM for
connection credentials, bound to the user, connection and purpose. Development
auth and plaintext storage remain forbidden in production. Optional upstream
demo datasets are disabled.

OpenUI Cloud remains hosted: dashboard generation, dashboard/chat storage and
model calls use Thesys. The API key stays server-side. This is a single-user
installation; do not share the admin account between people needing isolated data.

## Runtime contract

- Port: 3000, HTTP behind an HTTPS reverse proxy. No host publication is needed.
- `APP_ORIGIN`: exact external HTTPS origin, without a trailing slash or path.
- `/run/secrets/thesys-api-key`: read-only file containing the Thesys API key;
  override its path with `THESYS_API_KEY_FILE`.
- `/data`: persistent credential state, read-only for the application. Run the
  same image with argument `init` and a writable mount before starting Postgres.
  Initialization creates three random 256-bit values with mode 0600 and never
  overwrites them: `database-password`, `admin-password`, `encryption-key`.
  Run init and app as UID 1000, GID 100; the directory must be writable by that UID.
- Postgres 17 at `opendashboards-db:5432` (override `DATABASE_HOST`), database and
  username `opendashboards`. Set `POSTGRES_PASSWORD_FILE` to a mount of the generated
  `database-password`. Keep database storage persistent and its port unpublished.
- Init needs no network. The app needs network access to Postgres and Thesys,
  and to any data sources the user configures.
- Login username: `admin`. Retrieve its password locally from
  `/var/lib/opendashboards/state/admin-password`; it is never printed by the image.

The entrypoint applies additive versioned migrations before `next start`.
Use Docker's init process for signal forwarding. The root filesystem can be
read-only with tmpfs mounts at `/tmp` and `/app/.next/cache` (UID 1000/GID 100).

Back up Postgres together with `/data`, especially the encryption key. Losing the
key makes saved connection credentials unreadable. Changing the database password
file does not change an already initialized Postgres role. To rotate the API key,
update its mounted file and restart the app. To rotate the login password, replace
`admin-password` with at least 32 random characters and restart. Do not replace the
encryption key without decrypting/re-encrypting existing connection credentials.
Rolling back the image does not roll back database migrations or hosted artifacts.

## Validation

```sh
docker build -t opendashboards:test services/opendashboards
bash scripts/smoke-opendashboards.sh opendashboards:test
```

The build runs adapter tests and Next.js's production build/type check. The smoke
test uses an isolated real Postgres container and a synthetic Thesys key; it
checks login, app rendering, encryption mode, migrations and credential retention.
It never calls Thesys with a real key or creates a billable dashboard.
