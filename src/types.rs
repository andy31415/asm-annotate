//! Common data types used throughout the application.

use crate::backends::disasm::Instruction;
use crate::ui::colors::UI_PALETTE;
use colored::*;
use std::collections::HashMap;

/// Represents a specific location in a source file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceLocation {
    /// The path to the source file.
    pub file: String,
    /// The 1-based line number in the file.
    pub line: usize,
}

/// An assembly instruction annotated with its optional source code location.
#[derive(Debug, Clone)]
pub struct AnnotatedInstruction {
    /// The core disassembled instruction.
    pub instruction: Instruction,
    /// The source location (file and line) this instruction originated from, if known.
    pub source: Option<SourceLocation>,
}

impl AnnotatedInstruction {
    /// Creates a vector of `AnnotatedInstruction` from raw instructions and an address-to-source map.
    ///
    /// The source location for an instruction is determined by looking up its address in the `addr_to_src` map.
    /// If an instruction doesn't have a direct entry, it inherits the source location from the nearest preceding
    /// instruction that does have an entry. This helps associate blocks of assembly with a single source line.
    ///
    /// # Arguments
    ///
    /// * `instructions` - A slice of `Instruction` structs to annotate.
    /// * `addr_to_src` - A map from memory address to `SourceLocation` derived from debug info.
    ///
    /// # Returns
    ///
    /// A vector of `AnnotatedInstruction`s.
    pub fn from_many(
        instructions: &[Instruction],
        addr_to_src: &HashMap<u64, SourceLocation>,
    ) -> Vec<AnnotatedInstruction> {
        let mut result = Vec::new();
        let mut last_src: Option<SourceLocation> = None;

        for inst in instructions {
            if let Some(src) = addr_to_src.get(&inst.address) {
                last_src = Some(src.clone());
            }
            result.push(AnnotatedInstruction {
                instruction: inst.clone(),
                source: last_src.clone(),
            });
        }
        result
    }
}

/// Represents an item to be displayed in the TUI, including color information.
#[derive(Debug, Clone)]
pub struct DisplayItem {
    /// The core disassembled instruction.
    pub instruction: Instruction,
    /// The source location, if any.
    pub source: Option<SourceLocation>,
    /// The color to use when displaying this item, typically based on the source location.
    pub color: Color,
}

impl DisplayItem {
    /// Converts a slice of `AnnotatedInstruction`s into `DisplayItem`s, assigning colors.
    ///
    /// Each unique `SourceLocation` is assigned a color from the `UI_PALETTE`.
    /// Instructions with no source or the same source location share the same color.
    ///
    /// # Arguments
    ///
    /// * `annotated_instructions` - A slice of `AnnotatedInstruction`s.
    ///
    /// # Returns
    ///
    /// A `Result` containing a vector of `DisplayItem`s.
    pub fn from_annotated(
        annotated_instructions: &[AnnotatedInstruction],
    ) -> color_eyre::Result<Vec<DisplayItem>> {
        let mut display_items = Vec::new();
        let mut color_map: HashMap<SourceLocation, Color> = HashMap::new();
        let mut color_idx = 0;

        for ai in annotated_instructions {
            let mut color = Color::White;
            if let Some(ref src) = ai.source {
                color = *color_map.entry(src.clone()).or_insert_with(|| {
                    let c = UI_PALETTE[color_idx % UI_PALETTE.len()];
                    color_idx += 1;
                    c
                });
            }

            display_items.push(DisplayItem {
                instruction: ai.instruction.clone(),
                source: ai.source.clone(),
                color,
            });
        }

        Ok(display_items)
    }
}
