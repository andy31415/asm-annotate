use crate::backends::disasm::Instruction;

#[derive(Debug, Clone)]
pub struct RenderGroup {
    pub color: String,
    pub src_file: Option<String>,
    pub src_line_start: Option<usize>,
    pub src_lines: Vec<String>,
    pub instructions: Vec<Instruction>,
    pub show_file_header: bool,
}
