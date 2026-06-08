use clap::Parser;
use std::process::ExitCode;

mod cli;

#[tokio::main]
async fn main() -> ExitCode {
    careerbot_core::log::init_tracing();
    let args = cli::Cli::parse();
    cli::run(args).await
}
