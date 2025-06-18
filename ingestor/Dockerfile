ARG RUST_IMAGE=rust:latest

FROM ${RUST_IMAGE} AS builder
WORKDIR /app
COPY . .
ENV SQLX_OFFLINE=true
RUN cargo build --locked --release -p ingestor

FROM debian:bookworm-slim
RUN useradd -m appuser
COPY --from=builder /app/target/release/ingestor /usr/local/bin/ingestor
USER appuser
ENTRYPOINT [ "ingestor" ]
