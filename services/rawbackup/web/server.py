#!/usr/bin/env python3
"""Dashboard for the host-generated Raw Backup status and reconcile requests."""

import json
import os
import tempfile
import threading
import uuid
from datetime import datetime, timedelta, timezone
from http import HTTPStatus
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


APP_ROOT = Path(__file__).parent
STATUS_PATH = Path("/data/status.json")
JOBS_ROOT = Path("/data/jobs")
RECONCILE_ROOT = Path("/reconcile")
RECONCILE_STATUS_PATH = RECONCILE_ROOT / "status.json"
RECONCILE_REQUEST_DIR = RECONCILE_ROOT / "requests"
RECONCILE_LOCK = threading.Lock()
JOB_STALE_AFTER = timedelta(hours=24)


def read_json(path: Path, fallback: dict) -> dict:
    try:
        return json.loads(path.read_text())
    except (FileNotFoundError, json.JSONDecodeError, OSError):
        return fallback


def write_json(path: Path, data: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_path = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.")
    try:
        with os.fdopen(descriptor, "w") as temporary_file:
            json.dump(data, temporary_file)
            temporary_file.flush()
            os.fsync(temporary_file.fileno())
        os.replace(temporary_path, path)
    finally:
        if os.path.exists(temporary_path):
            os.unlink(temporary_path)


def read_active_jobs() -> list[dict]:
    jobs = []
    stale_before = datetime.now(timezone.utc) - JOB_STALE_AFTER
    try:
        paths = JOBS_ROOT.glob("*.json")
        for path in paths:
            job = read_json(path, {})
            try:
                updated_at = datetime.fromisoformat(job.get("updatedAt", ""))
            except (TypeError, ValueError):
                continue
            if updated_at.tzinfo is None:
                updated_at = updated_at.replace(tzinfo=timezone.utc)
            if (
                job.get("state") == "running"
                and isinstance(job.get("id"), str)
                and updated_at >= stale_before
            ):
                jobs.append(job)
    except OSError:
        return []
    return sorted(jobs, key=lambda job: (job.get("startedAt") or "", job["id"]))


class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(APP_ROOT), **kwargs)

    def do_GET(self):
        request_path = self.path.split("?", 1)[0]
        if request_path == "/api/status":
            self.send_status()
            return
        if request_path == "/api/jobs":
            self.send_jobs()
            return
        if request_path == "/api/reconcile":
            self.send_reconcile_status()
            return
        if request_path in {"/", "/index.html"}:
            self.path = "/index.html"
        super().do_GET()

    def do_POST(self):
        if self.path.split("?", 1)[0] != "/api/reconcile":
            self.send_error(HTTPStatus.NOT_FOUND)
            return
        self.enqueue_reconciliation()

    def send_status(self):
        if not STATUS_PATH.is_file():
            self.send_error(HTTPStatus.SERVICE_UNAVAILABLE, "Waiting for the first status snapshot")
            return
        content = STATUS_PATH.read_bytes()
        self.send_response(HTTPStatus.OK)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(content)))
        self.end_headers()
        self.wfile.write(content)

    def send_jobs(self):
        self.send_json({"jobs": read_active_jobs()})

    def send_reconcile_status(self):
        self.send_json(
            read_json(
                RECONCILE_STATUS_PATH,
                {
                    "state": "idle",
                    "result": None,
                    "message": "No reconciliation has been requested.",
                    "startedAt": None,
                    "finishedAt": None,
                    "reportPath": None,
                    "inventories": None,
                },
            )
        )

    def enqueue_reconciliation(self):
        with RECONCILE_LOCK:
            status = read_json(RECONCILE_STATUS_PATH, {"state": "idle"})
            if status.get("state") in {"queued", "running"}:
                self.send_json(status, HTTPStatus.CONFLICT)
                return
            request = {"id": str(uuid.uuid4())}
            try:
                write_json(RECONCILE_REQUEST_DIR / f"{request['id']}.json", request)
                status = {
                    "state": "queued",
                    "result": None,
                    "message": "Reconciliation request queued.",
                    "startedAt": None,
                    "finishedAt": None,
                    "reportPath": None,
                    "inventories": None,
                }
                write_json(RECONCILE_STATUS_PATH, status)
            except OSError:
                self.send_error(HTTPStatus.SERVICE_UNAVAILABLE, "Could not queue reconciliation")
                return
        self.send_json(status, HTTPStatus.ACCEPTED)

    def send_json(self, data: dict, status: HTTPStatus = HTTPStatus.OK):
        content = json.dumps(data).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(content)))
        self.end_headers()
        self.wfile.write(content)

    def end_headers(self):
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "same-origin")
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


if __name__ == "__main__":
    ThreadingHTTPServer(("0.0.0.0", 8765), Handler).serve_forever()
