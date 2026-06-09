use careerbot_core::commands::add_company as add_company_cmd;
use careerbot_core::commands::profile as profile_cmd;
use careerbot_core::commands::remove_company as remove_company_cmd;
use careerbot_core::commands::CommandError;
use careerbot_core::config::{self, Config};
use careerbot_core::daemon;
use careerbot_core::daemon::ipc_client;
use careerbot_core::daemon::scheduler;
use careerbot_core::mcp;
use careerbot_core::paths::Paths;
use careerbot_core::runtime::Runtime;
use clap::{CommandFactory, Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

#[derive(Parser)]
#[command(
    name = "careerbot",
    version,
    about = "Single-user local job-matching daemon driven by an LLM agent",
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// First-time interactive setup
    Init {
        /// Wipe local state and re-initialize
        #[arg(long)]
        force: bool,
    },

    /// Get, set, list, edit, or unset config.toml keys
    Config {
        /// Dot-key to read or write (e.g., service.port)
        key: Option<String>,
        /// New value (set if present)
        value: Option<String>,
        /// List all set keys with current values
        #[arg(long, conflicts_with_all = ["key", "value", "edit", "unset"])]
        list: bool,
        /// Open config.toml in $EDITOR
        #[arg(long, conflicts_with_all = ["key", "value", "list", "unset"])]
        edit: bool,
        /// Remove a key
        #[arg(long, value_name = "KEY", conflicts_with_all = ["key", "value", "list", "edit"])]
        unset: Option<String>,
    },

    /// Print profile.md, edit it, or re-ingest from a resume file
    Profile {
        /// Open profile.md in $EDITOR
        #[arg(long, conflicts_with = "from_resume")]
        edit: bool,
        /// Re-ingest resume from path (runs profile_init agent)
        #[arg(long, value_name = "PATH")]
        from_resume: Option<PathBuf>,
    },

    /// Print filters.json or open it in $EDITOR
    Filters {
        /// Open filters.json in $EDITOR
        #[arg(long)]
        edit: bool,
    },

    /// Add a company (url optional; agent auto-discovers)
    AddCompany {
        name: String,
        url: Option<String>,
    },

    /// Remove a company (deletes the script)
    RemoveCompany { name: String },

    /// List configured companies
    ListCompanies,

    /// Run the daemon in the foreground
    StartService,

    /// Stop a running daemon (POST /shutdown)
    StopService,

    /// Print daemon status; works with or without the daemon running
    Status,

    /// Print daemon logs (SSE if --follow)
    Logs {
        #[arg(short, long)]
        follow: bool,
    },

    /// Trigger a tick out of schedule (all companies or one)
    RunNow { company: Option<String> },

    /// Send free-form feedback to the agent
    Feedback { text: String },

    /// Run the stdio MCP server (invoked by Claude Code)
    McpServer,
}

pub async fn run(cli: Cli) -> ExitCode {
    match cli.command {
        Command::Config {
            key,
            value,
            list,
            edit,
            unset,
        } => handle_config(key, value, list, edit, unset),
        Command::Profile { edit, from_resume } => handle_profile(edit, from_resume).await,
        Command::AddCompany { name, url } => handle_add_company(name, url).await,
        Command::StartService => handle_start_service().await,
        Command::StopService => handle_stop_service().await,
        Command::Status => handle_status().await,
        Command::RunNow { company } => handle_run_now(company).await,
        Command::ListCompanies => handle_list_companies().await,
        Command::RemoveCompany { name } => handle_remove_company(name).await,
        Command::McpServer => handle_mcp_server().await,
        _ => {
            println!("not implemented yet");
            ExitCode::SUCCESS
        }
    }
}

fn handle_config(
    key: Option<String>,
    value: Option<String>,
    list: bool,
    edit: bool,
    unset: Option<String>,
) -> ExitCode {
    let paths = match Paths::from_env() {
        Ok(p) => p,
        Err(e) => return die(format_args!("{e}")),
    };
    let path = paths.config_file();

    if let Some(k) = unset {
        return config_unset(&path, &k);
    }
    if list {
        return config_list(&path);
    }
    if edit {
        return config_edit(&path);
    }
    match (key, value) {
        (None, None) => {
            let mut cmd = Cli::command();
            if let Some(sub) = cmd.find_subcommand_mut("config") {
                sub.print_help().ok();
                println!();
            }
            ExitCode::SUCCESS
        }
        (Some(k), None) => config_get(&path, &k),
        (Some(k), Some(v)) => config_set(&path, &k, &v),
        (None, Some(_)) => unreachable!("clap parses value only when key is present"),
    }
}

fn config_get(path: &Path, key: &str) -> ExitCode {
    let cfg = match Config::load(path) {
        Ok(c) => c,
        Err(e) => return die(format_args!("{e}")),
    };
    if let Some(v) = cfg.get(key) {
        println!("{}", config::render_value(&v));
    }
    ExitCode::SUCCESS
}

fn config_set(path: &Path, key: &str, value: &str) -> ExitCode {
    let mut cfg = match Config::load(path) {
        Ok(c) => c,
        Err(e) => return die(format_args!("{e}")),
    };
    if let Err(e) = cfg.set(key, config::parse_value(value)) {
        return die(format_args!("{e}"));
    }
    if let Err(e) = cfg.save() {
        return die(format_args!("{e}"));
    }
    ExitCode::SUCCESS
}

fn config_unset(path: &Path, key: &str) -> ExitCode {
    let mut cfg = match Config::load(path) {
        Ok(c) => c,
        Err(e) => return die(format_args!("{e}")),
    };
    cfg.unset(key);
    if let Err(e) = cfg.save() {
        return die(format_args!("{e}"));
    }
    ExitCode::SUCCESS
}

fn config_list(path: &Path) -> ExitCode {
    let cfg = match Config::load(path) {
        Ok(c) => c,
        Err(e) => return die(format_args!("{e}")),
    };
    for (k, v) in cfg.list() {
        println!("{} = {}", k, config::render_toml_literal(&v));
    }
    ExitCode::SUCCESS
}

fn config_edit(path: &Path) -> ExitCode {
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return die(format_args!("{e}"));
    }
    if !path.exists()
        && let Err(e) = std::fs::write(path, "")
    {
        return die(format_args!("{e}"));
    }

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    match std::process::Command::new(&editor).arg(path).status() {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => die(format_args!("{editor} exited with status {s}")),
        Err(e) => die(format_args!("failed to spawn {editor}: {e}")),
    }
}

fn die(args: std::fmt::Arguments<'_>) -> ExitCode {
    eprintln!("error: {args}");
    ExitCode::FAILURE
}

async fn handle_profile(edit: bool, from_resume: Option<PathBuf>) -> ExitCode {
    let rt = match Runtime::open().await {
        Ok(r) => r,
        Err(e) => return die(format_args!("{e}")),
    };

    if let Some(path) = from_resume {
        return run_profile_from_resume(&rt, &path).await;
    }
    if edit {
        return run_profile_edit(&rt);
    }
    match profile_cmd::show(&rt).await {
        Ok(content) => {
            print!("{content}");
            ExitCode::SUCCESS
        }
        Err(CommandError::NotFound { .. }) => die(format_args!(
            "no profile yet — run `careerbot profile --from-resume <path>` to create one"
        )),
        Err(e) => die(format_args!("{e}")),
    }
}

async fn run_profile_from_resume(rt: &Runtime, path: &Path) -> ExitCode {
    match profile_cmd::from_resume(rt, path).await {
        Ok(output) => {
            println!("{}", output.text);
            if let Some(cost) = output.cost {
                eprintln!(
                    "tokens: input={} output={} (tool calls: {})",
                    cost.input_tokens, cost.output_tokens, output.tool_calls
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => die(format_args!("{e}")),
    }
}

async fn handle_add_company(name: String, url: Option<String>) -> ExitCode {
    let rt = match Runtime::open().await {
        Ok(r) => r,
        Err(e) => return die(format_args!("{e}")),
    };
    let output = match add_company_cmd::add_company(&rt, &name, url.as_deref()).await {
        Ok(o) => o,
        Err(e) => return die(format_args!("{e}")),
    };

    println!("{}", output.text);
    if let Some(cost) = output.cost {
        eprintln!(
            "tokens: input={} output={} (tool calls: {})",
            cost.input_tokens, cost.output_tokens, output.tool_calls
        );
    }
    if output.verified {
        eprintln!(
            "verified: {} initial job{} on first run",
            output.initial_jobs,
            if output.initial_jobs == 1 { "" } else { "s" }
        );
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "verification failed: {}",
            output.verification_error.as_deref().unwrap_or("unknown")
        );
        eprintln!("(script was saved; inspect or remove via `careerbot remove-company {name}`)");
        ExitCode::FAILURE
    }
}

async fn handle_start_service() -> ExitCode {
    let rt = match Runtime::open().await {
        Ok(r) => Arc::new(r),
        Err(e) => return die(format_args!("{e}")),
    };
    match daemon::run(rt).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => die(format_args!("{e}")),
    }
}

async fn handle_stop_service() -> ExitCode {
    let paths = match Paths::from_env() {
        Ok(p) => p,
        Err(e) => return die(format_args!("{e}")),
    };
    let socket = paths.socket_path();
    match ipc_client::is_running(&socket).await {
        Ok(false) => {
            eprintln!("daemon is not running");
            ExitCode::SUCCESS
        }
        Ok(true) => match ipc_client::request(&socket, "POST", "/shutdown", &[]).await {
            Ok((202, _)) => {
                println!("daemon shutting down");
                ExitCode::SUCCESS
            }
            Ok((status, body)) => die(format_args!(
                "unexpected response from daemon: {} {}",
                status, body
            )),
            Err(e) => die(format_args!("{e}")),
        },
        Err(e) => die(format_args!("{e}")),
    }
}

async fn handle_mcp_server() -> ExitCode {
    let rt = match Runtime::open().await {
        Ok(r) => Arc::new(r),
        Err(e) => return die(format_args!("{e}")),
    };
    match mcp::run(rt).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => die(format_args!("{e}")),
    }
}

async fn handle_list_companies() -> ExitCode {
    let rt = match Runtime::open().await {
        Ok(r) => r,
        Err(e) => return die(format_args!("{e}")),
    };
    let names = match scheduler::discover_companies(&rt) {
        Ok(n) => n,
        Err(e) => return die(format_args!("{e}")),
    };
    if names.is_empty() {
        eprintln!("(no companies — add one with `careerbot add-company <name> [url]`)");
    } else {
        for name in names {
            println!("{name}");
        }
    }
    ExitCode::SUCCESS
}

async fn handle_remove_company(name: String) -> ExitCode {
    let rt = match Runtime::open().await {
        Ok(r) => r,
        Err(e) => return die(format_args!("{e}")),
    };
    match remove_company_cmd::remove(&rt, &name).await {
        Ok(out) => {
            if out.script_removed {
                println!("removed scripts/{name}.py");
            } else {
                println!("no script for {name}");
            }
            eprintln!("deleted {} job row(s)", out.jobs_removed);
            ExitCode::SUCCESS
        }
        Err(e) => die(format_args!("{e}")),
    }
}

async fn handle_run_now(company: Option<String>) -> ExitCode {
    let paths = match Paths::from_env() {
        Ok(p) => p,
        Err(e) => return die(format_args!("{e}")),
    };
    let socket = paths.socket_path();
    match ipc_client::is_running(&socket).await {
        Ok(false) => die(format_args!(
            "daemon is not running; start it with `careerbot start-service`"
        )),
        Ok(true) => {
            let path = match company.as_deref() {
                Some(c) => format!("/run-now?company={c}"),
                None => "/run-now".to_string(),
            };
            match ipc_client::request(&socket, "POST", &path, &[]).await {
                Ok((202, _)) => {
                    match company {
                        Some(c) => println!("tick queued for {c}"),
                        None => println!("tick queued for all companies"),
                    }
                    ExitCode::SUCCESS
                }
                Ok((404, _)) => {
                    let name = company.unwrap_or_default();
                    die(format_args!(
                        "company {:?} is not registered (no scripts/{}.py)",
                        name, name
                    ))
                }
                Ok((status, body)) => die(format_args!("unexpected status: {} {}", status, body)),
                Err(e) => die(format_args!("{e}")),
            }
        }
        Err(e) => die(format_args!("{e}")),
    }
}

async fn handle_status() -> ExitCode {
    let paths = match Paths::from_env() {
        Ok(p) => p,
        Err(e) => return die(format_args!("{e}")),
    };
    let socket = paths.socket_path();
    match ipc_client::is_running(&socket).await {
        Ok(false) => {
            println!("not running");
            ExitCode::SUCCESS
        }
        Ok(true) => match ipc_client::request(&socket, "GET", "/status", &[]).await {
            Ok((200, body)) => {
                println!("{body}");
                ExitCode::SUCCESS
            }
            Ok((status, body)) => die(format_args!("unexpected status: {} {}", status, body)),
            Err(e) => die(format_args!("{e}")),
        },
        Err(e) => die(format_args!("{e}")),
    }
}

fn run_profile_edit(rt: &Runtime) -> ExitCode {
    let path = profile_cmd::profile_path(rt);
    if !path.exists() {
        return die(format_args!(
            "no profile yet — run `careerbot profile --from-resume <path>` to create one"
        ));
    }
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());
    match std::process::Command::new(&editor).arg(&path).status() {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => die(format_args!("{editor} exited with status {s}")),
        Err(e) => die(format_args!("failed to spawn {editor}: {e}")),
    }
}
