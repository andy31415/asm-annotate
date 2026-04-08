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
- `asm_annotate.py` — click CLI + rich terminal rendering. Assigns colors from `PALETTE` to `(file, line)` keys in first-seen order; emits interleaved source lines and asm.
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
