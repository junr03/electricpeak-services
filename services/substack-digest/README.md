# Substack morning digest

At **04:00 America/Los_Angeles**, Electricpeak collects articles from your
**subscribed publications**, including paid articles your session can read,
and uploads one PDF to **Substack Daily** in reMarkable Cloud. Device delivery
requires the Move to connect to Wi-Fi and sync with that same cloud account.
No Drive import or developer mode is needed.

The timer is **disabled until credentials are provisioned and
`services.substackDigest.enable = true` is set** in the private configuration.
Merging this PR alone does not authenticate either account or enable delivery.
The new secrets are conditional, so an unconfigured service cannot break OpNix
or other services during deployment.

## Implementation and scope

- The custom service is Rust, in `services/substack-digest/rust/crates/substack-digest`, with dependencies
  pinned in the workspace Cargo lockfile. It uses `reqwest` for authenticated
  collection and the OSS `headless_chrome` Rust library to drive Chromium.
- The container builds the Rust binary and reuses the official Playwright
  browser image for Chromium. The pinned [rmapi-js](https://github.com/jwoglom/rmapi-js)
  v12 CLI handles reMarkable Cloud. Node runs that existing OSS client; there is
  no custom Python service or Python runtime dependency. Small embedded JavaScript
  expressions extract the browser DOM; orchestration and state live in Rust.
- Fetches authenticated `/api/v1/subscriptions?tvOnly=false`, intersects actual
  subscription IDs with publication metadata, then paginates each archive.
  Never reads the reader/recommendation feed. No fixed article-count limit.
  Only newsletter/article posts are included; Notes, podcasts and video posts
  are excluded. Following a person alone does not include their publication.
- The first run covers 24 hours. Thereafter the cutoff is the start of the last
  successful collection. A 48-hour overlap plus delivered IDs catches delayed
  visibility without duplicating articles. Edits to old articles and posts
  backdated by more than this overlap are not republished.
- On a known free subscription, a locked paid-only article becomes an explicit
  linked notice. A paywall for a paid/unknown subscription fails the whole run;
  it never silently publishes a paid preview as the full article.
- A failed collection/render/upload preserves the checkpoint. The first-run
  boundary is persisted before network access. Upload failures retain the PDF
  and its outbox manifest; a retry uses exactly that file/name. A cloud listing
  detects a previously successful upload after an interrupted run. Existing
  documents are never replaced or deleted, preserving handwritten annotations.
  Do not rename/move the pending digest until a failed run has recovered.
- An empty interval produces no PDF. Persistent systemd timers catch a missed
  morning after downtime; failures retry every 15 minutes. Each attempt has a
  two-hour runtime limit. A successful retry of an old outbox advances only to
  that outbox's cutoff; later articles arrive in the next scheduled digest.

The page is **91.8 × 163.2 mm**, matching the Move's 954:1696 portrait ratio,
with 12 pt serif text, 1.5 line spacing, a 13 mm right annotation margin,
article page breaks, and a linked contents page. These dimensions derive from
[reMarkable's 264 PPI display specifications](https://remarkable.com/products/remarkable-paper/pro-move/details/features).
PDF text remains selectable; the tablet can annotate it normally. The CI
`move-sample-pdf` artifact lets you inspect the layout on your own device.

## Public and private configuration

The Rust service, container, reusable Nix module, PDF layout, example defaults,
and offline test fixtures belong in this services repository; host configuration belongs in Electricpeak. The `op://`
references name secrets but do not contain their values.

Keep deployment activation (`services.substackDigest.enable = true`) and
personal configuration overrides in `electricpeak-sensitive`, assembled as
`private-config/` during deployment. The public defaults disclose the 04:00
America/Los_Angeles schedule and `/Substack Daily` folder; these are non-secret
preferences, not account identifiers. The private module can override the
calendar and the Home Manager config source if different or private values
are needed. Do not add a private Git submodule to this repository.

Neither Git repository should contain browser cookies, reMarkable tokens,
subscription exports, downloaded articles, or generated digests. Credentials
belong in 1Password; delivery history, pending uploads, PDFs, and uploader
cache remain in `/var/lib/substack-digest` and any protected server backups.
Public CI uses only fictional article fixtures and never receives account
credentials. See [Secrets and private configuration](https://github.com/junr03/electricpeak/blob/main/docs/secrets.md).

## One-time setup

Do this on a trusted workstation. Do not paste credentials into chat or git.

1. Capture an authenticated Substack browser session with Playwright's local
   browser UI (`npx --package=playwright@1.55.0 playwright codegen
   --save-storage=/secure/path/substack.json https://substack.com`). Install the Playwright Chromium browser on that workstation
   if needed. Log in normally, then visit and open a paid article at **each
   custom-domain publication** while that browser is open; their login cookies
   may have a separate domain. Close the browser to write the storage file.
   The worker accepts this storage-state JSON or a Playwright cookie array.
   Protect it with mode 0600. The worker never logs or commits cookies.
2. Build the container from the repository:
   `docker build -t electricpeak-substack-digest:local services/substack-digest`.
   Register the OSS uploader once with an eight-letter code from
   [reMarkable's connection page](https://my.remarkable.com/device/browser/connect).
   Use a private directory owned by UID 1000 for the mounted config:

   ```sh
   umask 077
   mkdir -p /secure/path/rmapi
   read -r -s -p 'reMarkable one-time code: ' remarkable_code
   docker run --rm -v /secure/path/rmapi:/data/rmapi \
     --entrypoint rmapi-js electricpeak-substack-digest:local \
     auth register "$remarkable_code"
   unset remarkable_code
   docker run --rm -v /secure/path/rmapi:/data/rmapi \
     --entrypoint rmapi-js electricpeak-substack-digest:local \
     auth token --print-token > /secure/path/remarkable-token
   ```

3. Store the storage-state JSON as file attachment
   `op://electricpeak/nixos/substack-digest-cookies.json` and the long-lived
   device token as concealed field
   `op://electricpeak/nixos/substack-digest-remarkable-token`.
   The existing OpNix module materializes these as UID 1000-readable mode 0400
   files under `/run/onepassword-secrets`. Never resolve them into Nix/store.
4. Enable `services.substackDigest.enable = true;` in the private NixOS module,
   then use the usual reviewed deployment workflow. Ensure the existing
   `/var/lib/electricpeak/appdata` mapping reaches Home Manager's
   `~/containers_storage/appdata`, as it does for other utility containers.
5. Trigger a first run with `sudo systemctl start docker-substack-digest`.
   Check `journalctl -u docker-substack-digest` and
   `systemctl list-timers substack-digest.timer`, then confirm the PDF appears
   on the Move. This real-account acceptance check cannot be run without your
   Substack session and reMarkable device token.

The services workflow builds with digest-pinned base images, `cargo build
--locked`, and `npm ci`, then publishes a digest-addressed image. Electricpeak
pulls that image; it no longer builds application code on the production host.
Runtime has no Docker socket, runs as UID 1000, drops capabilities and uses a
read-only root. The service remains disabled until credentials are provisioned.

## Configuration and operation

| Surface | Settings |
| --- | --- |
| `services.substackDigest` | `enable`, `calendar` (default `*-*-* 04:00:00 America/Los_Angeles`) |
| `containers/config/substack-digest/config.json` | Cloud folder, display timezone, first-run and overlap hours, page dimensions, font/line spacing, margins, local PDF retention |
| `/var/lib/substack-digest` | `state.json` checkpoint/delivered IDs, `pending.json` outbox, PDFs, writable rmapi token/cache directory |
| 1Password | Browser session and reMarkable token; no secrets in the JSON config |

Home Manager mirrors the config using the existing `onChange` pattern. Each
run reads it again. Changing the calendar also requires changing `timezone`
in JSON if you want the filenames to use a different local date.
The Rust service reads the original checkpoint/outbox JSON format, so upgrading
preserves delivery history and resumes any pending upload. Back up the state directory: losing it loses delivery history. Local PDFs are
removed after 30 days on a successful run; cloud PDFs and annotations are
retained indefinitely. Do not edit/delete state while a job is running.

Both services use unofficial APIs. Renew the cookie attachment when sessions
expire, especially on custom domains. A visible paywall, missing article body,
failed image load, HTTP error or changed API schema fails the run. Chromium
renders only the article body, not comments/site navigation. The clean render
context has no cookies or JavaScript and permits images only from Substack's
CDN and post-media S3 host; an external image requires extending the reviewed
allowlist rather than silently omitting it. Embedded videos/audio are omitted.

To pause: stop `substack-digest.timer` and `docker-substack-digest.service`;
set `enable = false` to persist that across deployments. No article deletion
or "mark read" operation is sent to Substack or reMarkable.

## Validation

```sh
nix develop --command cargo test --manifest-path services/substack-digest/rust/Cargo.toml -p substack-digest --locked
docker build -t substack-digest:test services/substack-digest
nix flake check --no-build
```

The separate Substack workflow only validates the service; daily delivery runs
on Electricpeak through systemd. It runs Rust unit tests and renders an offline
fixture in the production
container restrictions, verifies PDF dimensions/text and paywall handling,
and attaches the sample PDF. The normal Compose CI generates the Nix container
module; do not hand-edit or commit the generated file for this change.

## Packaging and CI

This project owns the Rust service and locked Cargo workspace,
Chromium/Node container packaging, npm lock, offline PDF fixtures and checks,
and application documentation. Its image is
`ghcr.io/junr03/electricpeak-services/substack-digest`. The independent workflow
builds and unit-tests the Rust binary, renders an offline sample under production
container restrictions, checks PDF dimensions and text, publishes the image,
and pulls it anonymously on a fresh runner. No account access is used in CI.

```sh
docker build -t substack-digest:test services/substack-digest
scripts/smoke-substack-digest.sh substack-digest:test
```

The smoke test needs `pdfinfo` and `pdftotext` from Poppler. Host activation,
scheduling, configuration mirroring and secret references stay in Electricpeak.
