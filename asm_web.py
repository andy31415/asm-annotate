#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.9"
# dependencies = [
#   "pyelftools",
#   "coloredlogs",
# ]
# ///
"""
asm_web.py — Local web server: side-by-side source↔asm with live recompile.

Usage:
    python asm_web.py <elf_file> <function_name> [options]
    python asm_web.py firmware.elf my_function
    python asm_web.py firmware.elf my_function --compile-commands compile_commands.json
    python asm_web.py firmware.elf my_function --port 8080
"""

import argparse
import json
import logging
import os
import subprocess
import sys
import tempfile
from http.server import HTTPServer, BaseHTTPRequestHandler
import urllib.parse

import coloredlogs

from asm_core import (
    build_addr_to_src,
    disassemble_range,
    find_objdump,
    get_function_bounds,
    list_functions,
)

log = logging.getLogger(__name__)


def _insns_to_dicts(instructions):
    """Convert (addr, raw, mnem) tuples to the dicts expected by the JS frontend."""
    return [{"addr": a, "raw": r, "mnem": m} for a, r, m in instructions]


# ── compile_commands.json lookup ─────────────────────────────────────────────
def find_compile_command(compile_commands_path, source_file):
    """Find compile command for a source file."""
    if not compile_commands_path or not os.path.isfile(compile_commands_path):
        return None
    with open(compile_commands_path) as f:
        commands = json.load(f)
    source_abs = os.path.abspath(source_file)
    for entry in commands:
        entry_file = os.path.abspath(entry.get("file", ""))
        if entry_file == source_abs or entry_file.endswith(
            os.path.basename(source_file)
        ):
            return entry
    return None


def recompile_to_asm(source_content, compile_entry, func_name, objdump):
    """
    Write modified source to a temp file, compile to a temp ELF,
    extract the function, return (instructions, addr_to_src, error).
    """
    if not compile_entry:
        return None, None, "No compile_commands.json entry found for this source."

    src_file = compile_entry.get("file", "")
    ext = os.path.splitext(src_file)[1] or ".c"
    work_dir = compile_entry.get("directory", os.getcwd())
    command = compile_entry.get("command", "")
    arguments = compile_entry.get("arguments", [])

    with tempfile.TemporaryDirectory() as tmpdir:
        # write modified source
        tmp_src = os.path.join(tmpdir, f"modified{ext}")
        tmp_obj = os.path.join(tmpdir, "modified.o")
        tmp_elf = os.path.join(tmpdir, "modified.elf")

        with open(tmp_src, "w") as f:
            f.write(source_content)

        # build compiler command
        if arguments:
            cmd = list(arguments)
        elif command:
            import shlex

            cmd = shlex.split(command)
        else:
            return None, None, "No compile command found."

        # patch: replace source file with our temp, add -o tmp_obj
        new_cmd = []
        skip_next = False
        for i, arg in enumerate(cmd):
            if skip_next:
                skip_next = False
                continue
            if arg == "-o":
                skip_next = True
                continue
            if arg == src_file or arg == os.path.basename(src_file):
                new_cmd.append(tmp_src)
                continue
            new_cmd.append(arg)

        # add output and compile-only flags
        new_cmd += ["-o", tmp_obj, "-c", "-g"]

        # compile to object
        result = subprocess.run(new_cmd, capture_output=True, text=True, cwd=work_dir)
        if result.returncode != 0:
            return None, None, f"Compile error:\n{result.stderr}"

        # link to minimal ELF (for symbol table + DWARF)
        # For embedded, we just use the object directly if link fails
        try:
            # try linking
            ld_cmd = ["arm-none-eabi-ld", "-o", tmp_elf, tmp_obj, "--entry=0"]
            r = subprocess.run(ld_cmd, capture_output=True)
            elf_to_use = tmp_elf if r.returncode == 0 else tmp_obj
        except FileNotFoundError:
            elf_to_use = tmp_obj

        try:
            start, end = get_function_bounds(elf_to_use, func_name)
            addr_to_src = build_addr_to_src(elf_to_use)
            instructions = disassemble_range(elf_to_use, objdump, start, end)
            return instructions, addr_to_src, None
        except Exception as e:
            # fall back: disassemble whole object
            cmd2 = [objdump, "-d", elf_to_use]
            r2 = subprocess.run(cmd2, capture_output=True, text=True)
            return (
                None,
                None,
                f"Could not find function in output: {e}\n\nFull disassembly:\n{r2.stdout[:3000]}",
            )


# ── state shared with HTTP handler ───────────────────────────────────────────
class AppState:
    def __init__(self):
        self.elf_path = ""
        self.func_name = ""
        self.objdump = ""
        self.compile_commands = None
        self.instructions = []
        self.addr_to_src = {}
        self.source_files = {}  # path → content
        self.error = None


STATE = AppState()


# ── HTTP handler ──────────────────────────────────────────────────────────────
class Handler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass  # suppress default logging

    def send_json(self, data, status=200):
        body = json.dumps(data).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", len(body))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(body)

    def send_html(self, html):
        body = html.encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", len(body))
        self.end_headers()
        self.wfile.write(body)

    def read_body(self):
        length = int(self.headers.get("Content-Length", 0))
        return self.rfile.read(length).decode() if length else ""

    def do_OPTIONS(self):
        self.send_response(200)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.end_headers()

    def do_GET(self):
        parsed = urllib.parse.urlparse(self.path)
        path = parsed.path

        if path == "/" or path == "/index.html":
            self.send_html(HTML_PAGE)
        elif path == "/api/state":
            self.api_get_state()
        elif path == "/api/functions":
            self.api_list_functions()
        else:
            self.send_response(404)
            self.end_headers()

    def do_POST(self):
        parsed = urllib.parse.urlparse(self.path)
        path = parsed.path

        if path == "/api/recompile":
            self.api_recompile()
        elif path == "/api/switch_function":
            self.api_switch_function()
        else:
            self.send_response(404)
            self.end_headers()

    def api_get_state(self):
        addr_src_serializable = {str(k): list(v) for k, v in STATE.addr_to_src.items()}
        # collect unique source files referenced
        src_files = {}
        for addr, (fpath, _) in STATE.addr_to_src.items():
            if fpath not in src_files:
                try:
                    with open(fpath, "r", errors="replace") as f:
                        src_files[fpath] = f.read()
                except OSError:
                    src_files[fpath] = f"// Could not read: {fpath}"

        self.send_json(
            {
                "elf": STATE.elf_path,
                "func": STATE.func_name,
                "instructions": _insns_to_dicts(STATE.instructions),
                "addr_to_src": addr_src_serializable,
                "source_files": src_files,
                "error": STATE.error,
                "has_compile_commands": STATE.compile_commands is not None,
            }
        )

    def api_list_functions(self):
        funcs = list_functions(STATE.elf_path)
        self.send_json([{"name": n, "addr": a, "size": s} for n, a, s in funcs])

    def api_switch_function(self):
        body = json.loads(self.read_body())
        func = body.get("func", "")
        try:
            start, end = get_function_bounds(STATE.elf_path, func)
            STATE.instructions = disassemble_range(STATE.elf_path, STATE.objdump, start, end)
            STATE.func_name = func
            STATE.error = None
            self.send_json({"ok": True})
        except Exception as e:
            self.send_json({"ok": False, "error": str(e)})

    def api_recompile(self):
        body = json.loads(self.read_body())
        source_content = body.get("source", "")
        source_path = body.get("path", "")

        if not STATE.compile_commands:
            self.send_json({"ok": False, "error": "No compile_commands.json loaded."})
            return

        compile_entry = find_compile_command(STATE.compile_commands, source_path)
        instructions, addr_to_src, error = recompile_to_asm(
            source_content, compile_entry, STATE.func_name, STATE.objdump
        )

        if error:
            self.send_json({"ok": False, "error": error})
        else:
            addr_src_serializable = {str(k): list(v) for k, v in addr_to_src.items()}
            self.send_json(
                {
                    "ok": True,
                    "instructions": _insns_to_dicts(instructions),
                    "addr_to_src": addr_src_serializable,
                }
            )


# ── HTML page (single-file app) ───────────────────────────────────────────────
HTML_PAGE = r"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>ASM Explorer</title>
<style>
  @import url('https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@300;400;500;700&family=Space+Grotesk:wght@400;500;700&display=swap');

  :root {
    --bg: #0d1117;
    --bg2: #161b22;
    --bg3: #21262d;
    --border: #30363d;
    --text: #e6edf3;
    --dim: #7d8590;
    --accent: #58a6ff;
    --gold: #e3b341;
    --green: #3fb950;
    --red: #f85149;
    --purple: #bc8cff;
  }

  * { box-sizing: border-box; margin: 0; padding: 0; }

  body {
    background: var(--bg);
    color: var(--text);
    font-family: 'JetBrains Mono', monospace;
    font-size: 13px;
    height: 100vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  header {
    background: var(--bg2);
    border-bottom: 1px solid var(--border);
    padding: 10px 16px;
    display: flex;
    align-items: center;
    gap: 16px;
    flex-shrink: 0;
  }

  .logo {
    font-family: 'Space Grotesk', sans-serif;
    font-weight: 700;
    font-size: 15px;
    color: var(--accent);
    letter-spacing: -0.5px;
    white-space: nowrap;
  }

  .logo span { color: var(--gold); }

  select {
    background: var(--bg3);
    border: 1px solid var(--border);
    color: var(--text);
    padding: 4px 8px;
    border-radius: 4px;
    font-family: inherit;
    font-size: 12px;
    cursor: pointer;
    min-width: 200px;
  }

  button {
    background: var(--accent);
    color: #000;
    border: none;
    padding: 5px 12px;
    border-radius: 4px;
    font-family: inherit;
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
    white-space: nowrap;
  }

  button:hover { filter: brightness(1.15); }
  button.secondary { background: var(--bg3); color: var(--text); border: 1px solid var(--border); font-weight: 400; }

  .stats-bar {
    display: flex;
    gap: 16px;
    margin-left: auto;
    font-size: 11px;
    color: var(--dim);
  }

  .stats-bar strong { color: var(--text); }

  .main {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  .pane {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-right: 1px solid var(--border);
  }

  .pane:last-child { border-right: none; }

  .pane-header {
    background: var(--bg2);
    border-bottom: 1px solid var(--border);
    padding: 6px 12px;
    font-size: 11px;
    color: var(--dim);
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-shrink: 0;
  }

  .pane-header .title { color: var(--text); font-weight: 500; }

  .code-scroll {
    flex: 1;
    overflow-y: auto;
    overflow-x: auto;
  }

  .code-scroll::-webkit-scrollbar { width: 6px; height: 6px; }
  .code-scroll::-webkit-scrollbar-track { background: transparent; }
  .code-scroll::-webkit-scrollbar-thumb { background: var(--border); border-radius: 3px; }

  pre {
    padding: 8px 0;
    white-space: pre;
    min-width: max-content;
  }

  .src-line, .asm-line {
    display: flex;
    align-items: baseline;
    padding: 0 4px;
    transition: background 0.1s;
    cursor: default;
    border-left: 3px solid transparent;
  }

  .src-line:hover, .asm-line:hover { background: rgba(255,255,255,0.04); }

  .src-line.active, .asm-line.active {
    background: rgba(255,255,255,0.07);
  }

  .lineno {
    color: var(--dim);
    user-select: none;
    text-align: right;
    min-width: 44px;
    padding-right: 12px;
    flex-shrink: 0;
  }

  .addr {
    color: var(--dim);
    min-width: 90px;
    flex-shrink: 0;
  }

  .mnem {
    min-width: 100px;
    flex-shrink: 0;
    font-weight: 500;
  }

  .operands { color: var(--dim); }

  .swatch {
    display: inline-block;
    width: 6px;
    height: 14px;
    border-radius: 2px;
    margin-right: 6px;
    flex-shrink: 0;
  }

  .src-text { flex: 1; }

  .file-label {
    font-size: 10px;
    color: var(--purple);
    padding: 10px 12px 2px;
    border-top: 1px solid var(--border);
  }

  .file-label:first-child { border-top: none; padding-top: 4px; }

  #editor-area {
    width: 100%;
    height: 100%;
    resize: none;
    background: var(--bg);
    color: var(--text);
    font-family: 'JetBrains Mono', monospace;
    font-size: 13px;
    border: none;
    outline: none;
    padding: 8px 12px;
    line-height: 1.6;
    tab-size: 4;
  }

  .error-box {
    background: rgba(248,81,73,0.1);
    border: 1px solid var(--red);
    color: var(--red);
    padding: 12px 16px;
    margin: 12px;
    border-radius: 4px;
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .loading {
    color: var(--dim);
    padding: 20px;
    text-align: center;
    font-size: 12px;
  }

  .asm-cost {
    font-size: 10px;
    color: var(--dim);
    margin-left: 8px;
  }

  .tooltip {
    position: fixed;
    background: var(--bg3);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 4px 8px;
    font-size: 11px;
    color: var(--text);
    pointer-events: none;
    z-index: 100;
    display: none;
  }
</style>
</head>
<body>

<header>
  <div class="logo">ASM <span>Explorer</span></div>

  <select id="func-select" onchange="switchFunction(this.value)">
    <option>Loading functions…</option>
  </select>

  <button onclick="recompile()" id="recompile-btn" title="Recompile edited source and show new ASM">⚡ Recompile</button>
  <button class="secondary" onclick="resetSource()" title="Reset source to original">↺ Reset</button>

  <div class="stats-bar">
    <span>Instructions: <strong id="stat-insns">—</strong></span>
    <span>Bytes: <strong id="stat-bytes">—</strong></span>
    <span id="stat-diff" style="display:none"></span>
  </div>
</header>

<div class="main">
  <!-- SOURCE PANE -->
  <div class="pane" style="flex: 1.1;">
    <div class="pane-header">
      <span class="title">Source</span>
      <span id="src-file-label" style="font-size:10px;"></span>
    </div>
    <div class="code-scroll" id="src-scroll">
      <div class="loading">Loading…</div>
    </div>
  </div>

  <!-- EDITOR PANE (shown when compile_commands available) -->
  <div class="pane" id="editor-pane" style="flex: 1.1; display:none;">
    <div class="pane-header">
      <span class="title">Edit Source</span>
      <span style="font-size:10px; color: var(--gold);">⚡ Edit + Recompile</span>
    </div>
    <textarea id="editor-area" spellcheck="false" placeholder="Source will appear here for editing…"></textarea>
  </div>

  <!-- ASM PANE -->
  <div class="pane" style="flex: 1;">
    <div class="pane-header">
      <span class="title">Assembly</span>
      <span id="asm-arch-label" style="font-size:10px;"></span>
    </div>
    <div class="code-scroll" id="asm-scroll">
      <div class="loading">Loading…</div>
    </div>
  </div>
</div>

<div class="tooltip" id="tooltip"></div>

<script>
const PALETTE = [
  '#ff6b6b','#ffd93d','#6bcb77','#4d96ff','#ff922b',
  '#cc5de8','#20c997','#f783ac','#74c0fc','#a9e34b',
  '#ff8787','#ffe066','#8ce99a','#74c0fc','#ffa94d',
  '#da77f2','#63e6be','#faa2c1','#a5d8ff','#c0eb75',
];

let STATE = null;
let colorMap = {};     // "file:line" → color
let asmByLine = {};    // "file:line" → [asm indices]
let activeKey = null;
let originalSource = {};   // path → content

async function fetchState() {
  const r = await fetch('/api/state');
  STATE = await r.json();
  renderAll();
}

async function fetchFunctions() {
  const r = await fetch('/api/functions');
  const funcs = await r.json();
  const sel = document.getElementById('func-select');
  sel.innerHTML = funcs.map(f =>
    `<option value="${f.name}" ${f.name === STATE.func ? 'selected' : ''}>${f.name} (${f.size}B)</option>`
  ).join('');
}

async function switchFunction(name) {
  document.getElementById('src-scroll').innerHTML = '<div class="loading">Switching…</div>';
  document.getElementById('asm-scroll').innerHTML = '<div class="loading">…</div>';
  const r = await fetch('/api/switch_function', {
    method: 'POST',
    headers: {'Content-Type':'application/json'},
    body: JSON.stringify({func: name})
  });
  await fetchState();
}

async function recompile() {
  const btn = document.getElementById('recompile-btn');
  btn.textContent = '⏳ Compiling…';
  btn.disabled = true;

  const editorPath = Object.keys(STATE.source_files)[0] || '';
  const source = document.getElementById('editor-area').value;

  const r = await fetch('/api/recompile', {
    method: 'POST',
    headers: {'Content-Type':'application/json'},
    body: JSON.stringify({source, path: editorPath})
  });
  const data = await r.json();

  btn.textContent = '⚡ Recompile';
  btn.disabled = false;

  if (!data.ok) {
    document.getElementById('asm-scroll').innerHTML =
      `<div class="error-box">${escHtml(data.error)}</div>`;
    return;
  }

  const origBytes = countBytes(STATE.instructions);
  STATE.instructions = data.instructions;
  STATE.addr_to_src = data.addr_to_src;
  const newBytes = countBytes(data.instructions);

  buildColorMap();
  renderAsm();
  updateStats(newBytes, origBytes);
}

function resetSource() {
  const path = Object.keys(STATE.source_files)[0] || '';
  if (originalSource[path]) {
    document.getElementById('editor-area').value = originalSource[path];
  }
}

function countBytes(instructions) {
  return (instructions || []).reduce((s, i) => {
    if (!i.raw) return s;
    try { return s + i.raw.replace(/\s/g,'').length / 2; } catch { return s; }
  }, 0);
}

function updateStats(bytes, origBytes) {
  document.getElementById('stat-insns').textContent = STATE.instructions.length;
  document.getElementById('stat-bytes').textContent = bytes;
  const diff = document.getElementById('stat-diff');
  if (origBytes !== undefined && origBytes !== bytes) {
    const delta = bytes - origBytes;
    const sign = delta > 0 ? '+' : '';
    diff.textContent = `Δ ${sign}${delta}B`;
    diff.style.color = delta > 0 ? '#f85149' : '#3fb950';
    diff.style.display = '';
  } else {
    diff.style.display = 'none';
  }
}

function escHtml(s) {
  return String(s)
    .replace(/&/g,'&amp;').replace(/</g,'&lt;')
    .replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}

function buildColorMap() {
  colorMap = {};
  asmByLine = {};
  let idx = 0;

  for (const [addrStr, [file, line]] of Object.entries(STATE.addr_to_src)) {
    const key = `${file}:${line}`;
    if (!colorMap[key]) {
      colorMap[key] = PALETTE[idx % PALETTE.length];
      idx++;
    }
  }

  STATE.instructions.forEach((insn, i) => {
    const src = STATE.addr_to_src[insn.addr];
    if (src) {
      const key = `${src[0]}:${src[1]}`;
      if (!asmByLine[key]) asmByLine[key] = [];
      asmByLine[key].push(i);
    }
  });
}

function renderAll() {
  if (STATE.error) {
    document.getElementById('src-scroll').innerHTML = `<div class="error-box">${escHtml(STATE.error)}</div>`;
    return;
  }

  buildColorMap();
  renderSource();
  renderAsm();
  updateStats(countBytes(STATE.instructions));
  fetchFunctions();

  // show editor if compile_commands available
  if (STATE.has_compile_commands) {
    document.getElementById('editor-pane').style.display = 'flex';
    const path = Object.keys(STATE.source_files)[0] || '';
    const src = STATE.source_files[path] || '';
    document.getElementById('editor-area').value = src;
    originalSource[path] = src;
  }
}

function renderSource() {
  const files = STATE.source_files;
  const container = document.getElementById('src-scroll');

  if (!files || Object.keys(files).length === 0) {
    container.innerHTML = '<div class="loading" style="color:#f85149">No DWARF source info. Build with -g.</div>';
    return;
  }

  // find which lines are relevant to this function
  const relevantByFile = {};
  for (const [addrStr, [file, line]] of Object.entries(STATE.addr_to_src)) {
    if (!relevantByFile[file]) relevantByFile[file] = new Set();
    relevantByFile[file].add(line);
  }

  let html = '<pre>';

  for (const [fpath, content] of Object.entries(files)) {
    const relevant = relevantByFile[fpath] || new Set();
    const lines = content.split('\n');
    const shortPath = fpath.split('/').slice(-3).join('/');

    if (Object.keys(files).length > 1) {
      html += `<div class="file-label">── ${escHtml(shortPath)} ──</div>`;
      document.getElementById('src-file-label').textContent = shortPath;
    } else {
      document.getElementById('src-file-label').textContent = shortPath;
    }

    // find range to show: min/max relevant line ± context
    const relevantArr = [...relevant].sort((a,b) => a-b);
    if (relevantArr.length === 0) continue;
    const minLine = Math.max(1, relevantArr[0] - 3);
    const maxLine = Math.min(lines.length, relevantArr[relevantArr.length-1] + 3);

    for (let i = minLine - 1; i < maxLine && i < lines.length; i++) {
      const lineNo = i + 1;
      const key = `${fpath}:${lineNo}`;
      const color = colorMap[key];
      const hasAsm = relevant.has(lineNo);
      const borderColor = color || 'transparent';

      html += `<div class="src-line" data-key="${escHtml(key)}" style="border-left-color: ${borderColor}" onmouseenter="highlight('${escHtml(key)}')" onmouseleave="unhighlight()">`;
      html += `<span class="lineno">${lineNo}</span>`;
      if (color) {
        html += `<span class="swatch" style="background:${color}"></span>`;
      } else {
        html += `<span class="swatch" style="background:transparent"></span>`;
      }
      html += `<span class="src-text" style="color:${color || 'var(--dim)'}; ${hasAsm ? '' : 'opacity:0.4'}">${escHtml(lines[i])}</span>`;
      html += '</div>';
    }
  }

  html += '</pre>';
  container.innerHTML = html;
}

function renderAsm() {
  const container = document.getElementById('asm-scroll');
  if (!STATE.instructions || STATE.instructions.length === 0) {
    container.innerHTML = '<div class="loading" style="color:#f85149">No instructions found.</div>';
    return;
  }

  let html = '<pre>';

  STATE.instructions.forEach((insn, i) => {
    const src = STATE.addr_to_src[insn.addr];
    const key = src ? `${src[0]}:${src[1]}` : null;
    const color = key ? (colorMap[key] || '#888') : '#555';

    const mnemParts = insn.mnem.split(/\s+/, 2);
    const mnem = mnemParts[0] || '';
    const operands = mnemParts[1] || '';

    html += `<div class="asm-line" data-idx="${i}" data-key="${escHtml(key || '')}" style="border-left-color:${key ? color : 'transparent'}" onmouseenter="highlight('${escHtml(key || '')}')" onmouseleave="unhighlight()">`;
    html += `<span class="addr" style="color:var(--dim)">0x${insn.addr.toString(16).padStart(8,'0')}</span>`;
    html += `<span class="swatch" style="background:${key ? color : 'transparent'}"></span>`;
    html += `<span class="mnem" style="color:${color}">${escHtml(mnem)}</span>`;
    html += `<span class="operands">${escHtml(operands)}</span>`;
    html += '</div>';
  });

  html += '</pre>';
  container.innerHTML = html;
}

function highlight(key) {
  if (!key) return;
  activeKey = key;

  // highlight source lines
  document.querySelectorAll('.src-line').forEach(el => {
    el.classList.toggle('active', el.dataset.key === key);
  });

  // highlight asm lines
  document.querySelectorAll('.asm-line').forEach(el => {
    el.classList.toggle('active', el.dataset.key === key);
  });

  // scroll asm into view
  const firstAsm = document.querySelector(`.asm-line[data-key="${CSS.escape(key)}"]`);
  if (firstAsm) {
    firstAsm.scrollIntoView({block: 'nearest', behavior: 'smooth'});
  }
}

function unhighlight() {
  activeKey = null;
  document.querySelectorAll('.src-line, .asm-line').forEach(el => el.classList.remove('active'));
}

// kick off
fetchState();
</script>
</body>
</html>
"""


# ── entry point ───────────────────────────────────────────────────────────────
def main():
    parser = argparse.ArgumentParser(description="Local web UI: source↔asm explorer.")
    parser.add_argument("elf", help="Path to ELF file")
    parser.add_argument("function", help="Function name to explore")
    parser.add_argument("--objdump", help="objdump binary (auto-detected)")
    parser.add_argument(
        "--compile-commands", help="Path to compile_commands.json for live recompile"
    )
    parser.add_argument(
        "--port", type=int, default=7777, help="Port to serve on (default: 7777)"
    )
    parser.add_argument(
        "--log-level",
        default="INFO",
        metavar="LEVEL",
        help="Logging level: DEBUG, INFO, WARNING, ERROR (default: INFO)",
    )
    args = parser.parse_args()

    coloredlogs.install(level=args.log_level.upper(), logger=log)

    if not os.path.isfile(args.elf):
        log.error("ELF file not found: %s", args.elf)
        sys.exit(1)

    try:
        objdump = find_objdump(args.objdump)
    except RuntimeError as e:
        log.error("%s", e)
        sys.exit(1)

    log.info("Loading ELF: %s", args.elf)
    log.info("Function:    %s", args.function)
    log.info("Objdump:     %s", objdump)

    STATE.elf_path = args.elf
    STATE.func_name = args.function
    STATE.objdump = objdump
    STATE.compile_commands = args.compile_commands

    try:
        start, end = get_function_bounds(args.elf, args.function)
        log.info("Address:     0x%08x – 0x%08x (%d bytes)", start, end, end - start)
        STATE.addr_to_src = build_addr_to_src(args.elf)
        STATE.instructions = disassemble_range(args.elf, STATE.objdump, start, end)
        log.info("Instructions: %d", len(STATE.instructions))
        if not STATE.addr_to_src:
            log.warning("No DWARF info. Build with -g for source mapping.")
    except Exception as e:
        STATE.error = str(e)
        log.error("%s", e)

    log.info("Open http://localhost:%d", args.port)
    server = HTTPServer(("0.0.0.0", args.port), Handler)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        log.info("Stopped.")


main()
