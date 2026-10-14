import importlib.util
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
FILE_TYPES = REPOSITORY_ROOT / "photo-workflow/file-types.sh"
IMPORTER = REPOSITORY_ROOT / "photo-workflow/import.py"


def configured_extensions(variable):
    command = f'source "$1"; printf "%s" "${{{variable}}}"'
    result = subprocess.run(
        ["bash", "-c", command, "bash", str(FILE_TYPES)],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout


os.environ["PHOTO_RAW_EXTENSIONS"] = configured_extensions("PHOTO_RAW_EXTENSIONS")
os.environ["PHOTO_SIDECAR_EXTENSIONS"] = configured_extensions(
    "PHOTO_SIDECAR_EXTENSIONS"
)


def load_importer():
    specification = importlib.util.spec_from_file_location("photo_import", IMPORTER)
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


importer = load_importer()


def files_below(root):
    return {
        path.relative_to(root).as_posix()
        for path in root.rglob("*")
        if path.is_file()
    }


class FileScopeTests(unittest.TestCase):
    def make_source(self, root):
        source = root / "source"
        nested = source / "DCIM" / "100CANON"
        spotlight = source / ".Spotlight-V100"
        nested.mkdir(parents=True)
        spotlight.mkdir()
        fixtures = {
            nested / "IMG_0001.CR3": b"raw",
            nested / "IMG_0002.dng": b"dng",
            nested / "IMG_0001.xmp": b"xmp",
            nested / "IMG_0001.CR3.acr": b"acr",
            nested / "IMG_0001.photo-edit": b"photo-edit",
            nested / "._IMG_0003.CR3": b"appledouble-raw",
            nested / "._IMG_0001.xmp": b"appledouble-sidecar",
            nested / "IMG_0001.JPG": b"jpeg",
            nested / "catalog.ctg": b"catalog",
            source / ".DS_Store": b"finder",
            spotlight / "store.db": b"spotlight",
        }
        for path, content in fixtures.items():
            path.write_bytes(content)
        return source

    def run_rsync(self, filter_array, source, destination):
        subprocess.run(
            [
                "bash",
                "-c",
                'source "$1"; rsync -r "${!2}" "$3/" "$4/"',
                "bash",
                str(FILE_TYPES),
                f"{filter_array}[@]",
                str(source),
                str(destination),
            ],
            check=True,
        )

    def run_rclone(self, filter_array, source, destination):
        subprocess.run(
            [
                "bash",
                "-c",
                'source "$1"; rclone copy "$3" "$4" "${!2}"',
                "bash",
                str(FILE_TYPES),
                f"{filter_array}[@]",
                str(source),
                str(destination),
            ],
            check=True,
        )

    def rsync_conflicts(self, source, destination):
        result = subprocess.run(
            [
                "bash",
                "-c",
                'source "$1"; rsync -rcn --existing --itemize-changes '
                '"${PHOTO_RSYNC_ALL_FILTERS[@]}" "$2/" "$3/"',
                "bash",
                str(FILE_TYPES),
                str(source),
                str(destination),
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        return result.stdout

    def find_managed(self, source):
        result = subprocess.run(
            [
                "bash",
                "-c",
                'source "$1"; photo_find_files "$2" all -printf "%P\\n"',
                "bash",
                str(FILE_TYPES),
                str(source),
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        return set(result.stdout.splitlines())

    def test_rsync_raw_filter_copies_only_raw_extensions(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = self.make_source(root)
            destination = root / "destination"
            self.run_rsync("PHOTO_RSYNC_RAW_FILTERS", source, destination)
            self.assertEqual(
                files_below(destination),
                {"DCIM/100CANON/IMG_0001.CR3", "DCIM/100CANON/IMG_0002.dng"},
            )

    def test_rsync_all_filter_copies_raws_and_sidecars_only(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = self.make_source(root)
            destination = root / "destination"
            self.run_rsync("PHOTO_RSYNC_ALL_FILTERS", source, destination)
            self.assertEqual(
                files_below(destination),
                {
                    "DCIM/100CANON/IMG_0001.CR3",
                    "DCIM/100CANON/IMG_0002.dng",
                    "DCIM/100CANON/IMG_0001.xmp",
                    "DCIM/100CANON/IMG_0001.CR3.acr",
                    "DCIM/100CANON/IMG_0001.photo-edit",
                },
            )

    @unittest.skipUnless(shutil.which("rclone"), "rclone is not installed")
    def test_rclone_all_filter_copies_raws_and_sidecars_only(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = self.make_source(root)
            destination = root / "destination"
            self.run_rclone("PHOTO_RCLONE_ALL_FILTERS", source, destination)
            self.assertEqual(
                files_below(destination),
                {
                    "DCIM/100CANON/IMG_0001.CR3",
                    "DCIM/100CANON/IMG_0002.dng",
                    "DCIM/100CANON/IMG_0001.xmp",
                    "DCIM/100CANON/IMG_0001.CR3.acr",
                    "DCIM/100CANON/IMG_0001.photo-edit",
                },
            )

    def test_find_inventory_ignores_appledouble_names(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = self.make_source(root)
            self.assertEqual(
                self.find_managed(source),
                {
                    "DCIM/100CANON/IMG_0001.CR3",
                    "DCIM/100CANON/IMG_0002.dng",
                    "DCIM/100CANON/IMG_0001.xmp",
                    "DCIM/100CANON/IMG_0001.CR3.acr",
                    "DCIM/100CANON/IMG_0001.photo-edit",
                },
            )

    def test_conflict_scan_ignores_every_non_managed_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source"
            destination = root / "destination"
            for library in (source, destination):
                (library / ".Spotlight-V100").mkdir(parents=True)
                (library / "2026").mkdir()
            (source / ".Spotlight-V100" / "store.db").write_bytes(b"source")
            (destination / ".Spotlight-V100" / "store.db").write_bytes(
                b"destination"
            )
            (source / "2026" / "photo.jpg").write_bytes(b"source-jpeg")
            (destination / "2026" / "photo.jpg").write_bytes(
                b"destination-jpeg"
            )
            (source / "2026" / "photo.cr3").write_bytes(b"source-raw")
            (destination / "2026" / "photo.cr3").write_bytes(
                b"destination-raw"
            )
            (source / "2026" / "photo.xmp").write_bytes(b"source-sidecar")
            (destination / "2026" / "photo.xmp").write_bytes(
                b"destination-sidecar"
            )
            (source / "2026" / "._photo.cr3").write_bytes(
                b"source-appledouble"
            )
            (destination / "2026" / "._photo.cr3").write_bytes(
                b"destination-appledouble"
            )
            (source / "2026" / "._photo.xmp").write_bytes(
                b"source-appledouble"
            )
            (destination / "2026" / "._photo.xmp").write_bytes(
                b"destination-appledouble"
            )

            report = self.rsync_conflicts(source, destination)
            self.assertIn("photo.cr3", report)
            self.assertIn("photo.xmp", report)
            self.assertNotIn("store.db", report)
            self.assertNotIn("photo.jpg", report)
            self.assertNotIn("._photo.cr3", report)
            self.assertNotIn("._photo.xmp", report)

    def test_importer_ignores_non_raws_and_keeps_associated_sidecars(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = self.make_source(root)
            output = root / "output"
            original = source / "DCIM" / "100CANON" / "IMG_0001.CR3"
            renamed = original.with_name("junr_20260826_123456_abcdef.CR3")

            def fake_renamer(_source):
                original.rename(renamed)

            with mock.patch.object(importer, "run_renamer", fake_renamer):
                importer.build_layout(source, output)

            managed_output = {
                path.relative_to(output).as_posix()
                for path in importer.files_below(output)
            }
            self.assertEqual(
                managed_output,
                {
                    "raw/2026/junr_20260826_123456_abcdef.CR3",
                    "raw/2026/junr_20260826_123456_abcdef.xmp",
                    "raw/2026/junr_20260826_123456_abcdef.acr",
                    "raw/2026/junr_20260826_123456_abcdef.photo-edit",
                    "needs-review/unknown/DCIM/100CANON/IMG_0002.dng",
                },
            )
            self.assertNotIn("IMG_0001.JPG", "\n".join(files_below(output)))
            self.assertNotIn("store.db", "\n".join(files_below(output)))
            self.assertFalse(
                any(
                    path.name.startswith("._")
                    for path in importer.files_below(output)
                )
            )


if __name__ == "__main__":
    unittest.main()
