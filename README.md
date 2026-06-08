# careerbot

A single-user local background daemon that monitors company careers sites for
jobs matching your resume, driven by an agentic LLM harness (Claude Code,
Anthropic API, others later).

State lives on your own machine — SQLite plus flat files under
`~/.local/share/careerbot/` (or `%LOCALAPPDATA%\careerbot\` on Windows). The
daemon spawns short-lived per-company collector scripts on a schedule and
surfaces matches through OS-native notifications.

## Status

Active rewrite. The CLI surface from the plan is wired up — `careerbot
--help` lists every subcommand — but most handlers still print "not
implemented yet" while the agent harness, scheduler, and notification
surface land incrementally. End-to-end so far: `careerbot config` reads
and writes `config.toml` (under the XDG-compliant data directory) and
the SQLite schema is created on first daemon start. The prior
multi-tenant SaaS implementation lives on the `legacy-saas` tag.

## Layout

```
careerbot-core/   library — config, storage, tools, agent drivers, daemon logic
careerbot/        binary — thin CLI shell over careerbot-core
```

## Build and run

```bash
cargo build --workspace
cargo run -- --help
```

Logs are JSON via `tracing`; control level with `RUST_LOG`
(e.g. `RUST_LOG=careerbot=debug`).

## License

MIT — see [LICENSE](LICENSE).
