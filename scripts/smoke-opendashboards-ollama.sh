#!/usr/bin/env bash
set -euo pipefail
image=${1:?usage: smoke-opendashboards-ollama.sh IMAGE}
fixture=$(mktemp -d)
container="ollama-bootstrap-smoke-$$"
cleanup() {
  docker rm -f "$container" >/dev/null 2>&1 || true
  rm -rf "$fixture"
}
trap cleanup EXIT
chmod 755 "$fixture"
mkdir "$fixture/bin" "$fixture/cache"
# A CLI fixture makes startup, caching and failure/signal handling repeatable
# without downloading weights or replacing the real image's entrypoint.
cat > "$fixture/bin/ollama" <<'SH'
#!/bin/sh
set -eu
case "$1" in
  serve)
    trap 'touch /fixture/server-stopped; exit 0' TERM INT
    touch /fixture/server-started
    while :; do sleep 1; done ;;
  list) test -f /fixture/server-started ;;
  show) test -f "/fixture/cache/$2" ;;
  pull)
    printf '%s\n' "$2" >> /fixture/pulls
    [ "$2" != broken-model ] || exit 42
    touch "/fixture/cache/$2" ;;
  *) exit 2 ;;
esac
SH
chmod 755 "$fixture/bin/ollama"
start() {
  rm -f "$fixture/server-started" "$fixture/server-stopped"
  docker run -d --name "$container" --network none --user "$(id -u):$(id -g)" \
    --read-only --cap-drop ALL --security-opt no-new-privileges:true --tmpfs /tmp \
    -e PATH=/fixture/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    -e "OLLAMA_INITIAL_MODELS=$1" -v "$fixture:/fixture" "$image" >/dev/null
}
wait_ready() {
  for _ in {1..30}; do
    if docker exec "$container" sh -c 'test -f /tmp/ollama-ready && ollama list'; then return; fi
    if [[ $(docker inspect "$container" --format '{{.State.Running}}') != true ]]; then break; fi
    sleep 1
  done
  docker logs "$container"
  return 1
}
start 'fixture-chat fixture-dashboard'
wait_ready
[[ $(cat "$fixture/pulls") == $'fixture-chat\nfixture-dashboard' ]]
docker stop --time 5 "$container" >/dev/null
[[ -f "$fixture/server-stopped" ]]
docker rm "$container" >/dev/null
before=$(sha256sum "$fixture/pulls")
start 'fixture-chat fixture-dashboard'
wait_ready
[[ "$before" == "$(sha256sum "$fixture/pulls")" ]]
docker stop --time 5 "$container" >/dev/null
docker rm "$container" >/dev/null
start broken-model
[[ $(docker wait "$container") == 42 ]]
[[ -f "$fixture/server-stopped" ]]
echo 'Ollama container: model provisioning, cache retention, failure propagation and signal forwarding passed'
