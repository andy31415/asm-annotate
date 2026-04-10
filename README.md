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

### List all functions in an ELF

```bash
asm-annotate list firmware.elf
```

### Annotate a function

```bash
# Side-by-side source│asm layout (default)
asm-annotate annotate firmware.elf my_function

# Classic interleaved source+asm output
asm-annotate annotate firmware.elf my_function --format unified

# Split with a custom source column width
asm-annotate annotate firmware.elf my_function --format split:60

# Full source context panel left, asm right
asm-annotate annotate firmware.elf my_function --format sidebyside

# With byte cost table
asm-annotate annotate firmware.elf my_function --stats

# Show raw instruction bytes too
asm-annotate annotate firmware.elf my_function --bytes

# Override objdump binary
asm-annotate annotate firmware.elf my_function --objdump arm-none-eabi-objdump

# Remap source paths (e.g. if build tree moved)
asm-annotate annotate firmware.elf my_function --remap /workspace /home/user/src
```

Subcommand aliases: `l` for `list`, `a` for `annotate`.

### What you get

- Each source line gets a distinct color
- The assembly instructions from that source line share the same color
- `split` (default): source and asm in side-by-side columns separated by `│`
- `unified`: source lines shown inline just above the asm they generated
- `sidebyside`: full source context panel on the left, full asm listing on the right
- `--stats`: table sorted by byte cost per source line

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

### Toolchain detection order

The tool auto-detects objdump in this order:
1. `arm-none-eabi-objdump`
2. `llvm-objdump`
3. `objdump` (host)

Override with `--objdump <path>` if needed.

### Function disambiguation

If your query matches multiple functions, `sk` (skim) is launched for interactive selection. Install skim if you work with C++ code that has many similarly-named overloads.

---

## Workflow for debugging flash usage

1. Build your firmware with `-g`
2. Run `list` to find the expensive functions (check your map file too)
3. Use `annotate --stats` to see which source lines cost the most bytes
4. Use `--format split` or `--format sidebyside` to read the asm alongside source

---

## Limitations

- **No web UI** — this is a terminal-only tool.
- **Inline functions**: May appear under the caller's address. DWARF handles this but results depend on optimization level.
- Source paths in DWARF are absolute — if you move the build tree, use `--remap` to fix paths. Without remapping, source display degrades gracefully (shows asm only, no source coloring).
- The interactive picker requires `sk` (skim). There is no fzf fallback.
