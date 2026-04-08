#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.9"
# dependencies = [
#   "rich",
#   "pyelftools",
# ]
# ///
"""
asm_annotate.py — Colored source↔assembly annotator for ELF files.

Usage:
    python asm_annotate.py <elf_file> <function_name> [options]
    python asm_annotate.py firmware.elf my_function
    python asm_annotate.py firmware.elf my_function --objdump arm-none-eabi-objdump
    python asm_annotate.py firmware.elf my_function --stats
    python asm_annotate.py firmware.elf --list   # list all functions
"""

import argparse
import json
import os
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path
from typing import Optional

# ── dependency check ────────────────────────────────────────────────────────
try:
    from rich.console import Console
    from rich.table import Table
    from rich.text import Text
    from rich.panel import Panel
    from rich.columns import Columns
    from rich.syntax import Syntax
    from rich import print as rprint
    from rich.style import Style
    from rich.theme import Theme
except ImportError:
    print("Missing 'rich'. Install with:  pip install rich pyelftools")
    sys.exit(1)

try:
    from elftools.elf.elffile import ELFFile
    from elftools.dwarf.descriptions import describe_form_class
    from elftools.dwarf.lineprogram import LineProgram
except ImportError:
    print("Missing 'pyelftools'. Install with:  pip install rich pyelftools")
    sys.exit(1)

# ── color palette for source line → asm mapping ─────────────────────────────
PALETTE = [
    "#ff6b6b", "#ffd93d", "#6bcb77", "#4d96ff", "#ff922b",
    "#cc5de8", "#20c997", "#f783ac", "#74c0fc", "#a9e34b",
    "#ff8787", "#ffe066", "#8ce99a", "#74c0fc", "#ffa94d",
    "#da77f2", "#63e6be", "#faa2c1", "#a5d8ff", "#c0eb75",
]

console = Console(highlight=False)


# ── toolchain detection ──────────────────────────────────────────────────────
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
                    funcs.append((
                        sym.name,
                        sym.entry.st_value & ~1,
                        sym.entry.st_size,
                    ))
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
                    # resolve include directories
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
    # --start-address / --stop-address work on both GNU and LLVM objdump
    cmd = [
        objdump,
        "-d",
        "--no-show-raw-insn",  # cleaner; we add bytes separately
        f"--start-address=0x{start:x}",
        f"--stop-address=0x{end:x}",
        elf_path,
    ]
    # LLVM objdump needs --arch-name for bare-metal ELFs sometimes
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        # retry without --no-show-raw-insn (older GNU)
        cmd.remove("--no-show-raw-insn")
        result = subprocess.run(cmd, capture_output=True, text=True)

    instructions = []
    # Pattern:   8000120:   e92d 4ff0   push    {r4, r5, r6, r7, r8, r9, sl, fp, lr}
    pat = re.compile(r"^\s*([0-9a-f]+):\s+([0-9a-f ]+?)\s{2,}(.+)$")
    for line in result.stdout.splitlines():
        m = pat.match(line)
        if m:
            addr = int(m.group(1), 16)
            raw = m.group(2).strip()
            mnem = m.group(3).strip()
            if start <= addr < end:
                instructions.append((addr, raw, mnem))
    return instructions


# ── source file reading ──────────────────────────────────────────────────────
_source_cache: dict[str, list[str]] = {}

def read_source_lines(path: str) -> list[str]:
    if path in _source_cache:
        return _source_cache[path]
    try:
        with open(path, "r", errors="replace") as f:
            lines = f.readlines()
        _source_cache[path] = lines
        return lines
    except OSError:
        return []


# ── rendering ────────────────────────────────────────────────────────────────
def render_annotated(
    func_name: str,
    instructions: list[tuple[int, str, str]],
    addr_to_src: dict[int, tuple[str, int]],
    show_stats: bool,
    show_bytes: bool,
) -> None:
    # assign colors to (file, line) pairs
    color_map: dict[tuple[str, int], str] = {}
    color_idx = 0
    # Track which source lines are covered
    src_lines_seen: dict[str, set[int]] = defaultdict(set)

    for addr, raw, mnem in instructions:
        key = addr_to_src.get(addr)
        if key and key not in color_map:
            color_map[key] = PALETTE[color_idx % len(PALETTE)]
            color_idx += 1
        if key:
            src_lines_seen[key[0]].add(key[1])

    # ── header ──────────────────────────────────────────────────────────────
    total_bytes = sum(len(bytes.fromhex(r.replace(" ", ""))) for _, r, _ in instructions if r)
    console.print()
    console.print(Panel(
        f"[bold white]{func_name}[/]  "
        f"[dim]·[/]  [cyan]{len(instructions)} instructions[/]  "
        f"[dim]·[/]  [yellow]{total_bytes} bytes[/]",
        style="bold blue",
        expand=False,
    ))
    console.print()

    # ── main annotated listing ───────────────────────────────────────────────
    prev_src_key = None
    prev_src_file = None

    for addr, raw, mnem in instructions:
        src_key = addr_to_src.get(addr)
        color = color_map.get(src_key, "#888888") if src_key else "#555555"

        # emit source line when it changes
        if src_key and src_key != prev_src_key:
            src_file, src_line_no = src_key
            src_lines = read_source_lines(src_file)
            if src_lines and 0 < src_line_no <= len(src_lines):
                src_text = src_lines[src_line_no - 1].rstrip()
                # show filename only when file changes
                if src_file != prev_src_file:
                    short = src_file
                    # trim to last 3 path components for readability
                    parts = Path(src_file).parts
                    if len(parts) > 3:
                        short = os.path.join("…", *parts[-3:])
                    console.print(f"  [dim italic]{short}[/]")
                    prev_src_file = src_file

                line_text = Text()
                line_text.append(f"  {src_line_no:>5}  ", style="dim")
                line_text.append("▶ ", style=f"bold {color}")
                line_text.append(src_text, style=color)
                console.print(line_text)
            prev_src_key = src_key

        # asm line
        asm_text = Text()
        asm_text.append(f"    {addr:08x}  ", style="dim")
        if show_bytes and raw:
            asm_text.append(f"{raw:<24}", style="dim cyan")
        # color the mnemonic
        parts = mnem.split(None, 1)
        mnem_word = parts[0]
        operands = parts[1] if len(parts) > 1 else ""
        asm_text.append(f"  {mnem_word:<10}", style=f"bold {color}")
        asm_text.append(operands, style=color)
        console.print(asm_text)

    # ── stats ────────────────────────────────────────────────────────────────
    if show_stats:
        console.print()
        table = Table(title="Source line → byte cost", show_header=True, header_style="bold magenta")
        table.add_column("File:Line", style="dim", no_wrap=True)
        table.add_column("Source", overflow="fold")
        table.add_column("Insns", justify="right")
        table.add_column("Bytes", justify="right", style="yellow")

        # group instructions by source line
        line_stats: dict[tuple, list] = defaultdict(list)
        for addr, raw, mnem in instructions:
            key = addr_to_src.get(addr, ("??", 0))
            line_stats[key].append((addr, raw, mnem))

        rows = []
        for key, insns in line_stats.items():
            src_file, src_line_no = key
            byte_count = sum(
                len(bytes.fromhex(r.replace(" ", "")))
                for _, r, _ in insns if r
            )
            src_lines = read_source_lines(src_file)
            src_text = ""
            if src_lines and 0 < src_line_no <= len(src_lines):
                src_text = src_lines[src_line_no - 1].strip()[:60]
            parts_p = Path(src_file).parts
            short_file = os.path.join("…", *parts_p[-2:]) if len(parts_p) > 2 else src_file
            rows.append((byte_count, f"{short_file}:{src_line_no}", src_text, len(insns), byte_count))

        for _, file_line, src_text, n_insns, n_bytes in sorted(rows, key=lambda x: -x[0]):
            table.add_row(file_line, src_text, str(n_insns), str(n_bytes))

        console.print(table)

    console.print()


# ── main ─────────────────────────────────────────────────────────────────────
def main():
    parser = argparse.ArgumentParser(
        description="Colored source↔assembly annotator for ELF files.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    parser.add_argument("elf", help="Path to ELF file")
    parser.add_argument("function", nargs="?", help="Function name to disassemble")
    parser.add_argument("--objdump", help="Path/name of objdump binary (auto-detected if omitted)")
    parser.add_argument("--list", action="store_true", help="List all functions in the ELF")
    parser.add_argument("--stats", action="store_true", help="Show per-source-line byte cost table")
    parser.add_argument("--bytes", action="store_true", help="Show raw instruction bytes")
    parser.add_argument("--no-dwarf", action="store_true", help="Skip DWARF source mapping")
    args = parser.parse_args()

    if not os.path.isfile(args.elf):
        console.print(f"[red]Error:[/] ELF file not found: {args.elf}")
        sys.exit(1)

    # ── list mode ────────────────────────────────────────────────────────────
    if args.list:
        funcs = list_functions(args.elf)
        table = Table(title=f"Functions in {os.path.basename(args.elf)}")
        table.add_column("Address", style="cyan", no_wrap=True)
        table.add_column("Size (bytes)", justify="right", style="yellow")
        table.add_column("Name", style="white")
        for name, addr, size in funcs:
            table.add_row(f"0x{addr:08x}", str(size), name)
        console.print(table)
        return

    if not args.function:
        parser.error("Provide a function name, or use --list to see available functions.")

    # ── find objdump ─────────────────────────────────────────────────────────
    try:
        objdump = find_objdump(args.objdump)
    except RuntimeError as e:
        console.print(f"[red]Error:[/] {e}")
        sys.exit(1)

    console.print(f"[dim]Using objdump: {objdump}[/]")

    # ── resolve function bounds ──────────────────────────────────────────────
    try:
        start, end = get_function_bounds(args.elf, args.function)
    except ValueError as e:
        console.print(f"[red]Error:[/] {e}")
        console.print(f"[dim]Tip: run with --list to see all function names.[/]")
        sys.exit(1)

    console.print(f"[dim]Function range: 0x{start:08x} – 0x{end:08x}  ({end-start} bytes)[/]")

    # ── DWARF source mapping ─────────────────────────────────────────────────
    addr_to_src: dict[int, tuple[str, int]] = {}
    if not args.no_dwarf:
        console.print("[dim]Building DWARF address map…[/]")
        addr_to_src = build_addr_to_src(args.elf)
        if not addr_to_src:
            console.print("[yellow]Warning:[/] No DWARF info found. Build with -g to get source mapping.")

    # ── disassemble ──────────────────────────────────────────────────────────
    instructions = disassemble_range(args.elf, objdump, start, end)
    if not instructions:
        console.print("[red]No instructions found.[/] Check the ELF is not stripped.")
        sys.exit(1)

    # ── render ───────────────────────────────────────────────────────────────
    render_annotated(
        func_name=args.function,
        instructions=instructions,
        addr_to_src=addr_to_src,
        show_stats=args.stats,
        show_bytes=args.bytes,
    )


if __name__ == "__main__":
    main()
