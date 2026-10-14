# Electricpeak services

Application code and release artifacts used by the declarative
[junr03/electricpeak](https://github.com/junr03/electricpeak) NixOS configuration.
Applications own their implementation, contracts, tests, documentation, and
packaging here. Electricpeak owns deployment configuration, secret references,
mounts, device rules, systemd units, schedules, and network policy.

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
