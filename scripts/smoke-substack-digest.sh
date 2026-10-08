#!/usr/bin/env bash
set -euo pipefail
image=${1:?usage: smoke-substack-digest.sh IMAGE}
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output=$(mktemp -d)
trap 'rm -rf "$output"' EXIT
chmod 777 "$output"
docker run --rm --network none --read-only --cap-drop ALL \
  --security-opt no-new-privileges:true --shm-size 256m \
  --tmpfs /tmp:size=512m,mode=1777 \
  -v "$repo_root/services/substack-digest/fixtures:/config:ro" \
  -v "$output:/output" \
  "$image" --smoke-test /output/sample.pdf
bash "$repo_root/services/substack-digest/check-sample.sh" "$output/sample.pdf"
