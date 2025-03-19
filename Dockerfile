# does this first one need to be 1-bookworm-slim?
FROM rust:1 AS chef
RUN cargo install cargo-chef 
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder 
COPY --from=planner /app/recipe.json recipe.json

RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
RUN cargo build --release --features server


FROM debian:bookworm-slim AS runtime
WORKDIR /app
RUN apt-get update && apt-get install -y libssl-dev ca-certificates && apt clean && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/beam /usr/local/bin
ENV LOGLEVEL debug
ENV CONFIG /bytebeam.toml
ENTRYPOINT ["/usr/local/bin/beam", "server"]