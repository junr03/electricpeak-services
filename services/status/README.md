# Status page

A small public page showing whether the server is reachable and healthy:

- **ZeroTier**: the daemon is online and joined to at least one network that
  reports `OK`.
- **Tailscale**: the backend is `Running` and this node is online in the tailnet.
- **SSH (port 22)**: a connection to sshd is accepted and it sends an `SSH-`
  banner. The probe runs on the host against `127.0.0.1:22`, so it proves sshd
  is serving, not that the firewall exposes it on a given interface.
- **Containers**: every Docker container. Ones that are down, restarting,
  unhealthy, paused, or still starting get full-size cards; healthy ones are
  compact chips. A container that exited cleanly and is not meant to run
  continuously (restart policy `no`, for example the Substack job) shows as
  idle, not as a failure.

The page shows state only. It never publishes addresses, network IDs, versions,
or SSH banners.

## Layout

- `rust/`: the `electricpeak-status-collector` Nix package. A std-only crate that
  wraps the `tailscale`, `zerotier-cli`, and `docker` CLIs, with parser tests.
  `lib.mkStatusPackages { pkgs = ...; }` builds it with the consumer's nixpkgs.
- `web/` and `Dockerfile`: the static page on stock `nginx-unprivileged`, published
  as its own digest-addressed image. It serves the page and one JSON file under
  `/srv/status-data/status.json`, with a strict CSP.

## Runtime contract

The consumer owns scheduling and privileges: it runs the collector on the host
(root is needed for the ZeroTier auth token and the Docker socket) every 30
seconds with `ELECTRICPEAK_STATUS_OUTPUT` set, and mounts the output directory
read-only at `/srv/status-data`. The collector adds only the tools its PATH
provides; a missing tool is reported as "not installed". The container needs no
Docker socket, no host network, and no capabilities.

The page polls `data/status.json` every 15 seconds. If `generated_at` is older
than 150 seconds it reports that the data is stale, so a dead collector or timer
is visible instead of showing an old "all green". If the whole server is down the
page is unreachable, which is itself the signal. It is not an external monitor.

## Why custom

No maintained project covers this. Uptime Kuma and Gatus probe from outside or
need per-monitor setup, and cannot ask the local `tailscaled`, `zerotier-one`,
and Docker daemons whether the node is connected. The web side reuses nginx.

## Development

```sh
cargo run --manifest-path services/status/rust/Cargo.toml -p electricpeak-status -- --stdout
docker build -t status:test services/status
scripts/smoke-status.sh status:test
```

`--stdout` prints the snapshot instead of writing it. Set
`ELECTRICPEAK_STATUS_OUTPUT` and `ELECTRICPEAK_STATUS_SSH_ADDRESS` to override
the output path and the SSH probe address.
