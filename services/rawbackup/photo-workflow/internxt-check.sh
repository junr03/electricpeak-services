# shellcheck disable=SC1091
source @FILE_TYPES@

job_id="internxt-check-$(date -u +%Y%m%dT%H%M%SZ)"
progress_finished=false

finish() {
  local exit_code=$?
  local result phase message
  if [ "$progress_finished" = false ]; then
    if [ "$exit_code" -eq 0 ]; then
      result="success"
      phase="completed"
      message="Internxt verification completed successfully."
    else
      result="failed"
      phase="failed"
      message="Internxt verification found an error or mismatch."
    fi
    @PROGRESS@ finish --job-id "$job_id" --result "$result" --phase "$phase" --message "$message" >/dev/null 2>&1 || true
  fi
  trap - EXIT
  exit "$exit_code"
}

@PROGRESS@ start \
  --job-id "$job_id" \
  --kind "internxt-check" \
  --title "Verify Local ↔ Internxt" \
  --phase "waiting-for-lock" \
  --message "Waiting for other photo jobs to finish." \
  --direction "Local ↔ Internxt" >/dev/null 2>&1 || true
trap finish EXIT

exec 9>/run/lock/photo-workflow.lock
flock 9
@PROGRESS@ update --job-id "$job_id" --phase "verifying" --message "Checking every RAW and sidecar file against Internxt." >/dev/null 2>&1 || true
@PROGRESS@ rclone --job-id "$job_id" --direction "Local ↔ Internxt" -- rclone check "@LOCAL_ROOT@/raw" "internxt:@INTERNXT_REMOTE_PATH@/raw" \
  --config "@INTERNXT_CONFIG@" \
  --one-way \
  --checkers 2 \
  --tpslimit 10 \
  --retries 5 \
  --low-level-retries 20 \
  --timeout 1m \
  --contimeout 15s \
  "${PHOTO_RCLONE_ALL_FILTERS[@]}" \
  --use-json-log \
  --stats 1s \
  --stats-one-line \
  --stats-log-level NOTICE \
  --log-level NOTICE
if @PROGRESS@ finish --job-id "$job_id" --result success --phase completed --message "Internxt verification completed successfully." >/dev/null 2>&1; then
  progress_finished=true
fi
