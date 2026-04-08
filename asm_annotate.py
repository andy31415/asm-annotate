#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.9"
# dependencies = [
#   "rich",
#   "pyelftools",
#   "coloredlogs",
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
import logging
import os
import sys
from collections import defaultdict
from pathlib import Path

import coloredlogs
from rich.console import Console
from rich.panel import Panel
from rich.table import Table
from rich.text import Text

from asm_core import (
    apply_demangling,
    build_addr_to_src,
    demangle_batch,
    disassemble_range,
    find_objdump,
    get_function_bounds,
    list_functions,
    pick_function,
)

log = logging.getLogger(__name__)

# ── color palette for source line → asm mapping ─────────────────────────────
PALETTE = [
    "#ff6b6b",
    "#ffd93d",
    "#6bcb77",
    "#4d96ff",
    "#ff922b",
    "#cc5de8",
    "#20c997",
    "#f783ac",
    "#74c0fc",
    "#a9e34b",
    "#ff8787",
    "#ffe066",
    "#8ce99a",
    "#74c0fc",
    "#ffa94d",
    "#da77f2",
    "#63e6be",
    "#faa2c1",
    "#a5d8ff",
    "#c0eb75",
]

console = Console(highlight=False)


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
    src_lines_seen: dict[str, set[int]] = defaultdict(set)

    for addr, raw, mnem in instructions:
        key = addr_to_src.get(addr)
        if key and key not in color_map:
            color_map[key] = PALETTE[color_idx % len(PALETTE)]
            color_idx += 1
        if key:
            src_lines_seen[key[0]].add(key[1])

    # ── header ──────────────────────────────────────────────────────────────
    total_bytes = sum(
        len(bytes.fromhex(r.replace(" ", ""))) for _, r, _ in instructions if r
    )
    console.print()
    console.print(
        Panel(
            f"[bold white]{func_name}[/]  "
            f"[dim]·[/]  [cyan]{len(instructions)} instructions[/]  "
            f"[dim]·[/]  [yellow]{total_bytes} bytes[/]",
            style="bold blue",
            expand=False,
        )
    )
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
        parts = mnem.split(None, 1)
        mnem_word = parts[0]
        operands = parts[1] if len(parts) > 1 else ""
        asm_text.append(f"  {mnem_word:<10}", style=f"bold {color}")
        asm_text.append(operands, style=color)
        console.print(asm_text)

    # ── stats ────────────────────────────────────────────────────────────────
    if show_stats:
        console.print()
        table = Table(
            title="Source line → byte cost",
            show_header=True,
            header_style="bold magenta",
        )
        table.add_column("File:Line", style="dim", no_wrap=True)
        table.add_column("Source", overflow="fold")
        table.add_column("Insns", justify="right")
        table.add_column("Bytes", justify="right", style="yellow")

        line_stats: dict[tuple, list] = defaultdict(list)
        for addr, raw, mnem in instructions:
            key = addr_to_src.get(addr, ("??", 0))
            line_stats[key].append((addr, raw, mnem))

        rows = []
        for key, insns in line_stats.items():
            src_file, src_line_no = key
            byte_count = sum(
                len(bytes.fromhex(r.replace(" ", ""))) for _, r, _ in insns if r
            )
            src_lines = read_source_lines(src_file)
            src_text = ""
            if src_lines and 0 < src_line_no <= len(src_lines):
                src_text = src_lines[src_line_no - 1].strip()[:60]
            parts_p = Path(src_file).parts
            short_file = (
                os.path.join("…", *parts_p[-2:]) if len(parts_p) > 2 else src_file
            )
            rows.append(
                (
                    byte_count,
                    f"{short_file}:{src_line_no}",
                    src_text,
                    len(insns),
                    byte_count,
                )
            )

        for _, file_line, src_text, n_insns, n_bytes in sorted(
            rows, key=lambda x: -x[0]
        ):
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
    parser.add_argument(
        "--objdump", help="Path/name of objdump binary (auto-detected if omitted)"
    )
    parser.add_argument(
        "--list", action="store_true", help="List all functions in the ELF"
    )
    parser.add_argument(
        "--stats", action="store_true", help="Show per-source-line byte cost table"
    )
    parser.add_argument(
        "--bytes", action="store_true", help="Show raw instruction bytes"
    )
    parser.add_argument(
        "--no-dwarf", action="store_true", help="Skip DWARF source mapping"
    )
    parser.add_argument(
        "--no-demangle", action="store_true", help="Do not demangle C++ symbol names"
    )
    parser.add_argument(
        "--log-level",
        default="INFO",
        metavar="LEVEL",
        help="Logging level: DEBUG, INFO, WARNING, ERROR (default: INFO)",
    )
    args = parser.parse_args()

    coloredlogs.install(level=args.log_level.upper(), logger=log)

    if not os.path.isfile(args.elf):
        log.error("ELF file not found: %s", args.elf)
        sys.exit(1)

    # ── list mode ────────────────────────────────────────────────────────────
    if args.list:
        funcs = list_functions(args.elf)
        all_names = [n for n, _, _ in funcs]
        dm = {} if args.no_demangle else demangle_batch(all_names)
        table = Table(title=f"Functions in {os.path.basename(args.elf)}")
        table.add_column("Address", style="cyan", no_wrap=True)
        table.add_column("Size (bytes)", justify="right", style="yellow")
        table.add_column("Name", style="dim")
        table.add_column("Demangled", style="white")
        for name, addr, size in funcs:
            demangled = dm.get(name, name)
            table.add_row(
                f"0x{addr:08x}",
                str(size),
                name,
                "" if demangled == name else demangled,
            )
        console.print(table)
        return

    if not args.function:
        parser.error(
            "Provide a function name, or use --list to see available functions."
        )

    # ── find objdump ─────────────────────────────────────────────────────────
    try:
        objdump = find_objdump(args.objdump)
    except RuntimeError as e:
        log.error("%s", e)
        sys.exit(1)

    log.info("Using objdump: %s", objdump)

    # ── resolve function (fuzzy match / picker) ──────────────────────────────
    try:
        func_sym = pick_function(args.elf, args.function)
    except ValueError as e:
        log.error("%s", e)
        log.info("Tip: run with --list to see all function names.")
        sys.exit(1)

    # ── resolve function bounds ──────────────────────────────────────────────
    try:
        start, end = get_function_bounds(args.elf, func_sym)
    except ValueError as e:
        log.error("%s", e)
        sys.exit(1)

    log.info("Function range: 0x%08x – 0x%08x  (%d bytes)", start, end, end - start)

    # ── DWARF source mapping ─────────────────────────────────────────────────
    addr_to_src: dict[int, tuple[str, int]] = {}
    if not args.no_dwarf:
        log.info("Building DWARF address map…")
        addr_to_src = build_addr_to_src(args.elf)
        if not addr_to_src:
            log.warning("No DWARF info found. Build with -g to get source mapping.")

    # ── disassemble ──────────────────────────────────────────────────────────
    instructions = disassemble_range(args.elf, objdump, start, end)
    if not instructions:
        log.error("No instructions found. Check the ELF is not stripped.")
        sys.exit(1)

    # ── demangle ─────────────────────────────────────────────────────────────
    func_name = func_sym
    if not args.no_demangle:
        func_name, instructions = apply_demangling(func_name, instructions)

    # ── render ───────────────────────────────────────────────────────────────
    render_annotated(
        func_name=func_name,
        instructions=instructions,
        addr_to_src=addr_to_src,
        show_stats=args.stats,
        show_bytes=args.bytes,
    )


if __name__ == "__main__":
    main()
