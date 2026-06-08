# careerbot

A single-user local background daemon that monitors company careers sites for
jobs matching your resume, driven by an agentic LLM harness (Claude Code,
Anthropic API, others later).

State lives on your own machine — SQLite plus flat files under
`~/.local/share/careerbot/` (or `%LOCALAPPDATA%\careerbot\` on Windows). The
daemon spawns short-lived per-company collector scripts on a schedule and
surfaces matches through OS-native notifications.

## Status

Active rewrite. The binary currently builds but exposes no subcommands; the
agent harness, scheduler, and notification surface are being landed
incrementally. The prior multi-tenant SaaS implementation lives on the
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
