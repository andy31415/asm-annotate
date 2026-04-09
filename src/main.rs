#![allow(unused_imports)]
use clap::Parser;
use color_eyre::eyre::{self, Context, Result};
use env_logger::Env;
use log::{debug, info};

mod backends;
mod cli;
mod commands;
mod core;
mod ui;

use cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    env_logger::Builder::from_env(Env::default().default_filter_or(cli.log_level.to_string()))
        .init();
    color_eyre::install()?;

    // TODO: Implement command dispatch
    match &cli.command {
        Some(cli::Commands::List(args)) => {
            commands::list::handle_list(args)?;
        }
        Some(cli::Commands::Annotate(args)) => {
            commands::annotate::handle_annotate(args)?;
        }
        None => {
            // Default behavior if no subcommand is given
            // Maybe print help or a default annotation
            println!("No command given. Use --help to see options.");
        }
    }

    Ok(())
}
