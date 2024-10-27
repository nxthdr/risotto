FROM rust:latest

COPY ./ ./

RUN cargo build

EXPOSE 3000
EXPOSE 4000

CMD ["./target/debug/risotto"]