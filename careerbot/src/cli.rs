use clap::Parser;

#[derive(Parser)]
#[command(
    name = "careerbot",
    version,
    about = "Single-user local job-matching daemon driven by an LLM agent"
)]
pub struct Cli {
    // Subcommand variants are added by later phases per PLAN.md §13.
}

pub fn run(_cli: Cli) {
    // No subcommands yet; clap handles --help and --version on its own.
}
