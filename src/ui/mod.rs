use crate::source_reader::SourceReader;
use crate::types::DisplayItem;
use colored::*;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use unicode_width::UnicodeWidthStr;

pub trait Renderer {
    fn render(
        &self,
        func_name: &str,
        items: &[DisplayItem],
        source_reader: &SourceReader,
    ) -> color_eyre::Result<()>;
}

pub struct UnifiedRenderer {
    pub show_bytes: bool,
}

impl Renderer for UnifiedRenderer {
    fn render(
        &self,
        func_name: &str,
        items: &[DisplayItem],
        _source_reader: &SourceReader,
    ) -> color_eyre::Result<()> {
        render_header(func_name, items)?;

        for item in items {
            if item.is_new_line {
                if item.is_new_file
                    && let Some(ref src) = item.source
                {
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

            println!(
                "{}",
                format_asm_line(item, self.show_bytes, item.color, 120)
            );
        }
        println!();
        Ok(())
    }
}

pub struct SplitRenderer {
    pub show_bytes: bool,
    pub source_width: usize,
    pub asm_width: usize,
}

static ANSI_RE: OnceLock<Regex> = OnceLock::new();

/// Strips ANSI escape codes from a string.
fn strip_ansi(s: &str) -> String {
    let re = ANSI_RE.get_or_init(|| Regex::new(r"\x1B\[[0-?]*[ -/]*[@-~]").unwrap());
    re.replace_all(s, "").into_owned()
}

impl Renderer for SplitRenderer {
    fn render(
        &self,
        func_name: &str,
        items: &[DisplayItem],
        _source_reader: &SourceReader,
    ) -> color_eyre::Result<()> {
        render_header(func_name, items)?;

        let mut i = 0;
        let mut last_file: Option<String> = None;
        let mut last_line: Option<usize> = None;

        while i < items.len() {
            let current_source = items[i].source.clone();
            let color = items[i].color;
            // --- File Header ---
            if let Some(ref src) = current_source
                && last_file.as_ref() != Some(&src.file)
            {
                let short = short_path(&src.file, 3);
                println!(
                    "
-- {} --",
                    short.dimmed().italic()
                );
                last_file = Some(src.file.clone());
                last_line = None; // Reset line tracking when file changes
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
                                    if current_width + char_width > display_width.saturating_sub(3)
                                    {
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
                .map(|item| format_asm_line(item, self.show_bytes, color, self.asm_width))
                .collect();

            // --- Print Side by Side ---
            for (k, asm_line) in asm_lines.iter().enumerate() {
                let src_part = if k == 0 { &source_text } else { "" };
                let asm_part = asm_line;

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

fn format_asm_line(item: &DisplayItem, show_bytes: bool, color: Color, asm_width: usize) -> String {
    let inst = &item.instruction;
    let bytes_str = if show_bytes && !inst.bytes.is_empty() {
        format!("{:<16}  ", inst.bytes)
    } else {
        "".to_string()
    };
    let mut asm_text = format!(
        "    {:08x}  {}{}",
        inst.address,
        bytes_str.cyan().dimmed(),
        inst.mnemonic.color(color).bold(),
    );

    let visible_width = strip_ansi(&asm_text).width();
    if visible_width > asm_width {
        // Naive truncation for now, could be smarter about unicode
        let mut truncated = strip_ansi(&asm_text);
        truncated.truncate(asm_width.saturating_sub(1));
        asm_text = format!("{}…", truncated.color(color).bold());
    }

    format!("{}{}", asm_text, "".white()) // Reset color
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
        for component in components.iter().skip(start_index) {
            result.push(component);
        }
        result.to_string_lossy().to_string()
    } else {
        path_str.to_string()
    }
}

// New SideBySideRenderer
pub struct SideBySideRenderer {
    pub show_bytes: bool,
    pub context_lines: usize,
    pub source_width: usize,
    pub asm_width: usize,
}

impl Renderer for SideBySideRenderer {
    fn render(
        &self,
        func_name: &str,
        items: &[DisplayItem],
        source_reader: &SourceReader,
    ) -> color_eyre::Result<()> {
        use std::collections::{BTreeMap, HashMap};
        render_header(func_name, items)?;

        let mut source_map: BTreeMap<String, BTreeMap<usize, ()>> = BTreeMap::new();
        let mut file_line_color: HashMap<(String, usize), Color> = HashMap::new();

        for item in items {
            if let Some(ref src) = item.source {
                source_map
                    .entry(src.file.clone())
                    .or_default()
                    .insert(src.line, ());
                file_line_color
                    .entry((src.file.clone(), src.line))
                    .or_insert(item.color);
            }
        }

        let mut source_panel_lines: Vec<String> = Vec::new();

        // --- Collect Source Lines ---
        for (file, lines) in &source_map {
            source_panel_lines.push(format!("-- {} --", short_path(file, 3).dimmed().italic()));

            if lines.is_empty() {
                continue;
            }

            let mut sorted_asm_lines: Vec<usize> = lines.keys().cloned().collect();
            sorted_asm_lines.sort();

            let mut ranges: Vec<(usize, usize)> = Vec::new();
            let mut i = 0;
            while i < sorted_asm_lines.len() {
                let current_asm_line = sorted_asm_lines[i];
                let start = std::cmp::max(1, current_asm_line.saturating_sub(self.context_lines));
                let mut end = current_asm_line + self.context_lines;
                let mut j = i + 1;
                while j < sorted_asm_lines.len() {
                    let next_asm_line = sorted_asm_lines[j];
                    if std::cmp::max(1, next_asm_line.saturating_sub(self.context_lines)) <= end + 1
                    {
                        end = next_asm_line + self.context_lines;
                        j += 1;
                    } else {
                        break;
                    }
                }
                ranges.push((start, end));
                i = j;
            }

            let mut last_printed_line: Option<usize> = None;
            for (start, end) in ranges {
                if let Some(last) = last_printed_line
                    && start > last + 1
                {
                    let line_num_str = format!("{:>4}:", "").dimmed();
                    source_panel_lines.push(format!("{} ~", line_num_str));
                }

                for l in start..=end {
                    let color = file_line_color.get(&(file.clone(), l));
                    let line_content = source_reader
                        .read_line(file, l)
                        .unwrap_or(None)
                        .unwrap_or_default();
                    let is_main = lines.contains_key(&l);

                    let line_num_str = format!("{:>4}:", l);

                    let styled_content = if is_main {
                        let c = color.unwrap_or(&Color::White);
                        format!(
                            "{} {} {}",
                            line_num_str.color(*c).bold(),
                            "▶".color(*c).bold(),
                            line_content.color(*c)
                        )
                    } else {
                        format!(
                            "{}   {}",
                            line_num_str.dimmed(),
                            line_content.truecolor(100, 100, 100)
                        )
                    };

                    source_panel_lines.push(styled_content);
                }
                last_printed_line = Some(end);
            }
        }

        // --- Collect Assembly Lines ---
        let asm_panel_lines: Vec<String> = items
            .iter()
            .map(|item| format_asm_line(item, self.show_bytes, item.color, self.asm_width))
            .collect();

        // --- Render Side-by-Side ---
        let max_len = std::cmp::max(source_panel_lines.len(), asm_panel_lines.len());
        for i in 0..max_len {
            let src_line = source_panel_lines.get(i).cloned().unwrap_or_default();
            let asm_line = asm_panel_lines.get(i).cloned().unwrap_or_default();

            // Truncate Source Line
            let stripped_src = strip_ansi(&src_line);
            let src_display = if stripped_src.width() > self.source_width {
                let mut truncated = String::new();
                let mut current_width = 0;
                for char in src_line.chars() {
                    let char_width = UnicodeWidthStr::width(char.encode_utf8(&mut [0u8; 4]));
                    if current_width + char_width > self.source_width {
                        if current_width < self.source_width {
                            truncated.push('…');
                        }
                        break;
                    }
                    truncated.push(char);
                    current_width += char_width;
                }
                truncated
            } else {
                src_line
            };

            let src_width = strip_ansi(&src_display).width();
            let padding = self.source_width.saturating_sub(src_width);

            print!("{}", src_display);
            print!("{}", " ".repeat(padding));
            println!(" {} {}", "|".dimmed(), asm_line);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_path_longer_than_depth() {
        assert_eq!(short_path("/a/b/c/d.c", 3), "…/b/c/d.c");
    }

    #[test]
    fn test_short_path_shorter_than_depth() {
        // Fewer components than depth — returned as-is
        assert_eq!(short_path("/a/b.c", 3), "/a/b.c");
        assert_eq!(short_path("short.c", 3), "short.c");
    }

    #[test]
    fn test_strip_ansi_removes_color_codes() {
        assert_eq!(strip_ansi("\x1B[31mRed\x1B[0m"), "Red");
        assert_eq!(
            strip_ansi("\x1B[1;32mBold Green\x1B[0m text"),
            "Bold Green text"
        );
    }

    #[test]
    fn test_strip_ansi_passthrough() {
        assert_eq!(strip_ansi("no escape codes"), "no escape codes");
        assert_eq!(strip_ansi(""), "");
    }
}
