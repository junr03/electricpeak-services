#!/usr/bin/env bash
# Reconcile the photo-library union across local storage, the external SSD,
# and Internxt. This is intentionally a manual operation: it only creates
# missing files, never deletes or overwrites existing ones.

set -o pipefail
# shellcheck disable=SC1091
source @FILE_TYPES@

local_root="${PHOTO_WORKFLOW_LOCAL_ROOT:?PHOTO_WORKFLOW_LOCAL_ROOT is required}"
mount_root="${PHOTO_WORKFLOW_MOUNT_POINT:?PHOTO_WORKFLOW_MOUNT_POINT is required}"
external_root="$mount_root/${PHOTO_WORKFLOW_DESTINATION_SUBDIRECTORY?PHOTO_WORKFLOW_DESTINATION_SUBDIRECTORY is required}"
external_raw_subdirectory="${PHOTO_WORKFLOW_EXTERNAL_RAW_SUBDIRECTORY:?PHOTO_WORKFLOW_EXTERNAL_RAW_SUBDIRECTORY is required}"
external_device="/dev/disk/by-uuid/${PHOTO_WORKFLOW_FILESYSTEM_UUID:?PHOTO_WORKFLOW_FILESYSTEM_UUID is required}"
external_mount_unit="${PHOTO_WORKFLOW_EXTERNAL_MOUNT_UNIT:?PHOTO_WORKFLOW_EXTERNAL_MOUNT_UNIT is required}"
internxt_config="${PHOTO_WORKFLOW_INTERNXT_CONFIG:?PHOTO_WORKFLOW_INTERNXT_CONFIG is required}"
internxt_remote_path="${PHOTO_WORKFLOW_INTERNXT_REMOTE_PATH:?PHOTO_WORKFLOW_INTERNXT_REMOTE_PATH is required}"
state_dir="${PHOTO_WORKFLOW_RECONCILE_STATE_DIR:?PHOTO_WORKFLOW_RECONCILE_STATE_DIR is required}"
request_dir="$state_dir/requests"
status_path="$state_dir/status.json"
job_id="reconcile-$(date -u +%Y%m%dT%H%M%SZ)"
report_dir="$state_dir/reports/$job_id"
lock_file="/run/lock/photo-workflow.lock"
started_at="$(date --utc --iso-8601=seconds)"
total_steps=10
current_phase="preparing"
current_step=0
finalized=false
progress_finished=false

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

run_rclone_with_progress() {
  local direction="$1"
  shift
  @PROGRESS@ rclone --job-id "$job_id" --direction "$direction" -- "$@"
}

inventory() {
  local root="$1"
  if [ ! -d "$root" ]; then
    printf '%s' '{"fileCount":0,"bytes":0}'
    return
  fi
  photo_find_files "$root" all -printf '%s\n' \
    | awk '{ count += 1; bytes += $1 } END { printf "{\"fileCount\":%d,\"bytes\":%.0f}", count, bytes }'
}

write_status() {
  local state="$1"
  local result="$2"
  local message="$3"
  local phase="${4:-$current_phase}"
  local step="${5:-$current_step}"
  local local_inventory="${6:-null}"
  local ssd_inventory="${7:-null}"
  local internxt_inventory="${8:-null}"
  local finished_at="null"
  local temporary_status
  if [ "$state" != running ]; then
    finished_at="$(jq -Rn --arg value "$(date --utc --iso-8601=seconds)" '$value')"
  fi
  temporary_status="$(mktemp "$state_dir/status.json.XXXXXX")"
  jq -n \
    --arg state "$state" \
    --arg result "$result" \
    --arg message "$message" \
    --arg startedAt "$started_at" \
    --arg updatedAt "$(date --utc --iso-8601=seconds)" \
    --arg phase "$phase" \
    --argjson step "$step" \
    --argjson totalSteps "$total_steps" \
    --argjson finishedAt "$finished_at" \
    --arg reportPath "$report_dir" \
    --argjson local "$local_inventory" \
    --argjson ssd "$ssd_inventory" \
    --argjson internxt "$internxt_inventory" \
    '{
      state: $state,
      result: $result,
      message: $message,
      startedAt: $startedAt,
      updatedAt: $updatedAt,
      phase: $phase,
      step: $step,
      totalSteps: $totalSteps,
      finishedAt: $finishedAt,
      reportPath: $reportPath,
      inventories: { local: $local, ssd: $ssd, internxt: $internxt }
    }' > "$temporary_status"
  chmod 0644 "$temporary_status"
  mv "$temporary_status" "$status_path"
  if [ "$state" != running ]; then
    finalized=true
  fi
}

# Every state change is persisted for the dashboard and written to stderr so
# `journalctl -u photo-workflow-reconcile` shows the same live progress.
update_progress() {
  current_phase="$1"
  current_step="$2"
  local message="$3"
  printf '%s reconciliation [%s %s/%s] %s\n' \
    "$(date --utc --iso-8601=seconds)" "$current_phase" "$current_step" "$total_steps" "$message" >&2
  @PROGRESS@ update \
    --job-id "$job_id" \
    --phase "$current_phase" \
    --message "$message" \
    --step "$current_step" \
    --total-steps "$total_steps" \
    --reset-progress \
    --files-completed 0 \
    --bytes-completed 0 >/dev/null 2>&1 || true
  write_status "running" "running" "$message" "$current_phase" "$current_step"
}

fail() {
  local message="$1"
  printf '%s reconciliation [%s %s/%s] %s\n' \
    "$(date --utc --iso-8601=seconds)" "$current_phase" "$current_step" "$total_steps" "$message" >&2
  write_status "failed" "failed" "$message" "$current_phase" "$current_step"
  progress_finish "failed" "failed" "$message"
  exit 1
}

refresh_dashboard_status() {
  systemctl start --no-block rawbackup-status.service >/dev/null 2>&1 || true
}

finish() {
  local exit_code=$?
  if [ "$finalized" = false ]; then
    write_status "failed" "failed" "Reconciliation stopped before it completed. Review $report_dir." "$current_phase" "$current_step"
  fi
  if [ "$progress_finished" = false ]; then
    progress_finish "failed" "failed" "Reconciliation stopped before it completed."
  fi
  refresh_dashboard_status
  trap - EXIT
  exit "$exit_code"
}
trap finish EXIT

mkdir -p "$request_dir" "$report_dir" /run/lock
@PROGRESS@ start \
  --job-id "$job_id" \
  --kind "reconciliation" \
  --title "Reconcile Local, SSD & Internxt" \
  --phase "preparing" \
  --message "Preparing a no-delete reconciliation." \
  --direction "Local ↔ SSD ↔ Internxt" >/dev/null 2>&1 || true
update_progress "preparing" 0 "Preparing a no-delete reconciliation."
find "$request_dir" -maxdepth 1 -type f -name '*.json' -delete

update_progress "checking-ssd" 1 "Checking that the external SSD is connected and mounted."
if [ ! -e "$external_device" ]; then
  fail "The external SSD is not connected."
fi

if ! mountpoint -q "$mount_root"; then
  if ! systemctl start "$external_mount_unit"; then
    fail "Could not mount the external SSD."
  fi
fi

if ! mountpoint -q "$mount_root"; then
  fail "The external SSD did not mount."
fi

local_raw="$local_root/raw"
external_raw="$external_root/$external_raw_subdirectory"
if [ ! -d "$local_raw" ] || [ ! -d "$external_raw" ]; then
  fail "The local or external RAW directory is missing."
fi

update_progress "waiting-for-lock" 2 "Waiting for other photo-workflow jobs to finish."
exec 9>"$lock_file"
flock 9

# A same-path difference cannot be represented in a no-overwrite union. Stop
# before copying anything from the SSD and leave its detailed reports behind.
update_progress "checking-ssd-conflicts" 3 "Checking RAW and sidecar files for same-path differences."
local_ssd_conflicts="$report_dir/local-to-ssd-conflicts.txt"
ssd_local_conflicts="$report_dir/ssd-to-local-conflicts.txt"
rsync -rcn --existing --itemize-changes --out-format='%i %n%L' \
  "${PHOTO_RSYNC_ALL_FILTERS[@]}" "$local_raw/" "$external_raw/" > "$local_ssd_conflicts"
rsync -rcn --existing --itemize-changes --out-format='%i %n%L' \
  "${PHOTO_RSYNC_ALL_FILTERS[@]}" "$external_raw/" "$local_raw/" > "$ssd_local_conflicts"
if grep -Eq '^>f' "$local_ssd_conflicts" || grep -Eq '^>f' "$ssd_local_conflicts"; then
  message="SSD and local storage contain different files at the same path. Nothing was copied; review $report_dir."
  write_status "needs-review" "conflicts" "$message" "$current_phase" "$current_step"
  progress_finish "needs-review" "needs-review" "$message"
  exit 0
fi

# Copy from every source into local first, then fan the resulting union back
# out. --ignore-existing ensures an existing destination file is never altered.
update_progress "copying-internxt-to-local" 4 "Copying missing RAW and sidecar files from Internxt to local storage."
if ! run_rclone_with_progress "Internxt → Local" rclone copy "internxt:$internxt_remote_path/raw" "$local_raw" \
  --config "$internxt_config" --ignore-existing "${PHOTO_RCLONE_ALL_FILTERS[@]}" \
  --use-json-log --stats 1s --stats-one-line --stats-log-level NOTICE --log-level NOTICE > "$report_dir/internxt-to-local.log" 2>&1; then
  fail "Copy from Internxt to local storage failed; review $report_dir/internxt-to-local.log."
fi
update_progress "copying-ssd-to-local" 5 "Copying missing RAW and sidecar files from the SSD to local storage."
if ! run_rsync_with_progress "SSD → Local" rsync -rlt --ignore-existing --partial --delay-updates \
  --info=progress2 --out-format='FILE|%i|%l|%n' "${PHOTO_RSYNC_ALL_FILTERS[@]}" \
  "$external_raw/" "$local_raw/" > "$report_dir/ssd-to-local.log" 2>&1; then
  fail "Copy from the SSD to local storage failed; review $report_dir/ssd-to-local.log."
fi
update_progress "copying-local-to-ssd" 6 "Copying the local RAW and sidecar union to the SSD."
if ! run_rsync_with_progress "Local → SSD" rsync -rlt --ignore-existing --partial --delay-updates \
  --info=progress2 --out-format='FILE|%i|%l|%n' "${PHOTO_RSYNC_ALL_FILTERS[@]}" \
  "$local_raw/" "$external_raw/" > "$report_dir/local-to-ssd.log" 2>&1; then
  fail "Copy from local storage to the SSD failed; review $report_dir/local-to-ssd.log."
fi
update_progress "copying-local-to-internxt" 7 "Copying the local RAW and sidecar union to Internxt."
if ! run_rclone_with_progress "Local → Internxt" rclone copy "$local_raw" "internxt:$internxt_remote_path/raw" \
  --config "$internxt_config" --ignore-existing --immutable "${PHOTO_RCLONE_ALL_FILTERS[@]}" \
  --use-json-log --stats 1s --stats-one-line --stats-log-level NOTICE --log-level NOTICE > "$report_dir/local-to-internxt.log" 2>&1; then
  fail "Copy from local storage to Internxt failed; review $report_dir/local-to-internxt.log."
fi

update_progress "measuring-inventories" 8 "Measuring the reconciled local, SSD, and Internxt inventories."
local_inventory="$(inventory "$local_raw")"
ssd_inventory="$(inventory "$external_raw")"
internxt_inventory="$(rclone size "internxt:$internxt_remote_path/raw" --config "$internxt_config" "${PHOTO_RCLONE_ALL_FILTERS[@]}" --json \
  | jq '{ fileCount: .count, bytes: .bytes }')"

# Keep a post-run verification report. A collision on Internxt remains intact
# because every copy above ignored existing destination files.
review_needed=false
update_progress "verifying-local-to-internxt" 9 "Verifying that every local RAW and sidecar file is present on Internxt."
if ! run_rclone_with_progress "Local → Internxt" rclone check "$local_raw" "internxt:$internxt_remote_path/raw" \
  --config "$internxt_config" --one-way "${PHOTO_RCLONE_ALL_FILTERS[@]}" \
  --use-json-log --stats 1s --stats-one-line --stats-log-level NOTICE --log-level NOTICE > "$report_dir/local-to-internxt-check.log" 2>&1; then
  review_needed=true
fi
update_progress "verifying-internxt-to-local" 10 "Verifying that every Internxt RAW and sidecar file is present locally."
if ! run_rclone_with_progress "Internxt → Local" rclone check "internxt:$internxt_remote_path/raw" "$local_raw" \
  --config "$internxt_config" --one-way "${PHOTO_RCLONE_ALL_FILTERS[@]}" \
  --use-json-log --stats 1s --stats-one-line --stats-log-level NOTICE --log-level NOTICE > "$report_dir/internxt-to-local-check.log" 2>&1; then
  review_needed=true
fi

if [ "$review_needed" = true ]; then
  message="Missing files were copied, but one or more same-path Internxt differences need review at $report_dir."
  write_status "needs-review" "conflicts" "$message" "$current_phase" "$current_step" "$local_inventory" "$ssd_inventory" "$internxt_inventory"
  progress_finish "needs-review" "needs-review" "$message"
else
  message="All missing RAW files and sidecars were copied without deleting or overwriting files."
  write_status "completed" "success" "$message" "$current_phase" "$current_step" "$local_inventory" "$ssd_inventory" "$internxt_inventory"
  progress_finish "success" "completed" "$message"
fi
