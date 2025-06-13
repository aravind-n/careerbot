ARG RUST_IMAGE=rust:latest

FROM ${RUST_IMAGE} as builder
WORKDIR /app
COPY . .
RUN cargo build --release -p api

FROM debian:bookworm-slim
RUN useradd -m appuser
COPY --from=builder /app/target/release/api /usr/local/bin/api
USER appuser
ENTRYPOINT [ "api" ]
