ARG RUST_IMAGE=rust:latest

FROM ${RUST_IMAGE} AS builder
WORKDIR /app
COPY . .
ENV SQLX_OFFLINE=true
RUN cargo build --locked --release -p api

FROM debian:bookworm-slim
RUN useradd -m appuser
COPY --from=builder /app/target/release/api /usr/local/bin/api
USER appuser
ENTRYPOINT [ "api" ]
