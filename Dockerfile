FROM rust:latest AS builder

RUN apt-get update \
    && apt-get install -y capnproto \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY . .

RUN cargo install --path ./risotto

FROM debian:stable-slim

RUN apt-get update \
    && apt-get install -y openssl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/cargo/bin/risotto /app/risotto

EXPOSE 3000
EXPOSE 4000

ENTRYPOINT [ "/app/risotto" ]
CMD [ "--help" ]
