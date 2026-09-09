# Build image for superquery-node.
#
# Two stages: a builder that compiles the workspace, and a minimal runtime that
# carries only the binary. The dependency layer is cached separately from the
# source so an ordinary code change does not rebuild alloy and wasmtime.

FROM rust:1-slim-bookworm AS builder

WORKDIR /build

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Manifests first, so the dependency build layer is cached independently of the
# source tree.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/chain-api/Cargo.toml       crates/chain-api/
COPY crates/config/Cargo.toml          crates/config/
COPY crates/store/Cargo.toml           crates/store/
COPY crates/core/Cargo.toml            crates/core/
COPY crates/dispatcher/Cargo.toml      crates/dispatcher/
COPY crates/runtime/Cargo.toml         crates/runtime/
COPY crates/chains/evm/Cargo.toml      crates/chains/evm/
COPY bins/superquery-node/Cargo.toml   bins/superquery-node/
COPY tests/Cargo.toml                  tests/

# Stub sources so cargo can resolve and build the dependency graph.
RUN mkdir -p crates/chain-api/src crates/config/src crates/store/src crates/core/src \
             crates/dispatcher/src crates/runtime/src crates/chains/evm/src \
             bins/superquery-node/src tests \
    && for c in chain-api config store core dispatcher runtime chains/evm; do \
         echo "" > crates/$c/src/lib.rs; \
       done \
    && echo "" > tests/lib.rs \
    && echo "fn main() {}" > bins/superquery-node/src/main.rs \
    && cargo build --release --bin superquery-node \
    && rm -rf crates bins tests

COPY . .

# Touch the real sources so cargo does not reuse the stub artefacts.
RUN find crates bins tests -name '*.rs' -exec touch {} + \
    && cargo build --release --bin superquery-node


FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --uid 10001 superquery

COPY --from=builder /build/target/release/superquery-node /usr/local/bin/superquery-node

# Unprivileged: the indexer needs a network socket and a database, nothing else.
USER superquery
WORKDIR /home/superquery

# The admin surface (guide Milestone 13).
EXPOSE 3000

ENTRYPOINT ["superquery-node"]
CMD ["--help"]
