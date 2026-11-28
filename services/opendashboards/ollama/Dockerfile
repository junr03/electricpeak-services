FROM ollama/ollama@sha256:1cabd20e1762a7a8444bf4a7110b346d923cd77ab6c97c984434140638c913ec
COPY entrypoint.sh /usr/local/bin/ollama-start
ENTRYPOINT ["/bin/sh", "/usr/local/bin/ollama-start"]
HEALTHCHECK --interval=5s --timeout=5s --retries=20 CMD test -f /tmp/ollama-ready && ollama list >/dev/null
