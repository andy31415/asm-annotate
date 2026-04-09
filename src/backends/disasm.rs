use color_eyre::eyre::Result;
use std::path::Path;

pub struct Instruction {
    pub addr: u64,
    pub bytes: String,
    pub mnemonic: String,
}

pub trait DisassemblerBackend {
    fn disassemble_range(
        &self,
        elf_path: &Path,
        start_addr: u64,
        end_addr: u64,
    ) -> Result<Vec<Instruction>>;
}

// TODO: Implement ObjdumpBackend
pub struct ObjdumpBackend {
    pub objdump_path: PathBuf,
}

use std::path::PathBuf;
impl DisassemblerBackend for ObjdumpBackend {
    fn disassemble_range(
        &self,
        _elf_path: &Path,
        _start_addr: u64,
        _end_addr: u64,
    ) -> Result<Vec<Instruction>> {
        unimplemented!()
    }
}
