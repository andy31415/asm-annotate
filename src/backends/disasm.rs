#![allow(unused_imports)]
use color_eyre::eyre::{Context, Result};
use goblin::elf;
use log;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

// TODO: Add capstone to Cargo.toml and uncomment
// use capstone::prelude::*;
use cpp_demangle::{DemangleOptions, ParseOptions};

#[derive(Debug, Clone)]
pub struct Instruction {
    pub address: u64,
    pub bytes: String,
    pub mnemonic: String,
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
    let mut instructions = Vec::new();
    let mut lines = stdout.lines().peekable();

    // Skip header lines until we find the disassembly
    while let Some(line) = lines.peek() {
        if line.ends_with(">:") {
            break;
        }
        lines.next();
    }
    lines.next(); // Skip the line with the function name

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

        let bytes = parts[1].trim();
        let mnemonic = parts[2].trim();

        instructions.push(Instruction {
            address: addr,
            bytes: bytes.to_string(),
            mnemonic: mnemonic.to_string(),
        });
    }

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
    let default_bins = vec!["arm-none-eabi-objdump", "objdump"];
    let mut objdump_bins: Vec<&str> = default_bins.clone();

    if let Some(bin) = user_objdump
        && !default_bins.contains(&bin)
    {
        objdump_bins.push(bin);
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

pub fn demangle_batch(names: Vec<String>) -> HashMap<String, String> {
    let mut demangled_map = HashMap::new();
    for name in names {
        if let Ok(symbol) = cpp_demangle::Symbol::new(name.as_bytes())
            && let Ok(demangled) = symbol.demangle(&DemangleOptions::default())
        {
            demangled_map.insert(name, demangled);
        }
    }
    demangled_map
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
    fn test_demangle_batch() {
        let names = vec![
            "_Z3foov".to_string(),
            "_Z3bariz".to_string(),
            "not_mangled".to_string(),
        ];
        let demangled = demangle_batch(names);

        let mut expected = HashMap::new();
        expected.insert("_Z3foov".to_string(), "foo()".to_string());
        expected.insert("_Z3bariz".to_string(), "bar(int, ...)".to_string());

        assert_eq!(demangled, expected);
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

        let func_name = "_Z3bariz".to_string();
        let new_func_name = apply_demangling(func_name, &mut instructions, &demangled_map);

        assert_eq!(new_func_name, "bar(int, ...)");
        assert_eq!(instructions[0].mnemonic, "ret");
        assert_eq!(instructions[1].mnemonic, "call foo()");
    }
}
