"""
asm_core.py — Shared ELF/disassembly utilities for asm_annotate and asm_web.
"""

import logging
import os
import re
import subprocess
from pathlib import Path
from typing import Optional

from elftools.elf.elffile import ELFFile

log = logging.getLogger(__name__)

_ANGLE_BRACKET_SYM = re.compile(r"<([^>]+)>")


# ── demangling ───────────────────────────────────────────────────────────────
def demangle_batch(names: list[str]) -> dict[str, str]:
    """Return a mangled→demangled mapping via a single c++filt call."""
    if not names:
        return {}
    try:
        result = subprocess.run(
            ["c++filt"],
            input="\n".join(names),
            capture_output=True,
            text=True,
            check=True,
        )
        return dict(zip(names, result.stdout.splitlines()))
    except FileNotFoundError:
        log.debug("c++filt not found; symbol names will not be demangled")
        return {}


def apply_demangling(
    func_name: str,
    instructions: list[tuple[int, str, str]],
) -> tuple[str, list[tuple[int, str, str]]]:
    """Demangle the function name and all <symbol> references in operands."""
    symbols: set[str] = {func_name}
    for _, _, mnem in instructions:
        for m in _ANGLE_BRACKET_SYM.finditer(mnem):
            symbols.add(m.group(1))

    dm = demangle_batch(list(symbols))
    if not dm:
        return func_name, instructions

    def sub_mnem(mnem: str) -> str:
        return _ANGLE_BRACKET_SYM.sub(lambda m: f"<{dm.get(m.group(1), m.group(1))}>", mnem)

    return (
        dm.get(func_name, func_name),
        [(addr, raw, sub_mnem(mnem)) for addr, raw, mnem in instructions],
    )


# ── toolchain detection ──────────────────────────────────────────────────────
def find_fuzzy_picker() -> Optional[str]:
    """Return the path to skim (sk) or fzf, whichever is available."""
    for tool in ["sk", "fzf"]:
        try:
            subprocess.run([tool, "--version"], capture_output=True, check=True)
            return tool
        except (FileNotFoundError, subprocess.CalledProcessError):
            continue
    return None


def find_objdump(hint: Optional[str] = None) -> str:
    candidates = []
    if hint:
        candidates.append(hint)
    candidates += [
        "arm-none-eabi-objdump",
        "llvm-objdump",
        "objdump",
    ]
    for c in candidates:
        try:
            subprocess.run([c, "--version"], capture_output=True, check=True)
            return c
        except (FileNotFoundError, subprocess.CalledProcessError):
            continue
    raise RuntimeError(
        "No objdump found. Install arm-none-eabi-binutils or llvm, "
        "or pass --objdump <path>."
    )


# ── ELF helpers ──────────────────────────────────────────────────────────────
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


# ── DWARF: addr → (file, line) ───────────────────────────────────────────────
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


# ── disassembly ──────────────────────────────────────────────────────────────
def disassemble_range(
    elf_path: str,
    objdump: str,
    start: int,
    end: int,
) -> list[tuple[int, str, str]]:
    """
    Run objdump and return list of (addr, bytes_hex, mnemonic) for
    instructions in [start, end).
    """
    # --start-address / --stop-address work on both GNU and LLVM objdump.
    # Do NOT use --no-show-raw-insn: the regex relies on raw bytes being present
    # to separate the byte field from the mnemonic field.
    cmd = [
        objdump,
        "-d",
        f"--start-address=0x{start:x}",
        f"--stop-address=0x{end:x}",
        elf_path,
    ]
    log.debug("Running: %s", " ".join(cmd))

    result = subprocess.run(cmd, capture_output=True, text=True)
    out_lines = result.stdout.splitlines()

    log.debug(
        "objdump exit code: %d  (%d lines of output)", result.returncode, len(out_lines)
    )
    if result.stderr.strip():
        log.debug("objdump stderr: %s", result.stderr.strip()[:300])
    for ln in out_lines[:30]:
        log.debug("  %r", ln)

    if result.returncode != 0:
        log.error(
            "objdump failed (exit %d): %s", result.returncode, result.stderr[:500]
        )
        return []

    instructions = []
    # Pattern with raw bytes (GNU/LLVM default):
    #   8000120:   e92d 4ff0   push    {r4, r5, r6, r7, r8, r9, sl, fp, lr}
    pat_bytes = re.compile(r"^\s*([0-9a-f]+):\s+([0-9a-f][0-9a-f ]*?)\s{2,}(.+)$")
    # Fallback when bytes are absent (some toolchains / --no-show-raw-insn):
    #   8000120:   push    {r4, r5, r6, r7, r8, r9, sl, fp, lr}
    pat_no_bytes = re.compile(r"^\s*([0-9a-f]+):\s+([^\s][^\t]+)$")

    for line in out_lines:
        m = pat_bytes.match(line)
        if m:
            addr = int(m.group(1), 16)
            raw = m.group(2).strip()
            mnem = m.group(3).strip()
        else:
            m = pat_no_bytes.match(line)
            if not m:
                continue
            # Skip if the "mnemonic" looks like a pure hex string (i.e. raw bytes
            # with no mnemonic following — malformed line).
            candidate = m.group(2).strip()
            if re.fullmatch(r"[0-9a-f ]+", candidate):
                continue
            addr = int(m.group(1), 16)
            raw = ""
            mnem = candidate

        if start <= addr < end:
            instructions.append((addr, raw, mnem))

    log.debug(
        "Instructions matched in range 0x%x–0x%x: %d", start, end, len(instructions)
    )
    if not instructions:
        log.debug(
            "Zero instructions matched. Check that the address range appears in "
            "the objdump output above. If it is empty or shows a different range, "
            "the ELF may use a non-standard section layout or symbol bounds are wrong."
        )

    return instructions


# ── function picker ──────────────────────────────────────────────────────────
def pick_function(elf_path: str, query: str) -> str:
    """
    Resolve *query* to a mangled function name.

    - Exact mangled match → use directly.
    - Substring match (mangled or demangled) on exactly one function → use it.
    - Substring match on many functions → launch sk/fzf for interactive pick.
    """
    funcs = list_functions(elf_path)  # [(mangled, addr, size), …]
    all_names = [n for n, _, _ in funcs]
    dm = demangle_batch(all_names)

    # Exact mangled match wins immediately.
    if any(n == query for n, _, _ in funcs):
        return query

    q = query.lower()
    matches = [
        (name, addr, size, dm.get(name, name))
        for name, addr, size in funcs
        if q in name.lower() or q in dm.get(name, name).lower()
    ]

    if not matches:
        raise ValueError(f"No function matching '{query}' found in ELF.")

    if len(matches) == 1:
        log.info("Matched function: %s", dm.get(matches[0][0], matches[0][0]))
        return matches[0][0]

    picker = find_fuzzy_picker()
    if picker is None:
        msg_lines = "\n".join(
            f"  {dm.get(n, n)}" for n, _, _, _ in matches
        )
        raise ValueError(
            f"{len(matches)} functions match '{query}'. "
            f"Install sk or fzf for interactive selection, or be more specific.\n"
            f"Matches:\n{msg_lines}"
        )

    # Build picker input: one line per match, address is the key to map back.
    lines = [
        f"0x{addr:08x}  {size:>6}  {demangled}"
        for name, addr, size, demangled in matches
    ]
    result = subprocess.run(
        [picker, "--query", query],
        input="\n".join(lines),
        stdout=subprocess.PIPE,
        text=True,
    )
    if result.returncode != 0 or not result.stdout.strip():
        raise ValueError("No function selected.")

    selected = result.stdout.strip().splitlines()[0]
    selected_addr = int(selected.split()[0], 16)
    for name, addr, size, _ in matches:
        if addr == selected_addr:
            return name
    raise ValueError("Could not map picker selection back to a function name.")
