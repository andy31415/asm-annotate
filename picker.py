"""
picker.py — Interactive fuzzy function selector using sk or fzf.
"""

import logging
import subprocess
from typing import Optional

from disasm import demangle_batch
from elf import list_functions

log = logging.getLogger(__name__)


def find_fuzzy_picker() -> Optional[str]:
    """Return the path to skim (sk) or fzf, whichever is available."""
    for tool in ["sk", "fzf"]:
        try:
            subprocess.run([tool, "--version"], capture_output=True, check=True)
            return tool
        except (FileNotFoundError, subprocess.CalledProcessError):
            continue
    return None


def pick_function(elf_path: str, query: str) -> str:
    """
    Resolve *query* to a mangled function name.

    - Exact mangled match → use directly.
    - Substring match (mangled or demangled) on exactly one function → use it.
    - Substring match on many functions → launch sk/fzf for interactive pick.
    """
    funcs = list_functions(elf_path)
    all_names = [n for n, _, _ in funcs]
    dm = demangle_batch(all_names)

    if any(n == query for n, _, _ in funcs):
        return query

    q = query.lower()
    matches = [
        (name, addr, size, dm.get(name, name))
        for name, addr, size in funcs
        if q in name.lower() or q in dm.get(name, name).lower()
    ]

    if not matches:
        raise ValueError(f"No function matching '{query}' found in ELF.")

    if len(matches) == 1:
        log.info("Matched function: %s", dm.get(matches[0][0], matches[0][0]))
        return matches[0][0]

    picker = find_fuzzy_picker()
    if picker is None:
        msg_lines = "\n".join(f"  {dm.get(n, n)}" for n, _, _, _ in matches)
        raise ValueError(
            f"{len(matches)} functions match '{query}'. "
            f"Install sk or fzf for interactive selection, or be more specific.\n"
            f"Matches:\n{msg_lines}"
        )

    lines = [
        f"0x{addr:08x}  {size:>6}  {demangled}"
        for name, addr, size, demangled in matches
    ]
    result = subprocess.run(
        [picker, "--query", query],
        input="\n".join(lines),
        stdout=subprocess.PIPE,
        text=True,
    )
    if result.returncode != 0 or not result.stdout.strip():
        raise ValueError("No function selected.")

    selected = result.stdout.strip().splitlines()[0]
    selected_addr = int(selected.split()[0], 16)
    for name, addr, size, _ in matches:
        if addr == selected_addr:
            return name
    raise ValueError("Could not map picker selection back to a function name.")
