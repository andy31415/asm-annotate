//! Disassembly backend using Capstone.

use crate::backends::elf::{ElfBackend, GoblinElfBackend};
use color_eyre::eyre::Result;
use std::collections::HashMap;
use std::{fs, path::Path};

use capstone::{Capstone, Insn, arch::BuildsCapstone};
use goblin::elf::Elf as ElfFile;
use goblin::elf::sym;
use lazy_static::lazy_static;
use regex::Regex;

/// Represents a single disassembled instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instruction {
    /// The memory address of the instruction.
    pub address: u64,
    /// A string representation of the instruction's bytes.
    pub bytes: String,
    /// The mnemonic and operands of the instruction.
    pub mnemonic: String,
}

// Holds the ELF file buffer and the parsed ElfFile object
// to ensure the ElfFile does not outlive the buffer it references.
struct LoadedElf<'a> {
    buffer: Vec<u8>,
    elf_obj: ElfFile<'a>,
}

impl<'a> LoadedElf<'a> {
    fn new(buffer: Vec<u8>) -> Result<LoadedElf<'a>> {
        let elf_obj = ElfFile::parse(&buffer)
            .map_err(|e| color_eyre::eyre::eyre!("Failed to parse ELF file: {:#?}", e))?;
        // This is unsafe because we are aliasing the lifetime of elf_obj to 'a,
        // but we know that buffer will live as long as LoadedElf.
        let elf_obj =
            unsafe { std::mem::transmute::<goblin::elf::Elf<'_>, goblin::elf::Elf<'_>>(elf_obj) };
        Ok(LoadedElf { buffer, elf_obj })
    }
}

lazy_static! {
    static ref HEX_ADDR_RE: Regex = Regex::new(r"#?(0x[0-9a-fA-F]+)").unwrap();
}

fn load_and_parse_elf(elf_path: &Path) -> Result<LoadedElf<'static>> {
    let buffer = fs::read(elf_path)
        .map_err(|e| color_eyre::eyre::eyre!("Failed to read ELF file: {:#?}", e))?;
    LoadedElf::new(buffer)
}

fn determine_arm_mode(elf_obj: &ElfFile, start: u64) -> capstone::arch::arm::ArchMode {
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

    if is_thumb {
        capstone::arch::arm::ArchMode::Thumb
    } else {
        capstone::arch::arm::ArchMode::Arm
    }
}

fn initialize_capstone(elf_obj: &ElfFile, start: u64) -> Result<Capstone> {
    let arch = elf_obj.header.e_machine;
    match arch {
        goblin::elf::header::EM_X86_64 => Capstone::new().x86().detail(true).build(),
        goblin::elf::header::EM_ARM => {
            let mode = determine_arm_mode(elf_obj, start);
            Capstone::new().arm().mode(mode).detail(true).build()
        }
        goblin::elf::header::EM_AARCH64 => Capstone::new()
            .arm64()
            .mode(capstone::arch::arm64::ArchMode::Arm)
            .detail(true)
            .build(),
        _ => Err(capstone::Error::CustomError("Unsupported architecture")),
    }
    .map_err(|e| color_eyre::eyre::eyre!("Failed to initialize Capstone: {}", e))
}

fn find_containing_section<'a>(
    loaded_elf: &'a LoadedElf,
    start: u64,
    end: u64,
) -> Result<(&'a [u8], u64)> {
    for section in &loaded_elf.elf_obj.section_headers {
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
            if offset + size > loaded_elf.buffer.len() {
                return Err(color_eyre::eyre::eyre!(
                    "Section bounds exceed buffer size for section at offset {:#x}",
                    offset
                ));
            }
            return Ok((&loaded_elf.buffer[offset..offset + size], sh_addr));
        }
    }
    Err(color_eyre::eyre::eyre!(
        "No executable section found fully containing the range {:#x}-{:#x}",
        start,
        end
    ))
}

fn extract_code_range(
    section_data: &[u8],
    section_addr: u64,
    start: u64,
    end: u64,
) -> Result<&[u8]> {
    if start < section_addr {
        return Err(color_eyre::eyre::eyre!(
            "Start address {:#x} is before section start {:#x}",
            start,
            section_addr
        ));
    }

    let offset_in_section = start - section_addr;
    let length = end - start;

    if (offset_in_section + length) > section_data.len() as u64 {
        return Err(color_eyre::eyre::eyre!(
            "Disassembly range {:#x}-{:#x} (offset: {}, length: {}) exceeds section bounds (data size: {})",
            start,
            end,
            offset_in_section,
            length,
            section_data.len()
        ));
    }

    Ok(&section_data[offset_in_section as usize..(offset_in_section + length) as usize])
}

fn annotate_instruction(insn: &Insn, elf_obj: &ElfFile, elf_backend: &impl ElfBackend) -> String {
    let mnemonic = insn.mnemonic().unwrap_or("");
    let op_str = insn.op_str().unwrap_or("");
    let mut full_mnemonic = format!("{} {}", mnemonic, op_str).trim().to_string();

    if (mnemonic.starts_with('b') || mnemonic == "call" || mnemonic == "jmp")
        && let Some(caps) = HEX_ADDR_RE.captures(op_str)
        && let Some(addr_str) = caps.get(1)
        && let Ok(target_addr) = u64::from_str_radix(&addr_str.as_str()[2..], 16)
        && let Ok(Some(symbol)) = elf_backend.get_symbol_at(elf_obj, target_addr)
    {
        full_mnemonic.push_str(&format!("  ; <{}>", symbol));
    }
    full_mnemonic
}

fn perform_disassembly(
    cs: &Capstone,
    code: &[u8],
    start: u64,
    elf_obj: &ElfFile,
    elf_backend: &impl ElfBackend,
) -> Result<Vec<Instruction>> {
    let insns = cs
        .disasm_all(code, start)
        .map_err(|e| color_eyre::eyre::eyre!("Failed to disassemble: {}", e))?;

    Ok(insns
        .iter()
        .map(|insn: &Insn| {
            let bytes_str = insn
                .bytes()
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<Vec<String>>()
                .join(" ");

            Instruction {
                address: insn.address(),
                bytes: bytes_str,
                mnemonic: annotate_instruction(insn, elf_obj, elf_backend),
            }
        })
        .collect())
}

/// Disassembles a range of addresses within an ELF file.
///
/// This function reads the ELF file, finds the appropriate section,
/// initializes the Capstone disassembler for the detected architecture,
/// and disassembles the code within the given range [start, end).
///
/// # Arguments
///
/// * `elf_path` - Path to the ELF file.
/// * `start` - The starting memory address of the range to disassemble.
/// * `end` - The ending memory address (exclusive) of the range to disassemble.
///
/// # Returns
///
/// A `Result` containing a vector of `Instruction` structs or an error.
pub fn disassemble_range(elf_path: &Path, start: u64, end: u64) -> Result<Vec<Instruction>> {
    let loaded_elf = load_and_parse_elf(elf_path)?;
    let cs = initialize_capstone(&loaded_elf.elf_obj, start)?;
    let (section_data, section_addr) = find_containing_section(&loaded_elf, start, end)?;
    let code = extract_code_range(section_data, section_addr, start, end)?;

    let elf_backend = GoblinElfBackend;
    perform_disassembly(&cs, code, start, &loaded_elf.elf_obj, &elf_backend)
}

/// Replaces mangled symbols in instruction mnemonics with their demangled versions.
///
/// # Arguments
///
/// * `instructions` - A mutable slice of `Instruction` structs to modify.
/// * `demangled_map` - A HashMap where keys are mangled names and values are demangled names.
pub fn apply_demangling(instructions: &mut [Instruction], demangled_map: &HashMap<String, String>) {
    for inst in instructions {
        let mut new_mnemonic = inst.mnemonic.clone();
        for (mangled, demangled) in demangled_map {
            if new_mnemonic.contains(mangled) {
                new_mnemonic = new_mnemonic.replace(mangled, demangled);
            }
        }
        inst.mnemonic = new_mnemonic;
    }
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

        apply_demangling(&mut instructions, &demangled_map);

        assert_eq!(instructions[0].mnemonic, "ret");
        assert_eq!(instructions[1].mnemonic, "call foo()");
    }

    // TODO: Add tests for disassemble_range with a test ELF file
}
