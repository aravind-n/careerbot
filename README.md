# careerbot

Single-user local daemon. Checks company careers sites for jobs
matching your resume on a per-company schedule, dedupes locally, and
fires OS notifications when new matches show up. Driven by Claude
Code or the Anthropic API.

## Quick start

```bash
cargo install --path careerbot
careerbot init           # pick driver, ingest resume, add a first company
careerbot start-service  # run the daemon in the foreground
```

`careerbot --help` lists the full subcommand surface.

## State

Everything lives under `~/.local/share/careerbot/` on Linux and macOS
(`%LOCALAPPDATA%\careerbot\` on Windows): `config.toml`, `profile.md`,
per-company collector scripts, SQLite. Logs go to stderr as JSON —
control level with `RUST_LOG`.

## Layout

```
careerbot-core/   library — config, storage, tools, drivers, daemon
careerbot/        binary — thin CLI shell
```

## License

MIT — see [LICENSE](LICENSE).
