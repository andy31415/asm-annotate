use crate::cli::AnnotateArgs;
use crate::backends::elf::{ElfBackend, GoblinElfBackend, FunctionInfo};
use crate::backends::picker::{PickerBackend, SkimBackend};
use color_eyre::eyre::{Result, Context, eyre};
use log::info;

pub fn handle_annotate(args: &AnnotateArgs) -> Result<()> {
    let elf_backend = GoblinElfBackend;

    let functions = elf_backend.list_functions(&args.elf)
        .wrap_err("Failed to list functions")?;

    if functions.is_empty() {
        return Err(eyre!("No functions found in ELF file."));
    }

    let function_name = match &args.function {
        Some(name) => name,
        None => return Err(eyre!("Function name not provided. Use --list to see available functions.")),
    };

    let mut matched_functions: Vec<FunctionInfo> = functions
        .into_iter()
        .filter(|f| f.name.contains(function_name))
        .collect();

    let selected_function: FunctionInfo = match matched_functions.len() {
        0 => {
            return Err(eyre!("No function found matching '{}'.", function_name));
        }
        1 => {
            matched_functions.pop().unwrap()
        }
        _ => {
            info!("Multiple functions match '{}'. Please choose one:", function_name);
            let picker_backend = SkimBackend;
            picker_backend.pick_function(matched_functions)?
                .ok_or_else(|| eyre!("No function selected from picker."))?
        }
    };

    info!("Selected function: {} at {:#x}", selected_function.name, selected_function.addr);

    // TODO: Implement the rest of the annotation logic
    // 1. Get function bounds
    // 2. Get DWARF mapping
    // 3. Disassemble range
    // 4. Demangle names
    // 5. Build render groups
    // 6. Render output

    Ok(())
}
