# Raw Backup

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
