use crate::backends::disasm::Instruction;
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

pub fn annotate_instructions(
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

#[derive(Debug, Clone)]
pub struct RenderGroup {
    pub color: String,
    pub src_file: Option<String>,
    pub src_line_start: Option<usize>,
    pub src_lines: Vec<String>,
    pub instructions: Vec<Instruction>,
    pub show_file_header: bool,
}

const PALETTE: &[&str] = &[
    "#ff6b6b", "#ffd93d", "#6bcb77", "#4d96ff", "#ff922b", "#cc5de8", "#20c997", "#f783ac",
    "#74c0fc", "#a9e34b", "#ff8787", "#ffe066", "#8ce99a", "#74c0fc", "#ffa94d", "#da77f2",
    "#63e6be", "#faa2c1", "#a5d8ff", "#c0eb75",
];

// Basic build_groups implementation
pub fn build_groups(
    instructions: &[Instruction],
    addr_to_src: &HashMap<u64, SourceLocation>,
    // remappings: &[(String, String)], // TODO: Add remappings
) -> color_eyre::Result<Vec<RenderGroup>> {
    let mut groups = Vec::new();
    if instructions.is_empty() {
        return Ok(groups);
    }

    let mut color_map: HashMap<SourceLocation, &str> = HashMap::new();
    let mut color_idx = 0;

    for inst in instructions {
        if let Some(key) = addr_to_src.get(&inst.address) {
            if !color_map.contains_key(key) {
                color_map.insert(key.clone(), PALETTE[color_idx % PALETTE.len()]);
                color_idx += 1;
            }
        }
    }

    let mut current_group: Option<RenderGroup> = None;
    let mut prev_src_key: Option<SourceLocation> = None;
    let mut prev_src_file: Option<String> = None;

    for inst in instructions {
        let src_key = addr_to_src.get(&inst.address).cloned();

        if src_key != prev_src_key {
            if let Some(group) = current_group.take() {
                groups.push(group);
            }

            let color = match &src_key {
                Some(key) => color_map.get(key).unwrap_or(&"#aaaaaa").to_string(),
                None => "#aaaaaa".to_string(),
            };

            if let Some(src_loc) = &src_key {
                // TODO: Read actual source lines
                let src_lines = vec![format!("{}:{}", src_loc.file, src_loc.line)];
                let show_file_header = prev_src_file.as_ref() != Some(&src_loc.file);

                current_group = Some(RenderGroup {
                    color,
                    src_file: Some(src_loc.file.clone()),
                    src_line_start: Some(src_loc.line),
                    src_lines,
                    instructions: Vec::new(),
                    show_file_header,
                });
                prev_src_file = Some(src_loc.file.clone());
            } else {
                current_group = Some(RenderGroup {
                    color,
                    src_file: None,
                    src_line_start: None,
                    src_lines: Vec::new(),
                    instructions: Vec::new(),
                    show_file_header: false,
                });
                prev_src_file = None;
            }
            prev_src_key = src_key;
        }

        if let Some(group) = current_group.as_mut() {
            group.instructions.push(inst.clone());
        }
    }

    if let Some(group) = current_group.take() {
        groups.push(group);
    }

    Ok(groups)
}
