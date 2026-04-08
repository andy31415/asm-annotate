#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.9"
# dependencies = [
#   "rich",
#   "pyelftools",
#   "coloredlogs",
#   "click",
# ]
# ///
"""
asm_annotate.py — Colored source↔assembly annotator for ELF files.
"""

import logging
import os
from collections import defaultdict
from pathlib import Path

import click
import coloredlogs
from rich.console import Console
from rich.panel import Panel
from rich.table import Table
from rich.text import Text

from disasm import apply_demangling, demangle_batch, disassemble_range, find_objdump
from elf import build_addr_to_src, get_function_bounds, list_functions
from picker import pick_function

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
_missing_warned: set[str] = set()


def _apply_remappings(path: str, remappings: tuple[tuple[str, str], ...]) -> str:
    for old, new in remappings:
        if path.startswith(old):
            return new + path[len(old):]
    return path


def read_source_lines(
    path: str,
    remappings: tuple[tuple[str, str], ...] = (),
) -> list[str]:
    resolved = _apply_remappings(path, remappings)

    if resolved in _source_cache:
        return _source_cache[resolved]

    try:
        with open(resolved, "r", errors="replace") as f:
            lines = f.readlines()
        _source_cache[resolved] = lines
        return lines
    except OSError:
        if path not in _missing_warned:
            _missing_warned.add(path)
            if resolved != path:
                log.warning("Source not found: %s  (remapped from %s)", resolved, path)
            else:
                log.warning("Source not found: %s  (use --remap to redirect paths)", path)
        _source_cache[resolved] = []
        return []


# ── rendering ────────────────────────────────────────────────────────────────
def render_annotated(
    func_name: str,
    instructions: list[tuple[int, str, str]],
    addr_to_src: dict[int, tuple[str, int]],
    show_stats: bool,
    show_bytes: bool,
    remappings: tuple[tuple[str, str], ...] = (),
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
            src_lines = read_source_lines(src_file, remappings)
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
            src_lines = read_source_lines(src_file, remappings)
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
@click.command(context_settings={"help_option_names": ["-h", "--help"]})
@click.argument("elf", type=click.Path(exists=True, dir_okay=False))
@click.argument("function", required=False, default=None)
@click.option("--objdump", metavar="BINARY",
              help="objdump binary to use (auto-detected if omitted)")
@click.option("--list", "do_list", is_flag=True,
              help="List all functions in the ELF and exit")
@click.option("--stats", is_flag=True,
              help="Show per-source-line instruction/byte cost table")
@click.option("--bytes", "show_bytes", is_flag=True,
              help="Show raw instruction bytes alongside mnemonics")
@click.option("--no-dwarf", is_flag=True,
              help="Skip DWARF source mapping")
@click.option("--no-demangle", is_flag=True,
              help="Do not demangle C++ symbol names")
@click.option("--remap", type=(str, str), multiple=True, metavar="OLD NEW",
              help="Remap a source path prefix. E.g. --remap /workspace /home/user/src  (repeatable)")
@click.option("--log-level", default="INFO", metavar="LEVEL", show_default=True,
              help="Logging verbosity: DEBUG, INFO, WARNING, ERROR")
def main(elf, function, objdump, do_list, stats, show_bytes, no_dwarf, no_demangle,
         remap, log_level):
    coloredlogs.install(level=log_level.upper(), logger=log)

    # ── list mode ────────────────────────────────────────────────────────────
    if do_list:
        funcs = list_functions(elf)
        all_names = [n for n, _, _ in funcs]
        dm = {} if no_demangle else demangle_batch(all_names)
        table = Table(title=f"Functions in {os.path.basename(elf)}")
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

    if not function:
        raise click.UsageError("Provide a function name, or use --list to see available functions.")

    # ── find objdump ─────────────────────────────────────────────────────────
    try:
        objdump_bin = find_objdump(objdump)
    except RuntimeError as e:
        raise click.ClickException(str(e))

    log.info("Using objdump: %s", objdump_bin)

    # ── resolve function (fuzzy match / picker) ──────────────────────────────
    try:
        func_sym = pick_function(elf, function)
    except ValueError as e:
        raise click.ClickException(str(e))

    # ── resolve function bounds ──────────────────────────────────────────────
    try:
        start, end = get_function_bounds(elf, func_sym)
    except ValueError as e:
        raise click.ClickException(str(e))

    log.info("Function range: 0x%08x – 0x%08x  (%d bytes)", start, end, end - start)

    # ── DWARF source mapping ─────────────────────────────────────────────────
    addr_to_src: dict[int, tuple[str, int]] = {}
    if not no_dwarf:
        log.info("Building DWARF address map…")
        addr_to_src = build_addr_to_src(elf)
        if not addr_to_src:
            log.warning("No DWARF info found. Build with -g to get source mapping.")

    # ── disassemble ──────────────────────────────────────────────────────────
    instructions = disassemble_range(elf, objdump_bin, start, end)
    if not instructions:
        raise click.ClickException("No instructions found. Check the ELF is not stripped.")

    # ── demangle ─────────────────────────────────────────────────────────────
    func_name = func_sym
    if not no_demangle:
        func_name, instructions = apply_demangling(func_name, instructions)

    # ── render ───────────────────────────────────────────────────────────────
    render_annotated(
        func_name=func_name,
        instructions=instructions,
        addr_to_src=addr_to_src,
        show_stats=stats,
        show_bytes=show_bytes,
        remappings=remap,
    )


main()
