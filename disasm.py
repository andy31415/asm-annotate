"""
disasm.py — Objdump execution, output parsing, and C++ demangling.
"""

import logging
import re
import subprocess
from typing import Optional

log = logging.getLogger(__name__)

_ANGLE_BRACKET_SYM = re.compile(r"<([^>]+)>")


# ── demangling ───────────────────────────────────────────────────────────────
def demangle_batch(names: list[str]) -> dict[str, str]:
    """Return a mangled→demangled mapping via a single c++filt call."""
    if not names:
        return {}
    try:
        result = subprocess.run(
            ["c++filt"],
            input="\n".join(names),
            capture_output=True,
            text=True,
            check=True,
        )
        return dict(zip(names, result.stdout.splitlines()))
    except FileNotFoundError:
        log.debug("c++filt not found; symbol names will not be demangled")
        return {}


def apply_demangling(
    func_name: str,
    instructions: list[tuple[int, str, str]],
) -> tuple[str, list[tuple[int, str, str]]]:
    """Demangle the function name and all <symbol> references in operands."""
    symbols: set[str] = {func_name}
    for _, _, mnem in instructions:
        for m in _ANGLE_BRACKET_SYM.finditer(mnem):
            symbols.add(m.group(1))

    dm = demangle_batch(list(symbols))
    if not dm:
        return func_name, instructions

    def sub_mnem(mnem: str) -> str:
        return _ANGLE_BRACKET_SYM.sub(
            lambda m: f"<{dm.get(m.group(1), m.group(1))}>", mnem
        )

    return (
        dm.get(func_name, func_name),
        [(addr, raw, sub_mnem(mnem)) for addr, raw, mnem in instructions],
    )


# ── objdump ──────────────────────────────────────────────────────────────────
def find_objdump(hint: Optional[str] = None) -> str:
    candidates = []
    if hint:
        candidates.append(hint)
    candidates += [
        "arm-none-eabi-objdump",
        "llvm-objdump",
        "objdump",
    ]
    for c in candidates:
        try:
            subprocess.run([c, "--version"], capture_output=True, check=True)
            return c
        except (FileNotFoundError, subprocess.CalledProcessError):
            continue
    raise RuntimeError(
        "No objdump found. Install arm-none-eabi-binutils or llvm, "
        "or pass --objdump <path>."
    )


def disassemble_range(
    elf_path: str,
    objdump: str,
    start: int,
    end: int,
) -> list[tuple[int, str, str]]:
    """
    Run objdump and return list of (addr, bytes_hex, mnemonic) for
    instructions in [start, end).
    """
    cmd = [
        objdump,
        "-d",
        f"--start-address=0x{start:x}",
        f"--stop-address=0x{end:x}",
        elf_path,
    ]
    log.debug("Running: %s", " ".join(cmd))

    result = subprocess.run(cmd, capture_output=True, text=True)
    out_lines = result.stdout.splitlines()

    log.debug(
        "objdump exit code: %d  (%d lines of output)", result.returncode, len(out_lines)
    )
    if result.stderr.strip():
        log.debug("objdump stderr: %s", result.stderr.strip()[:300])
    for ln in out_lines[:30]:
        log.debug("  %r", ln)

    if result.returncode != 0:
        log.error(
            "objdump failed (exit %d): %s", result.returncode, result.stderr[:500]
        )
        return []

    instructions = []
    # Pattern with raw bytes (GNU/LLVM default):
    #   8000120:   e92d 4ff0   push    {r4, r5, r6, r7, r8, r9, sl, fp, lr}
    pat_bytes = re.compile(r"^\s*([0-9a-f]+):\s+([0-9a-f][0-9a-f ]*?)\s{2,}(.+)$")
    # Fallback when bytes are absent (some toolchains):
    #   8000120:   push    {r4, r5, r6, r7, r8, r9, sl, fp, lr}
    pat_no_bytes = re.compile(r"^\s*([0-9a-f]+):\s+([^\s][^\t]+)$")

    for line in out_lines:
        m = pat_bytes.match(line)
        if m:
            addr = int(m.group(1), 16)
            raw = m.group(2).strip()
            mnem = m.group(3).strip()
        else:
            m = pat_no_bytes.match(line)
            if not m:
                continue
            candidate = m.group(2).strip()
            if re.fullmatch(r"[0-9a-f ]+", candidate):
                continue
            addr = int(m.group(1), 16)
            raw = ""
            mnem = candidate

        if start <= addr < end:
            instructions.append((addr, raw, mnem))

    log.debug(
        "Instructions matched in range 0x%x–0x%x: %d", start, end, len(instructions)
    )
    if not instructions:
        log.debug(
            "Zero instructions matched. Check that the address range appears in "
            "the objdump output above. If it is empty or shows a different range, "
            "the ELF may use a non-standard section layout or symbol bounds are wrong."
        )

    return instructions
