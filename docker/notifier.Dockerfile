ARG RUST_IMAGE=rust:latest

FROM ${RUST_IMAGE} AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p notifier

FROM debian:bookworm-slim
RUN useradd -m appuser
COPY --from=builder /app/target/release/notifier /usr/local/bin/notifier
USER appuser
ENTRYPOINT [ "notifier" ]
