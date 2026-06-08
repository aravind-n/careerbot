use careerbot_core::commands::profile as profile_cmd;
use careerbot_core::commands::CommandError;
use careerbot_core::config::{self, Config};
use careerbot_core::paths::Paths;
use careerbot_core::runtime::Runtime;
use clap::{CommandFactory, Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

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
