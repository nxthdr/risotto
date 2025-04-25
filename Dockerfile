FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
RUN apt-get update \
    && apt-get install -y capnproto \
    && rm -rf /var/lib/apt/lists/*
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin risotto

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y openssl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/risotto /app/risotto

EXPOSE 3000
EXPOSE 4000

ENTRYPOINT [ "/app/risotto" ]
CMD [ "--help" ]
