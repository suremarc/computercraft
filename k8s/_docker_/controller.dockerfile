FROM rust:1.89-bookworm AS base

RUN apt-get -qq update && apt-get -y -qq install mold

FROM base AS builder
WORKDIR /tmp/computercraft

ARG RUSTFLAGS="-C link-arg=-fuse-ld=mold"

COPY crates/ crates/
COPY ./Cargo.* .

ARG RELEASE_BUILD=

RUN cargo build ${RELEASE_BUILD:+--release} --bin controller

FROM debian:bookworm
WORKDIR /opt/computercraft

COPY --from=builder /tmp/computercraft/target/*/controller ./bin/

ENTRYPOINT ["/opt/computercraft/bin/controller"]
