# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Building and running

```bash
cargo check
cargo build
cargo run -- firmware.elf my_function
cargo run -- firmware.elf # Shows function picker
```

Or after `cargo install --path .`:

```bash
asm-annotate firmware.elf my_function
```

## Code architecture

Single Rust binary with several layers:

**`src/backends/`** — backend logic, no rendering:
-   `elf.rs` — goblin + gimli: `list_functions`, `get_function_bounds`, `build_addr_to_src`, `get_symbol_at`. Parses DWARF line programs to build `addr → SourceLocation` mappings.
-   `disasm.rs` — capstone: `disassemble_range` returns `Vec<Instruction>`, plus `apply_demangling` to patch mangled names in mnemonics. Uses `elf.rs` to resolve branch targets.
-   `demangle.rs` — `CppDemangleBackend` wrapping the `cpp_demangle` crate; `demangle` / `demangle_batch`.
-   `picker.rs` — `SkimBackend`: Uses the *embedded* `skim` crate for interactive function selection.

**`src/commands/`** — command handlers:
-   `annotate.rs` — `handle_annotate`: resolves function name → bounds → DWARF mapping → disassemble → demangle → `DisplayItem` list → render TUI.

**`src/ui/mod.rs`** — TUI rendering:
-   `tui.rs`: ratatui interface for side-by-side source and assembly viewing.
-   `colors.rs`: Defines the color palette used for highlighting.

**Other source files**:
-   `src/cli.rs` — clap CLI definitions (`Cli`, `LogLevel`).
-   `src/types.rs` — `SourceLocation`, `AnnotatedInstruction`, `DisplayItem`. `DisplayItem::from_annotated` assigns palette colors.
-   `src/source_reader.rs` — reads source files from disk; supports `--remap` prefix substitution.
-   `src/main.rs` — entry point, calls `handle_annotate`.

## CLI reference

```
asm-annotate [--log-level LEVEL] <ELF> [FUNCTION] [OPTIONS]

Options:
  --no-dwarf              skip DWARF source mapping
  --no-demangle           skip C++ demangling
  --remap <OLD> <NEW>     remap source path prefix (repeatable)
```

## External tool dependencies

None! The tool is self-contained.

## Key design notes

-   ELF parsing uses `goblin`; DWARF parsing uses `gimli` directly.
-   Disassembly uses the `capstone` crate.
-   C++ demangling uses the `cpp_demangle` crate.
-   Function picking uses the embedded `skim` crate.
-   `get_function_bounds` clears the Thumb bit (`& ~1`) from symbol addresses.
-   `build_addr_to_src` walks all DWARF CUs and line programs.
-   The TUI is built with `ratatui` and `crossterm`.
