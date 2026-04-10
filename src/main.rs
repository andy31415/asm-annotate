use clap::Parser;
use color_eyre::eyre::Result;
use log::LevelFilter;

mod backends;
mod cli;
mod commands;
mod source_reader;
mod types;
mod ui;

use cli::{Cli, LogLevel};

fn main() -> Result<()> {
    let cli = Cli::parse();
    color_eyre::install()?;

    let log_level = match cli.log_level {
        LogLevel::Off => LevelFilter::Off,
        LogLevel::Error => LevelFilter::Error,
        LogLevel::Warn => LevelFilter::Warn,
        LogLevel::Info => LevelFilter::Info,
        LogLevel::Debug => LevelFilter::Debug,
        LogLevel::Trace => LevelFilter::Trace,
    };
    tui_logger::init_logger(log_level).unwrap();
    tui_logger::set_default_level(log_level);

    // Directly use the arguments for annotate
    commands::annotate::handle_annotate(&cli)?;

    Ok(())
}
