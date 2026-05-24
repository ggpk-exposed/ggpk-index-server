FROM rust:bookworm AS builder

WORKDIR /code
RUN cargo init
COPY Cargo.toml /code/Cargo.toml
RUN cargo fetch
COPY . /code
RUN cargo build --release --offline

# Build the index
RUN mkdir -p /data/index && INDEX_DIR=/data/index BUILD_INDEX=1 ./target/release/ggpk-index-server

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /code/target/release/ggpk-index-server /serve
COPY --from=builder /data/index /data/index

ENV INDEX_DIR=/data/index
ENV READ_ONLY=1
ENV PORT=8080

EXPOSE 8080

CMD [ "/serve" ]
