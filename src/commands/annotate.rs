//! Command handler for the "annotate" action.

use crate::backends::demangle::{CppDemangleBackend, DemanglerBackend};
use crate::backends::disasm::Instruction;
use crate::backends::elf::{ElfBackend, FunctionInfo, GoblinElfBackend};
use crate::backends::picker::{PickerBackend, SkimBackend};
use crate::cli::Cli;
use crate::source_reader::SourceReader;
use crate::types::{AnnotatedInstruction, DisplayItem};
use crate::ui::tui::run_tui;

use color_eyre::eyre::{Context, Result, eyre};
use log::{error, info, warn};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use std::collections::HashSet;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Handles the main logic for annotating a function.
///
/// This includes:
/// - Listing functions from the ELF file.
/// - Allowing the user to select a function if necessary.
/// - Setting up a file watcher to reload data on changes.
/// - Loading the annotation data.
/// - Running the Terminal User Interface.
///
/// # Arguments
///
/// * `args` - The parsed command line arguments.
pub fn handle_annotate(args: &Cli) -> Result<()> {
    let elf_backend = GoblinElfBackend;
    let demangler_backend = CppDemangleBackend;

    let functions = elf_backend
        .list_functions(&args.elf)
        .wrap_err("Failed to list functions")?;

    if functions.is_empty() {
        return Err(eyre!("No functions found in ELF file."));
    }

    let function_name: &str = args.function.as_deref().unwrap_or("");

    let mut matched_functions: Vec<FunctionInfo> = if function_name.is_empty() {
        functions
    } else {
        functions
            .into_iter()
            .filter(|f| f.name.contains(function_name))
            .collect()
    };

    let selected_function: FunctionInfo = match matched_functions.len() {
        0 => {
            return Err(eyre!("No function found matching '{}'.", function_name));
        }
        1 if !function_name.is_empty() => matched_functions.pop().unwrap(),
        _ => {
            // Includes function_name.is_empty() case
            if function_name.is_empty() {
                info!("Please choose a function to annotate:");
            } else {
                info!(
                    "Multiple functions match '{}'. Please choose one:",
                    function_name
                );
            }
            let picker_backend = SkimBackend;
            picker_backend
                .pick_function(matched_functions, &demangler_backend)?
                .ok_or_else(|| eyre!("No function selected from picker."))?
        }
    };

    // Demangle the selected function name for display if not already done
    let display_name = if !args.no_demangle {
        demangler_backend
            .demangle(&selected_function.name)
            .unwrap_or_else(|_| selected_function.name.clone())
    } else {
        selected_function.name.clone()
    };

    info!(
        "Selected function: {} at {:#x}",
        display_name, selected_function.addr
    );

    // Initial data load
    let initial_data = load_annotation_data(args, &selected_function.name)?;

    if args.dump {
        crate::commands::dump::dump_annotation(&initial_data)?;
        return Ok(());
    }

    // Channel for file watch events
    let (tx, rx) = mpsc::channel();

    // Clone necessary data for the watcher thread
    let elf_path_clone = args.elf.clone();
    let watcher_tx = tx.clone();

    // Spawn watcher thread
    thread::spawn(move || {
        let mut watcher = match RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) => {
                    if matches!(event.kind, EventKind::Modify(_)) {
                        info!("File modified: {:?}, reloading...", event.paths);
                        if watcher_tx.send(()).is_err() {
                            error!("Failed to send reload signal, receiver likely dropped.");
                        }
                    }
                }
                Err(e) => error!("Error watching file: {:?}", e),
            },
            notify::Config::default(),
        ) {
            Ok(w) => w,
            Err(e) => {
                error!("Failed to create file watcher: {}", e);
                return;
            }
        };

        if let Err(e) = watcher.watch(&elf_path_clone, RecursiveMode::NonRecursive) {
            error!("Failed to watch file {}: {}", elf_path_clone.display(), e);
            return;
        }
        info!("Watching {} for changes...", elf_path_clone.display());

        // Keep the thread alive
        loop {
            thread::sleep(Duration::from_secs(3600)); // Sleep for a long time
        }
    });

    // Render output
    run_tui(args, &selected_function.name, initial_data, rx)?;

    Ok(())
}

/// Contains all the data required to display the annotation in the TUI.
pub struct AnnotationData {
    /// The list of items to display, including instructions, source, and color.
    pub display_items: Vec<DisplayItem>,
    /// The source reader instance for accessing source file contents.
    pub source_reader: SourceReader,
    /// The display name of the function being annotated (potentially demangled).
    pub display_name: String,
}

// Extracts mangled symbols from instruction mnemonics and comments.
fn extract_mangled_symbols(instructions: &[Instruction]) -> Vec<String> {
    let mut names_to_demangle = HashSet::new();
    let mangled_regex = Regex::new(r"_Z[a-zA-Z0-9_]+").unwrap();

    for inst in instructions {
        // Add symbols from branch targets in comments (e.g., "  ; <_Z1fv>")
        if let Some(comment) = inst.mnemonic.split_once(" ; <")
            && let Some(mangled) = comment.1.strip_suffix('>')
                && mangled.starts_with("_Z") {
                    names_to_demangle.insert(mangled.to_string());
                }
        // Add symbols from instruction operands
        for cap in mangled_regex.captures_iter(&inst.mnemonic) {
            names_to_demangle.insert(cap[0].to_string());
        }
    }

    let mut sorted_names: Vec<String> = names_to_demangle.into_iter().collect();
    sorted_names.sort();
    sorted_names
}

/// Loads all necessary data for annotating a function.
///
/// This involves:
/// - Getting function boundaries.
/// - Building address-to-source map from DWARF.
/// - Disassembling the function.
/// - Extracting and demangling symbols from the assembly.
/// - Preparing display items for the TUI.
///
/// # Arguments
///
/// * `args` - The parsed command line arguments.
/// * `func_name` - The (potentially mangled) name of the function to load.
pub fn load_annotation_data(args: &Cli, func_name: &str) -> Result<AnnotationData> {
    let elf_backend = GoblinElfBackend;
    let demangler_backend = CppDemangleBackend;
    let source_reader = SourceReader::new(&args.remap)?;

    // Demangle the function name for display
    let display_name = if !args.no_demangle {
        demangler_backend
            .demangle(func_name)
            .unwrap_or_else(|_| func_name.to_string())
    } else {
        func_name.to_string()
    };

    info!(
        "Loading data for function: {} from file {}",
        display_name,
        args.elf.display()
    );

    let (start_addr, end_addr) = elf_backend
        .get_function_bounds(&args.elf, func_name)
        .wrap_err(format!("Failed to get bounds for function {}", func_name))?;

    info!(
        "Function range: {:#x} - {:#x} ({} bytes)",
        start_addr,
        end_addr,
        end_addr - start_addr
    );

    let addr_to_src = elf_backend
        .build_addr_to_src(&args.elf)
        .wrap_err("Failed to build address to source mapping")?;
    if addr_to_src.is_empty() {
        warn!("No DWARF info found. Build with -g to get source mapping.");
    } else {
        info!(
            "Built address to source mapping with {} entries.",
            addr_to_src.len()
        );
    }

    let mut instructions =
        match crate::backends::disasm::disassemble_range(&args.elf, start_addr, end_addr) {
            Ok(inst) => inst,
            Err(e) => return Err(eyre!("Failed to disassemble: {}", e)),
        };

    if instructions.is_empty() {
        warn!("No instructions found in range.");
    } else {
        info!("Disassembled {} instructions.", instructions.len());
    }

    if !args.no_demangle {
        let names_to_demangle = extract_mangled_symbols(&instructions);
        if !names_to_demangle.is_empty() {
            info!(
                "Demangling {} symbols from instructions...",
                names_to_demangle.len()
            );
            let demangled_map = demangler_backend.demangle_batch(&names_to_demangle)?;
            crate::backends::disasm::apply_demangling(&mut instructions, &demangled_map);
        }
    }

    let annotated_instructions = AnnotatedInstruction::from_many(&instructions, &addr_to_src);
    let display_items = DisplayItem::from_annotated(&annotated_instructions)?;

    Ok(AnnotationData {
        display_items,
        source_reader,
        display_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::disasm::Instruction;

    fn make_inst(address: u64, mnemonic: &str) -> Instruction {
        Instruction {
            address,
            bytes: "".to_string(),
            mnemonic: mnemonic.to_string(),
        }
    }

    #[test]
    fn test_extract_mangled_symbols() {
        let instructions = vec![
            make_inst(0x1000, "call _Z6foobarv"),
            make_inst(0x1004, "jmp  _Z3bazi"),
            make_inst(0x1008, "mov  eax, [_Z7dataVAR]"),
            make_inst(0x100c, "add  ebx, 1 ; <_Z9somethingv>"),
            make_inst(0x1010, "sub  ecx, ecx ; <not_mangled>"),
            make_inst(0x1014, "call _Z6foobarv"), // Duplicate
            make_inst(0x1018, "lea  rdi, [rip + _ZN1A1B1CEv]"),
        ];

        let expected = vec![
            "_Z3bazi".to_string(),
            "_Z6foobarv".to_string(),
            "_Z7dataVAR".to_string(),
            "_Z9somethingv".to_string(),
            "_ZN1A1B1CEv".to_string(),
        ];
        let mut result = extract_mangled_symbols(&instructions);
        result.sort();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_extract_mangled_symbols_empty() {
        let instructions = vec![make_inst(0x1000, "nop")];
        let result = extract_mangled_symbols(&instructions);
        assert!(result.is_empty());
    }
}
