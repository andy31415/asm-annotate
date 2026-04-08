# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Running the tools

Both entry points use `uv` inline script metadata so dependencies are managed automatically:

```bash
# Terminal annotator
uv run asm_annotate.py firmware.elf my_function
uv run asm_annotate.py firmware.elf --list
uv run asm_annotate.py firmware.elf my_function --stats --bytes

# Web UI (opens http://localhost:7777)
uv run asm_web.py firmware.elf my_function
uv run asm_web.py firmware.elf my_function --compile-commands build/compile_commands.json
```

Alternatively with plain `python` after `pip install rich pyelftools coloredlogs click`.

## Code architecture

Five files, two layers:

**Shared library modules** (used by both frontends):
- `elf.py` — pyelftools wrapper: `list_functions`, `get_function_bounds`, `build_addr_to_src`. Parses DWARF line programs to build `addr → (file, line)` mappings.
- `disasm.py` — objdump invocation and output parsing (`disassemble_range` returns `[(addr, bytes_hex, mnemonic)]`), plus C++ demangling via a single `c++filt` subprocess call.
- `picker.py` — resolves a partial/fuzzy function name query to an exact mangled symbol name. Falls through: exact match → single substring match → interactive `sk`/`fzf` picker.

**Frontend entry points**:
- `asm_annotate.py` — click CLI + rich terminal rendering. The render pipeline is:
  1. `build_groups(instructions, addr_to_src, remappings) → list[RenderGroup]` — assigns colors from `PALETTE` in first-seen order, computes source line ranges, loads source text. Each `RenderGroup` holds `(color, src_file, src_line_start, src_lines, instructions, show_file_header)`.
  2. `render_unified(func_name, groups, ...)` — classic interleaved source+asm output.
  3. `render_split(func_name, groups, ..., src_width)` — side-by-side columns separated by `│`. Left column shows `{lineno} {marker} {source}` truncated to `src_width` chars; right column shows asm. File headers rendered as a full-width `Rule`. Use `--split` to enable; `--src-width N` overrides the default (half terminal width).
- `asm_web.py` — stdlib `HTTPServer` serving a single-file HTML/JS app. All page HTML/CSS/JS is the `HTML_PAGE` string constant. The JS `PALETTE` is a copy of the Python one. `AppState` is a module-level singleton. API endpoints: `GET /api/state`, `GET /api/functions`, `POST /api/switch_function`, `POST /api/recompile`.

## External tool dependencies

- `arm-none-eabi-objdump` or `llvm-objdump` or `objdump` (auto-detected in that order)
- `c++filt` for C++ demangling (gracefully skipped if absent)
- `sk` (skim) or `fzf` for interactive function picker (optional)
- `arm-none-eabi-ld` for recompile linking in `asm_web.py` (falls back to `.o` if absent)

## Key design notes

- The `PALETTE` list is duplicated between `asm_annotate.py` and the `HTML_PAGE` JS string in `asm_web.py` — keep them in sync if changing colors.
- `get_function_bounds` clears the Thumb bit (`& ~1`) from symbol addresses.
- `build_addr_to_src` walks all DWARF CUs and line programs; it covers the entire ELF, not just one function.
- The web UI's recompile flow writes modified source to a temp file, re-runs the original compiler command (from `compile_commands.json`) with the source and output paths patched, then attempts linking with `arm-none-eabi-ld`. If linking fails, it disassembles the `.o` directly.
