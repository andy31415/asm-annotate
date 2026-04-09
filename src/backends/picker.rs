use crate::backends::elf::FunctionInfo;
use color_eyre::eyre::{Result, Context, eyre};
use std::io::{Write, BufRead, BufReader};
use std::process::{Command, Stdio};

pub trait PickerBackend {
    fn pick_function(&self, functions: Vec<FunctionInfo>) -> Result<Option<FunctionInfo>>;
}

pub struct SkimBackend;

impl PickerBackend for SkimBackend {
    fn pick_function(&self, functions: Vec<FunctionInfo>) -> Result<Option<FunctionInfo>> {
        let mut skim_child = Command::new("sk")
            .arg("-m") // Enable multi-select (though we only need one)
            .arg("--ansi")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .wrap_err("Failed to spawn skim (sk). Is it installed?")?;

        let stdin = skim_child.stdin.as_mut().unwrap();
        for func in &functions {
            writeln!(stdin, "{:#x} {} ({})", func.addr, func.name, func.size)
                .wrap_err("Failed to write to skim stdin")?;
        }

        let output = skim_child.wait_with_output().wrap_err("Failed to wait for skim")?;

        if !output.status.success() {
            return Err(eyre!("skim exited with error: {}", String::from_utf8_lossy(&output.stderr)));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let selected_line = stdout.lines().next();

        match selected_line {
            Some(line) => {
                // Expected format: "0xADDR NAME (SIZE)"
                let parts: Vec<&str> = line.splitn(3, ' ').collect();
                if parts.len() < 2 {
                    return Err(eyre!("Skim output format unexpected: {}", line));
                }
                let addr_str = parts[0];
                let name = parts[1];

                let addr = u64::from_str_radix(addr_str.trim_start_matches("0x"), 16)
                    .wrap_err(format!("Failed to parse address from skim: {}", addr_str))?;

                functions.into_iter()
                    .find(|f| f.addr == addr && f.name == name)
                    .map(Some)
                    .ok_or_else(|| eyre!("Selected function not found in original list"))
            }
            None => Ok(None), // No selection made
        }
    }
}
