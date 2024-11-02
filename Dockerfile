FROM rust:latest AS builder

WORKDIR /app

COPY ./ ./

RUN cargo build --release

FROM rust:latest

COPY --from=builder /app/target/release/risotto /app/risotto
COPY example/risotto.yml /config/risotto.yml

EXPOSE 3000
EXPOSE 4000

ENTRYPOINT [ "/app/risotto" ]
CMD [ "--help" ]