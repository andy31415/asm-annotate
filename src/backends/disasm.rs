use crate::backends::elf::{ElfBackend, GoblinElfBackend};
use color_eyre::eyre::Result;
use std::collections::HashMap;
use std::{fs, path::Path};

use capstone::{Capstone, Insn, arch::BuildsCapstone};
use goblin::elf::Elf as ElfFile;
use goblin::elf::sym;
use lazy_static::lazy_static;
use regex::Regex;

#[derive(Debug, Clone)]
pub struct Instruction {
    pub address: u64,
    pub bytes: String,
    pub mnemonic: String,
}

lazy_static! {
    static ref HEX_ADDR_RE: Regex = Regex::new(r"#?(0x[0-9a-fA-F]+)").unwrap();
}

pub fn disassemble_range(elf_path: &Path, start: u64, end: u64) -> Result<Vec<Instruction>> {
    let buffer = fs::read(elf_path)
        .map_err(|e| color_eyre::eyre::eyre!("Failed to read ELF file: {:#?}", e))?;
    let elf_obj = ElfFile::parse(&buffer)
        .map_err(|e| color_eyre::eyre::eyre!("Failed to parse ELF file: {:#?}", e))?;

    let arch = elf_obj.header.e_machine;
    let cs = match arch {
        goblin::elf::header::EM_X86_64 => Capstone::new().x86().detail(true).build(),
        goblin::elf::header::EM_ARM => {
            // Determine ARM vs Thumb mode by checking the LSB of the function's start address.
            let mut is_thumb = false;
            for sym in elf_obj.syms.iter() {
                if sym.st_type() == sym::STT_FUNC && (sym.st_value & !1) == start {
                    if (sym.st_value & 1) == 1 {
                        is_thumb = true;
                    }
                    break;
                }
            }
            if !is_thumb {
                for sym in elf_obj.dynsyms.iter() {
                    if sym.st_type() == sym::STT_FUNC && (sym.st_value & !1) == start {
                        if (sym.st_value & 1) == 1 {
                            is_thumb = true;
                        }
                        break;
                    }
                }
            }

            let mode = if is_thumb {
                capstone::arch::arm::ArchMode::Thumb
            } else {
                capstone::arch::arm::ArchMode::Arm
            };
            Capstone::new().arm().mode(mode).detail(true).build()
        }
        goblin::elf::header::EM_AARCH64 => Capstone::new()
            .arm64()
            .mode(capstone::arch::arm64::ArchMode::Arm)
            .detail(true)
            .build(),
        _ => Err(capstone::Error::CustomError("Unsupported architecture")),
    }
    .map_err(|e| color_eyre::eyre::eyre!("Failed to initialize Capstone: {}", e))?;

    // Find the section containing the range [start, end)
    let mut section_data: Option<&[u8]> = None;
    let mut section_addr = 0;

    for section in &elf_obj.section_headers {
        if section.sh_type != goblin::elf::section_header::SHT_PROGBITS
            || (section.sh_flags & goblin::elf::section_header::SHF_EXECINSTR as u64) == 0
        {
            continue;
        }

        let sh_addr = section.sh_addr;
        let sh_size = section.sh_size;
        let section_end = sh_addr.saturating_add(sh_size);

        if sh_addr <= start && end <= section_end {
            let offset = section.sh_offset as usize;
            let size = section.sh_size as usize;
            if offset + size > buffer.len() {
                return Err(color_eyre::eyre::eyre!(
                    "Section bounds exceed buffer size for section at offset {:#x}",
                    offset
                ));
            }
            section_data = Some(&buffer[offset..offset + size]);
            section_addr = sh_addr;
            break;
        }
    }

    let data = section_data.ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "No executable section found fully containing the range {:#x}-{:#x}",
            start,
            end
        )
    })?;

    if start < section_addr {
        return Err(color_eyre::eyre::eyre!(
            "Start address {:#x} is before section start {:#x}",
            start,
            section_addr
        ));
    }

    let offset_in_section = start - section_addr;
    let length = end - start;

    if (offset_in_section + length) > data.len() as u64 {
        return Err(color_eyre::eyre::eyre!(
            "Disassembly range {:#x}-{:#x} (offset: {}, length: {}) exceeds section bounds (data size: {})",
            start,
            end,
            offset_in_section,
            length,
            data.len()
        ));
    }

    let code = &data[offset_in_section as usize..(offset_in_section + length) as usize];

    let insns = cs
        .disasm_all(code, start)
        .map_err(|e| color_eyre::eyre::eyre!("Failed to disassemble: {}", e))?;

    let elf_backend = GoblinElfBackend;

    let instructions: Vec<Instruction> = insns
        .iter()
        .map(|insn: &Insn| {
            let bytes_str = insn
                .bytes()
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<Vec<String>>()
                .join(" ");

            let mnemonic = insn.mnemonic().unwrap_or("");
            let op_str = insn.op_str().unwrap_or("");

            let mut full_mnemonic = format!("{} {}", mnemonic, op_str).trim().to_string();

            if mnemonic.starts_with('b') || mnemonic == "call" || mnemonic == "jmp" {
                if let Some(caps) = HEX_ADDR_RE.captures(op_str) {
                    if let Some(addr_str) = caps.get(1) {
                        if let Ok(target_addr) = u64::from_str_radix(&addr_str.as_str()[2..], 16) {
                            if let Ok(Some(symbol)) = elf_backend.get_symbol_at(elf_path, target_addr) {
                                full_mnemonic.push_str(&format!("  ; <{}>", symbol));
                            }
                        }
                    }
                }
            }

            Instruction {
                address: insn.address(),
                bytes: bytes_str,
                mnemonic: full_mnemonic,
            }
        })
        .collect();

    Ok(instructions)
}

/// Applies demangled names to a function name and its instructions.
pub fn apply_demangling(
    func_name: String,
    instructions: &mut [Instruction],
    demangled_map: &HashMap<String, String>,
) -> String {
    let new_func_name = demangled_map.get(&func_name).unwrap_or(&func_name).clone();

    for inst in instructions {
        let mut new_mnemonic = inst.mnemonic.clone();
        for (mangled, demangled) in demangled_map {
            if new_mnemonic.contains(mangled) {
                new_mnemonic = new_mnemonic.replace(mangled, demangled);
            }
        }
        inst.mnemonic = new_mnemonic;
    }

    new_func_name
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_apply_demangling() {
        let mut instructions = vec![
            Instruction {
                address: 0x1000,
                bytes: "C3".to_string(),
                mnemonic: "ret".to_string(),
            },
            Instruction {
                address: 0x1001,
                bytes: "E8 00000000".to_string(),
                mnemonic: "call _Z3foov".to_string(),
            },
        ];
        let mut demangled_map = HashMap::new();
        demangled_map.insert("_Z3foov".to_string(), "foo()".to_string());
        demangled_map.insert("_Z3bariz".to_string(), "bar(int, ...)".to_string());

        let new_func_name =
            apply_demangling("_Z3bariz".to_string(), &mut instructions, &demangled_map);

        assert_eq!(new_func_name, "bar(int, ...)");
        assert_eq!(instructions[0].mnemonic, "ret");
        assert_eq!(instructions[1].mnemonic, "call foo()");
    }

    #[test]
    fn test_apply_demangling_unknown_func() {
        let mut instructions = vec![];
        let demangled_map = HashMap::new();
        // Function name not in map → returned unchanged
        let name = apply_demangling("plain_func".to_string(), &mut instructions, &demangled_map);
        assert_eq!(name, "plain_func");
    }

    // TODO: Add tests for disassemble_range with a test ELF file
}
