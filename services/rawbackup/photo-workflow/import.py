#!/usr/bin/env python3

import argparse
import hashlib
import os
import re
import shutil
import subprocess
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path


RENAMED_PICTURE = re.compile(r"^junr_(\d{8})_\d{6}_[0-9a-f]{6}$", re.IGNORECASE)


def configured_suffixes(variable):
    value = os.environ.get(variable)
    if not value:
        raise RuntimeError(f"{variable} must be provided by file-types.sh")
    return {f".{extension.casefold()}" for extension in value.split()}


RAW_SUFFIXES = configured_suffixes("PHOTO_RAW_EXTENSIONS")
SIDECAR_SUFFIXES = configured_suffixes("PHOTO_SIDECAR_EXTENSIONS")
MANAGED_SUFFIXES = RAW_SUFFIXES | SIDECAR_SUFFIXES


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def files_below(root):
    return sorted(
        path
        for path in root.rglob("*")
        if path.is_file()
        and not path.name.startswith("._")
        and path.suffix.casefold() in MANAGED_SUFFIXES
    )


def relative_to(path, root):
    return path.relative_to(root)


def run_renamer(source):
    directories = sorted(
        (path for path in source.rglob("*") if path.is_dir()),
        key=lambda path: (len(path.parts), str(path)),
    )
    directories.insert(0, source)
    for directory in directories:
        subprocess.run(["rename_picture", str(directory)], check=True)


def picture_year(path):
    match = RENAMED_PICTURE.fullmatch(path.stem)
    return match.group(1)[:4] if match else None


def make_mapping(before, after, before_hashes, after_hashes):
    before_by_key = defaultdict(list)
    after_by_key = defaultdict(list)
    for path in before:
        if path.suffix.casefold() in RAW_SUFFIXES:
            before_by_key[(path.parent, before_hashes[path])].append(path)
    for path in after:
        if path.suffix.casefold() in RAW_SUFFIXES:
            after_by_key[(path.parent, after_hashes[path])].append(path)

    mapping = {}
    for key, original_paths in before_by_key.items():
        original_names = {item.name for item in original_paths}
        renamed_paths = [
            path for path in after_by_key[key] if path.name not in original_names
        ]
        if not renamed_paths:
            continue
        renamed_path = sorted(renamed_paths)[0]
        for original_path in original_paths:
            mapping[original_path] = renamed_path
    return mapping


def associated_year(path, mapping):
    sidecar_stem = path.stem
    nested_suffix = Path(sidecar_stem).suffix.casefold()
    if nested_suffix in RAW_SUFFIXES:
        sidecar_stem = Path(sidecar_stem).stem
    for original, renamed in mapping.items():
        if original.parent != path.parent:
            continue
        if original.stem.casefold() == sidecar_stem.casefold():
            return picture_year(renamed), renamed
    return None, None


def add_to_layout(source_path, destination, report, source, output):
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists():
        if sha256(source_path) == sha256(destination):
            source_path.unlink()
            return
        collision_name = (
            f"{destination.stem}.{sha256(source_path)[:12]}{destination.suffix}"
        )
        collision = output / "needs-review" / "collisions" / collision_name
        collision.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(source_path), str(collision))
        report.append(f"collision\t{relative_to(source_path, source)}\t{relative_to(collision, source.parent)}")
        return
    shutil.move(str(source_path), str(destination))


def build_layout(source, output):
    before = files_below(source)
    before_hashes = {
        path: sha256(path)
        for path in before
        if path.suffix.casefold() in RAW_SUFFIXES
    }
    run_renamer(source)
    after = files_below(source)
    after_hashes = {
        path: sha256(path)
        for path in after
        if path.suffix.casefold() in RAW_SUFFIXES
    }
    mapping = make_mapping(before, after, before_hashes, after_hashes)
    renamed_paths = set(mapping.values())
    report = []
    review_files = []

    for path in after:
        if not path.exists():
            continue
        relative = relative_to(path, source)
        suffix = path.suffix.casefold()
        year = picture_year(path) if path in renamed_paths else None
        is_raw = suffix in RAW_SUFFIXES

        if is_raw and year:
            destination = output / "raw" / year / path.name
            add_to_layout(path, destination, report, source, output)
            continue

        associated, renamed_image = associated_year(path, mapping)
        if is_raw:
            review_year = associated or "unknown"
            destination = output / "needs-review" / review_year / relative
            review_files.append((relative, "RAW file was not renamed by rename_picture"))
        elif suffix in SIDECAR_SUFFIXES and associated:
            destination = output / "raw" / associated / f"{renamed_image.stem}{path.suffix}"
        else:
            report.append(f"ignored\t{relative}\tunassociated sidecar")
            continue
        add_to_layout(path, destination, report, source, output)

    if review_files or report:
        report_path = output / "needs-review" / "reports" / (
            datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + ".tsv"
        )
        report_path.parent.mkdir(parents=True, exist_ok=True)
        with report_path.open("w", encoding="utf-8") as report_file:
            for relative, reason in review_files:
                report_file.write(f"review\t{relative}\t{reason}\n")
            for line in report:
                report_file.write(line + "\n")


def main():
    parser = argparse.ArgumentParser(description="Prepare a camera import for the photo workflow.")
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    if not args.source.is_dir():
        raise SystemExit(f"source directory does not exist: {args.source}")
    if args.output.exists():
        shutil.rmtree(args.output)
    args.output.mkdir(parents=True)
    build_layout(args.source, args.output)


if __name__ == "__main__":
    main()
