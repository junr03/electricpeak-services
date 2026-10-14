local_root="${PHOTO_WORKFLOW_LOCAL_ROOT:?PHOTO_WORKFLOW_LOCAL_ROOT is required}"
# shellcheck disable=SC1091
source @FILE_TYPES@

mount_root="${PHOTO_WORKFLOW_MOUNT_POINT:?PHOTO_WORKFLOW_MOUNT_POINT is required}"
external_root="$mount_root/${PHOTO_WORKFLOW_DESTINATION_SUBDIRECTORY?PHOTO_WORKFLOW_DESTINATION_SUBDIRECTORY is required}"
external_raw_subdirectory="${PHOTO_WORKFLOW_EXTERNAL_RAW_SUBDIRECTORY:?PHOTO_WORKFLOW_EXTERNAL_RAW_SUBDIRECTORY is required}"
local_raw="$local_root/raw"
device="/dev/disk/by-uuid/${PHOTO_WORKFLOW_FILESYSTEM_UUID:?PHOTO_WORKFLOW_FILESYSTEM_UUID is required}"
report_root="$local_root/needs-review/conflicts"
state_root="/var/lib/photo-workflow"
full_check_marker="$state_root/external-sync-last-full-check"
full_check_interval="${PHOTO_WORKFLOW_FULL_CHECK_INTERVAL:?PHOTO_WORKFLOW_FULL_CHECK_INTERVAL is required}"
job_id="external-$(date -u +%Y%m%dT%H%M%SZ)"
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
      progress_finish "success" "completed" "External SSD sync completed successfully."
    else
      progress_finish "failed" "failed" "External SSD sync stopped with an error."
    fi
  fi
  trap - EXIT
  exit "$exit_code"
}

@PROGRESS@ start \
  --job-id "$job_id" \
  --kind "external-sync" \
  --title "Local ↔ External SSD" \
  --phase "waiting-for-lock" \
  --message "Waiting for other photo jobs to finish." \
  --direction "Local ↔ SSD" >/dev/null 2>&1 || true
trap cleanup EXIT

mkdir -p "$local_root" "$mount_root" "$state_root" /run/lock
exec 9>"$lock_file"
flock 9
progress_update --phase "mounting" --message "Confirming the external SSD mount."

if [ ! -e "$device" ]; then
  echo "Configured external photo device is not present: $device" >&2
  exit 0
fi

existing_mount="$(findmnt --source "$device" --output TARGET --noheadings 2>/dev/null | head -n1 || true)"
if [ -n "$existing_mount" ]; then
  mount_root="$existing_mount"
  external_root="$mount_root/${PHOTO_WORKFLOW_DESTINATION_SUBDIRECTORY?PHOTO_WORKFLOW_DESTINATION_SUBDIRECTORY is required}"
elif ! mountpoint -q "$mount_root"; then
  mount -o nosuid,nodev "$device" "$mount_root"
  mounted_by_us=true
else
  mounted_device="$(findmnt --mountpoint "$mount_root" --output SOURCE --noheadings 2>/dev/null | head -n1 || true)"
  device_real="$(readlink -f "$device")"
  mounted_device_real="$(readlink -f "$mounted_device")"
  if [ "$mounted_device_real" != "$device_real" ]; then
    echo "Refusing to use a mount point owned by another device: $mount_root" >&2
    exit 1
  fi
fi
external_raw="$external_root/$external_raw_subdirectory"
mkdir -p "$external_root"
mkdir -p "$external_raw"

temp_report_root="/run/photo-workflow/$job_id"
mkdir -p "$temp_report_root"
local_to_external_raw_quick="$temp_report_root/local-to-external-raw-quick.txt"
external_to_local_raw_quick="$temp_report_root/external-to-local-raw-quick.txt"
local_to_external_raw="$temp_report_root/local-to-external-raw.txt"
external_to_local_raw="$temp_report_root/external-to-local-raw.txt"
local_to_external_raw_candidates="$temp_report_root/local-to-external-raw-candidates.txt"
external_to_local_raw_candidates="$temp_report_root/external-to-local-raw-candidates.txt"
raw_scopes="$temp_report_root/raw-scopes.txt"
raw_conflicts="$temp_report_root/raw-conflicts.txt"

scope_from_path() {
  local path="$1"
  local first rest second
  case "$path" in
    ""|.incoming|.incoming/*|needs-review|needs-review/*)
      return
      ;;
  esac
  first="$(printf '%s\n' "$path" | sed 's#/.*##')"
  rest="$(printf '%s\n' "$path" | sed 's#^[^/]*/##')"
  if [ "$rest" = "$path" ]; then
    printf '%s\n' "$first"
  else
    second="$(printf '%s\n' "$rest" | sed 's#/.*##')"
    printf '%s/%s\n' "$first" "$second"
  fi
}

scope_from_file_path() {
  local file_path="$1"
  local parent_path
  case "$file_path" in
    ""|.incoming|.incoming/*|needs-review|needs-review/*)
      return
      ;;
  esac

  parent_path="${file_path%/*}"
  if [ "$parent_path" = "$file_path" ]; then
    # A root-level file requires syncing the root directory itself.
    printf '.\n'
  else
    scope_from_path "$parent_path"
  fi
}

add_report_scopes() {
  local report_path="$1"
  local item path
  while IFS= read -r line; do
    item="$(printf '%s\n' "$line" | sed 's/ .*//')"
    case "$item" in
      *f*) ;;
      *) continue ;;
    esac
    path="$(printf '%s\n' "$line" | sed 's/^[^ ]* //; s/ -> .*//')"
    scope_from_file_path "$path"
  done < "$report_path" >> "$raw_scopes"
}

# A metadata pass identifies the small set of files that need a checksum
# comparison. Rechecking only these paths prevents one same-path conflict from
# forcing an expensive checksum sweep of the entire year directory on every
# hourly timer run.
add_report_file_paths() {
  local item path
  while IFS= read -r line; do
    item="$(printf '%s\n' "$line" | sed 's/ .*//')"
    case "$item" in
      *f*) ;;
      *) continue ;;
    esac
    path="$(printf '%s\n' "$line" | sed 's/^[^ ]* //; s/ -> .*//')"
    case "$path" in
      ""|.incoming|.incoming/*|needs-review|needs-review/*)
        continue
        ;;
    esac
    printf '%s\n' "$path"
  done < "$1"
}

save_conflict_reports() {
  local review_path="$1"
  mkdir -p "$review_path"
  cp -- "$local_to_external_raw_quick" "$review_path/"
  cp -- "$external_to_local_raw_quick" "$review_path/"
  cp -- "$local_to_external_raw" "$review_path/"
  cp -- "$external_to_local_raw" "$review_path/"
  cp -- "$raw_scopes" "$review_path/"
}

collect_raw_conflicts() {
  {
    collect_raw_conflicts_from_report "$local_to_external_raw"
    collect_raw_conflicts_from_report "$external_to_local_raw"
  } | sort -u > "$raw_conflicts"
}

collect_raw_conflicts_from_report() {
  local report_path="$1"
  local current_scope="" relative_path

  while IFS= read -r line; do
    case "$line" in
      "Scope: raw/"*)
        current_scope="${line#Scope: raw/}"
        ;;
      ">f"*)
        relative_path="$(printf '%s\n' "$line" | sed 's/^[^ ]* //; s/ -> .*//')"
        if [ -n "$current_scope" ] && [ "$current_scope" != "." ]; then
          printf '%s/%s\n' "$current_scope" "$relative_path"
        else
          printf '%s\n' "$relative_path"
        fi
        ;;
    esac
  done < "$report_path"
}

is_safe_relative_path() {
  case "$1" in
    ""|.|..|/*|./*|../*|*/.|*/..|*/./*|*/../*|*//*) return 1 ;;
    *) return 0 ;;
  esac
}

copy_raw_for_review() {
  source_path="$1"
  destination_path="$2"
  partial_path="${destination_path}.partial"

  mkdir -p "$(dirname "$destination_path")"
  if [ -e "$destination_path" ] || [ -e "$partial_path" ]; then
    echo "Refusing to overwrite an existing RAW review copy: $destination_path" >&2
    return 1
  fi

  if ! cp --preserve=mode,timestamps -- "$source_path" "$partial_path"; then
    rm -f -- "$partial_path"
    return 1
  fi
  if ! cmp -s -- "$source_path" "$partial_path"; then
    rm -f -- "$partial_path"
    echo "RAW review copy failed verification: $destination_path" >&2
    return 1
  fi
  if ! mv -- "$partial_path" "$destination_path"; then
    rm -f -- "$partial_path"
    return 1
  fi
}

quarantine_raw_conflicts() {
  review_path="$1"

  # Validate every conflict before copying or removing anything. Both versions
  # must still exist so the review directory always receives the complete pair.
  while IFS= read -r relative_path; do
    if ! is_safe_relative_path "$relative_path"; then
      echo "Refusing unsafe RAW conflict path: $relative_path" >&2
      return 1
    fi
    if [ ! -f "$local_raw/$relative_path" ] || [ ! -f "$external_raw/$relative_path" ]; then
      echo "A RAW conflict disappeared before it could be quarantined: $relative_path" >&2
      return 1
    fi
  done < "$raw_conflicts"

  # Copy and verify every pair before deleting any active copy. A failed copy
  # therefore leaves both libraries untouched.
  while IFS= read -r relative_path; do
    copy_raw_for_review \
      "$local_raw/$relative_path" \
      "$review_path/raw/local/$relative_path" || return 1
    copy_raw_for_review \
      "$external_raw/$relative_path" \
      "$review_path/raw/ssd/$relative_path" || return 1
  done < "$raw_conflicts"

  while IFS= read -r relative_path; do
    if ! cmp -s -- "$local_raw/$relative_path" "$review_path/raw/local/$relative_path" || \
       ! cmp -s -- "$external_raw/$relative_path" "$review_path/raw/ssd/$relative_path"; then
      echo "A RAW file changed while it was being quarantined: $relative_path" >&2
      return 1
    fi
  done < "$raw_conflicts"

  cp -- "$raw_conflicts" "$review_path/quarantined-raw.txt" || return 1
  while IFS= read -r relative_path; do
    rm -- "$local_raw/$relative_path" "$external_raw/$relative_path" || return 1
  done < "$raw_conflicts"
}

full_check=false
now="$(date +%s)"
if [ ! -s "$full_check_marker" ]; then
  full_check=true
else
  last_full="$(cat "$full_check_marker" 2>/dev/null || true)"
  if [ -z "$last_full" ]; then
    full_check=true
  else
    case "$last_full" in
      *[!0-9]*) full_check=true ;;
      *) [ "$((now - last_full))" -ge "$full_check_interval" ] && full_check=true ;;
    esac
  fi
fi

# A metadata-only pass over recognized RAW extensions is cheap for the common
# additive case. Every other file type is intentionally invisible.
progress_update \
  --phase "scanning" \
  --message "Scanning Local and the SSD for new or changed RAW files." \
  --reset-progress \
  --files-completed 0 \
  --bytes-completed 0
rsync -rlt --dry-run --itemize-changes --out-format='%i %n%L' \
  "${PHOTO_RSYNC_RAW_FILTERS[@]}" "$local_raw/" "$external_raw/" > "$local_to_external_raw_quick"
rsync -rlt --dry-run --itemize-changes --out-format='%i %n%L' \
  "${PHOTO_RSYNC_RAW_FILTERS[@]}" "$external_raw/" "$local_raw/" > "$external_to_local_raw_quick"
: > "$raw_scopes"
: > "$local_to_external_raw_candidates"
: > "$external_to_local_raw_candidates"

# Photomator sidecars are authoritative on the external SSD. Promote only
# newer SSD copies into local storage; local sidecars never overwrite the SSD.
progress_update \
  --phase "syncing-sidecars" \
  --message "Promoting newer Photomator sidecars from the SSD." \
  --direction "SSD → Local"
run_rsync_with_progress "SSD → Local" rsync -rlt --checksum --update --partial --delay-updates \
  --info=progress2 --out-format='FILE|%i|%l|%n' \
  "${PHOTO_RSYNC_SIDECAR_FILTERS[@]}" \
  "$external_raw/" "$local_raw/"

planned_files=0
planned_bytes=0
add_planned_files() {
  local report_path="$1"
  local source_path="$2"
  local line item relative_path size
  while IFS= read -r line; do
    item="${line%% *}"
    case "$item" in
      '>f+++++++++') ;;
      *) continue ;;
    esac
    relative_path="${line#* }"
    if [ -f "$source_path/$relative_path" ]; then
      size="$(stat --format=%s -- "$source_path/$relative_path")"
      planned_files="$((planned_files + 1))"
      planned_bytes="$((planned_bytes + size))"
    fi
  done < "$report_path"
}

add_planned_files "$local_to_external_raw_quick" "$local_raw"
add_planned_files "$external_to_local_raw_quick" "$external_raw"
progress_update \
  --phase "planning" \
  --message "Found $planned_files RAW files to copy between Local and the SSD." \
  --direction "Local ↔ SSD" \
  --reset-progress \
  --files-completed 0 \
  --files-total "$planned_files" \
  --bytes-completed 0 \
  --bytes-total "$planned_bytes"

if [ "$full_check" = true ]; then
  progress_update --phase "checksumming" --message "Running the scheduled RAW checksum comparison."
  {
    photo_find_files "$local_raw" raw -printf '%P\n'
    photo_find_files "$external_raw" raw -printf '%P\n'
  } | while IFS= read -r file_path; do
    scope_from_file_path "$file_path"
  done >> "$raw_scopes"
else
  progress_update --phase "checksumming-candidates" --message "Checksumming RAW files whose metadata changed."
  add_report_scopes "$local_to_external_raw_quick"
  add_report_scopes "$external_to_local_raw_quick"
  add_report_file_paths "$local_to_external_raw_quick" >> "$local_to_external_raw_candidates"
  add_report_file_paths "$external_to_local_raw_quick" >> "$external_to_local_raw_candidates"
fi
sort -u "$raw_scopes" | sed '/^$/d' > "$raw_scopes.sorted"
mv "$raw_scopes.sorted" "$raw_scopes"

if [ ! -s "$raw_scopes" ]; then
  if [ "$full_check" = true ]; then
    printf '%s\n' "$now" > "$full_check_marker"
  fi
  rm -rf "$temp_report_root"
  progress_update --phase "finalizing" --message "No files need to be copied."
  exit 0
fi

: > "$local_to_external_raw"
: > "$external_to_local_raw"
if [ "$full_check" = true ]; then
  checksum_scope_total="$(wc -l < "$raw_scopes")"
  checksum_scope_number=0
  while IFS= read -r scope; do
    checksum_scope_number="$((checksum_scope_number + 1))"
    local_scope="$local_raw/$scope"
    external_scope="$external_raw/$scope"
    if [ -d "$local_scope" ] && [ -d "$external_scope" ]; then
      progress_update \
        --phase "checksumming" \
        --message "Comparing RAW checksums in $scope (scope $checksum_scope_number of $checksum_scope_total)." \
        --direction "Local ↔ SSD"
      printf 'Scope: raw/%s\n' "$scope" >> "$local_to_external_raw"
      rsync -rlt --checksum --dry-run --existing --itemize-changes --out-format='%i %n%L' \
        "${PHOTO_RSYNC_RAW_FILTERS[@]}" "$local_scope/" "$external_scope/" >> "$local_to_external_raw"
      printf 'Scope: raw/%s\n' "$scope" >> "$external_to_local_raw"
      rsync -rlt --checksum --dry-run --existing --itemize-changes --out-format='%i %n%L' \
        "${PHOTO_RSYNC_RAW_FILTERS[@]}" "$external_scope/" "$local_scope/" >> "$external_to_local_raw"
    fi
  done < "$raw_scopes"
  # The expensive full pass completed even if it found conflicts. Record it
  # now so retries fall back to metadata candidates until the next interval.
  printf '%s\n' "$now" > "$full_check_marker"
else
  sort -u "$local_to_external_raw_candidates" > "$local_to_external_raw_candidates.sorted"
  mv "$local_to_external_raw_candidates.sorted" "$local_to_external_raw_candidates"
  sort -u "$external_to_local_raw_candidates" > "$external_to_local_raw_candidates.sorted"
  mv "$external_to_local_raw_candidates.sorted" "$external_to_local_raw_candidates"
  if [ -s "$local_to_external_raw_candidates" ]; then
    rsync -rlt --checksum --dry-run --existing --itemize-changes --out-format='%i %n%L' \
      --files-from="$local_to_external_raw_candidates" \
      "${PHOTO_RSYNC_RAW_FILTERS[@]}" \
      "$local_raw/" "$external_raw/" > "$local_to_external_raw"
  fi
  if [ -s "$external_to_local_raw_candidates" ]; then
    rsync -rlt --checksum --dry-run --existing --itemize-changes --out-format='%i %n%L' \
      --files-from="$external_to_local_raw_candidates" \
      "${PHOTO_RSYNC_RAW_FILTERS[@]}" \
      "$external_raw/" "$local_raw/" > "$external_to_local_raw"
  fi
fi

progress_update --phase "reviewing-conflicts" --message "Checking same-path files for conflicts."
if grep -Eq '^>f' "$local_to_external_raw" || grep -Eq '^>f' "$external_to_local_raw"; then
  review_root="$report_root/$job_id"
  save_conflict_reports "$review_root"
  progress_update --phase "quarantining-conflicts" --message "Moving conflicting RAW pairs into needs-review."
  if ! collect_raw_conflicts || ! quarantine_raw_conflicts "$review_root"; then
    echo "External sync stopped because RAW conflicts could not be safely quarantined. Review: $review_root" >&2
    exit 1
  fi
  echo "Quarantined conflicting RAW files and continuing external sync. Review: $review_root" >&2
fi

progress_update \
  --phase "copying" \
  --message "Copying the planned RAW union between Local and the SSD." \
  --direction "Local ↔ SSD"
while IFS= read -r scope; do
  local_scope="$local_raw/$scope"
  external_scope="$external_raw/$scope"
  mkdir -p "$local_scope" "$external_scope"
  run_rsync_with_progress "SSD → Local" rsync -rlt --ignore-existing --partial --delay-updates \
    --info=progress2 --out-format='FILE|%i|%l|%n' "${PHOTO_RSYNC_RAW_FILTERS[@]}" \
    "$external_scope/" "$local_scope/"
  run_rsync_with_progress "Local → SSD" rsync -rlt --ignore-existing --partial --delay-updates \
    --info=progress2 --out-format='FILE|%i|%l|%n' "${PHOTO_RSYNC_RAW_FILTERS[@]}" \
    "$local_scope/" "$external_scope/"
done < "$raw_scopes"

progress_update --phase "finalizing" --message "Finalizing the sync and preparing to unmount the SSD." --clear-current-file
rm -rf "$temp_report_root"
