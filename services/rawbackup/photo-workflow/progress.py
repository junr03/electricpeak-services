#!/usr/bin/env python3

"""Write atomic progress snapshots and stream rsync/rclone statistics."""

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path


DEFAULT_STATE_DIR = Path(
    os.environ.get("PHOTO_WORKFLOW_PROGRESS_DIR", "/var/lib/rawbackup/jobs")
)
RSYNC_FILE = re.compile(
    r"^FILE\|(?P<item>[^|]+)\|(?P<size>\d+)\|(?P<name>.*)$"
)
RSYNC_PROGRESS = re.compile(
    r"^\s*(?P<bytes>[\d,]+)\s+\d+%\s+(?P<speed>\S+/s).*"
    r"\(xfr#(?P<files>\d+),\s+(?:to-chk|ir-chk)="
)
RSYNC_PARTIAL_PROGRESS = re.compile(
    r"^\s*(?P<bytes>[\d,]+)\s+\d+%\s+(?P<speed>\S+/s)"
)
SPEED_UNITS = {
    "B/s": 1,
    "kB/s": 1000,
    "MB/s": 1000**2,
    "GB/s": 1000**3,
    "TB/s": 1000**4,
    "KiB/s": 1024,
    "MiB/s": 1024**2,
    "GiB/s": 1024**3,
    "TiB/s": 1024**4,
}
HISTORY_PROGRESS_FIELDS = (
    "filesCompleted",
    "filesTotal",
    "bytesCompleted",
    "bytesTotal",
    "speedBytesPerSecond",
    "etaSeconds",
    "percent",
)


def iso_now():
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def state_path(state_dir, job_id):
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", job_id):
        raise ValueError("job id contains unsupported characters")
    return state_dir / f"{job_id}.json"


def read_state(state_dir, job_id):
    try:
        return json.loads(state_path(state_dir, job_id).read_text(encoding="utf-8"))
    except (FileNotFoundError, json.JSONDecodeError, OSError):
        return {"id": job_id, "progress": {}, "recentFiles": []}


def write_state(state_dir, job_id, state):
    state_dir.mkdir(parents=True, exist_ok=True)
    destination = state_path(state_dir, job_id)
    descriptor, temporary_name = tempfile.mkstemp(
        dir=state_dir, prefix=f".{job_id}.", suffix=".json"
    )
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as temporary:
            json.dump(state, temporary, separators=(",", ":"))
            temporary.write("\n")
            temporary.flush()
            os.fsync(temporary.fileno())
        os.chmod(temporary_name, 0o644)
        os.replace(temporary_name, destination)
    finally:
        try:
            os.unlink(temporary_name)
        except FileNotFoundError:
            pass


def clean_filename(value):
    if not value:
        return None
    cleaned = value.replace("\\", "/").lstrip("/")
    parts = [part for part in cleaned.split("/") if part not in {"", ".", ".."}]
    return "/".join(parts)[-500:] or None


def calculate_percent(progress):
    bytes_total = progress.get("bytesTotal") or 0
    files_total = progress.get("filesTotal") or 0
    if bytes_total > 0:
        return min(100, max(0, progress.get("bytesCompleted", 0) / bytes_total * 100))
    if files_total > 0:
        return min(100, max(0, progress.get("filesCompleted", 0) / files_total * 100))
    return None


def history_progress(progress):
    return {
        key: progress[key]
        for key in HISTORY_PROGRESS_FIELDS
        if key in progress and progress[key] is not None
    }


def duration_seconds(started_at, finished_at):
    try:
        started = datetime.fromisoformat(started_at)
        finished = datetime.fromisoformat(finished_at)
    except (TypeError, ValueError):
        return None
    return max(0, round((finished - started).total_seconds(), 3))


def finish_history_entry(entry, finished_at, result=None):
    entry["finishedAt"] = finished_at
    entry["durationSeconds"] = duration_seconds(entry.get("startedAt"), finished_at)
    entry["status"] = result or "completed"
    if result:
        entry["result"] = result


def record_phase_history(state, timestamp, phase=None, message=None, direction=None):
    """Record phase boundaries and meaningful message changes in the job log."""
    phase = phase or state.get("phase") or "running"
    history = state.setdefault("history", [])
    progress = state.get("progress") or {}
    current_file = state.get("currentFile") or (state.get("recentFiles") or [None])[0]
    current = history[-1] if history else None

    if current is None or current.get("phase") != phase or current.get("finishedAt"):
        if current is not None and not current.get("finishedAt"):
            finish_history_entry(current, timestamp)
        current = {
            "phase": phase,
            "message": message if message is not None else state.get("message"),
            "direction": direction if direction is not None else state.get("direction"),
            "status": "active",
            "startedAt": timestamp,
            "finishedAt": None,
            "durationSeconds": None,
            "progress": history_progress(progress),
            "currentFile": current_file,
            "updates": [],
        }
        history.append(current)
    else:
        message_changed = message is not None and message != current.get("message")
        direction_changed = direction is not None and direction != current.get("direction")
        if message_changed or direction_changed:
            update = {"at": timestamp}
            if message_changed:
                current["message"] = message
                update["message"] = message
            if direction_changed:
                current["direction"] = direction
                update["direction"] = direction
            current.setdefault("updates", []).append(update)
        current["progress"] = history_progress(progress)
        current["currentFile"] = current_file

    return current


def update_state(state_dir, job_id, values, reset_progress=False, recent_file=None):
    state = read_state(state_dir, job_id)
    phase = values.get("phase")
    message = values.get("message")
    direction = values.get("direction")
    progress = {} if reset_progress else dict(state.get("progress") or {})
    progress_keys = {
        "files_completed": "filesCompleted",
        "files_total": "filesTotal",
        "bytes_completed": "bytesCompleted",
        "bytes_total": "bytesTotal",
        "speed_bytes_per_second": "speedBytesPerSecond",
        "eta_seconds": "etaSeconds",
    }
    for source, destination in progress_keys.items():
        value = values.pop(source, None)
        if value is not None:
            progress[destination] = value
    progress["percent"] = calculate_percent(progress)
    state["progress"] = progress

    current_file = values.pop("current_file", None)
    if current_file is not None:
        state["currentFile"] = clean_filename(current_file)
    if values.pop("clear_current_file", False):
        state["currentFile"] = None
    if recent_file:
        recent_file = clean_filename(recent_file)
        recent = [item for item in state.get("recentFiles", []) if item != recent_file]
        if recent_file:
            recent.insert(0, recent_file)
        state["recentFiles"] = recent[:6]
    for key, value in values.items():
        if value is not None:
            state[key] = value
    updated_at = iso_now()
    state["updatedAt"] = updated_at
    record_phase_history(state, updated_at, phase, message, direction)
    write_state(state_dir, job_id, state)
    return state


def command_start(args):
    started = iso_now()
    state = {
        "id": args.job_id,
        "kind": args.kind,
        "title": args.title,
        "state": "running",
        "result": None,
        "phase": args.phase,
        "message": args.message,
        "direction": args.direction,
        "startedAt": started,
        "updatedAt": started,
        "finishedAt": None,
        "currentFile": None,
        "recentFiles": [],
        "history": [],
        "progress": {
            "filesCompleted": 0,
            "filesTotal": None,
            "bytesCompleted": 0,
            "bytesTotal": None,
            "speedBytesPerSecond": None,
            "etaSeconds": None,
            "percent": None,
        },
    }
    record_phase_history(state, started, args.phase, args.message, args.direction)
    write_state(args.state_dir, args.job_id, state)


def values_from_args(args):
    return {
        "phase": getattr(args, "phase", None),
        "message": getattr(args, "message", None),
        "direction": getattr(args, "direction", None),
        "step": getattr(args, "step", None),
        "totalSteps": getattr(args, "total_steps", None),
        "current_file": getattr(args, "current_file", None),
        "clear_current_file": getattr(args, "clear_current_file", False),
        "files_completed": getattr(args, "files_completed", None),
        "files_total": getattr(args, "files_total", None),
        "bytes_completed": getattr(args, "bytes_completed", None),
        "bytes_total": getattr(args, "bytes_total", None),
        "speed_bytes_per_second": getattr(args, "speed_bytes_per_second", None),
        "eta_seconds": getattr(args, "eta_seconds", None),
    }


def command_update(args):
    update_state(
        args.state_dir,
        args.job_id,
        values_from_args(args),
        reset_progress=args.reset_progress,
        recent_file=args.recent_file,
    )


def command_finish(args):
    state = read_state(args.state_dir, args.job_id)
    finished_at = iso_now()
    final_phase = args.phase or state.get("phase") or "completed"
    final_message = args.message or state.get("message")
    state.update(
        {
            "state": "finished",
            "result": args.result,
            "message": final_message,
            "phase": final_phase,
            "currentFile": None,
            "finishedAt": finished_at,
            "updatedAt": finished_at,
        }
    )
    progress = state.setdefault("progress", {})
    progress["speedBytesPerSecond"] = 0
    progress["etaSeconds"] = 0 if args.result == "success" else None
    if args.result == "success" and progress.get("bytesTotal") is not None:
        progress["bytesCompleted"] = progress["bytesTotal"]
    if args.result == "success" and progress.get("filesTotal") is not None:
        progress["filesCompleted"] = progress["filesTotal"]
    progress["percent"] = calculate_percent(progress)
    final_entry = record_phase_history(
        state,
        finished_at,
        final_phase,
        final_message,
        state.get("direction"),
    )
    finish_history_entry(final_entry, finished_at, args.result)
    write_state(args.state_dir, args.job_id, state)


def parse_speed(value):
    match = re.fullmatch(r"([\d.]+)([A-Za-z]+/s)", value)
    if not match:
        return 0
    return int(float(match.group(1)) * SPEED_UNITS.get(match.group(2), 1))


class StreamProgress:
    def __init__(self, state_dir, job_id, direction):
        self.state_dir = state_dir
        self.job_id = job_id
        self.direction = direction
        state = read_state(state_dir, job_id)
        progress = state.get("progress") or {}
        self.base_files = progress.get("filesCompleted") or 0
        self.base_bytes = progress.get("bytesCompleted") or 0
        self.files = 0
        self.bytes = 0
        self.fallback_files = 0
        self.fallback_bytes = 0
        self.speed = 0
        self.eta = None
        self.current_file = None
        self.pending_recent = None
        self.last_write = 0

    def write(self, force=False):
        if not force and time.monotonic() - self.last_write < 0.5:
            return
        values = {
            "direction": self.direction,
            "current_file": self.current_file,
            "files_completed": self.base_files + max(self.files, self.fallback_files),
            "bytes_completed": self.base_bytes + max(self.bytes, self.fallback_bytes),
            "speed_bytes_per_second": self.speed,
            "eta_seconds": self.eta,
        }
        try:
            update_state(
                self.state_dir,
                self.job_id,
                values,
                recent_file=self.pending_recent,
            )
        except OSError as error:
            print(f"progress snapshot warning: {error}", file=sys.stderr)
        self.pending_recent = None
        self.last_write = time.monotonic()

    def rsync_record(self, record):
        record = record.strip()
        if not record:
            return
        file_match = RSYNC_FILE.match(record)
        if file_match:
            if "f" not in file_match.group("item"):
                return
            filename = clean_filename(file_match.group("name"))
            self.current_file = filename
            self.pending_recent = filename
            self.fallback_files += 1
            self.fallback_bytes += int(file_match.group("size"))
            self.write()
            return
        progress_match = RSYNC_PROGRESS.match(record)
        if progress_match:
            self.bytes = int(progress_match.group("bytes").replace(",", ""))
            self.files = int(progress_match.group("files"))
            self.speed = parse_speed(progress_match.group("speed"))
            self.write()
            return
        partial_match = RSYNC_PARTIAL_PROGRESS.match(record)
        if partial_match:
            self.bytes = int(partial_match.group("bytes").replace(",", ""))
            self.speed = parse_speed(partial_match.group("speed"))
            self.write()
            return
        print(record, file=sys.stderr)

    def rclone_record(self, record):
        record = record.strip()
        if not record:
            return
        try:
            entry = json.loads(record)
        except json.JSONDecodeError:
            print(record, file=sys.stderr)
            return
        stats = entry.get("stats")
        if isinstance(stats, dict):
            self.bytes = int(stats.get("bytes") or 0)
            self.files = int(stats.get("transfers") or stats.get("checks") or 0)
            self.speed = int(stats.get("speed") or 0)
            eta = stats.get("eta")
            self.eta = int(eta) if isinstance(eta, (int, float)) else None
            values = {
                "files_total": int(
                    stats.get("totalTransfers") or stats.get("totalChecks") or 0
                ),
                "bytes_total": int(stats.get("totalBytes") or 0),
            }
            try:
                update_state(self.state_dir, self.job_id, values)
            except OSError:
                pass
            self.write()
            return
        filename = clean_filename(entry.get("object"))
        if filename:
            self.current_file = filename
            self.pending_recent = filename
            self.write()
        level = str(entry.get("level", "")).lower()
        if level in {"warning", "error", "critical", "emergency"}:
            message = entry.get("msg") or record
            print(f"{level}: {message}", file=sys.stderr)


def run_streaming(args, parser_name):
    command = list(args.command)
    if command and command[0] == "--":
        command.pop(0)
    if not command:
        raise SystemExit("a command is required after --")
    stream = StreamProgress(args.state_dir, args.job_id, args.direction)
    try:
        process = subprocess.Popen(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            bufsize=0,
        )
    except OSError as error:
        print(f"could not start progress command: {error}", file=sys.stderr)
        return 127

    buffer = b""
    with process.stdout:
        while True:
            chunk = process.stdout.read(4096)
            if not chunk:
                break
            buffer += chunk
            records = re.split(br"[\r\n]", buffer)
            buffer = records.pop()
            for raw_record in records:
                record = raw_record.decode("utf-8", errors="replace")
                getattr(stream, parser_name)(record)
    if buffer:
        getattr(stream, parser_name)(buffer.decode("utf-8", errors="replace"))
    return_code = process.wait()
    stream.current_file = None
    stream.speed = 0
    stream.eta = 0 if return_code == 0 else None
    stream.write(force=True)
    return return_code


def add_progress_arguments(parser):
    parser.add_argument("--phase")
    parser.add_argument("--message")
    parser.add_argument("--direction")
    parser.add_argument("--step", type=int)
    parser.add_argument("--total-steps", type=int)
    parser.add_argument("--current-file")
    parser.add_argument("--clear-current-file", action="store_true")
    parser.add_argument("--recent-file")
    parser.add_argument("--files-completed", type=int)
    parser.add_argument("--files-total", type=int)
    parser.add_argument("--bytes-completed", type=int)
    parser.add_argument("--bytes-total", type=int)
    parser.add_argument("--speed-bytes-per-second", type=int)
    parser.add_argument("--eta-seconds", type=int)


def build_parser():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-dir", type=Path, default=DEFAULT_STATE_DIR)
    subparsers = parser.add_subparsers(dest="action", required=True)

    start = subparsers.add_parser("start")
    start.add_argument("--job-id", required=True)
    start.add_argument("--kind", required=True)
    start.add_argument("--title", required=True)
    start.add_argument("--phase", required=True)
    start.add_argument("--message", required=True)
    start.add_argument("--direction")
    start.set_defaults(function=command_start)

    update = subparsers.add_parser("update")
    update.add_argument("--job-id", required=True)
    update.add_argument("--reset-progress", action="store_true")
    add_progress_arguments(update)
    update.set_defaults(function=command_update)

    finish = subparsers.add_parser("finish")
    finish.add_argument("--job-id", required=True)
    finish.add_argument("--result", choices=("success", "failed", "needs-review"), required=True)
    finish.add_argument("--phase")
    finish.add_argument("--message")
    finish.set_defaults(function=command_finish)

    for name, parser_name in (("rsync", "rsync_record"), ("rclone", "rclone_record")):
        stream = subparsers.add_parser(name)
        stream.add_argument("--job-id", required=True)
        stream.add_argument("--direction", required=True)
        stream.add_argument("command", nargs=argparse.REMAINDER)
        stream.set_defaults(
            function=lambda arguments, selected=parser_name: sys.exit(
                run_streaming(arguments, selected)
            )
        )
    return parser


def main():
    args = build_parser().parse_args()
    args.function(args)


if __name__ == "__main__":
    main()
