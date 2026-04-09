use crate::backends::disasm::Instruction;
use crate::types::DisplayItem;
use colored::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// Basic unified renderer
pub fn render_unified(
    func_name: &str,
    items: &[DisplayItem],
    show_bytes: bool,
) -> color_eyre::Result<()> {
    render_header(func_name, items)?;

    for item in items {
        if item.is_new_line {
            if item.is_new_file {
                if let Some(ref src) = item.source {
                    let short = short_path(&src.file, 3);
                    println!("  {}", format!("{}:{}", short, src.line).dimmed().italic());
                }
            }
            if let Some(ref text) = item.source_text {
                let marker = "▶ ";
                println!(
                    "  {}{}{}",
                    marker.color(item.color).bold(),
                    text.color(item.color),
                    "".white() // Reset color
                );
            }
        }

        let inst = &item.instruction;
        let bytes_str = if show_bytes && !inst.bytes.is_empty() {
            format!("{:<24}  ", inst.bytes)
        } else {
            "".to_string()
        };
        let parts: Vec<&str> = inst.mnemonic.splitn(2, ' ').collect();
        let mnem_word = parts.first().unwrap_or(&"");
        let operands = parts.get(1).unwrap_or(&"");

        println!(
            "    {:08x}  {}{:<10}{}{}",
            inst.address,
            bytes_str.cyan().dimmed(),
            mnem_word.color(item.color).bold(),
            operands.color(item.color),
            "".white() // Reset color
        );
    }

    // TODO: Add render_stats_table if stats are enabled
    println!();
    Ok(())
}

// Helper to render the function header
fn render_header(func_name: &str, items: &[DisplayItem]) -> color_eyre::Result<()> {
    let total_insns = items.len();
    let total_bytes = items
        .iter()
        .map(|i| i.instruction.bytes.replace(" ", "").len() / 2)
        .sum::<usize>();

    println!();
    println!(
        " {} {} {} {} {}",
        func_name.white().bold(),
        "·".dimmed(),
        format!("{} instructions", total_insns).cyan(),
        "·".dimmed(),
        format!("{} bytes", total_bytes).yellow()
    );
    println!();
    Ok(())
}

// Helper to shorten paths
fn short_path(path_str: &str, depth: usize) -> String {
    let path = Path::new(path_str);
    let components: Vec<&std::ffi::OsStr> = path.components().map(|c| c.as_os_str()).collect();
    if components.len() > depth {
        let start_index = components.len() - depth;
        let mut result = PathBuf::from("…");
        for i in start_index..components.len() {
            result.push(components[i]);
        }
        result.to_string_lossy().to_string()
    } else {
        path_str.to_string()
    }
}

// TODO: Implement render_split
pub fn render_split() -> color_eyre::Result<()> {
    Ok(())
}

// TODO: Implement render_stats_table
pub fn render_stats_table() -> color_eyre::Result<()> {
    Ok(())
}
