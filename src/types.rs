use crate::backends::disasm::Instruction;
use crate::ui::colors::UI_PALETTE;
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
#[derive(Debug, Clone)]
pub struct DisplayItem {
    pub instruction: Instruction,
    pub source: Option<SourceLocation>,
    pub color: Color,
}

impl DisplayItem {
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
