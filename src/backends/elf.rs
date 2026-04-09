use color_eyre::eyre::{Context, Result};
use goblin::elf;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

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
                    // Avoid duplicates if already found in syms
                    if !funcs.iter().any(|f| f.name == name && f.addr == (sym.st_value & !1)) {
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

    fn get_function_bounds(&self, _elf_path: &Path, _func_name: &str) -> Result<(u64, u64)> {
        unimplemented!()
    }

    fn build_addr_to_src(&self, _elf_path: &Path) -> Result<HashMap<u64, (String, usize)>> {
        unimplemented!()
    }
}
