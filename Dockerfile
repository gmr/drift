ARG RUST_VERSION=1.96
ARG ALPINE_VERSION=3.22

FROM rust:${RUST_VERSION}-alpine${ALPINE_VERSION} AS builder

# musl-dev supplies the C runtime the linker needs; the crate itself is pure Rust.
RUN apk add --no-cache musl-dev

WORKDIR /src

# Cache the dependency build: it only changes when the manifests do.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src

COPY src ./src
# Cargo skips a rebuild when only the mtime changed, so make the real main.rs newer.
RUN touch src/main.rs \
    && cargo build --release --locked \
    && strip target/release/drift

FROM alpine:${ALPINE_VERSION}

COPY --from=builder /src/target/release/drift /usr/local/bin/drift

# The binary is static, so it also works copied into any other base image with
# COPY --from=ghcr.io/gmr/drift:latest /usr/local/bin/drift /usr/local/bin/drift
ENTRYPOINT ["/usr/local/bin/drift"]
