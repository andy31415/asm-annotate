//! Plain-text dump of annotated disassembly, suitable for LLM/automation use.
//!
//! Format: source location comment printed only when it changes (format C),
//! with no ANSI color codes.

use crate::commands::annotate::AnnotationData;
use crate::types::SourceLocation;
use color_eyre::eyre::Result;
use std::path::Path;

/// Dumps the annotation data as compact plain text to stdout.
///
/// Output format:
/// ```
/// ; function: MyFunc [foo.cpp]
/// 1200  push {r4, lr}
/// 1202  cmp r0, #0             ; foo.cpp:38: if (n <= 0)
/// 1204  ble .+14
/// ```
///
/// The source comment is emitted only when the source location changes, keeping
/// output compact for LLM context windows.
pub fn dump_annotation(data: &AnnotationData) -> Result<()> {
    // Print function header with source file hint (from first instruction with a source loc)
    let first_file = data
        .display_items
        .iter()
        .find_map(|item| item.source.as_ref().map(|s| s.file.as_str()));

    let file_hint = first_file
        .and_then(|p| Path::new(p).file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("?");

    println!("; function: {} [{}]", data.display_name, file_hint);

    let mut last_src: Option<SourceLocation> = None;

    for item in &data.display_items {
        let src_changed = item.source != last_src;

        // Format address as short hex (no 0x prefix)
        let addr = format!("{:x}", item.instruction.address);

        if src_changed {
            if let Some(ref src) = item.source {
                let filename = Path::new(&src.file)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&src.file);

                let src_text = data
                    .source_reader
                    .read_line(&src.file, src.line)?
                    .unwrap_or_default();
                let src_text = src_text.trim();

                let asm = &item.instruction.mnemonic;
                let location = format!("{}:{}", filename, src.line);

                if src_text.is_empty() {
                    println!("{addr}  {asm:<40}; {location}");
                } else {
                    println!("{addr}  {asm:<40}; {location}: {src_text}");
                }
            } else {
                // Source location dropped back to unknown
                println!("{addr}  {}", item.instruction.mnemonic);
            }
            last_src = item.source.clone();
        } else {
            println!("{addr}  {}", item.instruction.mnemonic);
        }
    }

    Ok(())
}
