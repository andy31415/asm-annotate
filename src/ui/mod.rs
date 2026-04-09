use crate::backends::disasm::Instruction;
use crate::types::{DisplayItem, SourceLocation};
use colored::*;
use std::path::{Path, PathBuf};

pub trait Renderer {
    fn render(&self, func_name: &str, items: &[DisplayItem]) -> color_eyre::Result<()>;
}

pub struct UnifiedRenderer {
    pub show_bytes: bool,
}

impl Renderer for UnifiedRenderer {
    fn render(&self, func_name: &str, items: &[DisplayItem]) -> color_eyre::Result<()> {
        render_header(func_name, items)?;

        for item in items {
            if item.is_new_line {
                if item.is_new_file {
                    if let Some(ref src) = item.source {
                        let short = short_path(&src.file, 3);
                        println!("  {}", format!("<{}:{}>", short, src.line).dimmed().italic());
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

            println!("{}", format_asm_line(item, self.show_bytes, item.color));
        }
        println!();
        Ok(())
    }
}

pub struct SplitRenderer {
    pub show_bytes: bool,
    pub source_width: usize,
}

impl Renderer for SplitRenderer {
    fn render(&self, func_name: &str, items: &[DisplayItem]) -> color_eyre::Result<()> {
        render_header(func_name, items)?;

        let mut i = 0;
        while i < items.len() {
            let current_source = items[i].source.clone();
            let color = items[i].color;

            // --- File Header ---
            if items[i].is_new_file {
                if let Some(ref src) = current_source {
                    let short = short_path(&src.file, 3);
                    let file_header = format!("<{}:{}>", short, src.line);
                    println!("{}", file_header.dimmed().italic());
                }
            }

            let mut j = i;
            while j < items.len() && items[j].source == current_source {
                j += 1;
            }
            let group = &items[i..j];

            // --- Source Side Text (printed only once for the group) ---
            let source_text = if current_source.is_some() {
                if let Some(ref text) = items[i].source_text {
                    let display_width = self.source_width.saturating_sub(4);
                    let mut src_text = text.clone();
                    if src_text.len() > display_width {
                        src_text.truncate(display_width.saturating_sub(3));
                        src_text.push_str("...");
                    }
                    format!("  {} {}", "▶ ".color(color).bold(), src_text.color(color))
                } else {
                    format!("  {} {}", "▶ ".color(color).bold(), "?".color(color))
                }
            } else {
                String::new() // No source text if no source location
            };

            // --- Assembly Lines --- (all lines for this source group)
            let asm_lines: Vec<String> = group
                .iter()
                .map(|item| format_asm_line(item, self.show_bytes, color))
                .collect();

            // --- Print Side by Side --- max of source vs asm lines
            for k in 0..asm_lines.len() {
                let src_part = if k == 0 { &source_text } else { "" };
                let asm_part = &asm_lines[k];
                println!(
                    "{:<width$} {} {}",
                    src_part,
                    "|".dimmed(),
                    asm_part,
                    width = self.source_width
                );
            }

            i = j;
        }
        println!();
        Ok(())
    }
}

fn format_asm_line(item: &DisplayItem, show_bytes: bool, color: Color) -> String {
    let inst = &item.instruction;
    let bytes_str = if show_bytes && !inst.bytes.is_empty() {
        format!("{:<16}  ", inst.bytes)
    } else {
        "".to_string()
    };
    let parts: Vec<&str> = inst.mnemonic.splitn(2, ' ').collect();
    let mnem_word = parts.first().unwrap_or(&"");
    let operands = parts.get(1).unwrap_or(&"");

    format!(
        "    {:08x}  {}{:<8}{}{}",
        inst.address,
        bytes_str.cyan().dimmed(),
        mnem_word.color(color).bold(),
        operands.color(color),
        "".white() // Reset color
    )
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

// TODO: Implement render_stats_table
pub fn render_stats_table() -> color_eyre::Result<()> {
    Ok(())
}
