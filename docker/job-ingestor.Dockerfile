ARG RUST_IMAGE=rust:latest

FROM ${RUST_IMAGE} AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p job-ingestor

FROM debian:bookworm-slim
RUN useradd -m appuser
COPY --from=builder /app/target/release/job-ingestor /usr/local/bin/job-ingestor
USER appuser
ENTRYPOINT [ "job-ingestor" ]
