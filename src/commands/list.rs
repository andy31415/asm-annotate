use crate::cli::ListArgs;
use crate::backends::elf::{ElfBackend, GoblinElfBackend};
use crate::backends::demangle::{DemanglerBackend, CppDemangleBackend};
use color_eyre::eyre::Result;
use colored::*;

pub fn handle_list(args: &ListArgs) -> Result<()> {
    let elf_backend = GoblinElfBackend;
    let demangler_backend = CppDemangleBackend;

    let functions = elf_backend.list_functions(&args.elf)?;

    println!("{}", format!("Functions in {}", args.elf.display()).bold());
    println!("{:<12} {:<10} {:<30} {}", "Address".cyan(), "Size".yellow(), "Name".white(), "Demangled".white());
    println!("{:<12} {:<10} {:<30} {}", "-------".cyan(), "----".yellow(), "----".white(), "---------".white());

    for func in functions {
        let demangled = if !args.no_demangle {
            demangler_backend.demangle(&func.name).unwrap_or_else(|_| func.name.clone())
        } else {
            func.name.clone()
        };

        println!("{:<#12x} {:<10} {:<30} {}",
                 func.addr,
                 func.size,
                 func.name,
                 if demangled == func.name { "".to_string() } else { demangled });
    }

    Ok(())
}
