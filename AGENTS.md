# Electricpeak services

Organize application implementations under services/<application>. Keep application
contracts, tests, documentation, and packaging with their owning application.
Release each application independently. Raw Backup, Substack Digest, and the status page publish separate images. Photo workflows and
the status and Raw Backup collectors are Nix packages.

Keep runtime behavior unchanged during extraction. Host devices, mounts, systemd
units, scheduling, firewall rules, and secret references belong in electricpeak.
Never include credentials or deployment-specific private configuration here.
Build new custom services in Rust; retain existing Python during extraction
to preserve behavior. Reuse OSS tools. Do not implement the draft v1 API as part of this repository extraction.

Validate with `nix flake check`, Rust formatting and Clippy, and the Raw Backup
container smoke test. Keep dependencies locked; use Nix to update flake.lock.
Use the connected GitHub connector for branch and pull request operations.
