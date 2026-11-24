FROM rust:1.98.0-bookworm@sha256:4e4a7e7939c17991ab35f2b8c2e67593980f771d28f6b1254b1850f860fd0c7f AS builder
RUN rustup component add rustfmt clippy
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY examples ./examples
COPY tests ./tests
RUN cargo fmt --all -- --check \
    && cargo clippy --all-targets --locked -- --deny warnings \
    && cargo test --locked \
    && cargo build --release --locked

# Pure Rust PostgreSQL protocol support: the binary only needs glibc/libgcc.
FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f
COPY --from=builder /build/target/release/transaction-import /usr/local/bin/transaction-import
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/transaction-import"]
CMD ["--help"]
