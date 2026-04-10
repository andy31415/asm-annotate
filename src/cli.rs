use clap::Parser;
use std::path::PathBuf;

/// A CLI tool for colored source <-> assembly annotation for ELF files.
#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// The ELF file to process
    #[arg(value_name = "ELF")]
    pub elf: PathBuf,

    /// The function to annotate
    #[arg(value_name = "FUNCTION")]
    pub function: Option<String>,

    /// Skip DWARF source mapping
    #[arg(long)]
    pub no_dwarf: bool,

    /// Do not demangle C++ symbol names
    #[arg(long)]
    pub no_demangle: bool,

    /// Remap a source path prefix. E.g. --remap /workspace /home/user/src (repeatable)
    #[arg(long, number_of_values = 2, value_names = &["OLD", "NEW"])]
    pub remap: Vec<String>,

    /// Set the logging level.
    #[arg(short, long, global = true, default_value_t = LogLevel::Info, ignore_case = true)]
    pub log_level: LogLevel,
}

/// Log verbosity levels accepted by --log-level.
#[derive(clap::ValueEnum, Debug, Clone, Default)]
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
