# careerbot

An ultra-fast, agentic, local-first job monitoring daemon, written in Rust.

Job demand is at an all time high. New postings get hundereds of applications
within the first few hours. careerbot helps you get an edge over the competition
by notifying you of the newest openings matching your profile as soon as they
go live. It agentically generates per-company scrapers fitted perfectly to
your resume and fires an OS notification as soon as a new match is posted ensuring
you can be one of the first profiles seen by companies.

Stores all it's state files and user information locally. User data
lives under `~/.local/share/careerbot/`.

## Highlights

- **Native daemon.** A low-overhead per-company scheduler in async
  Rust; idle cost is negligible.
- **Local-first.** No SaaS, no account, no shared data. Config,
  profile, filters, matched jobs, and generated scrapers all live on
  disk under your home directory.
- **Agent-authored collectors.** Your configured agent writes a Python scraper per
  company on demand. Adding a company is one CLI command.
  - Currently supports claude-code and Anthropic API keys. Support for other agents
    and local LLM is actively being worked on.
- **OS-native notifications.** Linux and macOS desktop notifications,
  hourly polling, configurable jitter.

## Requirements

- [uv](https://github.com/astral-sh/uv) on `PATH` — runs the Python
  collector scripts the agent generates.
- LLM Access
  - An [Anthropic API key][api-key]
  - `claude` CLI on `PATH`.
  - Support for openAI/gemini/Ollama models in active development

[api-key]: https://console.anthropic.com/settings/keys

## Installation

```bash
curl -fsSL https://aravind-n.github.io/careerbot/install.sh | sh
```

Need a different version? Check the [releases page](https://github.com/aravind-n/careerbot/releases)  
To build from source, see *Building from source* below.

## Getting started

```bash
careerbot init           # interactive: driver, resume, first company
careerbot start-service  # run the daemon in the foreground
careerbot status         # snapshot, from another terminal
```

`careerbot --help` lists the full command surface.

## Configuration

Settings live in `config.toml` under the data directory.

```bash
careerbot config --list
careerbot config <key>           # read one
careerbot config <key> <value>   # set one
careerbot config --unset <key>
careerbot config --edit          # opens $EDITOR
```

| Key | Default | Description |
| --- | --- | --- |
| `agent.driver` | *required* | `anthropic_api` or `claude_code` |
| `agent.anthropic_api.api_key` | *required for `anthropic_api`* | Anthropic API key |
| `agent.anthropic_api.model` | `claude-sonnet-4-5` | model override |
| `agent.anthropic_api.base_url` | `https://api.anthropic.com` | API base URL (proxies, mocks) |
| `service.poll_interval_hours` | `1` | per-company polling interval |
| `service.startup_jitter_seconds` | `60` | maximum random offset for first ticks |

## Daemon control

```bash
careerbot start-service  # foreground; Ctrl-C to stop
careerbot stop-service   # tell a running daemon to shut down
careerbot status         # works with or without the daemon running
```

`start-service` blocks the terminal. Run it under systemd, launchd,
or a terminal multiplexer to keep it alive across sessions.

## Observability

```bash
careerbot logs              # snapshot from the in-memory ring buffer
careerbot logs --follow     # stream live over the IPC socket
careerbot run-now           # trigger an immediate tick for every company
careerbot run-now stripe    # trigger a tick for one company
```

Logs are JSON on stderr. Control verbosity with
`RUST_LOG=careerbot=debug,info`.

## Companies, profile, filters

```bash
careerbot add-company stripe                                # agent discovers the careers URL
careerbot add-company stripe https://stripe.com/jobs/search # with an explicit URL hint
careerbot list-companies
careerbot remove-company stripe                             # also clears matched jobs

careerbot profile                                # print profile.md
careerbot profile --edit                         # open in $EDITOR
careerbot profile --from-resume ./me.pdf         # re-ingest from a resume

careerbot filters                                # print filters.json
careerbot filters --edit
```

## Feedback to the agent

```bash
careerbot feedback "stop matching grad roles"
```

Runs a short agent loop that adjusts `profile.md` or `filters.json`
in response to free-form guidance.

`careerbot mcp-server` is launched by Claude Code over stdio when the
`claude_code` driver is active. Do not invoke it directly.

## Building from source

Rust 1.85, edition 2024. The workspace contains two crates:
`careerbot-core` (library) and `careerbot` (a thin CLI shell).

```bash
cargo build --release           # binary at target/release/careerbot
cargo install --path careerbot  # install into ~/.cargo/bin
```

## Contributing

Open an issue or PR at
[github.com/aravind-n/careerbot](https://github.com/aravind-n/careerbot).

## License

MIT — see [LICENSE](LICENSE).
