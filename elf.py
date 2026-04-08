"""
elf.py — ELF symbol table and DWARF source-location queries.
"""

import logging
import os

from elftools.elf.elffile import ELFFile

log = logging.getLogger(__name__)


def get_function_bounds(elf_path: str, func_name: str) -> tuple[int, int]:
    """Return (start_addr, end_addr) for a function symbol."""
    with open(elf_path, "rb") as f:
        elf = ELFFile(f)
        for section in elf.iter_sections():
            if section.header.sh_type not in ("SHT_SYMTAB", "SHT_DYNSYM"):
                continue
            for sym in section.iter_symbols():
                if sym.name == func_name and sym.entry.st_info.type == "STT_FUNC":
                    start = sym.entry.st_value & ~1  # clear Thumb bit
                    size = sym.entry.st_size
                    return start, start + size
    raise ValueError(f"Function '{func_name}' not found in ELF symbol table.")


def list_functions(elf_path: str) -> list[tuple[str, int, int]]:
    """Return list of (name, addr, size) for all STT_FUNC symbols."""
    funcs = []
    with open(elf_path, "rb") as f:
        elf = ELFFile(f)
        for section in elf.iter_sections():
            if section.header.sh_type not in ("SHT_SYMTAB", "SHT_DYNSYM"):
                continue
            for sym in section.iter_symbols():
                if sym.entry.st_info.type == "STT_FUNC" and sym.name:
                    funcs.append(
                        (
                            sym.name,
                            sym.entry.st_value & ~1,
                            sym.entry.st_size,
                        )
                    )
    return sorted(funcs, key=lambda x: x[1])


def build_addr_to_src(elf_path: str) -> dict[int, tuple[str, int]]:
    """Build mapping from address → (source_file, line_number) via DWARF."""
    mapping: dict[int, tuple[str, int]] = {}
    with open(elf_path, "rb") as f:
        elf = ELFFile(f)
        if not elf.has_dwarf_info():
            return mapping
        dwarf = elf.get_dwarf_info()
        for cu in dwarf.iter_CUs():
            lp = dwarf.line_program_for_CU(cu)
            if lp is None:
                continue
            file_entries = lp.header.file_entry
            for entry in lp.get_entries():
                if entry.state is None:
                    continue
                state = entry.state
                if state.file and state.file <= len(file_entries):
                    fe = file_entries[state.file - 1]
                    fname = fe.name.decode() if isinstance(fe.name, bytes) else fe.name
                    if fe.dir_index > 0:
                        dirs = lp.header.include_directory
                        if fe.dir_index <= len(dirs):
                            d = dirs[fe.dir_index - 1]
                            d = d.decode() if isinstance(d, bytes) else d
                            fname = os.path.join(d, fname)
                    mapping[state.address] = (fname, state.line)
    return mapping
