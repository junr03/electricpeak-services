# Electricpeak services

Application code and release artifacts used by the declarative
[junr03/electricpeak](https://github.com/junr03/electricpeak) NixOS configuration.
Applications own their implementation, contracts, tests, documentation, and
packaging here. Electricpeak owns deployment configuration, secret references,
mounts, device rules, systemd units, schedules, and network policy.

## Raw Backup

The Raw Backup photo workflow contains:

- `services/rawbackup/web`: existing Python HTTP server and HTML dashboard.
- `services/rawbackup/photo-workflow`: import, sync, reconciliation, progress,
  file classification, Internxt configuration, tests, and Nix packaging.
- `services/rawbackup/rust`: status collector, Cargo lock, tests, and Nix package.
- `services/rawbackup/contracts`: draft v1 OpenAPI contract.
- `services/rawbackup/docs`: API design and Rust development guidance.

The web image is `ghcr.io/junr03/electricpeak-services/rawbackup`. Host tools
remain Nix packages; extracting them does not change their permissions or runtime
supervisor. The draft `/api/v1` contract remains a future implementation target;
the image preserves the existing unversioned endpoints.

## Development

```sh
nix develop
cargo fmt --manifest-path services/rawbackup/rust/Cargo.toml --all -- --check
cargo clippy --manifest-path services/rawbackup/rust/Cargo.toml --workspace --all-targets --locked -- --deny warnings
nix flake check --print-build-logs
docker build -t rawbackup:test services/rawbackup
scripts/smoke-rawbackup.sh rawbackup:test
```

`lib.mkRawbackupPackages { pkgs = ...; }` builds the collector with the consumer's
pinned nixpkgs. `lib.mkPhotoWorkflowPackages` accepts `pkgs`, `renamePicture`,
`localRoot`, `internxtRemotePath`, `internxtConfig`, `internxtEmailSecret`, and
`internxtPasswordSecret`. Secret arguments are file paths, never resolved values.
The consumer supplies its host configuration while this repository owns program
construction and runtime dependencies.

## Releases and deployment

Each application gets its own workflow and artifacts. Raw Backup PRs build and
smoke-test the web image and validate host packages, Python tests, Rust, and the
API contract. Pushes to main and trusted `jnr/**` branches publish candidate
images tagged with the full commit SHA. Fork PRs never publish. The publication
job records the digest; the final gate pulls it anonymously on a fresh runner and
repeats the smoke test after package tests pass. Only candidates with all checks
passing should be promoted in Electricpeak.

On first publication, GitHub defaults a new GHCR package to private. Its owner
must change the Raw Backup package visibility to public in package settings;
otherwise the anonymous pull gate intentionally fails. Public repository
visibility alone does not make the image public.

To deploy, update Electricpeak's locked services input and Compose image digest
in one PR. Its CI checks the pulled image's source revision matches the locked
input, starts the image with isolated fixtures, and builds the NixOS integration.
Merge this repository's initial PR first, retain its pinned source commit and
published image, then merge the consuming PR. Publishing a candidate does not
deploy anything. Retain deployed digests for rollback; data rollback is separate.

The current file-based host/web interface is documented in
[the runtime contract](services/rawbackup/docs/runtime.md). Changes to shared
files or API behavior require compatible producer and consumer updates.

## Substack Digest

`services/substack-digest` owns the Rust service and locked Cargo workspace,
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
See [Substack Digest](services/substack-digest/README.md) for application behavior.

## Origin

Extracted from the public `junr03/electricpeak` main at
`08230934ca4a112711d32cad9cec1e3f36f23dee`. Host configuration helpers such as
1Password setup, Home Manager mirroring, and third-party service initialization
remain in Electricpeak. Trusted branch pushes publish each application's image
so consumers can promote a coherent source revision with separate image digests.
