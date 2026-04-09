#![allow(unused_imports)]
use color_eyre::eyre::{Context, Result};
use goblin::elf;
use log;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use gimli::read::{self as _, Dwarf, EndianSlice};
use gimli::{RunTimeEndian, SectionId};

#[derive(Debug, Clone)]
pub struct FunctionInfo {
    pub name: String,
    pub addr: u64,
    pub size: u64,
}

pub trait ElfBackend {
    fn list_functions(&self, elf_path: &Path) -> Result<Vec<FunctionInfo>>;
    fn get_function_bounds(&self, elf_path: &Path, func_name: &str) -> Result<(u64, u64)>;
    fn build_addr_to_src(&self, elf_path: &Path) -> Result<HashMap<u64, (String, usize)>>;
}

pub struct GoblinElfBackend;

impl ElfBackend for GoblinElfBackend {
    fn list_functions(&self, elf_path: &Path) -> Result<Vec<FunctionInfo>> {
        let buffer = fs::read(elf_path).wrap_err("Failed to read ELF file")?;
        let elf = elf::Elf::parse(&buffer).wrap_err("Failed to parse ELF file")?;

        let mut funcs = Vec::new();

        for sym in elf.syms.iter() {
            if sym.st_type() == elf::sym::STT_FUNC && sym.st_size > 0 {
                if let Some(name) = elf.strtab.get_at(sym.st_name) {
                    funcs.push(FunctionInfo {
                        name: name.to_string(),
                        addr: sym.st_value & !1, // Clear Thumb bit
                        size: sym.st_size,
                    });
                }
            }
        }

        for sym in elf.dynsyms.iter() {
            if sym.st_type() == elf::sym::STT_FUNC && sym.st_size > 0 {
                if let Some(name) = elf.dynstrtab.get_at(sym.st_name) {
                    if !funcs
                        .iter()
                        .any(|f| f.name == name && f.addr == (sym.st_value & !1))
                    {
                        funcs.push(FunctionInfo {
                            name: name.to_string(),
                            addr: sym.st_value & !1, // Clear Thumb bit
                            size: sym.st_size,
                        });
                    }
                }
            }
        }

        funcs.sort_by_key(|f| f.addr);
        Ok(funcs)
    }

    fn get_function_bounds(&self, elf_path: &Path, func_name: &str) -> Result<(u64, u64)> {
        let buffer = fs::read(elf_path).wrap_err("Failed to read ELF file")?;
        let elf = elf::Elf::parse(&buffer).wrap_err("Failed to parse ELF file")?;

        for sym in elf.syms.iter() {
            if sym.st_type() == elf::sym::STT_FUNC {
                if let Some(name) = elf.strtab.get_at(sym.st_name) {
                    if name == func_name {
                        let start = sym.st_value & !1; // Clear Thumb bit
                        return Ok((start, start + sym.st_size));
                    }
                }
            }
        }

        for sym in elf.dynsyms.iter() {
            if sym.st_type() == elf::sym::STT_FUNC {
                if let Some(name) = elf.dynstrtab.get_at(sym.st_name) {
                    if name == func_name {
                        let start = sym.st_value & !1; // Clear Thub bit
                        return Ok((start, start + sym.st_size));
                    }
                }
            }
        }

        Err(color_eyre::eyre::eyre!(
            "Function '{}' not found in ELF symbol table.",
            func_name
        ))
    }

    fn build_addr_to_src(&self, elf_path: &Path) -> Result<HashMap<u64, (String, usize)>> {
        let buffer = fs::read(elf_path).wrap_err("Failed to read ELF file")?;
        let elf = elf::Elf::parse(&buffer).wrap_err("Failed to parse ELF file")?;

        let endian = if elf.header.e_ident[elf::header::EI_DATA] == elf::header::ELFDATA2LSB {
            RunTimeEndian::Little
        } else {
            RunTimeEndian::Big
        };

        let load_section = |id: SectionId| -> Result<EndianSlice<RunTimeEndian>, gimli::Error> {
            let data = elf
                .section_headers
                .iter()
                .find(|sh| elf.shdr_strtab.get_at(sh.sh_name) == Some(id.name()))
                .and_then(|sh| {
                    buffer.get(sh.sh_offset as usize..(sh.sh_offset + sh.sh_size) as usize)
                })
                .unwrap_or(&[]);
            Ok(EndianSlice::new(data, endian))
        };

        let dwarf: Dwarf<EndianSlice<RunTimeEndian>> = Dwarf::load(load_section)?;

        let mut mapping = HashMap::new();
        let mut iter = dwarf.units();
        while let Some(header) = iter.next()? {
            let unit = dwarf.unit(header)?;
            if let Some(program) = unit.line_program.clone() {
                let header = program.header();
                let mut rows = program.rows();
                while let Some((_, row)) = rows.next_row()? {
                    if row.end_sequence() {
                        continue;
                    }

                    if let Some(file_entry) = header.file(row.file_index()) {
                        let mut path = PathBuf::new();

                        // Base directory (DW_AT_comp_dir)
                        let mut base_dir = PathBuf::new();
                        if let Some(comp_dir_offset) = unit.comp_dir {
                            if let Ok(rb) = dwarf.string(comp_dir_offset) {
                                base_dir.push(rb.to_string_lossy().as_ref());
                            }
                        }

                        // File directory from line program header
                        let dir_index = file_entry.directory_index();
                        if dir_index != 0 {
                            if let Some(dir_offset) =
                                header.include_directories().get(dir_index as usize - 1)
                            {
                                if let Ok(rb) = dwarf.string(*dir_offset) {
                                    let dir_str = rb.to_string_lossy();
                                    let dir_path = Path::new(dir_str.as_ref());
                                    if dir_path.is_absolute() {
                                        path = dir_path.to_path_buf();
                                    } else {
                                        path = base_dir.join(dir_path);
                                    }
                                }
                            }
                        } else {
                            path = base_dir;
                        }

                        // File name from line program header
                        if let Ok(rb) = dwarf.string(file_entry.path_name()) {
                            path.push(rb.to_string_lossy().as_ref());
                        }

                        if !path.as_os_str().is_empty() {
                            if let Some(line) = row.line() {
                                mapping.insert(
                                    row.address(),
                                    (path.to_string_lossy().into_owned(), line.get() as usize),
                                );
                            }
                        }
                    }
                }
            }
        }

        if mapping.is_empty() {
            log::warn!("No DWARF line information was found. Build with -g to get source mapping.");
        }

        Ok(mapping)
    }
}
