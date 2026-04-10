use crate::backends::demangle::{CppDemangleBackend, DemanglerBackend};
use crate::backends::elf::{ElfBackend, FunctionInfo, GoblinElfBackend};
use crate::backends::picker::{PickerBackend, SkimBackend};
use crate::cli::AnnotateArgs;
use crate::source_reader::SourceReader;
use crate::types::{AnnotatedInstruction, DisplayItem};
use crate::ui::tui::run_tui;

use color_eyre::eyre::{Context, Result, eyre};
use log::{info, warn};

pub fn handle_annotate(args: &AnnotateArgs) -> Result<()> {
    let elf_backend = GoblinElfBackend;
    let demangler_backend = CppDemangleBackend;
    let source_reader = SourceReader::new(&args.remap)?;

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
    if addr_to_src.is_empty() {
        warn!("No DWARF info found. Build with -g to get source mapping.");
    } else {
        info!(
            "Built address to source mapping with {} entries.",
            addr_to_src.len()
        );
    }

    // 3. Disassemble range
    let mut instructions =
        crate::backends::disasm::disassemble_range(&args.elf, start_addr, end_addr)
            .wrap_err("Failed to disassemble")?;
    if instructions.is_empty() {
        warn!("No instructions found in range.");
    } else {
        info!("Disassembled {} instructions.", instructions.len());
    }

    // 4. Demangle names
    let final_func_name = if !args.no_demangle {
        let mut names_to_demangle = vec![selected_function.name.clone()];

        // Extract potential mangled names from instruction operands
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
            selected_function.name.clone(),
            &mut instructions,
            &demangled_map,
        )
    } else {
        selected_function.name.clone()
    };

    // 5. Create AnnotatedInstructions
    let annotated_instructions = AnnotatedInstruction::from_many(&instructions, &addr_to_src);

    // 6. Prepare for Display
    let display_items = DisplayItem::from_annotated(&annotated_instructions, &source_reader)?;

    // 7. Render output
    run_tui(&final_func_name, &display_items, &source_reader)?;

    Ok(())
}
