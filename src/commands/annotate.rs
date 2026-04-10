use crate::backends::demangle::{CppDemangleBackend, DemanglerBackend};
use crate::backends::elf::{ElfBackend, FunctionInfo, GoblinElfBackend};
use crate::backends::picker::{PickerBackend, SkimBackend};
use crate::cli::Cli;
use crate::source_reader::SourceReader;
use crate::types::{AnnotatedInstruction, DisplayItem};
use crate::ui::tui::run_tui;

use color_eyre::eyre::{Context, Result, eyre};
use log::{error, info, warn};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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

    // Initial data load
    let initial_data = load_annotation_data(args, &selected_function.name)?;

    // Render output
    run_tui(args, &selected_function.name, initial_data, rx)?;

    Ok(())
}

pub struct AnnotationData {
    pub display_items: Vec<DisplayItem>,
    pub source_reader: SourceReader,
    pub final_func_name: String,
    pub display_name: String,
}

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

    let final_func_name = if !args.no_demangle {
        let mut names_to_demangle = vec![func_name.to_string()];
        let mangled_regex = regex::Regex::new(r"_Z[a-zA-Z0-9_]+").unwrap();
        for inst in &instructions {
            for cap in mangled_regex.captures_iter(&inst.mnemonic) {
                names_to_demangle.push(cap[0].to_string());
            }
        }
        names_to_demangle.sort();
        names_to_demangle.dedup();

        let demangled_map = demangler_backend.demangle_batch(&names_to_demangle)?;
        crate::backends::disasm::apply_demangling(
            func_name.to_string(),
            &mut instructions,
            &demangled_map,
        )
    } else {
        func_name.to_string()
    };

    let annotated_instructions = AnnotatedInstruction::from_many(&instructions, &addr_to_src);
    let display_items = DisplayItem::from_annotated(&annotated_instructions)?;

    Ok(AnnotationData {
        display_items,
        source_reader,
        final_func_name,
        display_name,
    })
}
