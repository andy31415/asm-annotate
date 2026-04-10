use clap::Parser;
use color_eyre::eyre::Result;
use env_logger::Env;

mod backends;
mod cli;
mod commands;
mod source_reader;
mod types;
mod ui;

use cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    env_logger::Builder::from_env(Env::default().default_filter_or(cli.log_level.to_string()))
        .init();
    color_eyre::install()?;

    // Directly use the arguments for annotate
    commands::annotate::handle_annotate(&cli)?;

    Ok(())
}
