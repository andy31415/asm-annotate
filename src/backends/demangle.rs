use color_eyre::eyre::Result;
use std::collections::HashMap;

pub trait DemanglerBackend {
    fn demangle(&self, name: &str) -> Result<String>;
    fn demangle_batch(&self, names: &[String]) -> Result<HashMap<String, String>>;
}

// TODO: Implement CppDemangleBackend
pub struct CppDemangleBackend;

impl DemanglerBackend for CppDemangleBackend {
    fn demangle(&self, name: &str) -> Result<String> {
        cpp_demangle::Symbol::new(name.as_bytes())
            .map_err(|e| color_eyre::eyre::eyre!("Symbol creation failed: {}", e))?
            .demangle(&cpp_demangle::DemangleOptions::default())
            .map_err(|e| color_eyre::eyre::eyre!("Demangling failed: {}", e))
    }

    fn demangle_batch(&self, names: &[String]) -> Result<HashMap<String, String>> {
        let mut result = HashMap::new();
        for name in names {
            match self.demangle(name) {
                Ok(demangled) => {
                    log::trace!("Successfully demangled {} -> {}", name, demangled);
                    result.insert(name.clone(), demangled);
                }
                Err(e) => {
                    log::trace!("Failed to demangle {}: {}", name, e);
                }
            }
        }
        Ok(result)
    }
}
