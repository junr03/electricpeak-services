device="/dev/$1"
# shellcheck disable=SC1091
source @FILE_TYPES@

job_id="$(date -u +%Y%m%dT%H%M%SZ)-$1"
local_root="@LOCAL_ROOT@"
job_root="$local_root/.incoming/$job_id"
source_root="$job_root/source"
promoted_root="$job_root/promoted"
mount_root="/run/photo-workflow/sd-$1"
lock_file="/run/lock/photo-workflow.lock"
mounted_by_us=false
progress_finished=false

progress_update() {
  @PROGRESS@ update --job-id "$job_id" "$@" >/dev/null 2>&1 || true
}

progress_finish() {
  local result="$1"
  local phase="$2"
  local message="$3"
  @PROGRESS@ finish --job-id "$job_id" --result "$result" --phase "$phase" --message "$message" >/dev/null 2>&1 || true
  progress_finished=true
}

run_rsync_with_progress() {
  local direction="$1"
  shift
  @PROGRESS@ rsync --job-id "$job_id" --direction "$direction" -- "$@"
}

cleanup() {
  local exit_code=$?
  if [ "$mounted_by_us" = true ]; then
    umount "$mount_root" || true
  fi
  if [ "$progress_finished" = false ]; then
    if [ "$exit_code" -eq 0 ]; then
      progress_finish "success" "completed" "SD card import completed successfully."
    else
      progress_finish "failed" "failed" "SD card import stopped with an error."
    fi
  fi
  trap - EXIT
  exit "$exit_code"
}

@PROGRESS@ start \
  --job-id "$job_id" \
  --kind "sd-import" \
  --title "SD card → Local library" \
  --phase "waiting-for-lock" \
  --message "Waiting for other photo jobs to finish." \
  --direction "SD card → Local" >/dev/null 2>&1 || true
trap cleanup EXIT

mkdir -p "$local_root/.incoming" "$mount_root" /run/lock
exec 9>"$lock_file"
flock 9
progress_update --phase "mounting" --message "Mounting the SD card read-only."

existing_mount="$(findmnt --source "$device" --output TARGET --noheadings 2>/dev/null | head -n1 || true)"
if [ -n "$existing_mount" ]; then
  existing_options="$(findmnt --source "$device" --output OPTIONS --noheadings 2>/dev/null | head -n1 || true)"
  case ",$existing_options," in
    *,ro,*) source_root="$existing_mount" ;;
    *)
      echo "Refusing to import from a read-write mounted SD card: $device" >&2
      exit 1
      ;;
  esac
else
  mount --read-only -o nosuid,nodev,noexec "$device" "$mount_root"
  mounted_by_us=true
  source_root="$mount_root"
fi

rm -rf "$job_root"
mkdir -p "$source_root" "$job_root/source"
progress_update --phase "scanning" --message "Scanning the SD card and preparing the import."
read -r staged_files staged_bytes < <(
  rsync -rlt --checksum --dry-run --itemize-changes --out-format='%i|%l|%n' \
    "${PHOTO_RSYNC_ALL_FILTERS[@]}" \
    "$source_root/" "$job_root/source/" \
    | awk -F'|' '$1 ~ /^>f/ { count += 1; bytes += $2 } END { printf "%d %.0f\n", count, bytes }'
)
progress_update \
  --phase "staging" \
  --message "Copying $staged_files files from the SD card into safe staging." \
  --reset-progress \
  --files-completed 0 \
  --files-total "$staged_files" \
  --bytes-completed 0 \
  --bytes-total "$staged_bytes"
run_rsync_with_progress "SD card → staging" rsync -rlt --checksum \
  --info=progress2 --out-format='FILE|%i|%l|%n' \
  "${PHOTO_RSYNC_ALL_FILTERS[@]}" \
  "$source_root/" "$job_root/source/"

progress_update --phase "organizing" --message "Hashing, renaming, and organizing imported photos." --direction "Staging → organized library"
@IMPORTER@ \
  --source "$job_root/source" \
  --output "$promoted_root"

conflict_report="$job_root/local-conflicts.txt"
progress_update --phase "checking-conflicts" --message "Checking organized files against the local library."
rsync -rlt --checksum --dry-run --existing --itemize-changes \
  --out-format='%i %n%L' "${PHOTO_RSYNC_ALL_FILTERS[@]}" \
  "$promoted_root/" "$local_root/" > "$conflict_report"
if @GREP@ -Eq '^>f' "$conflict_report"; then
  review_root="$local_root/needs-review/conflicts/$job_id"
  mkdir -p "$review_root"
  rsync -rlt --checksum --ignore-existing "${PHOTO_RSYNC_ALL_FILTERS[@]}" \
    "$promoted_root/" "$review_root/"
  cp "$conflict_report" "$review_root/rsync-conflicts.txt"
  progress_finish "needs-review" "needs-review" "Imported files conflict with existing local files."
  echo "Import stopped because existing local files differ. Review: $review_root" >&2
  exit 1
fi

read -r promoted_files promoted_bytes < <(
  photo_find_files "$promoted_root" all -printf '%s\n' \
    | awk '{ count += 1; bytes += $1 } END { printf "%d %.0f\n", count, bytes }'
)
progress_update \
  --phase "promoting" \
  --message "Promoting $promoted_files organized files into the local library." \
  --direction "Staging → Local" \
  --reset-progress \
  --files-completed 0 \
  --files-total "$promoted_files" \
  --bytes-completed 0 \
  --bytes-total "$promoted_bytes"
run_rsync_with_progress "Staging → Local" rsync -rlt --checksum --ignore-existing --partial --delay-updates \
  --info=progress2 --out-format='FILE|%i|%l|%n' \
  "${PHOTO_RSYNC_ALL_FILTERS[@]}" \
  "$promoted_root/" "$local_root/"

# Keep the exact RAW paths from the most recent successful SD import.
# The dashboard uses this manifest for per-camera capture dates, rather
# than attempting to infer an import from the entire historical library.
raw_files_json="[]"
if [ -d "$promoted_root/raw" ]; then
  raw_files_json="$(photo_find_files "$promoted_root/raw" raw -printf 'raw/%P\n' | sort | jq -Rsc 'split("\n") | map(select(length > 0))')"
fi
if [ "$(jq 'length' <<< "$raw_files_json")" -gt 0 ]; then
  import_manifest="/var/lib/photo-workflow/latest-sd-import.json"
  temporary_manifest="$(mktemp "$import_manifest.XXXXXX")"
  jq -n \
    --arg completedAt "$(date --utc --iso-8601=seconds)" \
    --arg jobId "$job_id" \
    --arg sourceDevice "$device" \
    --argjson rawFiles "$raw_files_json" \
    '{ completedAt: $completedAt, jobId: $jobId, sourceDevice: $sourceDevice, rawFiles: $rawFiles }' \
    > "$temporary_manifest"
  chmod 0600 "$temporary_manifest"
  mv "$temporary_manifest" "$import_manifest"
fi

progress_update --phase "finalizing" --message "Writing the import manifest and cleaning up staging." --clear-current-file
rm -rf "$job_root"
