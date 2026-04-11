//! Builds a `Vec<SourceItem>` from assembly display items and source reader settings.
//!
//! This is the only place that groups display items by source file, computes context
//! ranges, and reads source text from disk. The result is a flat, ordered list that
//! the TUI (and other consumers) can render without any further grouping logic.

use crate::source_reader::SourceReader;
use crate::types::{DisplayItem, SourceItem};
use std::collections::{BTreeMap, HashMap};

/// Builds a source-ordered list of items to display in the source panel.
///
/// For each source file referenced by `display_items`, this function:
/// 1. Emits a `SourceItem::FileHeader`.
/// 2. Computes context ranges around the directly-mapped lines (controlled by
///    `pre_post_context` and `inter_context`).
/// 3. Emits `SourceItem::Line` entries for every line in those ranges, with the
///    assembly-palette color set on directly-mapped lines and `None` on context lines.
/// 4. Emits `SourceItem::Gap` between non-consecutive ranges within a file.
pub fn build_source_view(
    display_items: &[DisplayItem],
    source_reader: &SourceReader,
    pre_post_context: usize,
    inter_context: usize,
) -> Vec<SourceItem> {
    // Build two lookup tables keyed by (file, line):
    //   source_map: which lines are directly mapped to assembly, per file
    //   line_color:  the assembly-palette color for each such line
    let mut source_map: BTreeMap<String, BTreeMap<usize, ()>> = BTreeMap::new();
    let mut line_color: HashMap<(String, usize), colored::Color> = HashMap::new();

    for item in display_items {
        if let Some(ref src) = item.source {
            source_map
                .entry(src.file.clone())
                .or_default()
                .insert(src.line, ());
            line_color
                .entry((src.file.clone(), src.line))
                .or_insert(item.color);
        }
    }

    let mut result = Vec::new();

    for (file, asm_lines) in &source_map {
        result.push(SourceItem::FileHeader { path: file.clone() });

        if asm_lines.is_empty() {
            continue;
        }

        let mut sorted_lines: Vec<usize> = asm_lines.keys().cloned().collect();
        sorted_lines.sort_unstable();

        // Merge overlapping/adjacent context windows into ranges.
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        let mut i = 0;
        while i < sorted_lines.len() {
            let cur = sorted_lines[i];
            let ctx = if i == 0 {
                pre_post_context
            } else {
                inter_context
            };
            let start = cur.saturating_sub(ctx).max(1);
            let mut end = cur + inter_context;
            let mut j = i + 1;
            while j < sorted_lines.len() {
                let next = sorted_lines[j];
                if next.saturating_sub(inter_context).max(1) <= end + 1 {
                    end = next + inter_context;
                    j += 1;
                } else {
                    break;
                }
            }
            ranges.push((start, end));
            i = j;
        }

        // Use pre_post_context for the trailing edge of the last range.
        if let Some(last) = ranges.last_mut() {
            last.1 = sorted_lines.last().unwrap() + pre_post_context;
        }

        let mut last_end: Option<usize> = None;
        for (start, end) in ranges {
            if let Some(prev_end) = last_end
                && start > prev_end + 1
            {
                result.push(SourceItem::Gap);
            }

            for line_num in start..=end {
                let text = source_reader
                    .read_line(file, line_num)
                    .unwrap_or(None)
                    .unwrap_or_default();
                let is_main = asm_lines.contains_key(&line_num);
                let color = line_color.get(&(file.clone(), line_num)).copied();

                result.push(SourceItem::Line {
                    number: line_num,
                    text,
                    color: if is_main { color } else { None },
                    is_main,
                });
            }
            last_end = Some(end);
        }
    }

    result
}
