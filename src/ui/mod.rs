use crate::backends::disasm::Instruction;
use crate::types::{DisplayItem, SourceLocation};
use colored::*;
use regex::Regex;
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthStr;

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
                if item.is_new_file
                    && let Some(ref src) = item.source {
                        let short = short_path(&src.file, 3);
                        println!(
                            "  {}",
                            format!("<{}:{}>", short, src.line).dimmed().italic()
                        );
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

// Helper to strip ANSI escape codes
fn strip_ansi(s: &str) -> String {
    let re = Regex::new(r"\x1B\[[0-?]*[ -/]*[@-~]").unwrap();
    re.replace_all(s, "").into_owned()
}

impl Renderer for SplitRenderer {
    fn render(&self, func_name: &str, items: &[DisplayItem]) -> color_eyre::Result<()> {
        render_header(func_name, items)?;

        let mut i = 0;
        let mut last_file: Option<String> = None;
        let mut last_line: Option<usize> = None;

        while i < items.len() {
            let current_source = items[i].source.clone();
            let color = items[i].color;
            // --- File Header ---
            if let Some(ref src) = current_source {
                if last_file.as_ref() != Some(&src.file) {
                    let short = short_path(&src.file, 3);
                    println!(
                        "
-- {} --",
                        short.dimmed().italic()
                    );
                    last_file = Some(src.file.clone());
                    last_line = None; // Reset line tracking when file changes
                }
            }

            let mut j = i;
            while j < items.len() && items[j].source == current_source {
                j += 1;
            }
            let group = &items[i..j];

            // --- Source Side Text (prepared once for the group) ---
            let source_text = if let Some(ref src) = current_source {
                if last_line != Some(src.line) {
                    last_line = Some(src.line);
                    items[i]
                        .source_text
                        .as_ref()
                        .map(|text| {
                            let line_num_str = format!("{:>4}:", src.line);
                            let marker = "▶ ";
                            // Visible lengths: line number + marker
                            let prefix_len = line_num_str.width() + marker.width();
                            let display_width = self.source_width.saturating_sub(prefix_len);

                            let mut src_text = text.clone();
                            if text.width() > display_width {
                                // Truncate based on display width
                                let mut current_width = 0;
                                let mut truncate_at = text.len();
                                for (i, c) in text.char_indices() {
                                    let char_width =
                                        UnicodeWidthStr::width(c.encode_utf8(&mut [0u8; 4]));
                                    if current_width + char_width > display_width.saturating_sub(3) {
                                        truncate_at = i;
                                        break;
                                    }
                                    current_width += char_width;
                                }
                                src_text.truncate(truncate_at);
                                src_text.push('…');
                            }
                            format!(
                                "{} {} {}",
                                line_num_str.dimmed(),
                                marker.color(color).bold(),
                                src_text.color(color)
                            )
                        })
                        .unwrap_or_else(|| {
                            let line_num_str = format!("{:>4}:", src.line).dimmed();
                            format!(
                                "{} {} {}",
                                line_num_str,
                                "▶ ".color(color).bold(),
                                "?".color(color)
                            )
                        })
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            // --- Assembly Lines ---
            let asm_lines: Vec<String> = group
                .iter()
                .map(|item| format_asm_line(item, self.show_bytes, color))
                .collect();

            // --- Print Side by Side ---
            for k in 0..asm_lines.len() {
                let src_part = if k == 0 { &source_text } else { "" };
                let asm_part = &asm_lines[k];

                let stripped_src = strip_ansi(src_part);
                let src_part_width = stripped_src.width();
                let padding = self.source_width.saturating_sub(src_part_width);

                // Print in separate steps for accurate spacing
                print!("{}", src_part);
                print!("{}", " ".repeat(padding));
                println!(" {} {}", "|".dimmed(), asm_part);
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
    format!(
        "    {:08x}  {}{}{}",
        inst.address,
        bytes_str.cyan().dimmed(),
        inst.mnemonic.color(color).bold(),
        "".white() // Reset color
    )
}
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
