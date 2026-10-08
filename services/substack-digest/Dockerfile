FROM rust:1.98.0-bookworm@sha256:4e4a7e7939c17991ab35f2b8c2e67593980f771d28f6b1254b1850f860fd0c7f AS builder
RUN rustup component add rustfmt clippy
WORKDIR /build
COPY rust ./rust
COPY fixtures ./fixtures
RUN cargo fmt --manifest-path rust/Cargo.toml --all -- --check \
    && cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- --deny warnings \
    && cargo build --release --locked --manifest-path rust/Cargo.toml -p substack-digest \
    && cargo test --locked --manifest-path rust/Cargo.toml -p substack-digest

# Chromium and Node from the official browser image; no Python service runtime.
FROM mcr.microsoft.com/playwright:v1.55.0-noble@sha256:ffc33305f7b4b04057ae4a0caa70aad4fde87454fb403a1a22e7f931707dfcf9
USER root
WORKDIR /opt/uploader
COPY package.json package-lock.json ./
RUN npm ci --ignore-scripts --omit=dev --no-audit --no-fund \
    && ln -s /opt/uploader/node_modules/.bin/rmapi-js /usr/local/bin/rmapi-js \
    && rmapi-js --version \
    && browser="$(find /ms-playwright -type f -name chrome -print -quit)" \
    && test -n "$browser" && ln -s "$browser" /usr/local/bin/chromium
COPY --from=builder /build/rust/target/release/substack-digest /usr/local/bin/substack-digest
ENV RMAPI_CONFIG_DIR=/data/rmapi NO_COLOR=1 HOME=/tmp
USER 1000:100
ENTRYPOINT ["substack-digest"]
CMD ["--config", "/config/config.json"]
