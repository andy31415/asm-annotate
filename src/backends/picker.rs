use crate::backends::demangle::DemanglerBackend;
use crate::backends::elf::FunctionInfo;
use color_eyre::eyre::{eyre, Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

pub trait PickerBackend {
    fn pick_function(
        &self,
        functions: Vec<FunctionInfo>,
        demangler: &impl DemanglerBackend,
    ) -> Result<Option<FunctionInfo>>;
}

pub struct SkimBackend;

impl PickerBackend for SkimBackend {
    fn pick_function(
        &self,
        functions: Vec<FunctionInfo>,
        demangler: &impl DemanglerBackend,
    ) -> Result<Option<FunctionInfo>> {
        let mut skim_child = Command::new("sk")
            .arg("-m")
            .arg("--ansi")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .wrap_err("Failed to spawn skim (sk). Is it installed?")?;

        let stdin = skim_child.stdin.as_mut().unwrap();
        for func in &functions {
            let demangled = demangler
                .demangle(&func.name)
                .unwrap_or_else(|_| func.name.clone());
            let display_name = if demangled != func.name {
                format!("{}  [{}]", demangled, func.name)
            } else {
                func.name.clone()
            };
            writeln!(stdin, "{} ({}) {:#x}", display_name, func.size, func.addr)
                .wrap_err("Failed to write to skim stdin")?;
        }

        let output = skim_child
            .wait_with_output()
            .wrap_err("Failed to wait for skim")?;

        if !output.status.success() {
            return Err(eyre!(
                "skim exited with error: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let selected_line = stdout.lines().next();

        match selected_line {
            Some(line) => {
                // Expected format: "DISPLAY NAME (SIZE) 0xADDR"
                let Some(addr_str) = line.rsplit(' ').next() else {
                    return Err(eyre!(
                        "Skim output format unexpected (no address found): {}",
                        line
                    ));
                };

                let Ok(addr) = u64::from_str_radix(addr_str.trim_start_matches("0x"), 16) else {
                    return Err(eyre!("Failed to parse address from skim: {}", addr_str));
                };

                functions
                    .into_iter()
                    .find(|f| f.addr == addr)
                    .map(Some)
                    .ok_or_else(|| {
                        eyre!(
                            "Selected function address not found in original list: {:#x}",
                            addr
                        )
                    })
            }
            None => Ok(None), // No selection made
        }
    }
}
