use color_eyre::eyre::Result;
use std::collections::HashMap;

pub trait DemanglerBackend {
    fn demangle(&self, name: &str) -> Result<String>;
    fn demangle_batch(&self, names: &[String]) -> Result<HashMap<String, String>>;
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_demangle_basic() {
        let backend = CppDemangleBackend;
        assert_eq!(backend.demangle("_Z3foov").unwrap(), "foo()");
        assert_eq!(backend.demangle("_Z3bariz").unwrap(), "bar(int, ...)");
    }

    #[test]
    fn test_demangle_not_mangled() {
        let backend = CppDemangleBackend;
        assert!(backend.demangle("not_mangled").is_err());
    }

    #[test]
    fn test_demangle_batch_skips_failures() {
        let backend = CppDemangleBackend;
        let names = vec![
            "_Z3foov".to_string(),
            "not_mangled".to_string(),
        ];
        let result = backend.demangle_batch(&names).unwrap();
        assert_eq!(result.get("_Z3foov").unwrap(), "foo()");
        // Non-mangled names are silently skipped
        assert!(!result.contains_key("not_mangled"));
    }
}
