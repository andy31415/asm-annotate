# ASM Annotate

A CLI tool for understanding what your C/C++ source code costs in flash — colored, Godbolt-style source↔assembly annotation in your terminal.

Works with your existing ELF file (built by GN/CMake/whatever). No changes to your build needed — just build with `-g` for source mapping.

---

## Installation

```bash
cargo build --release
# Binary is at target/release/asm-annotate

# Or install to ~/.cargo/bin:
cargo install --path .
```

---

## Usage

### Annotate a function

```bash
# Interactive TUI for side-by-side source/asm
asm-annotate firmware.elf my_function

# If my_function is omitted, a fuzzy finder will be shown to select from all functions
asm-annotate firmware.elf

# Remap source paths (e.g. if build tree moved)
asm-annotate firmware.elf my_function --remap /workspace /home/user/src
```

### What you get

-   Each source line gets a distinct color.
-   The assembly instructions from that source line share the same color.
-   An interactive Terminal User Interface (TUI) with:
    -   Source code on the left pane.
    -   Disassembly on the right pane.
    -   Synchronized scrolling hints (colors).
    -   Keyboard navigation (h/j/k/l, PageUp/Down, Ctrl+U/D).
    -   Active pane highlighting.
    -   Resizable panes (Shift + Left/Right or Shift + H/L).

---

## Tips for best results

### Build with debug info

```
-g          # DWARF debug info (source↔asm mapping)
```

In GN:
```gn
cflags = [ "-g" ]
```

### Function selection

If you don't specify a function name, or if your query matches multiple functions, an interactive fuzzy finder (using the embedded `skim` crate) is launched for selection.

---

## Workflow for debugging flash usage

1.  Build your firmware with `-g`.
2.  Run `asm-annotate firmware.elf` to select a function.
3.  Use the TUI to examine the source and corresponding assembly.

---

## Limitations

-   **No web UI** — this is a terminal-only tool.
-   **Inline functions**: May appear under the caller's address. DWARF handles this but results depend on optimization level.
-   Source paths in DWARF are absolute — if you move the build tree, use `--remap` to fix paths. Without remapping, source display degrades gracefully (shows asm only, no source coloring).
