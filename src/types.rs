use crate::backends::disasm::Instruction;
use crate::source_reader::SourceReader;
use colored::*;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceLocation {
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct AnnotatedInstruction {
    pub instruction: Instruction,
    pub source: Option<SourceLocation>,
}

impl AnnotatedInstruction {
    pub fn from_many(
        instructions: &[Instruction],
        addr_to_src: &HashMap<u64, SourceLocation>,
    ) -> Vec<AnnotatedInstruction> {
        instructions
            .iter()
            .map(|inst| AnnotatedInstruction {
                instruction: inst.clone(),
                source: addr_to_src.get(&inst.address).cloned(),
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct DisplayItem {
    pub instruction: Instruction,
    pub source: Option<SourceLocation>,
    pub source_text: Option<String>, // Single line of source text
    pub color: Color,
    pub is_new_file: bool,
    pub is_new_line: bool,
}

impl DisplayItem {
    pub fn from_annotated(
        annotated_instructions: &[AnnotatedInstruction],
        source_reader: &SourceReader,
    ) -> color_eyre::Result<Vec<DisplayItem>> {
        let mut display_items = Vec::new();
        let mut color_map: HashMap<SourceLocation, Color> = HashMap::new();
        let mut color_idx = 0;
        let mut prev_source: Option<SourceLocation> = None;
        let mut prev_file: Option<String> = None;

        // Simplified color palette from colored crate
        const UI_PALETTE: &[Color] = &[
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Magenta,
            Color::Cyan,
        ];

        for ai in annotated_instructions {
            let mut color = Color::White;
            if let Some(ref src) = ai.source {
                color = *color_map.entry(src.clone()).or_insert_with(|| {
                    let c = UI_PALETTE[color_idx % UI_PALETTE.len()];
                    color_idx += 1;
                    c
                });
            }

            let source_text = if let Some(ref src) = ai.source {
                source_reader.read_line(&src.file, src.line)?
            } else {
                None
            };

            let is_new_file = match &ai.source {
                Some(src) => prev_file.as_ref() != Some(&src.file),
                None => prev_file.is_some(), // New block if we just came from a file
            };

            let is_new_line = ai.source != prev_source;

            display_items.push(DisplayItem {
                instruction: ai.instruction.clone(),
                source: ai.source.clone(),
                source_text,
                color,
                is_new_file,
                is_new_line,
            });

            prev_file = ai.source.as_ref().map(|s| s.file.clone());
            prev_source = ai.source.clone();
        }

        Ok(display_items)
    }
}
