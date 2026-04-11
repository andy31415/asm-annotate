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
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// An item in the source code view panel.
///
/// A `Vec<SourceItem>` fully describes what to show in the source pane — the UI
/// layer only needs to format and render each variant, not perform any grouping logic.
#[derive(Debug, Clone)]
pub enum SourceItem {
    /// A header line separating source files (e.g. `-- path/to/file.cpp --`).
    FileHeader { path: String },
    /// A line of source code.
    Line {
        number: usize,
        text: String,
        /// Color from the assembly palette when this line maps to assembly instructions;
        /// `None` for context-only lines that are shown for surrounding context.
        color: Option<Color>,
        /// `true` when this line is directly mapped to one or more assembly instructions
        /// (as opposed to a context line shown nearby). Used to apply bold styling.
        is_main: bool,
    },
    /// A gap marker (`~`) between non-consecutive line groups within a file.
    Gap,
}

/// Represents an item to be displayed in the TUI, including color information.
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::disasm::Instruction;
    use crate::ui::colors::UI_PALETTE;
    use colored::Color;
    use std::collections::HashMap;

    fn make_inst(address: u64, mnemonic: &str) -> Instruction {
        Instruction {
            address,
            bytes: "".to_string(),
            mnemonic: mnemonic.to_string(),
        }
    }

    fn make_src(file: &str, line: usize) -> SourceLocation {
        SourceLocation {
            file: file.to_string(),
            line,
        }
    }

    #[test]
    fn test_annotated_from_many() {
        let instructions = vec![
            make_inst(0x1000, "mov"),
            make_inst(0x1001, "add"),
            make_inst(0x1002, "sub"),
            make_inst(0x1004, "jmp"),
            make_inst(0x1005, "nop"),
        ];

        let mut addr_to_src = HashMap::new();
        addr_to_src.insert(0x1000, make_src("a.c", 10));
        addr_to_src.insert(0x1004, make_src("b.c", 20));

        let annotated = AnnotatedInstruction::from_many(&instructions, &addr_to_src);

        assert_eq!(annotated.len(), 5);
        assert_eq!(annotated[0].source, Some(make_src("a.c", 10)));
        assert_eq!(annotated[1].source, Some(make_src("a.c", 10))); // Propagated
        assert_eq!(annotated[2].source, Some(make_src("a.c", 10))); // Propagated
        assert_eq!(annotated[3].source, Some(make_src("b.c", 20)));
        assert_eq!(annotated[4].source, Some(make_src("b.c", 20))); // Propagated

        // Test empty instructions
        let empty_annotated = AnnotatedInstruction::from_many(&[], &addr_to_src);
        assert!(empty_annotated.is_empty());

        // Test empty addr_to_src
        let no_src_annotated = AnnotatedInstruction::from_many(&instructions, &HashMap::new());
        for item in no_src_annotated {
            assert_eq!(item.source, None);
        }
    }

    #[test]
    fn test_display_item_from_annotated() {
        let src1 = make_src("a.c", 10);
        let src2 = make_src("b.c", 20);
        let annotated = vec![
            AnnotatedInstruction {
                instruction: make_inst(0x1000, "nop"),
                source: None,
            },
            AnnotatedInstruction {
                instruction: make_inst(0x1001, "mov"),
                source: Some(src1.clone()),
            },
            AnnotatedInstruction {
                instruction: make_inst(0x1002, "add"),
                source: Some(src1.clone()),
            },
            AnnotatedInstruction {
                instruction: make_inst(0x1003, "sub"),
                source: Some(src2.clone()),
            },
            AnnotatedInstruction {
                instruction: make_inst(0x1004, "jmp"),
                source: None,
            },
        ];

        let display_items = DisplayItem::from_annotated(&annotated).unwrap();

        assert_eq!(display_items.len(), 5);

        // No source -> White
        assert_eq!(display_items[0].color, Color::White);
        assert_eq!(display_items[4].color, Color::White);

        // src1 gets first palette color
        assert_eq!(display_items[1].color, UI_PALETTE[0]);
        assert_eq!(display_items[2].color, UI_PALETTE[0]);

        // src2 gets second palette color
        assert_eq!(display_items[3].color, UI_PALETTE[1]);

        // Check struct fields are copied
        assert_eq!(display_items[1].instruction.address, 0x1001);
        assert_eq!(display_items[1].source, Some(src1));
    }
}
