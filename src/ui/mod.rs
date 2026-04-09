use crate::core::RenderGroup;
use colored::*;
use std::path::Path;

// Basic unified renderer
pub fn render_unified(
    func_name: &str,
    groups: &[RenderGroup],
    show_bytes: bool,
) -> color_eyre::Result<()> {
    render_header(func_name, groups)?;
    let mut shown_src_keys: std::collections::HashSet<(Option<String>, Option<usize>)> = std::collections::HashSet::new();

    for group in groups {
        let src_key = (group.src_file.clone(), group.src_line_start);
        let src_already_shown = group.src_file.is_some() && shown_src_keys.contains(&src_key);

        if !group.src_lines.is_empty() && !src_already_shown {
            shown_src_keys.insert(src_key);
        }

        if !src_already_shown {
            if group.show_file_header && group.src_file.is_some() && !group.src_lines.is_empty() {
                let short = short_path(group.src_file.as_ref().unwrap(), 3);
                let lineno = group
                    .src_line_start
                    .map(|l| format!(":{}", l))
                    .unwrap_or_else(String::new);
                println!("  {}", format!("{}{}", short, lineno).dimmed().italic());
            }

            for (i, src_text) in group.src_lines.iter().enumerate() {
                let marker = if i == 0 { "▶ " } else { "  " };
                println!(
                    "  {}{}{}",
                    marker.color(group.color.as_str()).bold(),
                    src_text.color(group.color.as_str()),
                    "".white() // Reset color
                );
            }
        }

        for inst in &group.instructions {
            let bytes_str = if show_bytes && !inst.bytes.is_empty() {
                format!("{:<24}  ", inst.bytes)
            } else {
                "".to_string()
            };
            let parts: Vec<&str> = inst.mnemonic.splitn(2, ' ').collect();
            let mnem_word = parts.get(0).unwrap_or(&"");
            let operands = parts.get(1).unwrap_or(&"");

            println!(
                "    {:08x}  {}{:<10}{}{}",
                inst.address,
                bytes_str.cyan().dimmed(),
                mnem_word.color(group.color.as_str()).bold(),
                operands.color(group.color.as_str()),
                "".white() // Reset color
            );
        }
    }

    // TODO: Add render_stats_table if stats are enabled
    println!();
    Ok(())
}

// Helper to render the function header
fn render_header(func_name: &str, groups: &[RenderGroup]) -> color_eyre::Result<()> {
    let all_insns: Vec<&crate::backends::disasm::Instruction> = groups.iter().flat_map(|g| &g.instructions).collect();
    let total_bytes = all_insns.iter().map(|i| i.bytes.replace(" ", "").len() / 2).sum::<usize>();

    println!();
    println!(
        " {} {} {} {} {}",
        func_name.white().bold(),
        "·".dimmed(),
        format!("{} instructions", all_insns.len()).cyan(),
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

use std::path::PathBuf;

// TODO: Implement render_split
pub fn render_split() -> color_eyre::Result<()> {
    Ok(())
}

// TODO: Implement render_stats_table
pub fn render_stats_table(_groups: &[RenderGroup]) -> color_eyre::Result<()> {
    Ok(())
}
