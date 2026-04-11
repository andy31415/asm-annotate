# ASM Annotate

A CLI tool for understanding what your C/C++ source code costs in flash — colored, Godbolt-style source↔assembly annotation in your terminal.

Works with your existing ELF file. No changes to your build needed — just build with `-g` for source mapping.

---

## Features

-   **Interactive TUI:** Side-by-side, colored view of source code and disassembly using `ratatui`.
-   **Plain-text dump mode:** `--dump` outputs a compact, color-free annotated listing suitable for LLM/automation use.
-   **Hot Reloading:** Automatically refreshes the TUI when the target ELF file is modified (e.g., after recompilation).
-   **Self-Contained:** No external dependencies like `objdump` or `sk`.
-   **DWARF Debug Info:** Uses DWARF data to map assembly back to source lines.
-   **C++ Demangling:** Displays demangled C++ function names.
-   **Fuzzy Function Finder:** Interactive function selection using an embedded version of `skim` if no function is specified or the name is ambiguous.
-   **Configurable Context:** Control how many lines of source code are shown around matching lines.
-   **Source Path Remapping:** Supports remapping source paths if the build environment differs from the current environment.
-   **ARM Thumb Detection:** Correctly disassembles ARM code, detecting Thumb mode.

---

## Installation

```bash
# Build the release binary
cargo build --release
# Binary is at target/release/asm-annotate

# Or install to ~/.cargo/bin:
cargo install --path .
```

---

## Usage

```bash
# Basic usage:
asm-annotate [OPTIONS] <ELF_FILE> [FUNCTION]
```

**Examples:**

```bash
# Annotate a specific function in the TUI
asm-annotate firmware.elf my_function

# Omit function name to use the fuzzy finder
asm-annotate firmware.elf

# Adjust source context lines: 2 lines before/after, 5 lines between blocks
asm-annotate firmware.elf my_function --context 2:5

# Use the same number of context lines everywhere
asm-annotate firmware.elf my_function --context 3

# Remap source paths if the build tree moved
asm-annotate firmware.elf my_function --remap /build/path /local/path

# Dump plain-text annotation (no TUI) — pipe to a file or an LLM
asm-annotate firmware.elf my_function --dump
```

### Dump output format

`--dump` prints a compact annotated listing with no ANSI color codes. The source
location comment is only emitted when the source line changes, keeping token
count low for LLM context windows:

```
; function: MyClass::compute(int) [foo.cpp]
1200  push {r4, lr}
1202  cmp r0, #0             ; foo.cpp:38: if (n <= 0)
1204  ble .+14
1206  add r4, r0, r1         ; foo.cpp:39:   return n + offset;
1208  mov r0, r4
120a  pop {pc}
```

### Key TUI Controls

-   `? / Esc / q`: Toggle Help window.
-   `q`: Quit (when help is not visible).
-   `j / Down`: Scroll Down.
-   `k / Up`: Scroll Up.
-   `h / Left`: Activate Source Pane.
-   `l / Right`: Activate Assembly Pane.
-   `g / G`: Toggle Logger Pane.
-   `Tab`: Switch active pane.
-   `PgDown / Ctrl+D`: Page Down.
-   `PgUp / Ctrl+U`: Page Up.
-   `Shift + Left / H`: Decrease Source Pane Width.
-   `Shift + Right / L`: Increase Source Pane Width.

---

## Tips for Best Results

-   **Build with Debug Info:** Ensure your project is compiled with `-g` to include DWARF debug symbols.
-   **Use Hot Reloading:** Keep `asm-annotate` running while you edit and recompile your code. The view will update automatically.

---

## Future Ideas

-   **Automatic Recompilation:** Integrate with build systems (like Cargo, CMake, GN) to offer an option to recompile the project when source files change.
-   **Object File Support:** Allow annotating functions directly from object files (`.o`).
