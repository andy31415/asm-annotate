use color_eyre::eyre::Result;

pub trait DemanglerBackend {
    fn demangle(&self, name: &str) -> Result<String>;
}

pub struct CppDemangleBackend;

impl DemanglerBackend for CppDemangleBackend {
    fn demangle(&self, name: &str) -> Result<String> {
        cpp_demangle::Symbol::new(name.as_bytes())
            .map_err(|e| color_eyre::eyre::eyre!("Symbol creation failed: {}", e))?
            .demangle(&cpp_demangle::DemangleOptions::default())
            .map_err(|e| color_eyre::eyre::eyre!("Demangling failed: {}", e))
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
}
