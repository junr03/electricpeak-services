import importlib.util
import json
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path
from types import SimpleNamespace


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


def load_module(name, path):
    specification = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


progress = load_module(
    "photo_workflow_progress",
    REPOSITORY_ROOT / "photo-workflow/progress.py",
)
server = load_module(
    "rawbackup_server",
    REPOSITORY_ROOT / "web/server.py",
)


class ProgressTests(unittest.TestCase):
    def test_job_lifecycle_is_atomic_and_preserves_progress(self):
        with tempfile.TemporaryDirectory() as directory:
            state_dir = Path(directory)
            progress.command_start(
                SimpleNamespace(
                    state_dir=state_dir,
                    job_id="external-1",
                    kind="external-sync",
                    title="Local ↔ External SSD",
                    phase="scanning",
                    message="Scanning files.",
                    direction="Local ↔ SSD",
                )
            )
            progress.update_state(
                state_dir,
                "external-1",
                {
                    "phase": "copying",
                    "files_completed": 4,
                    "files_total": 10,
                    "bytes_completed": 400,
                    "bytes_total": 1000,
                },
                recent_file="raw/2026/photo.arw",
            )
            state = json.loads((state_dir / "external-1.json").read_text())
            self.assertEqual(state["progress"]["percent"], 40)
            self.assertEqual(state["recentFiles"], ["raw/2026/photo.arw"])
            self.assertEqual([entry["phase"] for entry in state["history"]], ["scanning", "copying"])
            self.assertEqual(state["history"][0]["status"], "completed")
            self.assertEqual(state["history"][1]["status"], "active")
            self.assertEqual(state["history"][1]["progress"]["filesCompleted"], 4)
            self.assertEqual(state["history"][1]["currentFile"], "raw/2026/photo.arw")

            progress.command_finish(
                SimpleNamespace(
                    state_dir=state_dir,
                    job_id="external-1",
                    result="success",
                    phase="completed",
                    message="Done.",
                )
            )
            state = json.loads((state_dir / "external-1.json").read_text())
            self.assertEqual(state["state"], "finished")
            self.assertEqual(state["progress"]["percent"], 100)
            self.assertEqual([entry["phase"] for entry in state["history"]], ["scanning", "copying", "completed"])
            self.assertEqual(state["history"][-1]["result"], "success")
            self.assertIsNotNone(state["history"][-1]["durationSeconds"])

    def test_phase_history_keeps_meaningful_message_updates(self):
        with tempfile.TemporaryDirectory() as directory:
            state_dir = Path(directory)
            progress.command_start(
                SimpleNamespace(
                    state_dir=state_dir,
                    job_id="checksum-1",
                    kind="external-sync",
                    title="Checksum",
                    phase="checksumming",
                    message="Preparing checksum comparison.",
                    direction="Local ↔ SSD",
                )
            )
            progress.update_state(
                state_dir,
                "checksum-1",
                {
                    "phase": "checksumming",
                    "message": "Comparing RAW checksums in 2026/01 (scope 1 of 2).",
                },
            )
            progress.update_state(
                state_dir,
                "checksum-1",
                {
                    "phase": "checksumming",
                    "message": "Comparing RAW checksums in 2026/02 (scope 2 of 2).",
                },
            )
            state = json.loads((state_dir / "checksum-1.json").read_text())
            self.assertEqual(len(state["history"]), 1)
            self.assertEqual(state["history"][0]["message"], "Comparing RAW checksums in 2026/02 (scope 2 of 2).")
            self.assertEqual(
                [update["message"] for update in state["history"][0]["updates"]],
                [
                    "Comparing RAW checksums in 2026/01 (scope 1 of 2).",
                    "Comparing RAW checksums in 2026/02 (scope 2 of 2).",
                ],
            )

    def test_rsync_and_rclone_records_update_live_metrics(self):
        with tempfile.TemporaryDirectory() as directory:
            state_dir = Path(directory)
            progress.command_start(
                SimpleNamespace(
                    state_dir=state_dir,
                    job_id="transfer-1",
                    kind="external-sync",
                    title="Transfer",
                    phase="copying",
                    message="Copying.",
                    direction="SSD → Local",
                )
            )
            stream = progress.StreamProgress(state_dir, "transfer-1", "SSD → Local")
            stream.rsync_record("FILE|>f+++++++++|1200|raw/2026/photo.arw")
            stream.rsync_record("  1,200 100% 1.20MB/s 0:00:01 (xfr#1, to-chk=0/1)")
            stream.write(force=True)
            state = json.loads((state_dir / "transfer-1.json").read_text())
            self.assertEqual(state["progress"]["filesCompleted"], 1)
            self.assertEqual(state["progress"]["bytesCompleted"], 1200)
            self.assertEqual(state["currentFile"], "raw/2026/photo.arw")

            stream = progress.StreamProgress(state_dir, "transfer-1", "Local → Internxt")
            stream.rclone_record(
                json.dumps(
                    {
                        "stats": {
                            "bytes": 500,
                            "totalBytes": 1000,
                            "transfers": 1,
                            "totalTransfers": 2,
                            "speed": 250,
                            "eta": 2,
                        }
                    }
                )
            )
            stream.write(force=True)
            state = json.loads((state_dir / "transfer-1.json").read_text())
            self.assertEqual(state["progress"]["filesTotal"], 2)
            self.assertEqual(state["progress"]["bytesTotal"], 1000)
            self.assertEqual(state["progress"]["speedBytesPerSecond"], 250)

    def test_dashboard_returns_only_running_jobs(self):
        with tempfile.TemporaryDirectory() as directory:
            jobs_root = Path(directory)
            current_time = datetime.now(timezone.utc).isoformat()
            (jobs_root / "active.json").write_text(
                json.dumps(
                    {
                        "id": "active",
                        "state": "running",
                        "startedAt": current_time,
                        "updatedAt": current_time,
                    }
                )
            )
            (jobs_root / "done.json").write_text(
                json.dumps(
                    {
                        "id": "done",
                        "state": "finished",
                        "startedAt": current_time,
                        "updatedAt": current_time,
                    }
                )
            )
            (jobs_root / "stale.json").write_text(
                json.dumps(
                    {
                        "id": "stale",
                        "state": "running",
                        "startedAt": "2026-01-01T00:00:00+00:00",
                        "updatedAt": "2026-01-01T00:00:00+00:00",
                    }
                )
            )
            server.JOBS_ROOT = jobs_root
            self.assertEqual([job["id"] for job in server.read_active_jobs()], ["active"])

    def test_rsync_wrapper_tracks_a_real_transfer(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            state_dir = root / "state"
            source = root / "source"
            destination = root / "destination"
            source.mkdir()
            destination.mkdir()
            (source / "photo.arw").write_bytes(b"raw-photo-data" * 4096)
            progress.command_start(
                SimpleNamespace(
                    state_dir=state_dir,
                    job_id="rsync-1",
                    kind="external-sync",
                    title="Transfer",
                    phase="copying",
                    message="Copying.",
                    direction="SSD → Local",
                )
            )
            return_code = progress.run_streaming(
                SimpleNamespace(
                    state_dir=state_dir,
                    job_id="rsync-1",
                    direction="SSD → Local",
                    command=[
                        "rsync",
                        "-rlt",
                        "--info=progress2",
                        "--out-format=FILE|%i|%l|%n",
                        f"{source}/",
                        f"{destination}/",
                    ],
                ),
                "rsync_record",
            )
            state = json.loads((state_dir / "rsync-1.json").read_text())
            self.assertEqual(return_code, 0)
            self.assertEqual(state["progress"]["filesCompleted"], 1)
            self.assertEqual(state["recentFiles"], ["photo.arw"])
            self.assertEqual((destination / "photo.arw").read_bytes(), (source / "photo.arw").read_bytes())


if __name__ == "__main__":
    unittest.main()
