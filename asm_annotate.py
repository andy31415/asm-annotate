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
from dataclasses import dataclass
from pathlib import Path

import click
import coloredlogs
from rich.cells import cell_len
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
            return new + path[len(old) :]
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
                log.warning(
                    "Source not found: %s  (use --remap to redirect paths)", path
                )
        _source_cache[resolved] = []
        return []


# ── render data model ─────────────────────────────────────────────────────────
@dataclass
class RenderGroup:
    """One source→asm group: a run of instructions sharing a source location."""

    color: str
    src_file: str | None  # None if no DWARF info for this group
    src_line_start: int | None  # first source line number
    src_lines: list[str]  # source text lines (empty when file not found)
    instructions: list[tuple[int, str, str]]  # (addr, bytes_hex, mnemonic)
    show_file_header: bool  # True when the source file changes from the previous group


# ── build render groups ───────────────────────────────────────────────────────
def build_groups(
    instructions: list[tuple[int, str, str]],
    addr_to_src: dict[int, tuple[str, int]],
    remappings: tuple[tuple[str, str], ...] = (),
) -> list[RenderGroup]:
    """Group consecutive instructions by DWARF source key and load source text."""
    # assign colors in first-seen order
    color_map: dict[tuple[str, int], str] = {}
    color_idx = 0
    for addr, _, _ in instructions:
        key = addr_to_src.get(addr)
        if key and key not in color_map:
            color_map[key] = PALETTE[color_idx % len(PALETTE)]
            color_idx += 1

    # compute how many source lines each key covers (fill gap to next key in
    # the same file, so consecutive DWARF entries show the lines between them)
    src_key_seq: list[tuple[str, int]] = []
    seen: set[tuple[str, int]] = set()
    for addr, _, _ in instructions:
        key = addr_to_src.get(addr)
        if key and key not in seen:
            src_key_seq.append(key)
            seen.add(key)

    src_display_end: dict[tuple[str, int], int] = {}
    for i, key in enumerate(src_key_seq):
        file, line = key
        if i + 1 < len(src_key_seq):
            next_file, next_line = src_key_seq[i + 1]
            end = next_line - 1 if next_file == file else line
        else:
            end = line
        src_display_end[key] = end

    # build groups: start a new group on every source-key change
    groups: list[RenderGroup] = []
    prev_src_key: tuple[str, int] | None = None
    prev_src_file: str | None = None
    current: RenderGroup | None = None

    for addr, raw, mnem in instructions:
        src_key = addr_to_src.get(addr)

        if src_key != prev_src_key:
            if current is not None:
                groups.append(current)

            color = color_map.get(src_key, "#888888") if src_key else "#aaaaaa"

            if src_key:
                src_file, src_line_no = src_key
                end_line = src_display_end.get(src_key, src_line_no)
                raw_lines = read_source_lines(src_file, remappings)
                src_lines: list[str] = []
                if raw_lines and 0 < src_line_no <= len(raw_lines):
                    for ln in range(src_line_no, min(end_line, len(raw_lines)) + 1):
                        src_lines.append(raw_lines[ln - 1].rstrip())
                show_file = src_file != prev_src_file
                prev_src_file = src_file
                current = RenderGroup(
                    color=color,
                    src_file=src_file,
                    src_line_start=src_line_no,
                    src_lines=src_lines,
                    instructions=[],
                    show_file_header=show_file,
                )
            else:
                current = RenderGroup(
                    color=color,
                    src_file=None,
                    src_line_start=None,
                    src_lines=[],
                    instructions=[],
                    show_file_header=False,
                )
            prev_src_key = src_key

        current.instructions.append((addr, raw, mnem))  # type: ignore[union-attr]

    if current is not None:
        groups.append(current)

    return groups


# ── shared helpers ────────────────────────────────────────────────────────────
def _count_bytes(instructions: list[tuple[int, str, str]]) -> int:
    return sum(len(bytes.fromhex(r.replace(" ", ""))) for _, r, _ in instructions if r)


def _short_path(path: str, depth: int = 3) -> str:
    parts = Path(path).parts
    return os.path.join("…", *parts[-depth:]) if len(parts) > depth else path


def _render_header(func_name: str, groups: list[RenderGroup]) -> None:
    all_insns = [i for g in groups for i in g.instructions]
    total = _count_bytes(all_insns)
    console.print()
    console.print(
        Panel(
            f"[bold white]{func_name}[/]  "
            f"[dim]·[/]  [cyan]{len(all_insns)} instructions[/]  "
            f"[dim]·[/]  [yellow]{total} bytes[/]",
            style="bold blue",
            expand=False,
        )
    )
    console.print()


def _render_stats_table(groups: list[RenderGroup]) -> None:
    table = Table(
        title="Source line → byte cost",
        show_header=True,
        header_style="bold magenta",
    )
    table.add_column("File:Line", style="dim", no_wrap=True)
    table.add_column("Source", overflow="fold")
    table.add_column("Insns", justify="right")
    table.add_column("Bytes", justify="right", style="yellow")

    # aggregate by (file, line) to handle non-consecutive same-key groups
    key_insns: dict[tuple, list] = defaultdict(list)
    key_meta: dict[tuple, tuple[str, str]] = {}
    for group in groups:
        key = (group.src_file or "??", group.src_line_start or 0)
        key_insns[key].extend(group.instructions)
        if key not in key_meta:
            src_text = group.src_lines[0].strip()[:60] if group.src_lines else ""
            if group.src_file and group.src_line_start is not None:
                file_line = f"{_short_path(group.src_file, 2)}:{group.src_line_start}"
            else:
                file_line = "??"
            key_meta[key] = (file_line, src_text)

    rows = []
    for key, insns in key_insns.items():
        n_bytes = _count_bytes(insns)
        file_line, src_text = key_meta[key]
        rows.append((n_bytes, file_line, src_text, len(insns), n_bytes))

    for _, file_line, src_text, n_insns, n_bytes in sorted(rows, key=lambda x: -x[0]):
        table.add_row(file_line, src_text, str(n_insns), str(n_bytes))

    console.print(table)


# ── unified renderer ─────────────────────────────────────────────────────────
def render_unified(
    func_name: str,
    groups: list[RenderGroup],
    show_stats: bool,
    show_bytes: bool,
) -> None:
    _render_header(func_name, groups)
    shown_src_keys: set[tuple] = set()

    for group in groups:
        src_key = (group.src_file, group.src_line_start)
        src_already_shown = group.src_file is not None and src_key in shown_src_keys
        if group.src_lines and not src_already_shown:
            shown_src_keys.add(src_key)

        if not src_already_shown:
            # file header when file changes and source is readable
            if group.show_file_header and group.src_file and group.src_lines:
                short = _short_path(group.src_file, 3)
                lineno = (
                    f":{group.src_line_start}"
                    if group.src_line_start is not None
                    else ""
                )
                console.print(f"  [dim italic]{short}[/][dim]{lineno}[/]")

            for i, src_text in enumerate(group.src_lines):
                line_text = Text()
                line_text.append("  ", style="dim")
                marker = "▶ " if i == 0 else "  "
                line_text.append(marker, style=f"bold {group.color}")
                line_text.append(src_text, style=group.color)
                console.print(line_text)

        for addr, raw, mnem in group.instructions:
            asm_text = Text()
            asm_text.append(f"    {addr:08x}  ", style="#333333")
            if show_bytes and raw:
                asm_text.append(f"{raw:<24}", style="dim cyan")
            parts = mnem.split(None, 1)
            mnem_word = parts[0]
            operands = parts[1] if len(parts) > 1 else ""
            asm_text.append(f"  {mnem_word:<10}", style=f"bold {group.color}")
            asm_text.append(operands, style=group.color)
            console.print(asm_text)

    if show_stats:
        console.print()
        _render_stats_table(groups)

    console.print()


# ── split renderer ────────────────────────────────────────────────────────────
_SEP = " | "


def render_split(
    func_name: str,
    groups: list[RenderGroup],
    show_stats: bool,
    show_bytes: bool,
    src_width: int,
) -> None:
    """Side-by-side source (left) and assembly (right) columns.

    Each source→asm group is laid out as rows: source lines on the left,
    asm instructions on the right.  Whichever side is taller gets blank
    cells on the shorter side.  Source lines are truncated (with «…») to
    fit the column; asm gets the remaining terminal width.
    """
    _render_header(func_name, groups)
    asm_width = max(10, console.width - src_width - cell_len(_SEP))
    shown_src_keys: set[tuple] = set()

    for group in groups:
        # file-change header spans both columns as a rule
        if group.show_file_header and group.src_file:
            short = _short_path(group.src_file, 3)
            console.rule(f"[dim italic]{short}[/]", style="dim")

        src_key = (group.src_file, group.src_line_start)
        src_already_shown = group.src_file is not None and src_key in shown_src_keys
        if group.src_lines and not src_already_shown:
            shown_src_keys.add(src_key)

        n_src = 0 if src_already_shown else len(group.src_lines)
        n_asm = len(group.instructions)
        n_rows = max(n_src, n_asm, 1)

        for row_i in range(n_rows):
            # left: source line with line number (suppressed for repeated keys)
            left = Text()
            if row_i < n_src and group.src_line_start is not None:
                lineno = group.src_line_start + row_i
                marker = "▶" if row_i == 0 else " "
                left.append(f"{lineno:>4} ", style="dim")
                left.append(f"{marker} ", style=f"bold {group.color}")
                left.append(group.src_lines[row_i].expandtabs(4), style=group.color)
            left.truncate(src_width, overflow="ellipsis", pad=True)

            # right: asm instruction
            right = Text()
            if row_i < n_asm:
                addr, raw, mnem = group.instructions[row_i]
                right.append(f"{addr:08x}  ", style="#444444")
                if show_bytes and raw:
                    right.append(f"{raw:<24}", style="dim cyan")
                mnem_parts = mnem.split(None, 1)
                mnem_word = mnem_parts[0]
                operands = mnem_parts[1] if len(mnem_parts) > 1 else ""
                right.append(f"{mnem_word:<10}", style=f"bold {group.color}")
                right.append(operands, style=group.color)
            right.truncate(asm_width, overflow="ellipsis")

            row = Text()
            row.append_text(left)
            row.append(_SEP, style="dim")
            row.append_text(right)
            console.print(row, crop=False)

    if show_stats:
        console.print()
        _render_stats_table(groups)

    console.print()


# ── main ─────────────────────────────────────────────────────────────────────
@click.command(context_settings={"help_option_names": ["-h", "--help"]})
@click.argument("elf", type=click.Path(exists=True, dir_okay=False))
@click.argument("function", required=False, default=None)
@click.option(
    "--objdump",
    metavar="BINARY",
    help="objdump binary to use (auto-detected if omitted)",
)
@click.option(
    "--list", "do_list", is_flag=True, help="List all functions in the ELF and exit"
)
@click.option(
    "--stats", is_flag=True, help="Show per-source-line instruction/byte cost table"
)
@click.option(
    "--bytes",
    "show_bytes",
    is_flag=True,
    help="Show raw instruction bytes alongside mnemonics",
)
@click.option("--no-dwarf", is_flag=True, help="Skip DWARF source mapping")
@click.option("--no-demangle", is_flag=True, help="Do not demangle C++ symbol names")
@click.option(
    "--remap",
    type=(str, str),
    multiple=True,
    metavar="OLD NEW",
    help="Remap a source path prefix. E.g. --remap /workspace /home/user/src  (repeatable)",
)
@click.option(
    "--format",
    "fmt",
    default="split",
    metavar="FORMAT",
    help=(
        "Output format: split (default, 50/50 columns), "
        "unified (interleaved source+asm), "
        "or split:<N> (split with N chars for source column)"
    ),
)
@click.option(
    "--log-level",
    default="INFO",
    metavar="LEVEL",
    show_default=True,
    help="Logging verbosity: DEBUG, INFO, WARNING, ERROR",
)
def main(
    elf,
    function,
    objdump,
    do_list,
    stats,
    show_bytes,
    no_dwarf,
    no_demangle,
    remap,
    fmt,
    log_level,
):
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
        raise click.UsageError(
            "Provide a function name, or use --list to see available functions."
        )

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
        raise click.ClickException(
            "No instructions found. Check the ELF is not stripped."
        )

    # ── demangle ─────────────────────────────────────────────────────────────
    func_name = func_sym
    if not no_demangle:
        func_name, instructions = apply_demangling(func_name, instructions)

    # ── build groups ──────────────────────────────────────────────────────────
    groups = build_groups(instructions, addr_to_src, remap)

    # ── render ───────────────────────────────────────────────────────────────
    if fmt == "unified":
        render_unified(func_name, groups, stats, show_bytes)
    elif fmt == "split" or fmt.startswith("split:"):
        if ":" in fmt:
            try:
                src_width = int(fmt.split(":", 1)[1])
            except ValueError:
                raise click.BadParameter(
                    f"invalid format '{fmt}': expected split:<integer>",
                    param_hint="--format",
                )
        else:
            src_width = max(20, console.size.width // 2 - 2)
        render_split(func_name, groups, stats, show_bytes, src_width)
    else:
        raise click.BadParameter(
            f"unknown format '{fmt}': use split, unified, or split:<N>",
            param_hint="--format",
        )


main()
