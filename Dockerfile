ARG RUST_VERSION=1
FROM rust:${RUST_VERSION} AS chef
WORKDIR /app
RUN cargo install cargo-chef
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching layer!
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
ARG PACKAGE=skuffen
RUN cargo build --release --package ${PACKAGE}

FROM gcr.io/distroless/cc-debian12:nonroot
ARG PACKAGE=skuffen
COPY --from=builder --chown=nonroot:nonroot /app/target/release/${PACKAGE} /usr/local/bin/${PACKAGE}
CMD ["/usr/local/bin/skuffen"]
