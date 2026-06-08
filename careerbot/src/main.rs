use clap::Parser;

mod cli;

fn main() {
    careerbot_core::log::init_tracing();
    let args = cli::Cli::parse();
    cli::run(args);
}
