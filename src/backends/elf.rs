use crate::types::SourceLocation;
use color_eyre::eyre::{Context, Result};
use goblin::elf;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use gimli::read::{Dwarf, EndianSlice};
use gimli::{AttributeValue, LineProgramHeader, LineRow, RunTimeEndian, SectionId};

#[derive(Debug, Clone)]
pub struct FunctionInfo {
    pub name: String,
    pub addr: u64,
    pub size: u64,
}

fn source_location(
    base_dir: &Option<String>,
    header: &LineProgramHeader<EndianSlice<RunTimeEndian>>,
    row: &LineRow,
) -> Option<SourceLocation> {
    let line = row.line()?;

    let file_entry = header.file(row.file_index())?;
    let mut path = PathBuf::new();

    let base_dir = match base_dir {
        Some(path) => {
            let mut x = PathBuf::new();
            x.push(path);
            x
        }
        None => PathBuf::new(),
    };

    // File directory from line program header
    let dir_index = file_entry.directory_index();
    if dir_index != 0 {
        if let Some(dir_offset) = header.include_directories().get(dir_index as usize - 1)
            && let AttributeValue::String(slice) = dir_offset
        {
            let path_str = String::from_utf8_lossy(slice.slice());
            log::trace!("HAVE DIR OFFSET: {:#?}", path_str);
            let dir_path = Path::new(path_str.as_ref());
            if dir_path.is_absolute() {
                path.clear();
                path.push(dir_path);
            } else {
                path = base_dir.join(dir_path);
            }
        }
    } else {
        path.push(base_dir);
    }

    // File name from line program header
    if let AttributeValue::String(slice) = file_entry.path_name() {
        let path_str = String::from_utf8_lossy(slice.slice());
        log::trace!("FILE PATH NAME: {:#?}", path_str);
        path.push(path_str.as_ref());
    }

    if path.as_os_str().is_empty() {
        return None;
    }

    Some(SourceLocation {
        file: path.to_string_lossy().into_owned(),
        line: line.get() as usize,
    })
}

pub trait ElfBackend {
    fn list_functions(&self, elf_path: &Path) -> Result<Vec<FunctionInfo>>;
    fn get_function_bounds(&self, elf_path: &Path, func_name: &str) -> Result<(u64, u64)>;

    // Build a mapping for:
    //    - address -> (source file: line-number)
    fn build_addr_to_src(&self, elf_path: &Path) -> Result<HashMap<u64, SourceLocation>>;

    fn get_symbol_at(&self, elf: &elf::Elf, addr: u64) -> Result<Option<String>>;
}

pub struct GoblinElfBackend;

#[derive(Default)]
pub struct DwarfLoader {
    buffer: Vec<u8>,
}

impl DwarfLoader {
    fn load(&mut self, elf_path: &Path) -> Result<Dwarf<EndianSlice<'_, RunTimeEndian>>> {
        self.buffer = fs::read(elf_path).wrap_err("Failed to read ELF file")?;

        let elf = elf::Elf::parse(&self.buffer).wrap_err("Failed to parse ELF file")?;

        let endian = if elf.header.e_ident[elf::header::EI_DATA] == elf::header::ELFDATA2LSB {
            RunTimeEndian::Little
        } else {
            RunTimeEndian::Big
        };

        let load_section = |id: SectionId| -> Result<EndianSlice<RunTimeEndian>, gimli::Error> {
            log::trace!("Loading section {:#?}", id);
            let data = elf
                .section_headers
                .iter()
                .find(|sh| elf.shdr_strtab.get_at(sh.sh_name) == Some(id.name()))
                .and_then(|sh| {
                    self.buffer
                        .get(sh.sh_offset as usize..(sh.sh_offset + sh.sh_size) as usize)
                })
                .unwrap_or(&[]);
            Ok(EndianSlice::new(data, endian))
        };

        log::trace!("Loading dwarf...");
        Dwarf::load(load_section).wrap_err("Failed to load dwarf")
    }
}

impl ElfBackend for GoblinElfBackend {
    fn list_functions(&self, elf_path: &Path) -> Result<Vec<FunctionInfo>> {
        let buffer = fs::read(elf_path).wrap_err("Failed to read ELF file")?;
        let elf = elf::Elf::parse(&buffer).wrap_err("Failed to parse ELF file")?;

        let mut funcs = Vec::new();

        for sym in elf.syms.iter() {
            if sym.st_type() == elf::sym::STT_FUNC
                && sym.st_size > 0
                && let Some(name) = elf.strtab.get_at(sym.st_name)
            {
                funcs.push(FunctionInfo {
                    name: name.to_string(),
                    addr: sym.st_value & !1, // Clear Thumb bit
                    size: sym.st_size,
                });
            }
        }

        // In python, dynsyms are only added if not already present in syms.
        for sym in elf.dynsyms.iter() {
            if sym.st_type() == elf::sym::STT_FUNC
                && sym.st_size > 0
                && let Some(name) = elf.dynstrtab.get_at(sym.st_name)
                && !funcs
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

        funcs.sort_by_key(|f| f.addr);
        Ok(funcs)
    }

    fn get_function_bounds(&self, elf_path: &Path, func_name: &str) -> Result<(u64, u64)> {
        let buffer = fs::read(elf_path).wrap_err("Failed to read ELF file")?;
        let elf = elf::Elf::parse(&buffer).wrap_err("Failed to parse ELF file")?;

        // Prioritize symbols from the main symbol table
        for sym in elf.syms.iter() {
            if sym.st_type() == elf::sym::STT_FUNC
                && let Some(name) = elf.strtab.get_at(sym.st_name)
                && name == func_name
            {
                let start = sym.st_value & !1; // Clear Thumb bit
                return Ok((start, start + sym.st_size));
            }
        }

        // Fallback to dynamic symbols
        for sym in elf.dynsyms.iter() {
            if sym.st_type() == elf::sym::STT_FUNC
                && let Some(name) = elf.dynstrtab.get_at(sym.st_name)
                && name == func_name
            {
                let start = sym.st_value & !1; // Clear Thumb bit
                return Ok((start, start + sym.st_size));
            }
        }

        Err(color_eyre::eyre::eyre!(
            "Function '{}' not found in ELF symbol table.",
            func_name
        ))
    }

    fn build_addr_to_src(&self, elf_path: &Path) -> Result<HashMap<u64, SourceLocation>> {
        let mut loader = DwarfLoader::default();
        let dwarf = loader.load(elf_path)?;

        let mut mapping = HashMap::new();

        let mut iter = dwarf.units();
        while let Some(header) = iter.next()? {
            let unit = dwarf.unit(header)?;
            let program = unit.line_program;

            if program.is_none() {
                continue;
            }

            // Base directory (DW_AT_comp_dir)
            let base_dir = unit.comp_dir.map(|comp_dir_offset| {
                String::from_utf8_lossy(comp_dir_offset.slice()).into_owned()
            });

            // we checked none above
            let program = program.unwrap();
            // log::trace!("PROGRAM: {:#?}", program);

            let header = program.header().clone();
            let mut rows = program.rows();

            while let Some((_, row)) = rows.next_row()? {
                if row.end_sequence() {
                    continue;
                }

                if let Some(src_loc) = source_location(&base_dir, &header, row) {
                    log::debug!("FOUND {}:{}", src_loc.file, src_loc.line);
                    mapping.insert(row.address(), src_loc);
                }
            }
        }

        if mapping.is_empty() {
            log::warn!("No DWARF line information was found. Build with -g to get source mapping.");
        }

        Ok(mapping)
    }

    fn get_symbol_at(&self, elf: &elf::Elf, addr: u64) -> Result<Option<String>> {
        let addr = addr & !1; // Clear Thumb bit

        // First, check for an exact function match at the address
        for sym in elf.syms.iter().chain(elf.dynsyms.iter()) {
            if sym.st_type() == elf::sym::STT_FUNC {
                let sym_addr = sym.st_value & !1;
                if sym_addr == addr {
                    let name = elf
                        .strtab
                        .get_at(sym.st_name)
                        .or_else(|| elf.dynstrtab.get_at(sym.st_name));
                    if let Some(name) = name {
                        return Ok(Some(name.to_string()));
                    }
                }
            }
        }

        // Fallback: find the smallest symbol containing the address
        let mut best_match: Option<(String, u64)> = None;
        for sym in elf.syms.iter().chain(elf.dynsyms.iter()) {
            let sym_addr = sym.st_value & !1;
            // Only consider symbols that start at or before the address
            if sym_addr <= addr {
                let sym_size = sym.st_size;
                if addr < sym_addr + sym_size {
                    let name = elf
                        .strtab
                        .get_at(sym.st_name)
                        .or_else(|| elf.dynstrtab.get_at(sym.st_name));
                    if let Some(name) = name {
                        match best_match {
                            Some((_, best_size)) => {
                                if sym_size > 0 && sym_size < best_size {
                                    best_match = Some((name.to_string(), sym_size));
                                }
                            }
                            None => {
                                if sym_size > 0 {
                                    best_match = Some((name.to_string(), sym_size));
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(best_match.map(|(name, _)| name))
    }
}
