use color_eyre::eyre::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Instruction {
    pub address: u64,
    pub bytes: String,
    pub mnemonic: String,
}

/// Parses the text output of `objdump -d` into a list of instructions.
/// Skips header lines until the first function header (`<name>:`), then
/// parses tab-separated `address : bytes : mnemonic` lines.
pub(crate) fn parse_objdump_output(stdout: &str) -> Vec<Instruction> {
    let mut instructions = Vec::new();
    let mut lines = stdout.lines().peekable();

    // Skip header lines until we find the disassembly section
    while let Some(line) = lines.peek() {
        if line.ends_with(">:") {
            break;
        }
        lines.next();
    }
    lines.next(); // Skip the function name line itself

    for line in lines {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }

        let addr_str = parts[0].trim().strip_suffix(':').unwrap_or(parts[0].trim());
        let addr = match u64::from_str_radix(addr_str, 16) {
            Ok(a) => a,
            Err(_) => continue,
        };

        instructions.push(Instruction {
            address: addr,
            bytes: parts[1].trim().to_string(),
            mnemonic: parts[2..].join(" ").trim().to_string(),
        });
    }

    instructions
}

fn try_disassemble(
    elf_path: &Path,
    objdump_bin: &str,
    start: u64,
    end: u64,
) -> Result<Vec<Instruction>> {
    log::debug!("Trying objdump: {}", objdump_bin);
    let output = Command::new(objdump_bin)
        .arg("-d")
        .arg(format!("--start-address={}", start))
        .arg(format!("--stop-address={}", end))
        .arg(elf_path)
        .output()
        .wrap_err(format!("Failed to execute {}", objdump_bin))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("can't disassemble for architecture UNKNOWN") {
            return Err(color_eyre::eyre::eyre!(
                "{} architecture UNKNOWN",
                objdump_bin
            ));
        }
        return Err(color_eyre::eyre::eyre!(
            "{} failed: {}",
            objdump_bin,
            stderr
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let instructions = parse_objdump_output(&stdout);

    if instructions.is_empty() {
        return Err(color_eyre::eyre::eyre!(
            "No instructions found by {}",
            objdump_bin
        ));
    }

    Ok(instructions)
}

pub fn disassemble_range(
    elf_path: &Path,
    user_objdump: Option<&str>,
    start: u64,
    end: u64,
) -> Result<Vec<Instruction>> {
    // User-specified binary is tried first; defaults follow in detection order.
    const DEFAULT_BINS: &[&str] = &["arm-none-eabi-objdump", "llvm-objdump", "objdump"];
    let mut objdump_bins: Vec<&str> = Vec::new();
    if let Some(bin) = user_objdump {
        objdump_bins.push(bin);
    }
    for bin in DEFAULT_BINS {
        if !objdump_bins.contains(bin) {
            objdump_bins.push(bin);
        }
    }

    for bin in &objdump_bins {
        match try_disassemble(elf_path, bin, start, end) {
            Ok(instructions) => {
                log::info!("Using objdump: {}", bin);
                return Ok(instructions);
            }
            Err(e) => {
                if e.to_string().contains("architecture UNKNOWN") {
                    log::debug!("{} failed (Architecture UNKNOWN), trying next...", bin);
                } else {
                    log::debug!("{} failed: {}, trying next...", bin, e);
                }
            }
        }
    }

    Err(color_eyre::eyre::eyre!(
        "All objdump attempts failed for range {:#x}-{:#x}",
        start,
        end
    ))
}

/// Applies demangled names to a function name and its instructions.
pub fn apply_demangling(
    func_name: String,
    instructions: &mut [Instruction],
    demangled_map: &HashMap<String, String>,
) -> String {
    let new_func_name = demangled_map.get(&func_name).unwrap_or(&func_name).clone();

    for inst in instructions {
        let mut new_mnemonic = inst.mnemonic.clone();
        for (mangled, demangled) in demangled_map {
            if new_mnemonic.contains(mangled) {
                new_mnemonic = new_mnemonic.replace(mangled, demangled);
            }
        }
        inst.mnemonic = new_mnemonic;
    }

    new_func_name
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_parse_objdump_output() {
        let output = "\
firmware.elf:     file format elf32-littlearm

Disassembly of section .text:

00010000 <my_func>:
   10000:\tf0 b5       \tpush\t{r4, r5, r6, r7, lr}
   10002:\t00 af       \tadd\tr7, sp, #0
   10004:\te8 bd f0    \tpop\t{r4, r5, r6, r7, pc}
";
        let instructions = parse_objdump_output(output);
        assert_eq!(instructions.len(), 3);
        assert_eq!(instructions[0].address, 0x10000);
        assert_eq!(instructions[0].bytes, "f0 b5");
        // Fields are joined with spaces (tabs replaced), mnemonic may have extra columns
        assert_eq!(instructions[0].mnemonic, "push {r4, r5, r6, r7, lr}");
        assert_eq!(instructions[1].address, 0x10002);
        assert_eq!(instructions[2].address, 0x10004);
    }

    #[test]
    fn test_parse_objdump_output_empty() {
        let output = "firmware.elf:     file format elf32-littlearm\n\n";
        let instructions = parse_objdump_output(output);
        assert!(instructions.is_empty());
    }

    #[test]
    fn test_apply_demangling() {
        let mut instructions = vec![
            Instruction {
                address: 0x1000,
                bytes: "C3".to_string(),
                mnemonic: "ret".to_string(),
            },
            Instruction {
                address: 0x1001,
                bytes: "E8 00000000".to_string(),
                mnemonic: "call _Z3foov".to_string(),
            },
        ];
        let mut demangled_map = HashMap::new();
        demangled_map.insert("_Z3foov".to_string(), "foo()".to_string());
        demangled_map.insert("_Z3bariz".to_string(), "bar(int, ...)".to_string());

        let new_func_name =
            apply_demangling("_Z3bariz".to_string(), &mut instructions, &demangled_map);

        assert_eq!(new_func_name, "bar(int, ...)");
        assert_eq!(instructions[0].mnemonic, "ret");
        assert_eq!(instructions[1].mnemonic, "call foo()");
    }

    #[test]
    fn test_apply_demangling_unknown_func() {
        let mut instructions = vec![];
        let demangled_map = HashMap::new();
        // Function name not in map → returned unchanged
        let name = apply_demangling("plain_func".to_string(), &mut instructions, &demangled_map);
        assert_eq!(name, "plain_func");
    }
}
