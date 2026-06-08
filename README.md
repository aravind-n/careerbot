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
--help` lists every subcommand — and the first two agent-driven
commands are now real: `careerbot profile --from-resume <path>` ingests
a resume into `profile.md`, and `careerbot add-company <name> [url]`
generates a per-company Python collector and runs save-time
verification on it. Both go through the Anthropic `/v1/messages`
driver, which needs `agent.driver = anthropic_api` and an API key set
via `careerbot config`. The daemon scheduler, notification surface,
MCP server, and Claude Code driver are still pending; the affected
subcommands print "not implemented yet". The prior multi-tenant SaaS
implementation lives on the `legacy-saas` tag.

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
