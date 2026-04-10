# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Building and running

```bash
cargo build
cargo run -- list firmware.elf
cargo run -- annotate firmware.elf my_function
```

Or after `cargo install --path .`:

```bash
asm-annotate list firmware.elf
asm-annotate annotate firmware.elf my_function
asm-annotate annotate firmware.elf my_function --stats --bytes
asm-annotate annotate firmware.elf my_function --format unified
asm-annotate annotate firmware.elf my_function --format split:60
```

## Code architecture

Single Rust binary with two layers:

**`src/backends/`** — backend logic, no rendering:
- `elf.rs` — goblin + gimli: `list_functions`, `get_function_bounds`, `build_addr_to_src`. Parses DWARF line programs to build `addr → SourceLocation` mappings.
- `disasm.rs` — objdump invocation and output parsing (`disassemble_range` returns `Vec<Instruction>`), plus `apply_demangling` to patch mangled names in mnemonics.
- `demangle.rs` — `CppDemangleBackend` wrapping the `cpp_demangle` crate; `demangle` / `demangle_batch`.
- `picker.rs` — `SkimBackend`: when multiple functions match a query, pipes them into `sk` (skim) for interactive selection.

**`src/commands/`** — subcommand handlers:
- `list.rs` — `handle_list`: calls `list_functions`, prints address/size/name/demangled table.
- `annotate.rs` — `handle_annotate`: resolves function name → bounds → DWARF mapping → disassemble → demangle → `DisplayItem` list → render.

**`src/ui/mod.rs`** — renderers, all implementing the `Renderer` trait:
- `UnifiedRenderer` — classic interleaved source+asm output.
- `SplitRenderer` — side-by-side columns separated by `│`. Left column: `{lineno} ▶ {source}` truncated to `source_width`; right column: asm.
- `SideBySideRenderer` — full source context panel on the left (with context lines above/below each hit), full asm panel on the right.

**Other source files**:
- `src/cli.rs` — clap CLI definitions (`Cli`, `Commands`, `ListArgs`, `AnnotateArgs`).
- `src/types.rs` — `SourceLocation`, `AnnotatedInstruction`, `DisplayItem`, `UI_PALETTE`. `DisplayItem::from_annotated` assigns palette colors in first-seen order.
- `src/source_reader.rs` — reads source files from disk; supports `--remap` prefix substitution.
- `src/main.rs` — entry point, dispatches to subcommands.

## CLI reference

```
asm-annotate [--log-level LEVEL] <SUBCOMMAND>

Subcommands:
  list (l)      <ELF> [--no-demangle]
  annotate (a)  <ELF> [FUNCTION] [OPTIONS]

annotate options:
  --objdump <BINARY>      override objdump binary
  --stats                 show per-source-line byte cost table
  --bytes                 show raw instruction bytes
  --no-dwarf              skip DWARF source mapping
  --no-demangle           skip C++ demangling
  --remap <OLD> <NEW>     remap source path prefix (repeatable)
  --format <FORMAT>       split (default) | split:<N> | unified | sidebyside
```

## External tool dependencies

- `arm-none-eabi-objdump` or `llvm-objdump` or `objdump` (auto-detected in that order)
- `sk` (skim) for interactive function picker when multiple functions match (required when ambiguous)

## Key design notes

- ELF parsing uses `goblin`; DWARF parsing uses `gimli` directly (not addr2line).
- C++ demangling uses the `cpp_demangle` crate — no external `c++filt` subprocess.
- `get_function_bounds` clears the Thumb bit (`& ~1`) from symbol addresses.
- `build_addr_to_src` walks all DWARF CUs and line programs; covers the entire ELF.
- The picker (`SkimBackend`) only supports `sk` (skim) — no fzf fallback.
- `UI_PALETTE` is defined once in `src/types.rs` (unlike the old Python version where it was duplicated).
