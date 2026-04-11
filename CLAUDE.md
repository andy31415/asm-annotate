# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Building and running

```bash
cargo check
cargo test
cargo build --release
# Example run
cargo run --release -- firmware.elf my_function
# Show picker
cargo run --release -- firmware.elf
```

Or after `cargo install --path .`:

```bash
asm-annotate firmware.elf my_function
```

## Code architecture

Single Rust binary with several layers:

**`src/backends/`** — backend logic, no rendering:
-   `elf.rs` — goblin + gimli: `list_functions`, `get_function_bounds`, `build_addr_to_src`, `get_symbol_at`. Parses DWARF line programs to build `addr → SourceLocation` mappings.
-   `disasm.rs` — capstone: `disassemble_range` returns `Vec<Instruction>`. Uses `elf.rs` to resolve branch targets.
-   `demangle.rs` — `CppDemangleBackend` wrapping the `cpp_demangle` crate; `demangle`.
-   `picker.rs` — `SkimBackend`: Uses the *embedded* `skim` crate for interactive function selection.

**`src/commands/`** — command handlers:
-   `annotate.rs` — `handle_annotate`: Loads initial data. If `--dump`, delegates to `dump.rs` and exits. Otherwise sets up file watcher and launches the TUI. Contains `load_annotation_data` for (re)loading, and `AnnotationData` struct.
-   `dump.rs` — `dump_annotation`: Prints a compact plain-text annotated listing to stdout (format C: source comment on change only). No ANSI codes; suitable for LLM/automation use.

**`src/ui/`** — TUI rendering:
-   `tui.rs`: ratatui interface for side-by-side source and assembly viewing. Manages `AppState`, event loop, and drawing. Includes hot-reloading logic via a channel receiver.
-   `colors.rs`: Defines the color palette (based on Matplotlib `tab20`) used for highlighting.

**Other source files**:
-   `src/cli.rs` — clap CLI definitions (`Cli`, `LogLevel`, including `--context`).
-   `src/types.rs` — `SourceLocation`, `AnnotatedInstruction`, `DisplayItem`. `DisplayItem::from_annotated` assigns palette colors, propagating source locations.
-   `src/source_reader.rs` — reads source files from disk; supports `--remap` prefix substitution.
-   `src/main.rs` — entry point, initializes logger, calls `handle_annotate`.

## CLI reference

```
asm-annotate [--log-level LEVEL] <ELF> [FUNCTION] [OPTIONS]

Options:
  --context <N or N:M>    Set source context lines (default: 2:5)
  --no-demangle           skip C++ demangling
  --remap <OLD> <NEW>     remap source path prefix (repeatable)
  --dump                  print plain-text annotated listing instead of launching TUI
```

## External tool dependencies

None! The tool is self-contained.

## Key design notes

-   **TUI Only:** The application exclusively uses a `ratatui` based Terminal User Interface.
-   **Hot Reloading:** A background thread uses the `notify` crate to watch the ELF file. Changes trigger a reload of the annotation data and a TUI refresh.
-   **Crates Used:** `goblin` (ELF), `gimli` (DWARF), `capstone` (Disassembly), `cpp_demangle` (Demangling), `skim` (Fuzzy Finding), `ratatui` & `crossterm` (TUI), `notify` (File Watching), `tui-logger` (Logging).
-   **ARM Thumb Mode:** Automatically detected based on the symbol's LSB.
-   **DWARF Line Propagation:** Ensures all assembly instructions corresponding to a source line are colored correctly.
