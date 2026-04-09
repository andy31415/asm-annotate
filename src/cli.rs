use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// A CLI tool for colored source <-> assembly annotation for ELF files.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Set the logging level.
    #[arg(short, long, global = true, default_value_t = LogLevel::Info, ignore_case = true)]
    pub log_level: LogLevel,
}

/// Log verbosity levels accepted by --log-level.
#[derive(ValueEnum, Debug, Clone, Default)]
#[value(rename_all = "lowercase")]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            LogLevel::Off => "off",
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        };
        f.write_str(s)
    }
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// List all functions in the ELF and exit
    #[command(visible_alias = "l")]
    List(ListArgs),

    /// Annotate a function with source code
    #[command(visible_alias = "a")]
    Annotate(AnnotateArgs),
}

#[derive(Parser, Debug)]
pub struct ListArgs {
    /// The ELF file to process
    #[arg(value_name = "ELF")]
    pub elf: PathBuf,

    /// Do not demangle C++ symbol names
    #[arg(long)]
    pub no_demangle: bool,
}

#[derive(Parser, Debug)]
pub struct AnnotateArgs {
    /// The ELF file to process
    #[arg(value_name = "ELF")]
    pub elf: PathBuf,

    /// The function to annotate
    #[arg(value_name = "FUNCTION")]
    pub function: Option<String>,

    /// objdump binary to use (auto-detected if omitted)
    #[arg(long, value_name = "BINARY")]
    pub objdump: Option<String>,

    /// Show per-source-line instruction/byte cost table
    #[arg(long)]
    pub stats: bool,

    /// Show raw instruction bytes alongside mnemonics
    #[arg(long)]
    pub bytes: bool,

    /// Skip DWARF source mapping
    #[arg(long)]
    pub no_dwarf: bool,

    /// Do not demangle C++ symbol names
    #[arg(long)]
    pub no_demangle: bool,

    /// Remap a source path prefix. E.g. --remap /workspace /home/user/src (repeatable)
    #[arg(long, number_of_values = 2, value_names = &["OLD", "NEW"])]
    pub remap: Vec<String>,

    /// Output format: split (default, 50/50 columns), unified (interleaved source+asm), or split:<N> (split with N chars for source column)
    #[arg(long, default_value = "split", value_name = "FORMAT")]
    pub format: String,
}
