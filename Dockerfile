ARG RUST_VERSION=1.91

FROM rust:${RUST_VERSION}-bookworm AS chef
WORKDIR /app

# Use an older cargo-chef *and* its lockfile
RUN cargo install cargo-chef --version 0.1.73 --locked

FROM chef AS planner
COPY . .
RUN sed -i '/crates\/adr-fmt/d' Cargo.toml \
    && ! grep -q 'crates/adr-fmt' Cargo.toml \
    && cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
ARG PACKAGE=skuffen
RUN sed -i '/crates\/adr-fmt/d' Cargo.toml \
    && ! grep -q 'crates/adr-fmt' Cargo.toml \
    && cargo build --release --package ${PACKAGE}

FROM gcr.io/distroless/cc-debian12:nonroot
ARG PACKAGE=skuffen
COPY --from=builder --chown=nonroot:nonroot /app/target/release/${PACKAGE} /usr/local/bin/${PACKAGE}
CMD ["/usr/local/bin/skuffen"]
