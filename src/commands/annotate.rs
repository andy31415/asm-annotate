use crate::backends::demangle::{CppDemangleBackend, DemanglerBackend};
use crate::backends::elf::{ElfBackend, FunctionInfo, GoblinElfBackend};
use crate::backends::picker::{PickerBackend, SkimBackend};
use crate::cli::AnnotateArgs;
use color_eyre::eyre::{Context, Result, eyre};
use log::info;

pub fn handle_annotate(args: &AnnotateArgs) -> Result<()> {
    let elf_backend = GoblinElfBackend;
    let demangler_backend = CppDemangleBackend;

    let functions = elf_backend
        .list_functions(&args.elf)
        .wrap_err("Failed to list functions")?;

    if functions.is_empty() {
        return Err(eyre!("No functions found in ELF file."));
    }

    let function_name = match &args.function {
        Some(name) => name,
        None => {
            return Err(eyre!(
                "Function name not provided. Use --list to see available functions."
            ));
        }
    };

    let mut matched_functions: Vec<FunctionInfo> = functions
        .into_iter()
        .filter(|f| f.name.contains(function_name))
        .collect();

    let selected_function: FunctionInfo = match matched_functions.len() {
        0 => {
            return Err(eyre!("No function found matching '{}'.", function_name));
        }
        1 => matched_functions.pop().unwrap(),
        _ => {
            info!(
                "Multiple functions match '{}'. Please choose one:",
                function_name
            );
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

    let (start_addr, end_addr) = elf_backend
        .get_function_bounds(&args.elf, &selected_function.name)
        .wrap_err(format!(
            "Failed to get bounds for function {}",
            selected_function.name
        ))?;

    info!(
        "Function range: {:#x} - {:#x} ({} bytes)",
        start_addr,
        end_addr,
        end_addr - start_addr
    );
    // 2. Get DWARF mapping
    let addr_to_src = elf_backend
        .build_addr_to_src(&args.elf)
        .wrap_err("Failed to build address to source mapping")?;

    info!("Address to source mapping: (showing max 5)");
    for (addr, (file, line)) in addr_to_src.iter().take(5) {
        println!("  {:#x}: {}:{}", addr, file, line);
    }
    if addr_to_src.len() > 5 {
        println!("  ... and {} more entries", addr_to_src.len() - 5);
    }

    // 3. Disassemble range
    // 4. Demangle names
    // 5. Build render groups
    // 6. Render output

    Ok(())
}
