FROM rust:bookworm AS builder

WORKDIR /code
RUN cargo init
COPY Cargo.toml /code/Cargo.toml
RUN cargo fetch
COPY . /code
RUN cargo build --release --offline

FROM debian:bookworm-slim

EXPOSE 3000

COPY --from=builder /code/target/release/ggpk-index-server /serve

CMD [ "/serve" ]
