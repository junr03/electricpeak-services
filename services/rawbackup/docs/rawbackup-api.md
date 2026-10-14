# Raw Backup API contract

The Raw Backup API is the supported boundary between the Raw Backup server and
native or web dashboard clients. The server owns status collection, storage
inventory, credentials, workflow coordination, and reconciliation. Clients
only read safe status data and may enqueue the existing no-delete
reconciliation operation.

The draft v1 contract is defined by
[`contracts/rawbackup/v1/openapi.yaml`](../contracts/rawbackup/v1/openapi.yaml).
It is intentionally specified before the versioned endpoints are implemented
so the Rust model, HTTP adapter, and Swift client can converge on one boundary.

## Server boundary

The target architecture encapsulates all server-side Raw Backup logic in one
containerized service. The current Rust collector, systemd services, shell
workflows, and Python HTTP server are migration inputs behind that boundary;
they are not concepts exposed by the client contract.

The API therefore models operations and storage state without exposing process
supervisor fields, device paths, mount paths, report paths, or other deployment
details. The implementation may move into the service incrementally without
requiring client changes.

## Connectivity and trust boundary

The production server URL is deployment-specific and should be supplied to
clients outside this repository. Clients are expected to reach it through
Tailscale or ZeroTier. TLS remains mandatory and uses the deployment's
publicly trusted certificate.

Version 1 does not add application-layer authentication. Overlay membership is
the access-control boundary for reads and for the reconciliation request. The
API must not be exposed outside the firewall without first adding an explicit
authentication scheme to the contract and implementation.

## Resource boundaries

The API separates data by update cadence and ownership:

- `GET /status` returns the relatively slow storage and pipeline snapshot. It
  does not embed job or reconciliation documents.
- `GET /jobs` returns active workflow jobs and their rapidly changing progress.
- `GET /reconciliation` returns the latest reconciliation state.
- `POST /reconciliation` idempotently enqueues the no-delete reconciliation.

This removes the current duplication where a status snapshot can contain a
stale reconciliation document and operation status can contain arbitrary job
JSON. An operation references an active job by `jobId`; the full typed job is
available from `/jobs`.

## Compatibility rules

The `/api/v1` path is the major compatibility boundary.

- Existing fields keep their meaning and JSON type for the lifetime of v1.
- New optional response fields may be added without a new major version.
- Removing a field, making an optional field required, or changing its meaning
  requires a new major API version.
- Clients must ignore unknown response fields.
- Workflow kinds, phases, lifecycle states, results, and sync states are closed
  enums. Rust, generated Swift, and browser code all use the same allowed set.
- Adding, removing, or renaming an enum case is a breaking contract change and
  requires a new major API version. This deliberately favors compile-time and
  conformance-test guarantees over silently accepting an undefined state.
- Runtime-specific states and results are normalized into the smaller
  `OperationState` and `OperationResult` enums before crossing the API boundary.
- All timestamps are RFC 3339 strings with offsets. Server-generated timestamps
  use UTC.
- Byte and file counters are non-negative integers. Display units and locale
  formatting are client concerns.
- Nullable fields are present when the value is meaningful but not currently
  known. This gives generated Swift types a consistent shape.

The HTML dashboard and native app both consume `/api/v1`. The existing
unversioned handlers are replaced when the versioned server and updated HTML
ship together; they are not retained as a second API surface.

The server exposes one current major contract rather than maintaining parallel
dashboard APIs. A future breaking change coordinates the server, HTML client,
and native client contract update; an incompatible native client must present
an update-required state instead of partially interpreting new enum values.

## Reconciliation semantics

Reconciliation copies missing RAW files and sidecars among Local, the external
SSD, and Internxt. It never deletes or overwrites an existing file. Same-path
differences produce `needs-review` instead of being resolved automatically.

The client supplies a UUID in the `Idempotency-Key` header when enqueueing. A
retry with the same key must return the same accepted reconciliation rather
than creating a second request. When a reconciliation is already queued or
running, the server returns `409` with both a structured error and the current
reconciliation state.

The API exposes `reportAvailable`, not the server-only report path. A future
report-download endpoint can be added as an optional v1 capability without
leaking server filesystem layout.

## Refresh behavior

Clients should refresh status on launch, when returning to the foreground, and
periodically while visible. Jobs may be polled more frequently only while a job
is active. Reconciliation may follow the job cadence while queued or running.
The server sets `Cache-Control: no-store`; clients may persist the most recent
successful responses solely to present an explicit last-known/offline state.

Background refresh is opportunistic on Apple platforms. The v1 API therefore
does not promise background completion notifications. Reliable notifications
would require a later server-originated push capability.

## Implementation sequence

1. Add typed Rust operation, job, progress, history, and reconciliation models,
   with adapters for the current systemd and file-based producers.
2. Implement `/api/v1` in the Raw Backup server container, update the HTML
   dashboard to use it, and remove the Python and unversioned API handlers in
   the same deployment.
3. Move the remaining collectors and workflows behind server-owned interfaces
   without changing the client contract.
4. Add response fixtures and conformance tests shared by the Rust/API boundary
   and browser dashboard.
5. Generate the Swift client from the checked-in OpenAPI document and build the
   first read-only SwiftUI vertical slice.
6. Add the reconciliation action after idempotency and conflict behavior pass
   server tests.
