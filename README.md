# ASM Explorer

Two tools for understanding what your C/C++ source code costs in flash:

- **`asm_annotate.py`** — Colored terminal output, Godbolt-style
- **`asm_web.py`** — Local web UI: side-by-side source↔asm, hover to highlight, live recompile

Both work with your existing ELF file (built by GN/CMake/whatever). No changes to your build needed — just build with `-g` for source mapping.

---

## Install dependencies

```bash
pip install rich pyelftools
```

---

## CLI tool: `asm_annotate.py`

```bash
# List all functions in an ELF
python asm_annotate.py firmware.elf --list

# Annotate a specific function (auto-detects arm-none-eabi-objdump or llvm-objdump)
python asm_annotate.py firmware.elf my_function

# With byte cost table (shows which source lines cost the most flash)
python asm_annotate.py firmware.elf my_function --stats

# Show raw instruction bytes too
python asm_annotate.py firmware.elf my_function --bytes

# Specify objdump explicitly
python asm_annotate.py firmware.elf my_function --objdump arm-none-eabi-objdump
```

### What you get:
- Each source line gets a distinct color
- The assembly instructions from that source line share the same color
- Source lines are shown inline, just above the asm they generated
- `--stats` gives a table sorted by byte cost per source line

---

## Web UI: `asm_web.py`

```bash
# Basic: read-only view (just needs the ELF)
python asm_web.py firmware.elf my_function

# With live recompile (needs compile_commands.json)
python asm_web.py firmware.elf my_function --compile-commands build/compile_commands.json

# Custom port
python asm_web.py firmware.elf my_function --port 8080
```

Then open **http://localhost:7777** in your browser.

### Features:
- Side-by-side source and assembly with color coding
- Hover over any source line → corresponding asm highlights (and vice versa)
- Function switcher: jump between any function in the ELF
- **Live recompile** (when `compile_commands.json` provided):
  - Edit source in the right pane
  - Click ⚡ Recompile
  - See new assembly and Δ byte diff immediately

---

## Tips for best results

### Build with debug info
```
# GCC / Clang — add to your build
-g          # DWARF debug info (source↔asm mapping)
-g3         # also includes macro info
```

In GN:
```gn
cflags = [ "-g" ]
```

### Getting compile_commands.json

**GN:**
```bash
gn gen out/debug --export-compile-commands
# or
gn gen out/debug
ninja -C out/debug -t compdb > compile_commands.json
```

**CMake:**
```bash
cmake -DCMAKE_EXPORT_COMPILE_COMMANDS=ON ..
```

### Toolchain detection order
The tools auto-detect in this order:
1. `arm-none-eabi-objdump`
2. `llvm-objdump`
3. `objdump` (host)

Override with `--objdump <path>` if needed.

---

## Workflow for debugging flash usage

1. Build your firmware with `-g`
2. Run `--list` to find the expensive functions (check your map file too)
3. Use `asm_annotate.py --stats` to see which source lines cost the most bytes
4. If you have `compile_commands.json`, open the web UI and edit+recompile to try changes
5. Compare the Δ byte diff in the stats bar

---

## Limitations

- **Recompile requires** `compile_commands.json` — it uses the exact flags (includes, defines, optimization level) from your real build. This is key for embedded: it uses your actual arm-none-eabi-gcc with your real flags, not a host compiler.
- **Linking for recompile**: The tool compiles to a `.o` and tries to link with `arm-none-eabi-ld`. If that fails it disassembles the object directly (addresses will be relative, not absolute, but the asm is still correct).
- **Inline functions**: May appear under the caller's address. DWARF handles this but results depend on optimization level.
- Source paths in DWARF are absolute — if you move the build tree, source display will degrade gracefully (shows asm only, no source coloring).
