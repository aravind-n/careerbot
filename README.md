# CareerBot

CareerBot is a job notification engine written in Rust. It consists of three microservices:

- `ingestor`: collects job listings
- `notifier`: notifies users of relevant jobs
- `api`: provides a REST API interface

---

## 🚀 Quick Start (Recommended)

1. **Clone the repo**

```bash
git clone https://gitlab.com/aravind/careerbot.git
cd careerbot
```

2. **Create your environment config**

```bash
cp .env.example .env
```

Edit `.env` if needed. Default values will work for local development.

3. **Start the full system with Docker Compose**

```bash
docker compose up -d
```

This will:
- Build all three services (`ingestor`, `notifier`, `api`)
- Start a Postgres database (`postgres:17`)
- Start a Redis stream (`redis:8`)
---

## 🛠️ Local Development

You can also run services manually using Cargo:

```bash
# Run a single service locally
cargo run --bin ingestor
```

Make sure Postgres is running (you can use Docker Compose for that).

To run SQLx migrations manually:

```bash
cd shared
DATABASE_URL=postgres://... cargo sqlx migrate run
```

> Requires `sqlx-cli`: `cargo install sqlx-cli --no-default-features --features postgres`  
> Set `host` in `DATABASE_URL` to `localhost`

---

## 🐳 Building Containers Individually

If needed, you can build a single service:

```bash
docker compose build ingestor
```

Or all services:

```bash
docker compose build
```

---

## 🔧 Environment Configuration

All services use environment variables defined in `.env`. Here's a sample:

```bash
POSTGRES_USER=postgres
POSTGRES_PASSWORD=postgres
POSTGRES_DB=careerbot
DATABASE_URL=postgres://postgres:postgres@postgres:5432/careerbot
RUST_LOG=info
```

See [`tracing_subscriber::EnvFilter`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) for valid `RUST_LOG` values and filters.

---

## 🧪 SQLx Setup

Compile-time query checking is enabled via SQLx offline mode.

To prepare new queries (when they change), run:

```bash
cd shared
DATABASE_URL=postgres://... cargo sqlx prepare --workspace
```

This updates the `.sqlx/` folder used by Docker builds.

---

## 🤝 Contributing

Pull requests are welcome! For major changes, please open an issue first to discuss.

Make sure to:
- Format your code (`cargo fmt`)
- Run tests (`cargo test`)
- Update SQLx metadata if you modify queries

---

## 📄 License

[MIT](https://choosealicense.com/licenses/mit/)
