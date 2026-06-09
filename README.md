# careerbot

A single-user local background daemon that monitors company careers sites for
jobs matching your resume, driven by an agentic LLM harness (Claude Code,
Anthropic API, others later).

State lives on your own machine — SQLite plus flat files under
`~/.local/share/careerbot/` (or `%LOCALAPPDATA%\careerbot\` on Windows). The
daemon spawns short-lived per-company collector scripts on a schedule and
surfaces matches through OS-native notifications.

## Status

Active rewrite. The daemon is alive: `careerbot start-service` runs in
the foreground, talking to the CLI over a Unix socket under
`$XDG_RUNTIME_DIR` (Linux) or `~/.local/state/careerbot/` (macOS).
`careerbot status` reports liveness, `careerbot stop-service`
gracefully shuts it down, and `careerbot run-now [company]` triggers a
tick out of schedule. Behind the scenes a per-company scheduler ticks
each `scripts/*.py` collector on its `service.poll_interval_hours`
cadence with jitter, dedups against the SQLite jobs table, and fires
an OS-native notification per company-tick when there are new matches.
Agent-driven commands round out the loop: `careerbot profile
--from-resume <path>` and `careerbot add-company <name> [url]` go
through one of two drivers — `agent.driver = anthropic_api` (Anthropic
API key in config) or `agent.driver = claude_code` (spawns the user's
locally-installed `claude` CLI and exposes careerbot's tools to it
through `careerbot mcp-server` over MCP stdio). Inventory commands
`list-companies` and `remove-company` round out the deterministic
surface. The prior multi-tenant SaaS implementation lives on the
`legacy-saas` tag.

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
