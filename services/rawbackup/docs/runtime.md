# Current Raw Backup runtime contract

This describes the deployed interface, distinct from the draft v1 HTTP design.

The host collector writes `/var/lib/rawbackup/status.json`. Workflow programs
write job records under `/var/lib/rawbackup/jobs/`. The web image reads that tree
at `/data` through a read-only bind mount. It serves HTTP on port 8765:
`GET /`, `GET /api/status`, `GET /api/jobs`, `GET /api/reconcile`, and
`POST /api/reconcile`. A missing first status snapshot yields 503.

The host reconciliation state directory `/var/lib/rawbackup/reconcile` is mounted
at `/reconcile` read-write. A POST writes a UUID request to `requests/` and records
queued state in `status.json`. A queued or running reconciliation returns 409.
The host systemd path unit consumes requests by starting the existing no-delete
reconciliation workflow. The web container needs no device access, host systemd
socket, backup credentials, or Docker socket.

The collector and workflow package interfaces, file paths, JSON fields, and
permissions are preserved during extraction. Python tests cover file scope and
progress state; Rust tests cover collector behavior; the image smoke test checks
HTTP responses and request files using temporary fixture directories. Tests do
not run backup operations against real storage or production credentials.

Keep the image and host package source revisions aligned in the consuming repo.
A later migration to the draft v1 API must update its clients and conformance
fixtures together; the draft is not a claim about current endpoint support.
