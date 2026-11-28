#!/bin/sh
set -eu

# Ollama serves models; this wrapper provisions only missing configured weights.
# Keep the API unready until provisioning succeeds. Cached weights are retained.
: "${OLLAMA_INITIAL_MODELS:?Set OLLAMA_INITIAL_MODELS to the models to provision}"
rm -f /tmp/ollama-ready
ollama serve &
server_pid=$!
cleanup() {
  kill -TERM "$server_pid" 2>/dev/null || true
  wait "$server_pid" 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 143' TERM
trap 'exit 130' INT

ready=false
for _ in $(seq 1 90); do
  if ollama list >/dev/null 2>&1; then ready=true; break; fi
  kill -0 "$server_pid" 2>/dev/null || { wait "$server_pid"; exit 1; }
  sleep 1
done
[ "$ready" = true ] || { echo 'Ollama did not become ready' >&2; exit 1; }
for model in $OLLAMA_INITIAL_MODELS; do
  if ! ollama show "$model" >/dev/null 2>&1; then
    ollama pull "$model"
  fi
done
touch /tmp/ollama-ready
wait "$server_pid"
